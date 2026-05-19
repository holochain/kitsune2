//! Axum integration for the iroh relay server.
//!
//! This module provides an axum-compatible handler that can be mounted
//! as a standard route in the bootstrap server.

use axum::{
    extract::{
        FromRequestParts, State,
        ws::{Message as AxumMessage, WebSocket, WebSocketUpgrade},
    },
    http::HeaderMap,
    response::{IntoResponse, Response},
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
    protos::{handshake, streams::StreamError},
    server::{
        Access, AccessConfig, ClientRequest, Metrics, client::Config,
        clients::Clients, streams::RelayedStream,
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
    request: axum::extract::Request,
) -> Response {
    let (mut parts, _body) = request.into_parts();

    // Extract the client auth header if present
    let client_auth_header = parts
        .headers
        .get(iroh_relay::http::CLIENT_AUTH_HEADER)
        .cloned();

    let has_auth = client_auth_header.is_some();
    debug!(?has_auth, "Relay WebSocket upgrade request");

    // Run the WebSocketUpgrade extractor on the request parts so the
    // upgrade handle is captured. WebSocketUpgrade is a FromRequestParts
    // extractor; it borrows from `parts` and leaves it usable afterwards.
    let ws =
        match WebSocketUpgrade::from_request_parts(&mut parts, &state).await {
            Ok(ws) => ws,
            Err(rejection) => return rejection.into_response(),
        };

    // Snapshot the request bits an `AccessConfig::Restricted` hook might
    // need (URI, method, headers, version). `http::request::Parts` is not
    // `Clone` because it carries an `Extensions` bag, so we rebuild a
    // fresh `Parts` with only the values `ClientRequest` exposes.
    let client_request_parts = snapshot_request_parts(&parts);

    // iroh 0.98+ clients advertise a comma-separated list of supported relay
    // protocol versions via `Sec-Websocket-Protocol` and fail the connection
    // unless the server echoes back one it recognises. Our handshake/framing
    // here is still V1-only (see `protocol_version` below), so restrict the
    // negotiation to V1.
    let ws = ws.protocols([ProtocolVersion::V1.to_str()]);

    ws.on_upgrade(move |socket| async move {
        if let Err(e) = handle_relay_websocket(
            socket,
            state,
            client_auth_header,
            client_request_parts,
        )
        .await
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
                    return Poll::Ready(Some(Err(StreamError::from_std(e))));
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
            Poll::Ready(Err(e)) => Poll::Ready(Err(StreamError::from_std(e))),
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
            .map_err(StreamError::from_std)
    }

    fn poll_flush(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        match self.inner.as_mut().poll_flush(cx) {
            Poll::Ready(Ok(())) => Poll::Ready(Ok(())),
            Poll::Ready(Err(e)) => Poll::Ready(Err(StreamError::from_std(e))),
            Poll::Pending => Poll::Pending,
        }
    }

    fn poll_close(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        match self.inner.as_mut().poll_close(cx) {
            Poll::Ready(Ok(())) => Poll::Ready(Ok(())),
            Poll::Ready(Err(e)) => Poll::Ready(Err(StreamError::from_std(e))),
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
    request_parts: http::request::Parts,
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

        let client_request =
            ClientRequest::new(authentication.client_key, request_parts);
        let is_authorized = state.access.is_allowed(&client_request).await;
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
    // TODO Update in sync with the client and deploy at V2
    let mut client_conn_builder =
        Config::new(client_key, io, ProtocolVersion::V1);
    client_conn_builder.write_timeout = state.write_timeout;

    // Register the client with the relay server
    state
        .clients
        .register(client_conn_builder, state.metrics.clone());

    info!(key = %client_key.fmt_short(), "relay client registered");

    Ok(())
}

/// Rebuild a fresh `http::request::Parts` with the fields a
/// `ClientRequest` exposes (URI, method, headers, version).
///
/// `Parts` is not `Clone` because of its `Extensions` bag, so we
/// construct a new one and copy over what's relevant. The
/// extensions bag is intentionally left empty: nothing in
/// `AccessConfig` reads from it.
fn snapshot_request_parts(
    parts: &http::request::Parts,
) -> http::request::Parts {
    let mut new_parts = http::Request::builder()
        .method(parts.method.clone())
        .uri(parts.uri.clone())
        .body(())
        .expect("building an empty http::Request must succeed")
        .into_parts()
        .0;
    new_parts.headers = parts.headers.clone();
    new_parts.version = parts.version;
    new_parts
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
    let access = Arc::new(AccessConfig::Restricted(Box::new(
        move |request: &ClientRequest| {
            let allowlist = allowlist.clone();
            let endpoint_id = request.endpoint_id();
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
        },
    )));
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
    use std::net::Ipv4Addr;
    use tokio::net::TcpListener;
    use tracing::{info, instrument};

    use iroh_dns::dns::DnsResolver;
    use iroh_relay::{
        client::ClientBuilder,
        protos::relay::{ClientToRelayMsg, Datagrams, RelayToClientMsg},
        tls::CaRootsConfig,
    };

    /// Integration test: Start an axum server with the relay handler and connect clients
    #[tokio::test]
    #[instrument]
    async fn axum_relay_integration() -> Result<(), Box<dyn std::error::Error>>
    {
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
}
