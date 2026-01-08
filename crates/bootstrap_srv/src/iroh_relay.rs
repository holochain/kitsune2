//! Iroh relay integration for kitsune2 bootstrap server.
//!
//! This module provides axum integration for the iroh relay server, allowing
//! it to be embedded as a standard route in the bootstrap server.
//!
//! # Rate Limiting
//!
//! Connection-level rate limiting is implemented using the `governor` crate with
//! a token bucket algorithm. The rate limiter is applied as tower middleware on
//! the relay routes.
//!
//! - `accept_conn_limit`: Maximum connections per second (sustained rate)
//! - `accept_conn_burst`: Maximum burst size (token bucket capacity)
//!
//! When rate limits are exceeded, clients receive a 429 Too Many Requests response.
//!
//! Note: Byte-level rate limiting (`client_rx`) is not supported in the axum
//! integration since WebSocket connections are already established when they
//! reach the handler.

use axum::response::{IntoResponse, Response};
use governor::clock::DefaultClock;
use governor::state::{InMemoryState, NotKeyed};
use governor::{Quota, RateLimiter};
use http::{Request, StatusCode};
use iroh_relay::server::axum_integration::RelayState;
use iroh_relay::server::{AccessConfig, Metrics};
use iroh_relay::KeyCache;
use std::future::Future;
use std::num::NonZeroU32;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use tower::{Layer, Service};

pub use iroh_relay::server::axum_integration::relay_handler;
pub use iroh_relay::server::ClientRateLimit;

pub type ConnectionRateLimiter =
    Arc<RateLimiter<NotKeyed, InMemoryState, DefaultClock>>;

/// Layer for rate limiting relay connections
#[derive(Clone)]
pub struct RateLimitLayer {
    limiter: ConnectionRateLimiter,
}

impl RateLimitLayer {
    pub fn new(limiter: ConnectionRateLimiter) -> Self {
        Self { limiter }
    }
}

impl<S> Layer<S> for RateLimitLayer {
    type Service = RateLimitService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        RateLimitService {
            inner,
            limiter: self.limiter.clone(),
        }
    }
}

/// Service that applies rate limiting
#[derive(Clone)]
pub struct RateLimitService<S> {
    inner: S,
    limiter: ConnectionRateLimiter,
}

impl<S, ReqBody> Service<Request<ReqBody>> for RateLimitService<S>
where
    S: Service<Request<ReqBody>, Response = Response> + Clone + Send + 'static,
    S::Future: Send + 'static,
    ReqBody: Send + 'static,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = Pin<
        Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>,
    >;

    fn poll_ready(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        // Check rate limit
        if self.limiter.check().is_err() {
            // Rate limit exceeded
            let response = (
                StatusCode::TOO_MANY_REQUESTS,
                "Connection rate limit exceeded",
            )
                .into_response();
            return Box::pin(async move { Ok(response) });
        }

        // Allow request through
        let future = self.inner.call(req);
        Box::pin(async move { future.await })
    }
}

/// Handler for the relay probe endpoint (`/ping`).
/// This is used for latency testing and availability checks.
pub async fn relay_probe_handler() -> (
    axum::http::StatusCode,
    axum::response::AppendHeaders<[(axum::http::HeaderName, &'static str); 1]>,
) {
    (
        axum::http::StatusCode::OK,
        axum::response::AppendHeaders([(
            axum::http::header::ACCESS_CONTROL_ALLOW_ORIGIN,
            "*",
        )]),
    )
}

/// Rate limits configuration for the relay server
#[derive(Debug, Default)]
pub struct Limits {
    /// Rate limit for accepting new connection. Unlimited if not set.
    pub accept_conn_limit: Option<f64>,
    /// Burst limit for accepting new connection. Unlimited if not set.
    pub accept_conn_burst: Option<usize>,
    /// Rate limits for incoming traffic from a client connection.
    pub client_rx: Option<ClientRateLimit>,
}

/// Configuration for iroh relay server
#[derive(Debug)]
pub struct Config {
    /// Rate limiting configuration for iroh relay server
    pub limits: Limits,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            limits: Limits {
                accept_conn_limit: Some(250.0),
                accept_conn_burst: Some(100),
                client_rx: Some(ClientRateLimit {
                    bytes_per_second: NonZeroU32::new(2 * 1024 * 1024).unwrap(), // 2 MiB/s
                    max_burst_bytes: Some(NonZeroU32::new(256 * 1024).unwrap()), // 256 KiB burst
                }),
            },
        }
    }
}

/// Creates a connection rate limiter from the limits configuration
///
/// Returns None if no connection rate limiting is configured.
fn create_connection_rate_limiter(
    limits: &Limits,
) -> Option<ConnectionRateLimiter> {
    let rate = limits.accept_conn_limit?;
    let burst = limits.accept_conn_burst?;

    // Convert rate (connections per second) to a quota
    // Rate is the sustained rate, burst is the maximum burst size
    let rate_u32 = rate.max(1.0) as u32;
    let burst_u32 = burst.max(1) as u32;

    let rate_nonzero = std::num::NonZeroU32::new(rate_u32)?;
    let burst_nonzero = std::num::NonZeroU32::new(burst_u32)?;

    let quota = Quota::per_second(rate_nonzero).allow_burst(burst_nonzero);

    Some(Arc::new(RateLimiter::direct(quota)))
}

/// Creates a RelayState instance for the axum relay handler
///
/// This creates the state needed for the relay handler which can be mounted
/// as a standard axum route.
///
/// Returns the RelayState and an optional connection rate limiter. The rate limiter
/// should be applied as middleware to the relay router.
///
/// # Rate Limiting
///
/// - **Connection-level rate limiting**: Supported via `accept_conn_limit` and `accept_conn_burst`.
///   Applied as tower middleware using a token bucket algorithm. When the limit is exceeded,
///   requests receive a 429 Too Many Requests response.
///
/// - **Client-side byte-level rate limiting**: NOT supported (`client_rx` is ignored).
///   The axum integration receives already-established WebSocket connections without access
///   to the underlying TCP/TLS stream, making byte-level rate limiting impractical at this layer.
pub(super) fn create_relay_state(
    limits: &Limits,
) -> (RelayState, Option<ConnectionRateLimiter>) {
    let key_cache = KeyCache::new(1024);
    let access = Arc::new(AccessConfig::Everyone);
    let metrics = Arc::new(Metrics::default());
    let state = RelayState::new(key_cache, access, metrics);

    let rate_limiter = create_connection_rate_limiter(limits);

    (state, rate_limiter)
}
