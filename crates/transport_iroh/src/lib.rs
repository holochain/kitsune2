#![deny(missing_docs)]
//! Kitsune2 transport implementation backed by iroh.
//!
//! This transport establishes peer-to-peer connections using iroh's QUIC-based networking.
//! It manages outgoing and incoming connections dynamically, sending data as framed messages
//! over unidirectional streams.
//!
//! Each message is framed with a header that specifies the frame type (payload or peer URL) and
//! the payload length, leading to ordered and bounded message delivery. The peer URL is sent
//! to inform the remote about it and make it available to respond to on the transport level.
//! Incoming streams are accepted and handled asynchronously per connection, supporting concurrent
//! processing of multiple messages.

use bytes::Bytes;
use iroh::{
    endpoint::{ConnectionType, RecvStream},
    Endpoint, EndpointAddr, RelayMap, RelayMode, RelayUrl, Watcher,
};
use kitsune2_api::*;
use std::{
    collections::HashMap,
    str::FromStr,
    sync::{Arc, Mutex, RwLock},
    time::{Duration, SystemTime},
};
use tokio::task::AbortHandle;
use tracing::{error, info, warn};

mod frame;
use frame::*;
mod url;
use url::*;
mod connection_context;
use connection_context::*;

#[cfg(any(test, feature = "test-utils"))]
pub mod test_utils;

#[cfg(test)]
mod tests;

const ALPN: &[u8] = b"kitsune2/0";
// TODO: make configurable
const MAX_FRAME_BYTES: usize = 1024 * 1024;

/// IrohTransport configuration types
pub mod config {
    /// Configuration for the [`IrohTransportFactory`](super::IrohTransportFactory).
    #[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
    #[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
    #[serde(rename_all = "camelCase")]
    pub struct IrohTransportConfig {
        /// Explicit relay URL to use as home relay. If none is set,
        /// relays provided by n0 will be used.
        #[cfg_attr(feature = "schema", schemars(default))]
        pub relay_url: Option<String>,
        /// Allow connecting to plaintext (http) relay server
        /// instead of the default requiring TLS (https).
        ///
        /// Default: false.
        #[cfg_attr(feature = "schema", schemars(default))]
        pub relay_allow_plain_text: bool,
    }

    /// Module-level config wrapper.
    #[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
    #[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
    #[serde(rename_all = "camelCase")]
    pub struct IrohTransportModConfig {
        /// The actual config for the transport.
        pub iroh_transport: IrohTransportConfig,
    }
}

pub use config::*;

/// Kitsune2 transport factory backed by iroh.
#[derive(Debug)]
pub struct IrohTransportFactory;

impl IrohTransportFactory {
    /// Create a new factory instance.
    pub fn create() -> DynTransportFactory {
        Arc::new(Self)
    }
}

impl TransportFactory for IrohTransportFactory {
    fn default_config(&self, config: &mut Config) -> K2Result<()> {
        config.set_module_config(&IrohTransportModConfig::default())
    }

    fn validate_config(&self, config: &Config) -> K2Result<()> {
        let config: IrohTransportModConfig = config.get_module_config()?;

        if let Some(relay) = &config.iroh_transport.relay_url {
            let relay_url = ::url::Url::parse(relay)
                .map_err(|err| K2Error::other_src("invalid relay URL", err))?;
            let uses_tls = relay_url.scheme() == "https";
            if !&config.iroh_transport.relay_allow_plain_text && !uses_tls {
                return Err(K2Error::other(format!(
                    "disallowed plaintext relay url, either specify https or set relay_allow_plain_text to true: {relay_url}"
                )));
            }
        }
        Ok(())
    }

    fn create(
        &self,
        builder: Arc<Builder>,
        handler: DynTxHandler,
    ) -> BoxFut<'static, K2Result<DynTransport>> {
        Box::pin(async move {
            let handler = TxImpHnd::new(handler);
            let config: IrohTransportModConfig =
                builder.config.get_module_config()?;
            let imp =
                IrohTransport::create(config.iroh_transport, handler.clone())
                    .await?;
            Ok(DefaultTransport::create(&handler, imp))
        })
    }
}

type Connections = Arc<RwLock<HashMap<Url, Arc<ConnectionContext>>>>;

#[derive(Debug)]
struct IrohTransport {
    endpoint: Arc<Endpoint>,
    handler: Arc<TxImpHnd>,
    local_url: Arc<RwLock<Option<Url>>>,
    connections: Connections,
    connection_locks: Arc<Mutex<HashMap<Url, Arc<tokio::sync::Mutex<()>>>>>,
    watch_addr_task: AbortHandle,
    accept_task: AbortHandle,
}

impl Drop for IrohTransport {
    fn drop(&mut self) {
        self.watch_addr_task.abort();
        self.accept_task.abort();
    }
}

impl IrohTransport {
    async fn create(
        config: IrohTransportConfig,
        handler: Arc<TxImpHnd>,
    ) -> K2Result<DynTxImp> {
        // If a relay server is configured, only use that.
        // Otherwise use the default relay servers provided by n0.
        let mut builder = if let Some(relay_url) = config.relay_url {
            let relay_url =
                RelayUrl::from_str(&relay_url).map_err(K2Error::other)?;
            let relay_map = RelayMap::from_iter([relay_url]);
            Endpoint::empty_builder(RelayMode::Custom(relay_map))
        } else {
            Endpoint::empty_builder(RelayMode::Default)
        };
        // Set kitsune2 protocol for handling data.
        builder = builder.alpns(vec![ALPN.to_vec()]);

        let endpoint = builder.bind().await.map_err(|err| {
            K2Error::other_src("failed to bind iroh endpoint", err)
        })?;

        let endpoint = Arc::new(endpoint);
        let local_url = Arc::new(RwLock::new(None));
        let connections = Arc::new(RwLock::new(HashMap::new()));
        let connection_locks = Arc::new(Mutex::new(HashMap::new()));

        let watch_addr_task = Self::spawn_watch_addr_task(
            endpoint.clone(),
            handler.clone(),
            local_url.clone(),
        );

        let accept_task = Self::spawn_accept_task(
            endpoint.clone(),
            handler.clone(),
            connections.clone(),
        );

        let out: DynTxImp = Arc::new(Self {
            endpoint,
            handler,
            local_url,
            connections,
            connection_locks,
            watch_addr_task,
            accept_task,
        });
        Ok(out)
    }

    /// Spawns a background task to watch for changes in the endpoint's listening address.
    ///
    /// The task monitors the iroh endpoint's address watcher, updating the local URL
    /// when it changes and notifying the handler of new listening addresses.
    /// It runs asynchronously until the watcher encounters an error.
    fn spawn_watch_addr_task(
        endpoint: Arc<Endpoint>,
        handler: Arc<TxImpHnd>,
        local_url: Arc<RwLock<Option<Url>>>,
    ) -> AbortHandle {
        let mut watcher = endpoint.watch_addr();
        tokio::spawn(async move {
            loop {
                match watcher.updated().await {
                    Ok(addr) => {
                        if let Some(url) = get_url_with_first_relay(&addr) {
                            {
                                let mut guard = local_url.write().unwrap();
                                if guard.as_ref() != Some(&url) {
                                    *guard = Some(url.clone());
                                }
                            }
                            handler.new_listening_address(url.clone()).await;
                        }
                    }
                    Err(err) => {
                        error!(
                            ?err,
                            "address watcher update failed, stopping watch loop"
                        );
                        break;
                    }
                }
            }
        })
        .abort_handle()
    }

    /// Spawns a background task to accept incoming connections from the iroh endpoint.
    ///
    /// The task runs in a loop, accepting incoming connections asynchronously.
    /// For each accepted connection, it creates a new [`ConnectionContext`] and spawns
    /// a connection reader to handle incoming unidirectional streams.
    fn spawn_accept_task(
        endpoint: Arc<Endpoint>,
        handler: Arc<TxImpHnd>,
        connections: Connections,
    ) -> AbortHandle {
        tokio::spawn(async move {
            loop {
                match endpoint.accept().await {
                    Some(incoming) => match incoming.await {
                        Ok(conn) => {
                            let conn_opened_at_s = SystemTime::UNIX_EPOCH
                                .elapsed()
                                .unwrap_or_else(|err| {
                                    warn!(?err, "failed to get system time");
                                    Duration::from_secs(0)
                                })
                                .as_secs();
                            let conn = Arc::new(conn);
                            // Create a new connection context in the connections map.
                            // Remote URL is not set yet and will be sent by the remote
                            // after the preflight.
                            let ctx = Arc::new(ConnectionContext::new(
                                handler.clone(),
                                conn.clone(),
                                None,
                                conn_opened_at_s,
                                None,
                            ));
                            // Spawn connection reader to accept incoming uni-directional
                            // streams from the remote.
                            spawn_connection_reader(ctx, connections.clone());
                        }
                        Err(err) => {
                            error!(?err, "iroh incoming connection failed");
                        }
                    },
                    None => {
                        error!(
                            "iroh incoming connection failed - endpoint closed"
                        );
                        break;
                    }
                }
            }
        })
        .abort_handle()
    }

    /// Creates a new connection and its associated context for a peer.
    ///
    /// Establishes a QUIC connection to the target, creates a ConnectionContext,
    /// adds it to the connections map, spawns a connection reader for incoming streams,
    /// performs the preflight handshake, and sends the local URL to the remote.
    async fn create_connection_and_context(
        endpoint: Arc<Endpoint>,
        target: EndpointAddr,
        handler: Arc<TxImpHnd>,
        peer: Url,
        connections: Connections,
        local_url: Option<Url>,
    ) -> K2Result<Arc<ConnectionContext>> {
        // Establish connection
        let conn = endpoint
            .connect(target.clone(), ALPN)
            .await
            .map_err(|err| K2Error::other_src("iroh connect failed", err))?;
        let conn_opened_at_s = SystemTime::UNIX_EPOCH
            .elapsed()
            .unwrap_or_else(|err| {
                warn!(?err, "failed to get system time");
                Duration::from_secs(0)
            })
            .as_secs();
        let conn = Arc::new(conn);
        let conn_type_watcher = endpoint.conn_type(target.id);
        // Create context and add it to the connections map.
        let ctx = Arc::new(ConnectionContext::new(
            handler.clone(),
            conn.clone(),
            Some(peer.clone()),
            conn_opened_at_s,
            conn_type_watcher,
        ));

        connections
            .write()
            .unwrap()
            .insert(peer.clone(), ctx.clone());

        // Spawn connection reader that will accept incoming uni-directional
        // streams from the remote.
        spawn_connection_reader(ctx.clone(), connections.clone());

        // Send peer URL to remote first.
        if let Some(local_url) = local_url {
            let payload = Bytes::copy_from_slice(local_url.as_str().as_bytes());
            send_frame(&conn, FrameType::PeerUrl, payload).await?;
            // TODO: will be fixed with issue #403
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        // Perform preflight as second message on the new connection.
        let preflight = handler.peer_connect(peer.clone()).await?;
        let result = send_frame(&conn, FrameType::Payload, preflight).await;
        if let Err(err) = result {
            // Preflight failed, remove connection context from map.
            connections.write().unwrap().remove(&peer);
            // Close connection gracefully.
            conn.close(0u8.into(), b"preflight failed");
            return Err(err);
        }
        // TODO: will be fixed with issue #403
        tokio::time::sleep(Duration::from_millis(1000)).await;

        Ok(ctx)
    }
}

impl TxImp for IrohTransport {
    fn url(&self) -> Option<Url> {
        self.local_url.read().unwrap().clone()
    }

    fn disconnect(
        &self,
        peer: Url,
        _payload: Option<(String, Bytes)>,
    ) -> BoxFut<'_, ()> {
        if let Some(connection) =
            self.connections.write().unwrap().remove(&peer)
        {
            info!(?peer, "disconnecting from peer");
            connection.connection().close(0u8.into(), b"disconnected");
        }
        Box::pin(async {})
    }

    fn send(&self, peer: Url, data: Bytes) -> BoxFut<'_, K2Result<()>> {
        let local_url = self.local_url.read().unwrap().clone();
        let endpoint = self.endpoint.clone();
        let handler = self.handler.clone();
        let connections = self.connections.clone();
        let connection_locks = self.connection_locks.clone();

        Box::pin(async move {
            let remote = endpoint_from_url(&peer)?;

            // Get or create the connection lock for this peer to serialize connection creation.
            let peer_lock = {
                let mut locks = connection_locks.lock().unwrap();
                locks
                    .entry(peer.clone())
                    .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
                    .clone()
            };

            // Acquire the write lock to serialize connection creation for this peer.
            //
            // Other send requests to the same peer will wait here to acquire the lock.
            // The lock is released immediately if there is a connection, Otherwise
            // a connection is established and the preflight and host URL is sent
            // to the remote, before the lock is released.
            let _lock_guard = peer_lock.lock().await;

            // Atomically check and create connection and context if needed.
            let connection_context = {
                // Check if connection already exists, as another call might have
                // created it while this one was waiting for the lock.
                let existing = connections.read().unwrap().get(&peer).cloned();
                if let Some(ctx) = existing {
                    // Connection already exists, use it (preflight already done).
                    drop(_lock_guard);
                    ctx
                } else {
                    // Connection doesn't exist, create it.
                    // This establishes the connection, adds the context to the connections map,
                    // sends the preflight and the host URL to the remote.
                    tracing::info!(?peer, "establishing connection to peer");
                    let ctx = Self::create_connection_and_context(
                        endpoint,
                        remote,
                        handler,
                        peer,
                        connections,
                        local_url,
                    )
                    .await?;

                    // Lock is released after connection is established and preflight is done.
                    ctx
                }
            };

            // Send actual message.
            let data_len = data.len() as u64;
            send_frame(
                &connection_context.connection(),
                FrameType::Payload,
                data,
            )
            .await?;

            // Update stats
            connection_context.increment_send_message_count();
            connection_context.increment_send_bytes(data_len);

            Ok(())
        })
    }

    fn get_connected_peers(&self) -> BoxFut<'_, K2Result<Vec<Url>>> {
        Box::pin(async {
            Ok(self.connections.read().unwrap().keys().cloned().collect())
        })
    }

    fn dump_network_stats(&self) -> BoxFut<'_, K2Result<TransportStats>> {
        Box::pin(async move {
            let connections = self.connections.read().unwrap().clone();
            let mut peer_urls = Vec::new();
            if let Some(own_url) = self.local_url.read().unwrap().clone() {
                peer_urls.push(own_url);
            }
            let stat_connections = connections
                .into_values()
                .map(|context| {
                    TransportConnectionStats {
                        // When the context is added to the connections map, the handshake
                        // with the URL exchange is already complete. URL must be `Some`.
                        pub_key: context
                            .remote()
                            .unwrap()
                            .peer_id()
                            .unwrap()
                            .to_string(),
                        send_message_count: context.get_send_message_count(),
                        send_bytes: context.get_send_bytes(),
                        recv_message_count: context.get_recv_message_count(),
                        recv_bytes: context.get_recv_bytes(),
                        opened_at_s: context.get_opened_at_s(),
                        is_direct: matches!(
                            context.get_connection_type(),
                            ConnectionType::Direct(_)
                        ),
                    }
                })
                .collect();
            Ok(TransportStats {
                backend: "iroh".to_string(),
                peer_urls,
                connections: stat_connections,
            })
        })
    }
}

/// Spawns an asynchronous task to continuously read and handle incoming unidirectional streams
/// from an iroh connection. Each stream contains framed messages (PeerUrl or Payload), which are
/// parsed and processed accordingly. This enables concurrent handling of multiple streams per connection.
/// In essence a unidirectional stream is created per message.
///
/// The task loops indefinitely, accepting streams and spawning individual handlers for each.
/// Errors in stream processing trigger warnings and potential peer disconnections.
///
/// # Parameters
/// - `ctx`: The connection context containing handler and remote URL state.
/// - `connections`: Shared map of peer URLs to their connection contexts, updated for PeerUrl frames.
fn spawn_connection_reader(
    ctx: Arc<ConnectionContext>,
    connections: Connections,
) {
    tokio::spawn(async move {
        loop {
            // Main loop to accept incoming unidirectional streams from the remote peer.
            match ctx.connection().accept_uni().await {
                Ok(stream) => {
                    let ctx = ctx.clone();
                    let connections = connections.clone();
                    // Spawn a dedicated task for each incoming stream to handle concurrently.
                    tokio::spawn(handle_incoming_stream(
                        stream,
                        ctx,
                        connections,
                    ));
                }
                Err(err) => {
                    // Connection closed, notify disconnect and exit loop.
                    let peer = ctx.remote();
                    error!(?err, ?peer, "connection closed by peer");
                    if let Some(peer) = peer {
                        if let Some(connection) =
                            connections.write().unwrap().remove(&peer)
                        {
                            connection
                                .connection()
                                .close(0u8.into(), b"peer disconnected");
                        }
                    }
                    ctx.notify_disconnect();
                    break;
                }
            }
        }
    });
}

async fn handle_incoming_stream(
    mut stream: RecvStream,
    ctx: Arc<ConnectionContext>,
    connections: Connections,
) {
    let result = async {
        let data =
            stream.read_to_end(MAX_FRAME_BYTES).await.map_err(|err| {
                K2Error::other_src("failed to read iroh frame", err)
            })?;
        let (frame_type, payload) = decode_frame(data)?;
        match frame_type {
            // Handle PeerUrl frame: update connections map and context.
            FrameType::PeerUrl => {
                let url = Url::from_str(
                    std::str::from_utf8(&payload).map_err(|err| {
                        K2Error::other_src("invalid peer url payload", err)
                    })?,
                )?;
                connections
                    .write()
                    .unwrap()
                    .insert(url.clone(), ctx.clone());
                ctx.set_remote_url(url);
                K2Result::Ok(())
            }
            // Handle Payload frame: forward data to handler if remote URL is set.
            FrameType::Payload => {
                let peer = ctx.remote().ok_or_else(|| {
                    K2Error::other(
                        "received payload before peer url".to_string(),
                    )
                })?;
                let payload_len = payload.len() as u64;
                ctx.handler().recv_data(peer, payload).await?;
                ctx.increment_recv_message_count();
                ctx.increment_recv_bytes(payload_len);
                Ok(())
            }
        }
    }
    .await;

    if let Err(err) = result {
        warn!(?err, "iroh stream error, closing connection");
        if let Some(peer) = ctx.remote() {
            let _ = ctx
                .handler()
                .set_unresponsive(peer.clone(), Timestamp::now())
                .await;
            ctx.handler().peer_disconnect(peer, None);
        }
    }
}
