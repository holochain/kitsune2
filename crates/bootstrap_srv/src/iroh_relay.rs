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
use tracing::warn;

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
/// Returns None if no connection rate limiting is configured or if the
/// configuration values are invalid.
fn create_connection_rate_limiter(
    limits: &Limits,
) -> Option<ConnectionRateLimiter> {
    let rate = limits.accept_conn_limit?;
    let burst = limits.accept_conn_burst?;

    // Define valid ranges for rate limiting parameters
    const MIN_RATE: f64 = 1.0;
    const MAX_RATE: f64 = u32::MAX as f64;
    const MIN_BURST: f64 = 1.0;
    const MAX_BURST: f64 = u32::MAX as f64;

    // Clamp rate to valid range and warn if adjusted
    let clamped_rate = rate.clamp(MIN_RATE, MAX_RATE);
    if (rate - clamped_rate).abs() > f64::EPSILON {
        warn!(
            original_rate = rate,
            clamped_rate = clamped_rate,
            "Relay connection rate limit out of bounds, clamping to valid range [{}, {}]",
            MIN_RATE, MAX_RATE
        );
    }

    // Clamp burst to valid range and warn if adjusted
    let clamped_burst = (burst as f64).clamp(MIN_BURST, MAX_BURST);
    if ((burst as f64) - clamped_burst).abs() > f64::EPSILON {
        warn!(
            original_burst = burst,
            clamped_burst = clamped_burst,
            "Relay connection burst limit out of bounds, clamping to valid range [{}, {}]",
            MIN_BURST, MAX_BURST
        );
    }

    // Convert to u32 using ceil() for deterministic rounding
    // Ceiling ensures we don't accidentally reduce limits below configured values
    let rate_u32 = clamped_rate.ceil() as u32;
    let burst_u32 = clamped_burst.ceil() as u32;

    // Create NonZeroU32 values, returning None if somehow we got zero
    // (should be impossible given MIN values of 1.0, but defensive programming)
    let rate_nonzero = NonZeroU32::new(rate_u32)?;
    let burst_nonzero = NonZeroU32::new(burst_u32)?;

    // Create quota with clamped and validated values
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rate_limiter_creation_valid_values() {
        let limits = Limits {
            accept_conn_limit: Some(100.0),
            accept_conn_burst: Some(50),
            client_rx: None,
        };

        let limiter = create_connection_rate_limiter(&limits);
        assert!(
            limiter.is_some(),
            "Valid values should create a rate limiter"
        );
    }

    #[test]
    fn test_rate_limiter_creation_none_when_not_configured() {
        let limits = Limits {
            accept_conn_limit: None,
            accept_conn_burst: Some(50),
            client_rx: None,
        };

        let limiter = create_connection_rate_limiter(&limits);
        assert!(
            limiter.is_none(),
            "Should return None when rate limit is not configured"
        );

        let limits = Limits {
            accept_conn_limit: Some(100.0),
            accept_conn_burst: None,
            client_rx: None,
        };

        let limiter = create_connection_rate_limiter(&limits);
        assert!(
            limiter.is_none(),
            "Should return None when burst is not configured"
        );
    }

    #[test]
    fn test_rate_limiter_clamping_minimum() {
        // Test values below minimum get clamped to 1
        let limits = Limits {
            accept_conn_limit: Some(0.5),
            accept_conn_burst: Some(0),
            client_rx: None,
        };

        let limiter = create_connection_rate_limiter(&limits);
        assert!(
            limiter.is_some(),
            "Should clamp low values and create limiter"
        );
    }

    #[test]
    fn test_rate_limiter_clamping_maximum() {
        // Test values at maximum range
        let limits = Limits {
            accept_conn_limit: Some(u32::MAX as f64 + 1000.0),
            accept_conn_burst: Some((u32::MAX as usize) + 1000),
            client_rx: None,
        };

        let limiter = create_connection_rate_limiter(&limits);
        assert!(
            limiter.is_some(),
            "Should clamp high values and create limiter"
        );
    }

    #[test]
    fn test_rate_limiter_ceil_rounding() {
        // Test that fractional values are rounded up with ceil()
        let limits = Limits {
            accept_conn_limit: Some(10.3),
            accept_conn_burst: Some(5),
            client_rx: None,
        };

        let limiter = create_connection_rate_limiter(&limits);
        assert!(limiter.is_some(), "Should handle fractional rate values");
    }

    #[test]
    fn test_default_config_creates_limiter() {
        let config = Config::default();
        let limiter = create_connection_rate_limiter(&config.limits);
        assert!(
            limiter.is_some(),
            "Default config should create a valid rate limiter"
        );
    }
}
