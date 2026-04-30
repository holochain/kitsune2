//! Axum integration for the iroh relay server.
//!
//! This module provides an axum-compatible handler that can be mounted
//! as a standard route in the bootstrap server.

use axum::{
    extract::{
        State,
        ws::{Message as AxumMessage, WebSocket, WebSocketUpgrade},
    },
    http::HeaderMap,
    response::Response,
};
use bytes::Bytes;
use futures::{Sink, Stream};
use std::{
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};
use tokio::time::timeout;
use tracing::{debug, info, trace, warn};

use futures::FutureExt;
use iroh_relay::http::ProtocolVersion;
use iroh_relay::{
    ExportKeyingMaterial, KeyCache,
    protos::{
        handshake, relay::PER_CLIENT_SEND_QUEUE_DEPTH, streams::StreamError,
    },
    server::{
        Access, AccessConfig, Metrics, client::Config, clients::Clients,
        streams::RelayedStream,
    },
};
use std::future::Future;

/// Per-connection inbound byte rate limit for the relay WebSocket handler.
///
/// The limiter applies to every binary frame received from a single
/// WebSocket connection. Both fields are `NonZeroU32`, so neither value can
/// degenerate to "unlimited via zero"; an unset limiter is represented by
/// `None` on [`RelayState::rate_limit`].
#[derive(Clone, Copy, Debug)]
pub struct RelayClientRxRateLimit {
    /// Sustained refill rate in bytes per second.
    pub bytes_per_second: std::num::NonZeroU32,
    /// Maximum bucket capacity (i.e. allowed burst above the sustained rate).
    pub burst_bytes: std::num::NonZeroU32,
}

/// State required for the relay handler.
#[derive(Clone, Debug)]
pub struct RelayState {
    /// Key cache for the relay
    pub key_cache: KeyCache,
    /// Access control configuration
    pub access: Arc<AccessConfig>,
    /// Metrics for the relay server
    pub metrics: Arc<Metrics>,
    /// Write timeout for client connections
    pub write_timeout: std::time::Duration,
    /// Clients registry
    pub clients: Clients,
    /// Optional per-connection inbound byte rate limit.
    pub rate_limit: Option<RelayClientRxRateLimit>,
}

impl RelayState {
    /// Create a new RelayState with default write timeout
    pub fn new(
        key_cache: KeyCache,
        access: Arc<AccessConfig>,
        metrics: Arc<Metrics>,
        rate_limit: Option<RelayClientRxRateLimit>,
    ) -> Self {
        Self {
            key_cache,
            access,
            metrics,
            write_timeout: iroh_relay::defaults::timeouts::SERVER_WRITE_TIMEOUT,
            clients: Clients::default(),
            rate_limit,
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

    let has_auth = client_auth_header.is_some();
    debug!(?has_auth, "Relay WebSocket upgrade request");

    // iroh 0.98+ clients advertise a comma-separated list of supported relay
    // protocol versions via `Sec-Websocket-Protocol` and fail the connection
    // unless the server echoes back one it recognises. Our handshake/framing
    // here is still V1-only (see `protocol_version` below), so restrict the
    // negotiation to V1.
    let ws = ws.protocols([ProtocolVersion::V1.to_str()]);

    ws.on_upgrade(move |socket| async move {
        if let Err(e) =
            handle_relay_websocket(socket, state, client_auth_header).await
        {
            let msg = e.to_string().to_lowercase();
            // String-matching error classifier. The substrings come from a
            // few different sources; keep this list in sync if the upstream
            // crates' error messages change in a future version:
            //
            // - "timed out": the `io::Error` we produce ourselves at the
            //   handshake timeout below in `handle_relay_websocket`.
            // - "connection reset", "broken pipe": `std::io::ErrorKind`
            //   Display impls, surfaced via `tungstenite::Error::Io`.
            // - "connection closed", "websocket protocol error":
            //   `tokio_tungstenite::tungstenite::Error` Display.
            // - "not allowed", "denied": rejection messages from the
            //   `iroh_relay` handshake auth path
            //   (`handshake::serverside`/`authorize_if`).
            let is_expected = msg.contains("timed out")
                || msg.contains("connection reset")
                || msg.contains("connection closed")
                || msg.contains("broken pipe")
                || msg.contains("not allowed")
                || msg.contains("denied")
                || msg.contains("websocket protocol error");
            if is_expected {
                debug!("Relay WebSocket ended: {msg}");
            } else {
                warn!("Error handling relay WebSocket: {msg}");
            }
        }
    })
}

/// Per-connection rate-limit state held inside [`AxumWebSocketAdapter`].
///
/// Pairs an [`iroh_relay::Bucket`] with an optional pending sleep that gates
/// the next read when the bucket has been overrun. The bucket is charged
/// optimistically: every inbound binary frame's size is subtracted from the
/// bucket *after* the frame is read but *before* the next frame is polled,
/// so an overrun delays the *next* read rather than the current one. No
/// frames are dropped — every frame is delivered, only paced.
///
/// Lifetime is tied to a single WebSocket connection. iroh's
/// `Clients::register` is keyed on `EndpointId` and replaces existing
/// entries, so per-connection is equivalent to per-endpoint by
/// construction.
struct RateLimitState {
    /// Token bucket reused from `iroh_relay`. Counts inbound bytes.
    bucket: iroh_relay::Bucket,
    /// `Some` when the previous charge overran the bucket. Resolves at the
    /// deadline reported by [`iroh_relay::Bucket::consume`], at which point
    /// the next read is allowed.
    sleep: Option<Pin<Box<tokio::time::Sleep>>>,
}

impl RateLimitState {
    fn new(limit: RelayClientRxRateLimit) -> Self {
        let bucket = iroh_relay::Bucket::new(
            limit.burst_bytes.get() as i64,
            limit.bytes_per_second.get() as i64,
            std::time::Duration::from_millis(100),
        )
        .expect(
            "RelayClientRxRateLimit fields are NonZeroU32 so Bucket::new \
             cannot fail with InvalidBucketConfig",
        );
        Self {
            bucket,
            sleep: None,
        }
    }

    /// Returns `Poll::Pending` if the bucket is currently throttled,
    /// `Poll::Ready(())` otherwise. Always wakes when the throttle clears.
    fn poll_throttle(&mut self, cx: &mut Context<'_>) -> Poll<()> {
        if let Some(sleep) = self.sleep.as_mut() {
            match sleep.as_mut().poll(cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(()) => self.sleep = None,
            }
        }
        Poll::Ready(())
    }

    /// Charge `bytes` to the bucket. If the charge overruns, schedules a
    /// sleep until the bucket refills enough.
    fn charge(&mut self, bytes: usize) {
        if let Err(deadline) = self.bucket.consume(bytes) {
            self.sleep = Some(Box::pin(tokio::time::sleep_until(deadline)));
        }
    }
}

/// Adapter that wraps axum's WebSocket to implement the Stream/Sink traits needed by the relay
struct AxumWebSocketAdapter {
    inner: Pin<Box<WebSocket>>,
    rate_limit: Option<RateLimitState>,
}

impl AxumWebSocketAdapter {
    fn new(
        socket: WebSocket,
        rate_limit: Option<RelayClientRxRateLimit>,
    ) -> Self {
        Self {
            inner: Box::pin(socket),
            rate_limit: rate_limit.map(RateLimitState::new),
        }
    }
}

impl Stream for AxumWebSocketAdapter {
    type Item = Result<Bytes, StreamError>;

    fn poll_next(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        // If a previous frame overran the bucket, defer the next read
        // until the bucket has refilled enough.
        if let Some(rl) = self.rate_limit.as_mut()
            && rl.poll_throttle(cx).is_pending()
        {
            return Poll::Pending;
        }

        loop {
            match self.inner.as_mut().poll_next(cx) {
                Poll::Ready(Some(Ok(msg))) => match msg {
                    AxumMessage::Binary(data) => {
                        if let Some(rl) = self.rate_limit.as_mut() {
                            rl.charge(data.len());
                        }
                        return Poll::Ready(Some(Ok(data)));
                    }
                    AxumMessage::Close(_) => return Poll::Ready(None),
                    _ => continue,
                },
                Poll::Ready(Some(Err(e))) => {
                    return Poll::Ready(Some(Err(StreamError::Io(
                        std::io::Error::other(e.to_string()),
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
        match self.inner.as_mut().poll_ready(cx) {
            Poll::Ready(Ok(())) => Poll::Ready(Ok(())),
            Poll::Ready(Err(e)) => Poll::Ready(Err(StreamError::Io(
                std::io::Error::other(e.to_string()),
            ))),
            Poll::Pending => Poll::Pending,
        }
    }

    fn start_send(
        mut self: Pin<&mut Self>,
        item: Bytes,
    ) -> Result<(), Self::Error> {
        self.inner
            .as_mut()
            .start_send(AxumMessage::Binary(item))
            .map_err(|e| StreamError::Io(std::io::Error::other(e.to_string())))
    }

    fn poll_flush(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        match self.inner.as_mut().poll_flush(cx) {
            Poll::Ready(Ok(())) => Poll::Ready(Ok(())),
            Poll::Ready(Err(e)) => Poll::Ready(Err(StreamError::Io(
                std::io::Error::other(e.to_string()),
            ))),
            Poll::Pending => Poll::Pending,
        }
    }

    fn poll_close(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        match self.inner.as_mut().poll_close(cx) {
            Poll::Ready(Ok(())) => Poll::Ready(Ok(())),
            Poll::Ready(Err(e)) => Poll::Ready(Err(StreamError::Io(
                std::io::Error::other(e.to_string()),
            ))),
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
    let mut adapter = AxumWebSocketAdapter::new(socket, state.rate_limit);

    // Perform the relay protocol handshake with a timeout to prevent
    // stalled clients from holding the WebSocket open indefinitely.
    const HANDSHAKE_TIMEOUT: std::time::Duration =
        std::time::Duration::from_secs(30);

    let client_key = timeout(HANDSHAKE_TIMEOUT, async {
        let authentication =
            handshake::serverside(&mut adapter, client_auth_header).await?;

        debug!(?authentication.mechanism, "accept: verified authentication");

        let is_authorized =
            state.access.is_allowed(authentication.client_key).await;
        let client_key = authentication
            .authorize_if(is_authorized, &mut adapter)
            .await?;

        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(client_key)
    })
    .await
    .map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            "Relay handshake timed out",
        )
    })??;

    debug!(key = %client_key.fmt_short(), "accept: verified authorization");

    // Wrap in RelayedStream for encryption
    let io = RelayedStream::new(adapter, state.key_cache.clone());

    trace!("accept: build client conn");
    let client_conn_builder = Config {
        endpoint_id: client_key,
        stream: io,
        write_timeout: state.write_timeout,
        channel_capacity: PER_CLIENT_SEND_QUEUE_DEPTH,
        // TODO Update in sync with the client and deploy at V2
        protocol_version: ProtocolVersion::V1,
    };

    // Register the client with the relay server
    state
        .clients
        .register(client_conn_builder, state.metrics.clone());

    info!(key = %client_key.fmt_short(), "relay client registered");

    Ok(())
}

/// Handler for the relay probe endpoint (`/ping`).
///
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

/// Handler for the captive portal detection endpoint (`/generate_204`).
///
/// iroh clients probe this endpoint over plain HTTP to detect captive portals.
/// The handler returns 204 No Content and echoes back any challenge header
/// so the client can verify it's talking to the real relay, not a captive portal.
pub async fn captive_portal_handler(
    headers: HeaderMap,
) -> axum::response::Response {
    let mut response = axum::response::Response::builder()
        .status(axum::http::StatusCode::NO_CONTENT);

    if let Some(challenge) = headers.get("X-Iroh-Challenge")
        && let Ok(challenge_str) = challenge.to_str()
        && !challenge_str.is_empty()
        && challenge_str.len() < 64
        && challenge_str
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_')
    {
        response = response
            .header("X-Iroh-Response", format!("response {challenge_str}"));
    }

    response
        .body(axum::body::Body::empty())
        .unwrap_or_else(|_| {
            axum::response::Response::builder()
                .status(axum::http::StatusCode::NO_CONTENT)
                .body(axum::body::Body::empty())
                .unwrap()
        })
}

/// Creates a RelayState instance for the axum relay handler.
///
/// This creates the state needed for the relay handler which can be mounted
/// as a standard axum route. All connections are permitted.
pub fn create_relay_state(
    rate_limit: Option<RelayClientRxRateLimit>,
) -> RelayState {
    let key_cache = KeyCache::new(1024);
    let access = Arc::new(AccessConfig::Everyone);
    let metrics = Arc::new(Metrics::default());
    RelayState::new(key_cache, access, metrics, rate_limit)
}

/// Creates a RelayState that restricts connections to endpoints registered
/// in the provided allowlist.
///
/// Use this when the bootstrap server is configured with an authentication
/// hook server. Clients must call `PUT /authenticate` and then
/// `PUT /relay/register` before their relay connection will be accepted.
pub fn create_relay_state_with_allowlist(
    allowlist: crate::RelayAllowlist,
    rate_limit: Option<RelayClientRxRateLimit>,
) -> RelayState {
    let key_cache = KeyCache::new(1024);
    let access =
        Arc::new(AccessConfig::Restricted(Box::new(move |endpoint_id| {
            let allowlist = allowlist.clone();
            async move {
                let allowed = allowlist.is_allowed(&endpoint_id);
                tracing::debug!(
                    key = %endpoint_id.fmt_short(),
                    allowed,
                    allowlist_size = allowlist.len(),
                    "Relay access check"
                );
                if allowed { Access::Allow } else { Access::Deny }
            }
            .boxed()
        })));
    let metrics = Arc::new(Metrics::default());
    RelayState::new(key_cache, access, metrics, rate_limit)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::Router;
    use axum::routing::get;
    use futures::{SinkExt, StreamExt};
    use iroh_base::{RelayUrl, SecretKey};
    use std::net::Ipv4Addr;
    use tokio::net::TcpListener;
    use tracing::{info, instrument};

    use iroh_relay::{
        client::ClientBuilder,
        dns::DnsResolver,
        protos::relay::{ClientToRelayMsg, Datagrams, RelayToClientMsg},
        tls::CaRootsConfig,
    };

    /// Integration test: Start an axum server with the relay handler and connect clients
    #[tokio::test]
    #[instrument]
    async fn axum_relay_integration() -> Result<(), Box<dyn std::error::Error>>
    {
        // Create relay state
        let state = create_relay_state(None);

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

        // Create relay URL pointing to our axum server
        let relay_url = format!("http://{}/relay", addr);
        let relay_url: RelayUrl = relay_url.parse()?;

        let tls_client_config = CaRootsConfig::default()
            .client_config(iroh_relay::tls::default_provider())?;

        // Create client A
        let a_secret_key = SecretKey::generate();
        let a_key = a_secret_key.public();
        let resolver = DnsResolver::new();
        info!("Connecting client A");
        let mut client_a = ClientBuilder::new(
            relay_url.clone(),
            a_secret_key,
            resolver.clone(),
        )
        .tls_client_config(tls_client_config.clone())
        .connect()
        .await?;

        // Create client B
        let b_secret_key = SecretKey::generate();
        let b_key = b_secret_key.public();
        info!("Connecting client B");
        let mut client_b = ClientBuilder::new(
            relay_url.clone(),
            b_secret_key,
            resolver.clone(),
        )
        .tls_client_config(tls_client_config)
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

    #[tokio::test]
    #[instrument]
    async fn axum_relay_rate_limited() -> Result<(), Box<dyn std::error::Error>>
    {
        use std::num::NonZeroU32;
        use std::time::Instant as StdInstant;

        // Tight limit: 8 KiB/s sustained, 8 KiB burst. With 1 KiB
        // datagrams that is 8 frames per second after the initial burst.
        let rate_limit = RelayClientRxRateLimit {
            bytes_per_second: NonZeroU32::new(8 * 1024).unwrap(),
            burst_bytes: NonZeroU32::new(8 * 1024).unwrap(),
        };

        let state = create_relay_state(Some(rate_limit));

        let app = Router::new()
            .route("/relay", get(relay_handler))
            .with_state(state.clone());

        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
        let addr = listener.local_addr()?;
        let server_handle = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("server error");
        });

        let relay_url = format!("http://{}/relay", addr);
        let relay_url: RelayUrl = relay_url.parse()?;

        let tls_client_config = CaRootsConfig::default()
            .client_config(iroh_relay::tls::default_provider())?;

        let a_secret_key = SecretKey::generate();
        let resolver = DnsResolver::new();
        let mut client_a = ClientBuilder::new(
            relay_url.clone(),
            a_secret_key,
            resolver.clone(),
        )
        .tls_client_config(tls_client_config.clone())
        .connect()
        .await?;

        let b_secret_key = SecretKey::generate();
        let b_key = b_secret_key.public();
        let mut client_b = ClientBuilder::new(
            relay_url.clone(),
            b_secret_key,
            resolver.clone(),
        )
        .tls_client_config(tls_client_config)
        .connect()
        .await?;

        // Send 24 KiB total in 24 x 1 KiB datagrams. With an 8 KiB bucket
        // and 8 KiB/s refill, total wall time on the recv side should be
        // significantly more than zero (>= ~1.5 s by the time the second
        // and later batches drain the bucket).
        let payload = vec![0u8; 1024];
        let datagrams = Datagrams::from(payload);

        let started = StdInstant::now();
        for _ in 0..24 {
            client_a
                .send(ClientToRelayMsg::Datagrams {
                    dst_endpoint_id: b_key,
                    datagrams: datagrams.clone(),
                })
                .await?;
        }

        let mut received = 0usize;
        while received < 24 {
            let msg = tokio::time::timeout(
                std::time::Duration::from_secs(10),
                client_b.next(),
            )
            .await
            .expect("timeout waiting for message")
            .expect("stream ended")?;
            if matches!(msg, RelayToClientMsg::Datagrams { .. }) {
                received += 1;
            }
        }
        let elapsed = started.elapsed();

        assert!(
            elapsed >= std::time::Duration::from_millis(1500),
            "rate-limit appears not to be applied: elapsed = {elapsed:?}"
        );
        assert!(
            elapsed < std::time::Duration::from_secs(9),
            "rate-limit appears stuck: elapsed = {elapsed:?}"
        );

        drop(client_a);
        drop(client_b);
        server_handle.abort();
        Ok(())
    }

    use std::num::NonZeroU32;

    fn poll_throttle_now(state: &mut RateLimitState) -> Poll<()> {
        let waker = futures::task::noop_waker();
        let mut cx = Context::from_waker(&waker);
        state.poll_throttle(&mut cx)
    }

    #[tokio::test(start_paused = true)]
    async fn rate_limit_passes_under_burst() {
        let limit = RelayClientRxRateLimit {
            bytes_per_second: NonZeroU32::new(1024).unwrap(),
            burst_bytes: NonZeroU32::new(1024).unwrap(),
        };
        let mut state = RateLimitState::new(limit);

        state.charge(512);
        assert!(matches!(poll_throttle_now(&mut state), Poll::Ready(())));
        assert!(state.sleep.is_none());
    }

    #[tokio::test(start_paused = true)]
    async fn rate_limit_paces_overrun() {
        let limit = RelayClientRxRateLimit {
            bytes_per_second: NonZeroU32::new(1024).unwrap(),
            burst_bytes: NonZeroU32::new(1024).unwrap(),
        };
        let mut state = RateLimitState::new(limit);

        // Charge 512 bytes keeps fill > 0, so no throttle.
        state.charge(512);
        assert!(matches!(poll_throttle_now(&mut state), Poll::Ready(())));
        assert!(state.sleep.is_none());

        // Charge another 1024 bytes drains the bucket past zero. iroh's
        // Bucket triggers throttle when fill is <= 0, so a sleep is
        // scheduled.
        state.charge(1024);
        assert!(state.sleep.is_some());
        assert!(matches!(poll_throttle_now(&mut state), Poll::Pending));

        // Drive the throttle to completion. With `start_paused = true`,
        // tokio auto-advances virtual time when the only work pending is
        // a sleep, so this completes deterministically once the bucket's
        // refill deadline passes.
        let started = tokio::time::Instant::now();
        std::future::poll_fn(|cx| state.poll_throttle(cx)).await;
        let elapsed = started.elapsed();

        assert!(state.sleep.is_none());
        // Some real virtual time must have passed.
        assert!(
            elapsed >= std::time::Duration::from_millis(100),
            "throttle cleared too soon: elapsed = {elapsed:?}"
        );
    }

    #[tokio::test]
    async fn rate_limit_state_construction_safe_for_low_rates() {
        // bytes_per_second = 100 over a 100ms refill window resolves to
        // a per-period refill of 10 bytes (>0), so Bucket::new accepts
        // it. Validates that small-but-nonzero rates do not panic.
        let limit = RelayClientRxRateLimit {
            bytes_per_second: NonZeroU32::new(100).unwrap(),
            burst_bytes: NonZeroU32::new(10).unwrap(),
        };
        let _ = RateLimitState::new(limit);
    }
}
