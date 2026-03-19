#![deny(missing_docs)]
//! Kitsune2 transport implementation backed by iroh.
//!
//! This transport establishes peer-to-peer connections using iroh's QUIC-based networking.
//! It manages outgoing and incoming connections dynamically, sending and receiving data
//! as framed messages over persistent uni-directional streams.
//!
//! Each message is framed with a header that specifies the frame type (preflight or data) and
//! the data length, leading to ordered and bounded message delivery. The peer URL is sent
//! as part of the preflight to inform the remote about it and make it available to respond to
//! on the transport level. Since there is no discovery service present in the kitsune2
//! architecture, the remote URL must be sent with the preflight.
//! Incoming streams are accepted and handled asynchronously per connection. There is one
//! stream open per direction, over which all frames are sent.
//!
//! # Architecture
//!
//! Complete trait abstraction of all I/O operations, enabling full testability without network dependencies.
//!
//! ```text
//!        Traits                   Implementations
//!
//!     ┌──────────┐               ┌──────────────┐
//!     │ Endpoint │               │ IrohEndpoint │
//!     └────┬─────┘               └──────┬───────┘
//!          │                            │
//!          ▼                            ▼
//!    ┌────────────┐             ┌────────────────┐
//!    │ Connection │             │ IrohConnection │
//!    └─────┬──────┘             └───────┬────────┘
//!          │                            │
//!     ┌────┴────┐                  ┌────┴────┐
//!     ▼         ▼                  ▼         ▼
//! ┌────────┐ ┌────────┐   ┌────────────┐ ┌────────────┐
//! │  Send  │ │  Recv  │   │  IrohSend  │ │  IrohRecv  │
//! │ Stream │ │ Stream │   │   Stream   │ │   Stream   │
//! └────────┘ └────────┘   └────────────┘ └────────────┘
//! ```
//!
//! # IrohTransport task management
//!
//! ```text
//!                       ┌───────────────┐
//!                       │ IrohTransport │
//!                       └───────┬───────┘
//!                               │
//!               ┌───────────────┴───────────────┐
//!               │                               │
//!               ▼                               ▼
//!     ┌─────────────────┐             ┌─────────────────┐
//!     │ watch_addr_task │             │   accept_task   │
//!     └────────┬────────┘             └───┬─────────┬───┘
//!              │                          │         │
//!              │ monitors                 │         └──────────┬──────────┐
//!              ▼                          │ accepts            │          │
//!     ┌─────────────────┐                 ▼                    ▼          ▼
//!     │  Relay Address  │          ┌────────────┐       ┌──────────┐┌──────────┐┌──────────┐
//!     │    Changes      │          │  Incoming  │       │ conn_    ││ conn_    ││ conn_    │
//!     └─────────────────┘          │ Connections│       │ reader 1 ││ reader 2 ││ reader N │
//!                                  └────────────┘       └────┬─────┘└────┬─────┘└────┬─────┘
//!                                                            │           │           │
//!                                                            │ reads     │ reads     │ reads
//!                                                            ▼           ▼           ▼
//!                                                      ┌─────────┐ ┌─────────┐ ┌─────────┐
//!                                                      │ Peer 1  │ │ Peer 2  │ │ Peer N  │
//!                                                      │ Frames  │ │ Frames  │ │ Frames  │
//!                                                      └─────────┘ └─────────┘ └─────────┘
//! ```
//!
//! # Connection establishment
//!
//! The transport handlers [`TxImp::send`] implementation contains the logic
//! for connection establishment.
//!
//! ```text
//!                  ┌────────────────┐
//!                  │ send to peer X │
//!                  └───────┬────────┘
//!                          │
//!                          ▼
//!                ┌───────────────────┐
//!                │ Connection exists?│
//!                └─────────┬─────────┘
//!                          │
//!            ┌─────────────┴─────────────┐
//!            │ Yes                    No │
//!            ▼                           ▼
//!   ┌────────────────────┐    ┌─────────────────────────┐
//!   │ Use existing       │    │ Acquire peer-specific   │
//!   │ connection         │    │ lock                    │
//!   └─────────┬──────────┘    └────────────┬────────────┘
//!             │                            │
//!             │                            ▼
//!             │               ┌────────────────────────┐
//!             │               │ Recheck connection     │
//!             │               │ after lock             │
//!             │               └───────────┬────────────┘
//!             │                           │
//!             │              ┌────────────┴────────────┐
//!             │              │ Created by           No │
//!             │              │ another task            │
//!             │              ▼                         ▼
//!             │         ┌────┘          ┌──────────────────────┐
//!             │         │               │ Create new connection│
//!             │         │               └──────────┬───────────┘
//!             │         │                          │
//!             │         │                          ▼
//!             │         │               ┌──────────────────┐
//!             │         │               │ Send preflight   │
//!             │         │               └────────┬─────────┘
//!             │         │                        │
//!             │         │                        ▼
//!             │         │               ┌──────────────────┐
//!             │         │               │ Store in map     │
//!             │         │               └────────┬─────────┘
//!             │         │                        │
//!             ▼         ▼                        │
//!   ┌────────────────────┐◄──────────────────────┘
//!   │ Use existing       │
//!   │ connection         │
//!   └─────────┬──────────┘
//!             │
//!             ▼
//!      ┌────────────┐
//!      │ Send data  │
//!      └────────────┘
//! ```
//!
//! Every connection starts with a mandatory bidirectional handshake:
//!
//! ```text
//!     Peer A                                       Peer B
//!        │                                            │
//!        │         ┌────────────────────────┐         │
//!        │         │ Connection Established │         │
//!        │         └────────────────────────┘         │
//!        │                                            │
//!        │  Preflight Frame (Type 0)                  │
//!        │  [URL + Handshake Data]                    │
//!        │ ──────────────────────────────────────────>│
//!        │                                            │
//!        │                          ┌───────────────┐ │
//!        │                          │  10s timeout  │ │
//!        │                          │   enforced    │ │
//!        │                          └───────────────┘ │
//!        │                                            │
//!        │                 Return Preflight Frame     │
//!        │                 [URL + Handshake Data]     │
//!        │<───────────────────────────────────────────│
//!        │                                            │
//!        │          ┌────────────────────┐            │
//!        │          │ Connection Ready   │            │
//!        │          └────────────────────┘            │
//!        │                                            │
//!        │  Data Frame (Type 1)                       │
//!        │ ──────────────────────────────────────────>│
//!        │                                            │
//!        │                      Data Frame (Type 1)   │
//!        │<───────────────────────────────────────────│
//!        │                                            │
//!     Peer A                                       Peer B
//!
//! ```
//!
//! # iroh transport frames
//!
//! ```text
//! Preflight Frame (Type 0):
//! ┌─────┬────────┬─────────┬─────┬───────────┐
//! │ 0x0 │ Length │ URL Len │ URL │ Preflight │
//! │ 1 B │  4 B   │   4 B   │ Var │   Data    │
//! └─────┴────────┴─────────┴─────┴───────────┘
//!
//! Data Frame (Type 1):
//! ┌─────┬────────┬──────┐
//! │ 0x1 │ Length │ Data │
//! │ 1 B │  4 B   │ Var  │
//! └─────┴────────┴──────┘
//! ```

use crate::endpoint::{DynIrohEndpoint, IrohEndpoint};
use bytes::Bytes;
use iroh::{
    Endpoint, EndpointAddr, RelayConfig, RelayMap, RelayMode, RelayUrl,
};
use kitsune2_api::*;
use std::{
    collections::HashMap,
    str::FromStr,
    sync::{Arc, Mutex, RwLock},
    time::{Duration, SystemTime},
};
use tokio::task::AbortHandle;
use tracing::{debug, error, info, warn};

mod frame;
use frame::*;
mod url;
use url::*;
mod connection;
mod connection_context;
mod endpoint;
mod stream;
use connection_context::*;
#[cfg(feature = "metrics")]
mod metrics;

#[cfg(any(test, feature = "test-utils"))]
pub mod test_utils;

#[cfg(test)]
mod tests;

const ALPN: &[u8] = b"kitsune2/0";

/// IrohTransport configuration types
pub mod config {
    /// Configuration for the [`IrohTransportFactory`](super::IrohTransportFactory).
    #[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
    #[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
    #[serde(rename_all = "camelCase")]
    pub struct IrohTransportConfig {
        /// Explicit relay URL to use as home relay. If none is set,
        /// relays provided by n0 will be used.
        ///
        /// Defaults to `None`.
        #[cfg_attr(feature = "schema", schemars(default))]
        pub relay_url: Option<String>,

        /// Allow connecting to plaintext (http) relay server
        /// instead of the default requiring TLS (https).
        ///
        /// Default: false.
        #[cfg_attr(feature = "schema", schemars(default))]
        pub relay_allow_plain_text: bool,

        /// Set the maximum size in bytes for a frame that the transport
        /// can transmit.
        ///
        /// Defaults to 1 MiB.
        #[cfg_attr(feature = "schema", schemars(default))]
        pub max_frame_bytes: usize,

        /// The timeout for establishing a connection to a peer.
        ///
        /// Defaults to 60 seconds.
        #[cfg_attr(feature = "schema", schemars(default))]
        pub connect_timeout_s: u32,
    }

    impl Default for IrohTransportConfig {
        fn default() -> Self {
            Self {
                relay_url: None,
                relay_allow_plain_text: false,
                max_frame_bytes: 1024 * 1024,
                connect_timeout_s: 60,
            }
        }
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
            let relay_server_url = ::url::Url::parse(relay)
                .map_err(|err| K2Error::other_src("Invalid relay URL", err))?;
            if relay_server_url.scheme() == "http"
                && !config.iroh_transport.relay_allow_plain_text
            {
                return Err(K2Error::other("Disallowed plaintext relay URL"));
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

            // Ensure the relay URL ends with '/' so that iroh appends
            // paths correctly rather than replacing the last segment.
            let mut transport_config = config.iroh_transport;
            transport_config.relay_url =
                transport_config.relay_url.map(|url| {
                    if url.ends_with('/') {
                        url
                    } else {
                        format!("{url}/")
                    }
                });

            let auth_material = builder.auth_material.clone();
            let imp = IrohTransport::create(
                transport_config,
                handler.clone(),
                auth_material,
            )
            .await?;
            Ok(DefaultTransport::create(&handler, imp))
        })
    }
}

type Connections = Arc<RwLock<HashMap<Url, Arc<ConnectionContext>>>>;

/// Iroh-based transport implementation.
#[derive(Debug)]
struct IrohTransport {
    endpoint: DynIrohEndpoint,
    handler: Arc<TxImpHnd>,
    local_url: Arc<RwLock<Option<Url>>>,
    connections: Connections,
    connection_locks: Arc<Mutex<HashMap<Url, Arc<tokio::sync::Mutex<()>>>>>,
    watch_addr_task: AbortHandle,
    accept_task: AbortHandle,
    config: IrohTransportConfig,
}

impl Drop for IrohTransport {
    fn drop(&mut self) {
        info!(local_url = ?self.local_url, "Dropping transport");
        self.watch_addr_task.abort();
        self.accept_task.abort();
        // The connection reader task inside the connection context
        // holds a reference to the context. Thus the context can
        // only be dropped once that reference is dropped, which
        // happens when the task is aborted.
        self.connections
            .write()
            .expect("poisoned")
            .drain()
            .for_each(|(remote_url, ctx)| {
                debug!(?remote_url, "Aborting connection context tasks");
                ctx.abort_tasks();
            });
    }
}

impl IrohTransport {
    async fn create(
        config: IrohTransportConfig,
        handler: Arc<TxImpHnd>,
        auth_material: Option<Vec<u8>>,
    ) -> K2Result<DynTxImp> {
        // Determine whether we need to register with the relay before connecting.
        // Registration is required when both a relay URL and auth material are provided.
        let needs_relay_registration =
            config.relay_url.is_some() && auth_material.is_some();

        // If a relay server is configured, only use that.
        // Otherwise, use the default relay servers provided by n0.
        let mut builder = if let Some(relay_url) = &config.relay_url {
            if needs_relay_registration {
                // Start with an empty relay map so the endpoint binds without
                // immediately connecting to the relay. The relay transport is
                // kept intact so that insert_relay (called after registration)
                // can establish the WebSocket connection.
                Endpoint::empty_builder(RelayMode::Custom(RelayMap::empty()))
            } else {
                let relay_url =
                    RelayUrl::from_str(relay_url).map_err(|err| {
                        K2Error::other_src("Invalid relay URL", err)
                    })?;
                let relay_map = RelayMap::from_iter([relay_url]);
                Endpoint::empty_builder(RelayMode::Custom(relay_map))
            }
        } else {
            Endpoint::empty_builder(RelayMode::Default)
        };
        // Set kitsune2 protocol for handling data.
        builder = builder.alpns(vec![ALPN.to_vec()]);

        // Test relay server uses self-signed certificate, so skip certificate verification.
        #[cfg(feature = "test-utils")]
        {
            builder = builder.insecure_skip_relay_cert_verify(true);
        }

        let endpoint = builder.bind().await.map_err(|err| {
            K2Error::other_src("Failed to bind iroh endpoint", err)
        })?;

        // If we need relay registration, authenticate and register our public
        // key with the server before inserting the relay into the endpoint.
        // insert_relay is deferred until after the watcher task is spawned so
        // that the address update it fires is guaranteed to be observed.
        if needs_relay_registration {
            let relay_url_str = config
                .relay_url
                .as_deref()
                .expect("relay_url checked above");
            let auth_bytes =
                auth_material.expect("auth_material checked above");

            // Derive the server base URL from the relay URL by removing the path.
            // e.g. "http://addr/relay/" -> "http://addr/"
            let server_url = ::url::Url::parse(relay_url_str).map_err(|e| {
                K2Error::other_src("Invalid relay URL for registration", e)
            })?;
            let mut server_url = server_url;
            server_url.set_path("/");

            let key_bytes = *endpoint.id().as_bytes();
            let auth_material =
                kitsune2_bootstrap_client::AuthMaterial::new(auth_bytes);

            // Perform the authentication and key registration synchronously.
            tokio::task::spawn_blocking(move || {
                kitsune2_bootstrap_client::blocking_register_relay_key(
                    server_url,
                    &auth_material,
                    &key_bytes,
                )
            })
            .await
            .map_err(|e| K2Error::other_src("Registration task failed", e))??;
        }

        // Clone the raw endpoint before consuming it into IrohEndpoint so that
        // insert_relay can be called after the watcher task is subscribed.
        // iroh::Endpoint is Arc-backed so this is a cheap reference copy.
        let raw_endpoint_for_relay = if needs_relay_registration {
            Some(endpoint.clone())
        } else {
            None
        };

        let endpoint = Arc::new(IrohEndpoint::new(endpoint));
        let local_url = Arc::new(RwLock::new(None));
        let connections = Arc::new(RwLock::new(HashMap::new()));
        let connection_locks = Arc::new(Mutex::new(HashMap::new()));

        let watch_addr_task = Self::spawn_watch_addr_task(
            endpoint.clone(),
            handler.clone(),
            local_url.clone(),
        );

        // The watcher is now subscribed. Insert the relay so that the address
        // update it fires is captured by the running watcher task, ensuring
        // local_url is populated before any outbound send can run.
        if let Some(raw_ep) = raw_endpoint_for_relay {
            let relay_url_str = config
                .relay_url
                .as_deref()
                .expect("relay_url checked above");
            let relay_url = RelayUrl::from_str(relay_url_str)
                .map_err(|err| K2Error::other_src("Invalid relay URL", err))?;
            raw_ep
                .insert_relay(
                    relay_url.clone(),
                    Arc::new(RelayConfig {
                        url: relay_url,
                        quic: None,
                    }),
                )
                .await;
        }

        let accept_task = Self::spawn_accept_task(
            endpoint.clone(),
            handler.clone(),
            connections.clone(),
            local_url.clone(),
            config.max_frame_bytes,
        );

        let out: DynTxImp = Arc::new(Self {
            endpoint,
            handler,
            local_url,
            connections,
            connection_locks,
            watch_addr_task,
            accept_task,
            config,
        });
        Ok(out)
    }

    /// Spawns a background task to watch for changes in the endpoint's listening address.
    ///
    /// The task monitors the iroh endpoint's address watcher, updating the local URL
    /// when it changes and notifying the handler of a new listening address.
    /// It runs asynchronously until the watcher encounters an error.
    fn spawn_watch_addr_task(
        endpoint: DynIrohEndpoint,
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
                                info!(?url, "Received a new listening address from relay server");
                                let mut guard =
                                    local_url.write().expect("poisoned");
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
                            "Address watcher update failed, stopping watch loop"
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
    /// a connection reader to handle incoming uni-directional streams.
    fn spawn_accept_task(
        endpoint: DynIrohEndpoint,
        handler: Arc<TxImpHnd>,
        connections: Connections,
        local_url: Arc<RwLock<Option<Url>>>,
        max_frame_bytes: usize,
    ) -> AbortHandle {
        tokio::spawn(async move {
            loop {
                match endpoint.accept().await {
                    Some(Ok(connection)) => {
                        info!(remote_id = ?connection.remote_id(),"Receiving incoming connection");
                        let conn_opened_at_s = SystemTime::UNIX_EPOCH
                            .elapsed()
                            .unwrap_or_else(|err| {
                                warn!(?err, "Failed to get system time");
                                Duration::from_secs(0)
                            })
                            .as_secs();

                        // Create a new connection context.
                        ConnectionContext::new(
                            ConnectionContextParams{
                            handler: handler.clone(),
                            connection,
                            remote_url: None,
                            preflight_sent: false,
                            opened_at_s: conn_opened_at_s,
                            connections: connections.clone(),
                            local_url: local_url.clone(),
                            max_frame_bytes,
                        });
                    }
                    Some(Err(err)) => {
                        error!(?err, "iroh incoming connection failed");
                    }
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
    /// The connection is established and the preflight frame is sent. If this
    /// action succeeds, the context is returned. In case of error during the
    /// preflight, the context is dropped and an error returned.
    async fn create_connection_and_context(
        endpoint: DynIrohEndpoint,
        target: EndpointAddr,
        handler: Arc<TxImpHnd>,
        remote_url: Url,
        connections: Connections,
        local_url: Arc<RwLock<Option<Url>>>,
        config: &IrohTransportConfig,
    ) -> K2Result<Arc<ConnectionContext>> {
        // Establish connection
        let conn = match tokio::time::timeout(
            Duration::from_secs(config.connect_timeout_s as u64),
            endpoint.connect(target.clone(), ALPN),
        )
        .await
        {
            Err(e) => {
                // On connection establishment error, mark the peer unresponsive
                let _ = handler
                    .set_unresponsive(remote_url.clone(), Timestamp::now())
                    .await;

                Err(K2Error::other_src("iroh connect timed out", e))
            }
            Ok(res) => res,
        }?;

        let conn_opened_at_s = SystemTime::UNIX_EPOCH
            .elapsed()
            .unwrap_or_else(|err| {
                warn!(?err, "Failed to get system time");
                Duration::from_secs(0)
            })
            .as_secs();

        // Send preflight as first message on the new connection.
        let maybe_local_url = local_url.read().expect("poisoned").clone();
        if let Some(current_local_url) = maybe_local_url {
            let preflight_bytes =
                handler.peer_connect(remote_url.clone()).await?;

            let ctx = ConnectionContext::new(ConnectionContextParams {
                handler: handler.clone(),
                connection: conn,
                remote_url: Some(remote_url.clone()),
                preflight_sent: true,
                opened_at_s: conn_opened_at_s,
                connections: connections.clone(),
                local_url: local_url.clone(),
                max_frame_bytes: config.max_frame_bytes,
            });

            if let Err(e) = ctx
                .send_preflight_frame(
                    current_local_url.clone(),
                    preflight_bytes,
                )
                .await
            {
                // On send preflight error, mark the peer unresponsive
                let _ = handler
                    .set_unresponsive(remote_url.clone(), Timestamp::now())
                    .await;

                return Err(e);
            }

            Ok(ctx)
        } else {
            Err(K2Error::other(
                "Connection attempted before home relay URL is known",
            ))
        }
    }
}

impl TxImp for IrohTransport {
    fn url(&self) -> Option<Url> {
        self.local_url.read().expect("poisoned").clone()
    }

    fn disconnect(
        &self,
        peer: Url,
        _payload: Option<(String, Bytes)>,
    ) -> BoxFut<'_, ()> {
        if let Some(ctx) =
            self.connections.write().expect("poisoned").remove(&peer)
        {
            ctx.disconnect("Disconnecting from remote".to_string());
        }
        Box::pin(async {})
    }

    fn send(&self, remote_url: Url, data: Bytes) -> BoxFut<'_, K2Result<()>> {
        let local_url = self.local_url.clone();
        let endpoint = self.endpoint.clone();
        let handler = self.handler.clone();
        let connections = self.connections.clone();
        let connection_locks = self.connection_locks.clone();

        Box::pin(async move {
            let remote = match endpoint_from_url(
                &remote_url,
                self.config.relay_url.as_deref(),
            ) {
                Err(e) => {
                    // If we cannot convert the url to an endpoint address, mark the peer unresponsive
                    let _ = self
                        .handler
                        .set_unresponsive(remote_url.clone(), Timestamp::now())
                        .await;

                    Err(K2Error::other_src(
                        format!(
                            "iroh send error converting Url to EndpointAddr {remote_url}"
                        ),
                        e,
                    ))
                }
                ok => ok,
            }?;

            // Get or create the connection lock for this peer to serialize connection creation.
            let peer_lock = {
                let mut locks = connection_locks.lock().expect("poisoned");
                locks
                    .entry(remote_url.clone())
                    .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
                    .clone()
            };

            // Acquire the write lock to serialize connection creation for this peer.
            //
            // Other send requests to the same peer will wait here to acquire the lock.
            // The lock is released immediately if there is a connection, Otherwise
            // a connection is established and the preflight and host URL are sent
            // to the remote, before the lock is released.
            //
            // The alternative to this mechanism would be fold the function of this
            // lock into the connections map. That would slightly reduce the
            // complexity in this method, but would increase complexity in all places
            // where the connection map is used. The connecions_locks map is only
            // used in this method. Overall it is simpler as is.
            let _lock_guard = peer_lock.lock().await;

            // Atomically check and create connection and context if needed.
            let connection_context = {
                // Check if connection already exists, as another call might have
                // created it while this one was waiting for the lock.
                let existing = connections
                    .read()
                    .expect("poisoned")
                    .get(&remote_url)
                    .cloned();
                if let Some(ctx) = existing {
                    // Connection already exists, use it (preflight already done).
                    drop(_lock_guard);
                    ctx
                } else {
                    // Connection doesn't exist, create it.
                    // This establishes the connection and sends the preflight to the remote.
                    info!(remote = ?remote_url.peer_id(), "Establishing connection to remote");
                    let ctx = Self::create_connection_and_context(
                        endpoint,
                        remote,
                        handler,
                        remote_url.clone(),
                        connections.clone(),
                        local_url.clone(),
                        &self.config,
                    )
                    .await?;

                    // Now that preflight has been sent successfully, add context to
                    // connections map.
                    connections
                        .write()
                        .expect("poisoned")
                        .insert(remote_url, ctx.clone());

                    // Lock is released after connection is established and preflight is done.
                    ctx
                }
            };

            // Send actual message.
            connection_context.send_data_frame(data).await?;

            Ok(())
        })
    }

    fn get_connected_peers(&self) -> BoxFut<'_, K2Result<Vec<Url>>> {
        Box::pin(async {
            Ok(self
                .connections
                .read()
                .expect("poisoned")
                .keys()
                .cloned()
                .collect())
        })
    }

    fn dump_network_stats(&self) -> BoxFut<'_, K2Result<TransportStats>> {
        Box::pin(async move {
            let connections =
                self.connections.read().expect("poisoned").clone();
            let mut peer_urls = Vec::new();
            if let Some(own_url) =
                self.local_url.read().expect("poisoned").clone()
            {
                peer_urls.push(own_url);
            }
            let stat_connections = connections
                .into_values()
                .map(|context| {
                    TransportConnectionStats {
                        // When the context is added to the connections map, the handshake
                        // with the URL exchange is already complete. URL must be `Some`.
                        pub_key: context
                            .remote_url()
                            .unwrap()
                            .peer_id()
                            .unwrap()
                            .to_string(),
                        send_message_count: context.get_send_message_count(),
                        send_bytes: context.get_send_bytes(),
                        recv_message_count: context.get_recv_message_count(),
                        recv_bytes: context.get_recv_bytes(),
                        opened_at_s: context.get_opened_at_s(),
                        is_direct: context.is_direct(),
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

    fn insert_relay(
        &self,
        relay_url: String,
        auth_material: Option<Vec<u8>>,
    ) -> BoxFut<'_, K2Result<()>> {
        let endpoint = self.endpoint.clone();
        Box::pin(async move {
            let relay_url_str = if relay_url.ends_with('/') {
                relay_url
            } else {
                format!("{relay_url}/")
            };

            info!(
                %relay_url_str,
                has_auth_material = auth_material.is_some(),
                auth_material_len = auth_material.as_ref().map(|a| a.len()),
                "IrohTransport::insert_relay called"
            );

            // If auth material is provided, register our public key
            // with the relay server before connecting.
            if let Some(auth_bytes) = auth_material {
                let server_url =
                    ::url::Url::parse(&relay_url_str).map_err(|e| {
                        K2Error::other_src(
                            "Invalid relay URL for registration",
                            e,
                        )
                    })?;
                let mut server_url = server_url;
                server_url.set_path("/");

                let key_bytes = endpoint.id_bytes();

                info!(
                    %server_url,
                    endpoint_key = ?key_bytes,
                    "Registering endpoint public key with relay server (authenticate + relay/register)"
                );

                let auth_material =
                    kitsune2_bootstrap_client::AuthMaterial::new(auth_bytes);

                match tokio::task::spawn_blocking(move || {
                    kitsune2_bootstrap_client::blocking_register_relay_key(
                        server_url,
                        &auth_material,
                        &key_bytes,
                    )
                })
                .await
                {
                    Ok(Ok(())) => {
                        info!(
                            %relay_url_str,
                            "Relay key registration successful"
                        );
                    }
                    Ok(Err(e)) => {
                        error!(
                            %relay_url_str,
                            ?e,
                            "Relay key registration FAILED (server rejected)"
                        );
                        return Err(e);
                    }
                    Err(e) => {
                        error!(
                            %relay_url_str,
                            ?e,
                            "Relay key registration FAILED (task error)"
                        );
                        return Err(K2Error::other_src(
                            "Registration task failed",
                            e,
                        ));
                    }
                }
            } else {
                info!(
                    %relay_url_str,
                    "No auth material provided, skipping relay key registration"
                );
            }

            let relay_url = RelayUrl::from_str(&relay_url_str)
                .map_err(|err| K2Error::other_src("Invalid relay URL", err))?;

            info!(
                %relay_url,
                "Inserting relay into iroh endpoint"
            );

            endpoint
                .insert_relay(
                    relay_url.clone(),
                    Arc::new(RelayConfig {
                        url: relay_url.clone(),
                        quic: None,
                    }),
                )
                .await;

            info!(
                %relay_url,
                "Relay inserted into iroh endpoint successfully"
            );

            Ok(())
        })
    }
}
