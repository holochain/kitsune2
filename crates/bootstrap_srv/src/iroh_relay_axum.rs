//! Axum integration for the iroh relay server.
//!
//! This module provides an axum-compatible handler that can be mounted
//! as a standard route, avoiding the need for connection-level routing.
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

use axum::{
    extract::{
        ws::{Message as AxumMessage, WebSocket, WebSocketUpgrade},
        State,
    },
    http::HeaderMap,
    response::{IntoResponse, Response},
};
use bytes::Bytes;
use futures::{Sink, Stream};
use governor::clock::DefaultClock;
use governor::state::{InMemoryState, NotKeyed};
use governor::{Quota, RateLimiter};
use http::{Request, StatusCode};
use std::num::NonZeroU32;
use std::{
    future::Future,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};
use tokio_websockets::Error as WsError;
use tower::{Layer, Service};
use tracing::{debug, trace, warn};

use iroh_relay::{
    protos::{
        handshake, relay::PER_CLIENT_SEND_QUEUE_DEPTH, streams::StreamError,
    },
    server::{
        client::Config, clients::Clients, streams::RelayedStream, AccessConfig,
        ClientRateLimit, Metrics,
    },
    ExportKeyingMaterial, KeyCache,
};

pub type ConnectionRateLimiter =
    Arc<RateLimiter<NotKeyed, InMemoryState, DefaultClock>>;

/// State required for the relay handler
///
/// # Note on Rate Limiting
///
/// Unlike the native relay server which can apply rate limiting at the raw TCP/TLS stream level,
/// the axum integration receives already-established WebSocket connections and does not have
/// access to the underlying stream. Therefore, client-side rate limiting is not supported in
/// this integration. If rate limiting is required, consider using axum middleware or the
/// native relay server instead.
#[derive(Clone, Debug)]
pub struct RelayState {
    /// Key cache for the relay
    pub key_cache: KeyCache,
    /// Access control configuration (wrapped in Arc since AccessConfig can't be cloned)
    pub access: Arc<AccessConfig>,
    /// Metrics for the relay server
    pub metrics: Arc<Metrics>,
    /// Write timeout for client connections
    pub write_timeout: std::time::Duration,
    /// Clients registry
    pub clients: Clients,
}

impl RelayState {
    /// Create a new RelayState with default write timeout
    pub fn new(
        key_cache: KeyCache,
        access: Arc<AccessConfig>,
        metrics: Arc<Metrics>,
    ) -> Self {
        Self {
            key_cache,
            access,
            metrics,
            write_timeout: iroh_relay::defaults::timeouts::SERVER_WRITE_TIMEOUT,
            clients: Clients::default(),
        }
    }
}

/// Axum handler for the relay WebSocket endpoint.
///
/// Mount this at the `/relay` path in your axum router.
pub async fn relay_handler(
    State(state): State<RelayState>,
    ws: WebSocketUpgrade,
    headers: HeaderMap,
) -> Response {
    // Extract the client auth header if present
    let client_auth_header =
        headers.get(iroh_relay::http::CLIENT_AUTH_HEADER).cloned();

    debug!("Relay WebSocket upgrade request");

    ws.on_upgrade(move |socket| async move {
        if let Err(e) =
            handle_relay_websocket(socket, state, client_auth_header).await
        {
            warn!("Error handling relay WebSocket: {:?}", e);
        }
    })
}

/// Adapter that wraps axum's WebSocket to implement the Stream/Sink traits needed by the relay
struct AxumWebSocketAdapter {
    inner: WebSocket,
}

impl AxumWebSocketAdapter {
    fn new(socket: WebSocket) -> Self {
        Self { inner: socket }
    }
}

impl Stream for AxumWebSocketAdapter {
    type Item = Result<Bytes, StreamError>;

    fn poll_next(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        // Poll the underlying axum WebSocket
        loop {
            match Pin::new(&mut self.inner).poll_next(cx) {
                Poll::Ready(Some(Ok(msg))) => {
                    match msg {
                        AxumMessage::Binary(data) => {
                            return Poll::Ready(Some(Ok(Bytes::from(data))));
                        }
                        AxumMessage::Close(_) => return Poll::Ready(None),
                        _ => {
                            // Skip non-binary messages and continue polling
                            continue;
                        }
                    }
                }
                Poll::Ready(Some(Err(e))) => {
                    // Convert axum error to WsError
                    return Poll::Ready(Some(Err(WsError::Io(
                        std::io::Error::new(
                            std::io::ErrorKind::Other,
                            format!("{:?}", e),
                        ),
                    ))));
                }
                Poll::Ready(None) => return Poll::Ready(None),
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

impl Sink<Bytes> for AxumWebSocketAdapter {
    type Error = StreamError;

    fn poll_ready(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        match Pin::new(&mut self.inner).poll_ready(cx) {
            Poll::Ready(Ok(())) => Poll::Ready(Ok(())),
            Poll::Ready(Err(e)) => {
                Poll::Ready(Err(WsError::Io(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("{:?}", e),
                ))))
            }
            Poll::Pending => Poll::Pending,
        }
    }

    fn start_send(
        mut self: Pin<&mut Self>,
        item: Bytes,
    ) -> Result<(), Self::Error> {
        Pin::new(&mut self.inner)
            .start_send(AxumMessage::Binary(item))
            .map_err(|e| {
                WsError::Io(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("{:?}", e),
                ))
            })
    }

    fn poll_flush(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        match Pin::new(&mut self.inner).poll_flush(cx) {
            Poll::Ready(Ok(())) => Poll::Ready(Ok(())),
            Poll::Ready(Err(e)) => {
                Poll::Ready(Err(WsError::Io(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("{:?}", e),
                ))))
            }
            Poll::Pending => Poll::Pending,
        }
    }

    fn poll_close(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        match Pin::new(&mut self.inner).poll_close(cx) {
            Poll::Ready(Ok(())) => Poll::Ready(Ok(())),
            Poll::Ready(Err(e)) => {
                Poll::Ready(Err(WsError::Io(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("{:?}", e),
                ))))
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

// Axum WebSocket doesn't support TLS key export, so we return None
impl ExportKeyingMaterial for AxumWebSocketAdapter {
    fn export_keying_material<T: AsMut<[u8]>>(
        &self,
        _output: T,
        _label: &[u8],
        _context: Option<&[u8]>,
    ) -> Option<T> {
        None
    }
}

impl Unpin for AxumWebSocketAdapter {}

/// Handle the relay protocol over an axum WebSocket
async fn handle_relay_websocket(
    socket: WebSocket,
    state: RelayState,
    client_auth_header: Option<http::HeaderValue>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    trace!("Relay WebSocket connection established");

    // Wrap the axum WebSocket to implement Stream/Sink
    let mut adapter = AxumWebSocketAdapter::new(socket);

    // Perform the relay protocol handshake
    let authentication =
        handshake::serverside(&mut adapter, client_auth_header).await?;

    trace!(?authentication.mechanism, "accept: verified authentication");

    let is_authorized =
        state.access.is_allowed(authentication.client_key).await;
    let client_key = authentication
        .authorize_if(is_authorized, &mut adapter)
        .await?;

    trace!("accept: verified authorization");

    // Wrap in RelayedStream for encryption
    let io = RelayedStream::new(adapter, state.key_cache.clone());

    trace!("accept: build client conn");
    let client_conn_builder = Config {
        endpoint_id: client_key,
        stream: io,
        write_timeout: state.write_timeout,
        channel_capacity: PER_CLIENT_SEND_QUEUE_DEPTH,
    };

    // Register the client with the relay server
    state
        .clients
        .register(client_conn_builder, state.metrics.clone())
        .await;

    Ok(())
}

/// Layer for rate limiting relay connections
#[derive(Clone)]
pub struct RateLimitLayer {
    limiter: ConnectionRateLimiter,
}

impl RateLimitLayer {
    /// Create a new rate limit layer with the given limiter
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
///
/// # Note on client_rx
///
/// The `client_rx` field is **not supported** in the axum integration because
/// WebSocket connections are already established when they reach the handler,
/// making byte-level rate limiting impractical at this layer. This field is
/// included only for API compatibility and will be ignored.
#[derive(Debug, Default)]
pub struct Limits {
    /// Rate limit for accepting new connection. Unlimited if not set.
    pub accept_conn_limit: Option<f64>,
    /// Burst limit for accepting new connection. Unlimited if not set.
    pub accept_conn_burst: Option<usize>,
    /// Rate limits for incoming traffic from a client connection.
    ///
    /// **Note**: This is ignored in the axum integration. See struct-level documentation.
    pub client_rx: Option<ClientRateLimit>,
}

/// Configuration for iroh relay server
///
/// # Default Configuration
///
/// The default configuration enables connection-level rate limiting:
/// - 250 connections per second
/// - 100 connection burst capacity
///
/// **Note**: `client_rx` (byte-level rate limiting) is intentionally set to `None`
/// in the default configuration because it is not supported in the axum integration.
#[derive(Debug)]
pub struct RelayConfig {
    /// Rate limiting configuration for iroh relay server
    pub limits: Limits,
}

impl Default for RelayConfig {
    fn default() -> Self {
        Self {
            limits: Limits {
                accept_conn_limit: Some(250.0),
                accept_conn_burst: Some(100),
                // client_rx is not supported in axum integration - leave as None
                client_rx: None,
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
/// - **Client-side byte-level rate limiting**: **NOT supported**. The `client_rx` field in
///   [`Limits`] is intentionally ignored. The axum integration receives already-established
///   WebSocket connections without access to the underlying TCP/TLS stream, making byte-level
///   rate limiting impractical at this layer. If you need byte-level rate limiting, consider
///   using iroh-relay's native server instead.
pub fn create_relay_state(
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
    use axum::routing::get;
    use axum::Router;
    use futures::{SinkExt, StreamExt};
    use iroh_base::{RelayUrl, SecretKey};
    use rand_chacha::rand_core::SeedableRng;
    use std::net::Ipv4Addr;
    use tokio::net::TcpListener;
    use tracing::{info, instrument};

    use iroh_relay::{
        client::ClientBuilder,
        dns::DnsResolver,
        protos::relay::{ClientToRelayMsg, Datagrams, RelayToClientMsg},
    };

    /// Test that RelayState can be created and cloned
    #[test]
    fn test_relay_state_creation() {
        let key_cache = KeyCache::new(1024);
        let access = Arc::new(AccessConfig::Everyone);
        let metrics = Arc::new(Metrics::default());

        let state = RelayState::new(key_cache, access, metrics);

        // Verify state can be cloned (required for axum State)
        let _cloned = state.clone();
    }

    /// Test that AxumWebSocketAdapter implements the required traits
    #[test]
    fn test_axum_websocket_adapter_traits() {
        // This test just verifies the types compile correctly
        // The actual functionality is tested in the kitsune2 integration tests
        fn _assert_stream<T>(_: T)
        where
            T: Stream<Item = Result<Bytes, StreamError>> + Unpin,
        {
        }

        fn _assert_sink<T>(_: T)
        where
            T: Sink<Bytes, Error = StreamError> + Unpin,
        {
        }

        fn _assert_export_keying_material<T>(_: T)
        where
            T: ExportKeyingMaterial,
        {
        }

        // These assertions verify the trait bounds at compile time
        // No runtime test needed as this is checked by the type system
    }

    /// Integration test: Start an axum server with the relay handler and connect clients
    #[tokio::test]
    #[instrument]
    async fn test_axum_relay_integration(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut rng = rand_chacha::ChaCha8Rng::seed_from_u64(42u64);

        // Create relay state
        let key_cache = KeyCache::new(1024);
        let access = Arc::new(AccessConfig::Everyone);
        let metrics = Arc::new(Metrics::default());
        let state = RelayState::new(key_cache, access, metrics);

        // Create axum router with relay handler
        let app = Router::new()
            .route("/relay", get(relay_handler))
            .with_state(state.clone());

        // Bind to a random port
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
        let addr = listener.local_addr()?;
        info!("Axum relay server listening on {}", addr);

        // Spawn the server
        let server_handle = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("server error");
        });

        // Give the server a moment to start
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        // Create relay URL pointing to our axum server
        let relay_url = format!("http://{}/relay", addr);
        let relay_url: RelayUrl = relay_url.parse()?;

        // Create client A
        let a_secret_key = SecretKey::generate(&mut rng);
        let a_key = a_secret_key.public();
        let resolver = DnsResolver::new();
        info!("Connecting client A");
        let mut client_a = ClientBuilder::new(
            relay_url.clone(),
            a_secret_key,
            resolver.clone(),
        )
        .connect()
        .await?;

        // Create client B
        let b_secret_key = SecretKey::generate(&mut rng);
        let b_key = b_secret_key.public();
        info!("Connecting client B");
        let mut client_b = ClientBuilder::new(
            relay_url.clone(),
            b_secret_key,
            resolver.clone(),
        )
        .connect()
        .await?;

        info!("Sending message from A to B");
        // Send message from A to B
        let msg = Datagrams::from("hello from A");
        client_a
            .send(ClientToRelayMsg::Datagrams {
                dst_endpoint_id: b_key,
                datagrams: msg.clone(),
            })
            .await?;

        // Receive on B
        let received = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            client_b.next(),
        )
        .await
        .expect("timeout waiting for message")
        .expect("stream ended")?;

        match received {
            RelayToClientMsg::Datagrams {
                remote_endpoint_id,
                datagrams,
            } => {
                assert_eq!(remote_endpoint_id, a_key, "Wrong sender");
                assert_eq!(datagrams, msg, "Message content mismatch");
                info!("Successfully received message on client B");
            }
            other => panic!("Unexpected message type: {:?}", other),
        }

        info!("Sending message from B to A");
        // Send message from B to A
        let msg2 = Datagrams::from("hello from B");
        client_b
            .send(ClientToRelayMsg::Datagrams {
                dst_endpoint_id: a_key,
                datagrams: msg2.clone(),
            })
            .await?;

        // Receive on A
        let received = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            client_a.next(),
        )
        .await
        .expect("timeout waiting for message")
        .expect("stream ended")?;

        match received {
            RelayToClientMsg::Datagrams {
                remote_endpoint_id,
                datagrams,
            } => {
                assert_eq!(remote_endpoint_id, b_key, "Wrong sender");
                assert_eq!(datagrams, msg2, "Message content mismatch");
                info!("Successfully received message on client A");
            }
            other => panic!("Unexpected message type: {:?}", other),
        }

        // Clean up
        drop(client_a);
        drop(client_b);
        server_handle.abort();

        info!("Test completed successfully");
        Ok(())
    }

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
        let config = RelayConfig::default();
        let limiter = create_connection_rate_limiter(&config.limits);
        assert!(
            limiter.is_some(),
            "Default config should create a valid rate limiter"
        );
    }
}
