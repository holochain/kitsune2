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

    let has_auth = client_auth_header.is_some();
    debug!(?has_auth, "Relay WebSocket upgrade request");

    ws.on_upgrade(move |socket| async move {
        if let Err(e) =
            handle_relay_websocket(socket, state, client_auth_header).await
        {
            let msg = e.to_string();
            let is_expected = msg.contains("timed out")
                || msg.contains("Connection reset")
                || msg.contains("connection closed")
                || msg.contains("Broken pipe")
                || msg.contains("not allowed")
                || msg.contains("denied")
                || msg.contains("WebSocket protocol error");
            if is_expected {
                debug!("Relay WebSocket ended: {msg}");
            } else {
                warn!("Error handling relay WebSocket: {msg}");
            }
        }
    })
}

/// Adapter that wraps axum's WebSocket to implement the Stream/Sink traits needed by the relay
struct AxumWebSocketAdapter {
    inner: Pin<Box<WebSocket>>,
}

impl AxumWebSocketAdapter {
    fn new(socket: WebSocket) -> Self {
        Self {
            inner: Box::pin(socket),
        }
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
            match self.inner.as_mut().poll_next(cx) {
                Poll::Ready(Some(Ok(msg))) => {
                    match msg {
                        AxumMessage::Binary(data) => {
                            return Poll::Ready(Some(Ok(data)));
                        }
                        AxumMessage::Close(_) => return Poll::Ready(None),
                        _ => {
                            // Skip non-binary messages and continue polling
                            continue;
                        }
                    }
                }
                Poll::Ready(Some(Err(e))) => {
                    // Convert axum error to StreamError
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
    let mut adapter = AxumWebSocketAdapter::new(socket);

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
pub fn create_relay_state() -> RelayState {
    let key_cache = KeyCache::new(1024);
    let access = Arc::new(AccessConfig::Everyone);
    let metrics = Arc::new(Metrics::default());
    RelayState::new(key_cache, access, metrics)
}

/// Creates a RelayState that restricts connections to endpoints registered
/// in the provided allowlist.
///
/// Use this when the bootstrap server is configured with an authentication
/// hook server. Clients must call `PUT /authenticate` and then
/// `PUT /relay/register` before their relay connection will be accepted.
pub fn create_relay_state_with_allowlist(
    allowlist: crate::RelayAllowlist,
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
    RelayState::new(key_cache, access, metrics)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::Router;
    use axum::routing::get;
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

    /// Integration test: Start an axum server with the relay handler and connect clients
    #[tokio::test]
    #[instrument]
    async fn axum_relay_integration() -> Result<(), Box<dyn std::error::Error>>
    {
        let mut rng = rand_chacha::ChaCha8Rng::seed_from_u64(42u64);

        // Create relay state
        let state = create_relay_state();

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
}
