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
//! # Per-space configuration
//!
//! Each space can override transport settings by passing an
//! [`IrohTransportModConfig`] in the per-space config given to
//! [`Kitsune::space()`](kitsune2_api::Kitsune::space).
//! The same [`IrohTransportConfig`] type is used for both global and
//! per-space configuration.
//!
//! The fields relevant for per-space overrides are:
//!
//! - **`relay_url`**: A relay server URL specific to this space. The
//!   transport dynamically adds it via
//!   [`configure_for_space`](kitsune2_api::TxImp::configure_for_space) and
//!   delivers the resulting per-space URL through
//!   [`transport_url_changed`](kitsune2_api::TxImpHnd::transport_url_changed).
//! - **`relay_allow_plain_text`**: Must be set to `true` if `relay_url`
//!   uses `http://` instead of `https://`.
//! - **`auth_material_relay_base64`**: Base64-encoded auth material for
//!   relay registration. When set, the endpoint's public key is registered
//!   with the relay before connecting.
//!
//! Other fields (`max_frame_bytes`, `connect_timeout_s`) are endpoint-wide and
//! are ignored in per-space overrides.
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
use iroh::endpoint::presets::Minimal;
use iroh::{
    Endpoint, EndpointAddr, RelayConfig, RelayMap, RelayMode, RelayUrl,
};
use kitsune2_api::*;
use std::{
    collections::HashMap,
    str::FromStr,
    sync::{Arc, Mutex, RwLock},
    time::{Duration, Instant, SystemTime},
};
use tokio::task::AbortHandle;
use tracing::{debug, error, info, warn};

mod close_code;
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
        /// Defaults to 100 MiB.
        #[cfg_attr(feature = "schema", schemars(default))]
        pub max_frame_bytes: usize,

        /// The timeout for establishing a connection to a peer.
        ///
        /// Defaults to 60 seconds.
        #[cfg_attr(feature = "schema", schemars(default))]
        pub connect_timeout_s: u32,

        /// Base64-encoded auth material for relay registration.
        /// When set alongside `relay_url` in a per-space config override,
        /// the endpoint's public key is registered with the relay server
        /// before connecting. Ignored in the global config.
        ///
        /// Defaults to `None`.
        #[serde(default)]
        #[cfg_attr(feature = "schema", schemars(skip))]
        pub auth_material_relay_base64: Option<String>,

        /// Interval in seconds of the keepalive that re-registers the
        /// endpoint public key with an authenticated relay's bootstrap
        /// server.
        ///
        /// The keepalive keeps the server-side relay allowlist entry alive.
        /// Only used when auth material is configured.
        ///
        /// Defaults to 120 seconds, well within the server's default
        /// 5-minute auth token idle timeout.
        #[serde(default = "default_relay_keepalive_interval_s")]
        #[cfg_attr(feature = "schema", schemars(default))]
        pub relay_keepalive_interval_s: u32,
    }

    fn default_relay_keepalive_interval_s() -> u32 {
        120
    }

    impl Default for IrohTransportConfig {
        fn default() -> Self {
            Self {
                relay_url: None,
                relay_allow_plain_text: false,
                max_frame_bytes: 100 * 1024 * 1024,
                connect_timeout_s: 60,
                auth_material_relay_base64: None,
                relay_keepalive_interval_s: default_relay_keepalive_interval_s(
                ),
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

        // Prevent a zero-duration sleep from creating a busy
        // keepalive loop that continuously issues blocking HTTP requests.
        if config.iroh_transport.relay_keepalive_interval_s == 0 {
            return Err(K2Error::other(
                "Relay keepalive interval must be greater than zero",
            ));
        }

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

            let auth_material = builder.auth_material_relay.clone();
            let imp: DynTxImp = IrohTransport::create(
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

/// Per-space relay configuration and its current advertisable state.
#[derive(Clone, Debug)]
pub(crate) struct SpaceRelayState {
    relay_url: RelayUrl,
    local_url: Option<Url>,
    installed: bool,
}

type SpaceRelays = Arc<RwLock<HashMap<SpaceId, SpaceRelayState>>>;

/// Parameters needed to (re-)authenticate for relay access.
#[derive(Debug)]
struct RelayAuthParams {
    /// Base URL of the bootstrap server (e.g. `http://addr/`), used to
    /// reach the `/authenticate` and `/relay/keepalive` endpoints.
    server_url: ::url::Url,

    /// Credentials used to obtain a bearer token from the auth server.
    auth_material: kitsune2_bootstrap_client::AuthMaterial,

    /// The relay URL the bearer token is presented to.
    relay_url: RelayUrl,

    /// The 32-byte iroh endpoint public key registered on the relay
    /// allowlist.
    key_bytes: [u8; 32],
}

#[derive(Debug)]
struct PreparedSpaceRelay {
    relay_url: RelayUrl,
    auth_params: Option<Arc<RelayAuthParams>>,
}

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
    relay_lifecycle_task: Option<AbortHandle>,
    /// Background lifecycle tasks for per-space relays.
    space_relay_tasks: Arc<Mutex<HashMap<SpaceId, AbortHandle>>>,
    config: IrohTransportConfig,
    space_relays: SpaceRelays,
    space_relay_state_changed: Arc<tokio::sync::Notify>,
}

impl Drop for IrohTransport {
    fn drop(&mut self) {
        info!(local_url = ?self.local_url, "Dropping transport");
        self.watch_addr_task.abort();
        self.accept_task.abort();
        if let Some(handle) = self.relay_lifecycle_task.take() {
            handle.abort();
        }
        self.space_relay_tasks
            .lock()
            .expect("poisoned")
            .drain()
            .for_each(|(_, handle)| handle.abort());
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
        let endpoint = self.endpoint.clone();
        tokio::spawn(async move { endpoint.close().await });
    }
}

impl IrohTransport {
    async fn create(
        config: IrohTransportConfig,
        handler: Arc<TxImpHnd>,
        auth_material: Option<Vec<u8>>,
    ) -> K2Result<Arc<Self>> {
        // Determine whether we need to authenticate for relay access.
        // Authentication is required when both a relay URL and auth material
        // are provided.
        let needs_relay_auth =
            config.relay_url.is_some() && auth_material.is_some();

        // If a relay server is configured, only use that.
        // Otherwise, use the default relay servers provided by n0.
        let mut builder = if let Some(relay_url) = &config.relay_url {
            if needs_relay_auth {
                // Start with an empty relay map so the endpoint binds without
                // immediately connecting to the relay. The relay transport is
                // kept intact so that insert_relay (called after
                // authentication) can establish the WebSocket connection.
                Endpoint::builder(Minimal)
                    .relay_mode(RelayMode::Custom(RelayMap::empty()))
            } else {
                let relay_url =
                    RelayUrl::from_str(relay_url).map_err(|err| {
                        K2Error::other_src("Invalid relay URL", err)
                    })?;
                let relay_map = RelayMap::from_iter([relay_url]);
                Endpoint::builder(Minimal)
                    .relay_mode(RelayMode::Custom(relay_map))
            }
        } else {
            Endpoint::builder(Minimal).relay_mode(RelayMode::Default)
        };

        let transport_config = iroh::endpoint::QuicTransportConfig::builder()
            .keep_alive_interval(Duration::from_secs(5))
            .max_idle_timeout(Some(
                Duration::from_secs(60).try_into().map_err(K2Error::other)?,
            ))
            .build();
        builder = builder.transport_config(transport_config);

        // Set kitsune2 protocol for handling data.
        builder = builder.alpns(vec![ALPN.to_vec()]);

        // Test relay server uses self-signed certificate, so skip certificate verification.
        #[cfg(feature = "test-utils")]
        {
            builder = builder.ca_tls_config(
                iroh_relay::tls::CaTlsConfig::insecure_skip_verify(),
            );
        }

        let endpoint = builder.bind().await.map_err(|err| {
            K2Error::other_src("Failed to bind iroh endpoint", err)
        })?;

        // Authentication depends on external services, so retain only the
        // locally validated parameters here. The transport's recovery task
        // obtains and refreshes credentials after construction.
        let relay_auth = if needs_relay_auth {
            let relay_url_str = config
                .relay_url
                .as_deref()
                .expect("relay_url checked above");
            let auth_bytes =
                auth_material.expect("auth_material checked above");
            let mut server_url =
                ::url::Url::parse(relay_url_str).map_err(|error| {
                    K2Error::other_src(
                        "Invalid relay URL for authentication",
                        error,
                    )
                })?;
            server_url.set_path("/");
            let relay_url =
                RelayUrl::from_str(relay_url_str).map_err(|error| {
                    K2Error::other_src("Invalid relay URL", error)
                })?;

            Some(Arc::new(RelayAuthParams {
                server_url,
                auth_material: kitsune2_bootstrap_client::AuthMaterial::new(
                    auth_bytes,
                ),
                relay_url,
                key_bytes: *endpoint.id().as_bytes(),
            }))
        } else {
            None
        };

        let endpoint: DynIrohEndpoint =
            Arc::new(IrohEndpoint::new(endpoint.clone()));

        Self::create_with_endpoint(endpoint, handler, config, relay_auth).await
    }

    /// Starts background endpoint handling without requiring a relay-backed
    /// listening URL.
    async fn create_with_endpoint(
        endpoint: DynIrohEndpoint,
        handler: Arc<TxImpHnd>,
        config: IrohTransportConfig,
        relay_auth: Option<Arc<RelayAuthParams>>,
    ) -> K2Result<Arc<Self>> {
        let local_url = Arc::new(RwLock::new(None));
        let connections = Arc::new(RwLock::new(HashMap::new()));
        let connection_locks = Arc::new(Mutex::new(HashMap::new()));
        let space_relays: SpaceRelays = Arc::new(RwLock::new(HashMap::new()));
        let space_relay_state_changed = Arc::new(tokio::sync::Notify::new());

        let watch_addr_task = Self::spawn_watch_addr_task(
            endpoint.clone(),
            handler.clone(),
            local_url.clone(),
            space_relays.clone(),
            space_relay_state_changed.clone(),
        );

        let accept_task = Self::spawn_accept_task(
            endpoint.clone(),
            handler.clone(),
            connections.clone(),
            local_url.clone(),
            config.max_frame_bytes,
            space_relays.clone(),
        );

        let relay_lifecycle_task = relay_auth.map(|params| {
            Self::spawn_relay_lifecycle_task(
                endpoint.clone(),
                params,
                Duration::from_secs(config.relay_keepalive_interval_s as u64),
            )
        });

        Ok(Arc::new(Self {
            endpoint,
            handler,
            local_url,
            connections,
            connection_locks,
            watch_addr_task,
            accept_task,
            relay_lifecycle_task,
            space_relay_tasks: Arc::new(Mutex::new(HashMap::new())),
            config,
            space_relays,
            space_relay_state_changed,
        }))
    }

    /// Keep the endpoint public key registered with the bootstrap server's
    /// relay allowlist, re-authenticating on 401.
    async fn relay_keepalive(params: &Arc<RelayAuthParams>) -> K2Result<()> {
        let params = params.clone();
        tokio::task::spawn_blocking(move || {
            kitsune2_bootstrap_client::blocking_relay_keepalive(
                params.server_url.clone(),
                &params.auth_material,
                &params.key_bytes,
            )
        })
        .await
        .map_err(|e| K2Error::other_src("Registration task failed", e))?
    }

    /// Keep an authenticated relay configured, retrying temporary bootstrap,
    /// authentication, and relay failures without taking down the transport.
    fn spawn_relay_lifecycle_task(
        endpoint: DynIrohEndpoint,
        params: Arc<RelayAuthParams>,
        keepalive_interval: Duration,
    ) -> AbortHandle {
        tokio::spawn(async move {
            loop {
                let token = match Self::fetch_relay_token(&params).await {
                    Ok(token) => token,
                    Err(error) => {
                        debug!(?error, "Relay authentication unavailable");
                        tokio::time::sleep(Duration::from_secs(1)).await;
                        continue;
                    }
                };
                if let Err(error) = Self::relay_keepalive(&params).await {
                    debug!(?error, "Relay registration unavailable");
                    tokio::time::sleep(Duration::from_secs(1)).await;
                    continue;
                }

                let relay_url = params.relay_url.clone();
                endpoint
                    .insert_relay(
                        relay_url.clone(),
                        Self::relay_config_with_token(&relay_url, Some(&token)),
                    )
                    .await;
                endpoint.network_change().await;

                loop {
                    tokio::time::sleep(keepalive_interval).await;
                    if let Err(error) = Self::relay_keepalive(&params).await {
                        debug!(
                            ?error,
                            "Relay keepalive unavailable; re-authenticating"
                        );
                        break;
                    }
                }
            }
        })
        .abort_handle()
    }

    async fn report_space_transport_url(
        handler: &Arc<TxImpHnd>,
        space_relays: &SpaceRelays,
        space_id: &SpaceId,
        relay_url: &RelayUrl,
        local_url: Option<Url>,
    ) {
        let changed = {
            let mut space_relays = space_relays.write().expect("poison");
            let Some(relay_state) = space_relays.get_mut(space_id) else {
                return;
            };
            if !relay_urls_equal(&relay_state.relay_url, relay_url) {
                return;
            }
            if relay_state.local_url == local_url {
                false
            } else {
                relay_state.local_url = local_url.clone();
                true
            }
        };
        if changed {
            let state = match local_url {
                Some(url) => TransportUrl::Available(url),
                None => TransportUrl::Unavailable,
            };
            handler.transport_url_changed(state, Some(space_id)).await;
        }
    }

    fn set_space_relay_installed(
        space_relays: &SpaceRelays,
        space_id: &SpaceId,
        relay_url: &RelayUrl,
        installed: bool,
    ) -> bool {
        let mut space_relays = space_relays.write().expect("poison");
        let Some(relay_state) = space_relays.get_mut(space_id) else {
            return false;
        };
        if !relay_urls_equal(&relay_state.relay_url, relay_url)
            || relay_state.installed == installed
        {
            return false;
        }
        relay_state.installed = installed;
        true
    }

    fn spawn_space_relay_lifecycle_task(
        endpoint: DynIrohEndpoint,
        space_id: SpaceId,
        prepared: PreparedSpaceRelay,
        space_relays: SpaceRelays,
        space_relay_state_changed: Arc<tokio::sync::Notify>,
        keepalive_interval: Duration,
    ) -> AbortHandle {
        let PreparedSpaceRelay {
            relay_url,
            auth_params,
        } = prepared;
        tokio::spawn(async move {
            loop {
                let token = if let Some(params) = &auth_params {
                    let token = match Self::fetch_relay_token(params).await {
                        Ok(token) => token,
                        Err(error) => {
                            debug!(
                                ?space_id,
                                ?error,
                                "Per-space relay authentication unavailable"
                            );
                            tokio::time::sleep(Duration::from_secs(1)).await;
                            continue;
                        }
                    };
                    if let Err(error) = Self::relay_keepalive(params).await {
                        debug!(
                            ?space_id,
                            ?error,
                            "Per-space relay registration unavailable"
                        );
                        tokio::time::sleep(Duration::from_secs(1)).await;
                        continue;
                    }
                    Some(token)
                } else {
                    None
                };

                endpoint
                    .insert_relay(
                        relay_url.clone(),
                        Self::relay_config_with_token(
                            &relay_url,
                            token.as_deref(),
                        ),
                    )
                    .await;
                endpoint.network_change().await;

                if Self::set_space_relay_installed(
                    &space_relays,
                    &space_id,
                    &relay_url,
                    true,
                ) {
                    space_relay_state_changed.notify_one();
                }
                let Some(params) = &auth_params else {
                    return;
                };
                loop {
                    tokio::time::sleep(keepalive_interval).await;
                    if let Err(error) = Self::relay_keepalive(params).await {
                        debug!(
                            ?space_id,
                            ?error,
                            "Per-space relay keepalive unavailable; re-authenticating"
                        );
                        break;
                    }
                }
                if Self::set_space_relay_installed(
                    &space_relays,
                    &space_id,
                    &relay_url,
                    false,
                ) {
                    space_relay_state_changed.notify_one();
                }
            }
        })
        .abort_handle()
    }

    /// Authenticate against the bootstrap server and return the relay
    /// bearer token.
    async fn fetch_relay_token(
        params: &Arc<RelayAuthParams>,
    ) -> K2Result<String> {
        let params = params.clone();
        tokio::task::spawn_blocking(move || {
            kitsune2_bootstrap_client::blocking_fetch_relay_token(
                params.server_url.clone(),
                &params.auth_material,
            )
        })
        .await
        .map_err(|e| K2Error::other_src("Authentication task failed", e))?
    }

    /// Build a relay config, attaching the bearer token when provided.
    ///
    /// iroh sends the token as an `Authorization: Bearer` header on every
    /// relay WebSocket upgrade, so it is automatically re-presented on
    /// every reconnect.
    fn relay_config_with_token(
        relay_url: &RelayUrl,
        token: Option<&str>,
    ) -> Arc<RelayConfig> {
        let mut config = RelayConfig::from(relay_url.clone());
        if let Some(token) = token {
            config = config.with_auth_token(token);
        }
        Arc::new(config)
    }

    fn relay_is_available(
        relay_url: &RelayUrl,
        addr: &EndpointAddr,
        relay_statuses: Option<&endpoint::RelayStatuses>,
    ) -> bool {
        match relay_statuses {
            Some(statuses) => statuses.iter().any(|(url, connected)| {
                relay_urls_equal(url, relay_url) && *connected
            }),
            None => addr
                .relay_urls()
                .any(|url| relay_urls_equal(url, relay_url)),
        }
    }

    fn has_relay_connectivity(
        addr: &EndpointAddr,
        relay_statuses: Option<&endpoint::RelayStatuses>,
    ) -> bool {
        match relay_statuses {
            Some(statuses) => statuses.iter().any(|(_, connected)| *connected),
            None => addr.relay_urls().next().is_some(),
        }
    }

    fn space_relay_url(
        relay_state: &SpaceRelayState,
        addr: &EndpointAddr,
        relay_statuses: Option<&endpoint::RelayStatuses>,
    ) -> Option<Url> {
        if !relay_state.installed
            || !Self::has_relay_connectivity(addr, relay_statuses)
        {
            return None;
        }
        canonicalize_relay_url(&relay_state.relay_url, addr.id).ok()
    }

    async fn report_transport_urls(
        addr: &EndpointAddr,
        relay_statuses: Option<&endpoint::RelayStatuses>,
        handler: &Arc<TxImpHnd>,
        local_url: &Arc<RwLock<Option<Url>>>,
        space_relays: &SpaceRelays,
        previous_transport_url: &mut Option<TransportUrl>,
    ) {
        // Iroh currently advertises relay-backed URLs only while the relay is
        // usable. TransportUrl itself is route-agnostic: a transport with
        // another route may keep the same URL available without relay
        // connectivity.
        let next_url = get_url_with_first_relay(addr).filter(|url| {
            relay_url_from_peer_url(url).is_ok_and(|relay_url| {
                Self::relay_is_available(&relay_url, addr, relay_statuses)
            })
        });
        let state = next_url
            .clone()
            .map_or(TransportUrl::Unavailable, TransportUrl::Available);
        *local_url.write().expect("poisoned") = next_url;

        if previous_transport_url.as_ref() != Some(&state) {
            if let TransportUrl::Available(url) = &state {
                info!(?url, "Received a new listening URL from relay server");
            }
            *previous_transport_url = Some(state.clone());
            handler.transport_url_changed(state, None).await;
        }

        let configured_relays = space_relays.read().expect("poison").clone();
        for (space_id, relay_state) in configured_relays {
            // Iroh exposes status only for the selected home relay. Once the
            // endpoint is online, an installed non-home relay actor starts on
            // traffic, so each installed space relay is advertisable.
            let space_url =
                Self::space_relay_url(&relay_state, addr, relay_statuses);
            Self::report_space_transport_url(
                handler,
                space_relays,
                &space_id,
                &relay_state.relay_url,
                space_url,
            )
            .await;
        }
    }

    fn spawn_watch_addr_task(
        endpoint: DynIrohEndpoint,
        handler: Arc<TxImpHnd>,
        local_url: Arc<RwLock<Option<Url>>>,
        space_relays: SpaceRelays,
        space_relay_state_changed: Arc<tokio::sync::Notify>,
    ) -> AbortHandle {
        let mut addr_watcher = endpoint.watch_addr();
        let mut relay_watcher = endpoint.watch_relay_status();
        let mut addr = addr_watcher.get();
        let mut relay_statuses =
            relay_watcher.as_mut().map(|watcher| watcher.get());

        tokio::spawn(async move {
            let mut previous_transport_url = None;
            loop {
                Self::report_transport_urls(
                    &addr,
                    relay_statuses.as_ref(),
                    &handler,
                    &local_url,
                    &space_relays,
                    &mut previous_transport_url,
                )
                .await;

                enum Update {
                    Address(Result<EndpointAddr, n0_watcher::Disconnected>),
                    Relay(
                        Result<
                            endpoint::RelayStatuses,
                            n0_watcher::Disconnected,
                        >,
                    ),
                    SpaceRelayState,
                }

                let update = if let Some(relay_watcher) =
                    relay_watcher.as_mut()
                {
                    tokio::select! {
                        update = addr_watcher.updated() => Update::Address(update),
                        update = relay_watcher.updated() => Update::Relay(update),
                        _ = space_relay_state_changed.notified() => {
                            Update::SpaceRelayState
                        }
                    }
                } else {
                    tokio::select! {
                        update = addr_watcher.updated() => Update::Address(update),
                        _ = space_relay_state_changed.notified() => {
                            Update::SpaceRelayState
                        }
                    }
                };

                match update {
                    Update::Address(Ok(next_addr)) => addr = next_addr,
                    Update::Relay(Ok(next_statuses)) => {
                        relay_statuses = Some(next_statuses);
                    }
                    Update::SpaceRelayState => {}
                    Update::Address(Err(error))
                    | Update::Relay(Err(error)) => {
                        addr = EndpointAddr::from_parts(addr.id, Vec::new());
                        relay_statuses = relay_statuses.as_ref().map(|_| Vec::new());
                        Self::report_transport_urls(
                            &addr,
                            relay_statuses.as_ref(),
                            &handler,
                            &local_url,
                            &space_relays,
                            &mut previous_transport_url,
                        )
                        .await;
                        debug!(
                            ?error,
                            "Endpoint transport URL watcher unavailable"
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
        space_relays: SpaceRelays,
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
                            local_id: endpoint.id_bytes(),
                            dialed_by_us: false,
                            remote_url: None,
                            preflight_sent: false,
                            opened_at_s: conn_opened_at_s,
                            connections: connections.clone(),
                            local_url: local_url.clone(),
                            space_relays: space_relays.clone(),
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

    /// Choose which of our own URLs to advertise in a preflight to `peer_url`.
    ///
    /// If the peer is on one of our per-space relays, return our URL on
    /// that relay. If the peer is on our global relay, return our global
    /// URL. If the peer is on an unknown relay, return `None` — the
    /// preflight must fail rather than silently falling back to the wrong
    /// relay.
    pub(crate) fn own_url_for_preflight(
        peer_url: &Url,
        space_relays: &HashMap<SpaceId, SpaceRelayState>,
        global_url: &Option<Url>,
    ) -> Option<Url> {
        let peer_relay = match relay_url_from_peer_url(peer_url) {
            Ok(r) => r,
            Err(_) => {
                warn!(%peer_url, "Cannot extract relay from peer URL, failing preflight");
                return None;
            }
        };

        for relay_state in space_relays.values() {
            if relay_urls_equal(&relay_state.relay_url, &peer_relay)
                && let Some(url) = &relay_state.local_url
            {
                info!(
                    %peer_url,
                    own_url = %url,
                    "Using per-space URL for preflight"
                );
                return Some(url.clone());
            }
        }

        if let Some(global) = global_url
            && let Ok(our_relay) = relay_url_from_peer_url(global)
            && relay_urls_equal(&our_relay, &peer_relay)
        {
            return Some(global.clone());
        }

        warn!(
            %peer_url,
            %peer_relay,
            "Peer is on unknown relay, failing preflight"
        );
        None
    }

    /// Creates a new connection and its associated context for a peer.
    ///
    /// The connection is established and the preflight frame is sent. If this
    /// action succeeds, the context is returned. In case of error during the
    /// preflight, the context is dropped and an error returned.
    async fn create_connection_and_context(
        &self,
        target: EndpointAddr,
        remote_url: Url,
    ) -> K2Result<Arc<ConnectionContext>> {
        // Pick which of our URLs to advertise before opening the connection:
        // the per-space relay URL when the peer uses one of those relays, or
        // the global URL otherwise.
        let global_url = self.local_url.read().expect("poisoned").clone();
        let space_relays_snapshot =
            self.space_relays.read().expect("poisoned").clone();
        if Self::own_url_for_preflight(
            &remote_url,
            &space_relays_snapshot,
            &global_url,
        )
        .is_none()
        {
            return Err(K2Error::TransportUrlUnavailable);
        }
        debug!(?target, connect_timeout_s = self.config.connect_timeout_s, remote = ?remote_url.peer_id(), "Attempting QUIC connection");
        let start = Instant::now();
        let conn = match tokio::time::timeout(
            Duration::from_secs(self.config.connect_timeout_s as u64),
            self.endpoint.connect(target.clone(), ALPN),
        )
        .await
        {
            Err(e) => {
                let _ = self
                    .handler
                    .set_unresponsive(remote_url.clone(), Timestamp::now())
                    .await;

                Err(K2Error::other_src("iroh connect timed out", e))
            }
            Ok(Err(e)) => {
                let _ = self
                    .handler
                    .set_unresponsive(remote_url.clone(), Timestamp::now())
                    .await;

                Err(K2Error::other_src("iroh connect error", e))
            }
            Ok(Ok(conn)) => Ok(conn),
        }?;
        info!(remote = ?remote_url.peer_id(), direct = ?conn.is_direct(), duration = ?start.elapsed(), "Connection established");

        let global_url = self.local_url.read().expect("poison").clone();
        let space_relays_snapshot =
            self.space_relays.read().expect("poison").clone();
        Self::own_url_for_preflight(
            &remote_url,
            &space_relays_snapshot,
            &global_url,
        )
        .ok_or(K2Error::TransportUrlUnavailable)?;

        let conn_opened_at_s = SystemTime::UNIX_EPOCH
            .elapsed()
            .unwrap_or_else(|err| {
                warn!(?err, "Failed to get system time");
                Duration::from_secs(0)
            })
            .as_secs();
        let preflight_bytes =
            self.handler.peer_connect(remote_url.clone()).await?;

        let global_url = self.local_url.read().expect("poison").clone();

        let space_relays_snapshot =
            self.space_relays.read().expect("poison").clone();
        let current_local_url = Self::own_url_for_preflight(
            &remote_url,
            &space_relays_snapshot,
            &global_url,
        )
        .ok_or(K2Error::TransportUrlUnavailable)?;

        let ctx = ConnectionContext::new(ConnectionContextParams {
            handler: self.handler.clone(),
            connection: conn,
            local_id: self.endpoint.id_bytes(),
            dialed_by_us: true,
            remote_url: Some(remote_url.clone()),
            preflight_sent: true,
            opened_at_s: conn_opened_at_s,
            connections: self.connections.clone(),
            local_url: self.local_url.clone(),
            space_relays: self.space_relays.clone(),
            max_frame_bytes: self.config.max_frame_bytes,
        });

        if let Err(e) = ctx
            .send_preflight_frame(current_local_url, preflight_bytes)
            .await
        {
            let _ = self
                .handler
                .set_unresponsive(remote_url.clone(), Timestamp::now())
                .await;

            return Err(e);
        }

        Ok(ctx)
    }

    /// Validate and prepare a per-space relay without contacting it.
    fn prepare_space_relay(
        endpoint: &DynIrohEndpoint,
        relay_url: String,
        auth_material: Option<Vec<u8>>,
    ) -> K2Result<PreparedSpaceRelay> {
        let relay_url_str = if relay_url.ends_with('/') {
            relay_url
        } else {
            format!("{relay_url}/")
        };
        let relay_url = RelayUrl::from_str(&relay_url_str)
            .map_err(|error| K2Error::other_src("Invalid relay URL", error))?;
        let auth_params = if let Some(auth_material) = auth_material {
            let mut server_url =
                ::url::Url::parse(&relay_url_str).map_err(|error| {
                    K2Error::other_src(
                        "Invalid relay URL for authentication",
                        error,
                    )
                })?;
            server_url.set_path("/");
            Some(Arc::new(RelayAuthParams {
                server_url,
                auth_material: kitsune2_bootstrap_client::AuthMaterial::new(
                    auth_material,
                ),
                relay_url: relay_url.clone(),
                key_bytes: endpoint.id_bytes(),
            }))
        } else {
            None
        };
        Ok(PreparedSpaceRelay {
            relay_url,
            auth_params,
        })
    }
}

impl TxImp for IrohTransport {
    fn disconnect(
        &self,
        peer: Url,
        payload: Option<(String, Bytes)>,
    ) -> BoxFut<'_, ()> {
        if let Some(ctx) =
            self.connections.write().expect("poisoned").remove(&peer)
        {
            // The reason string travels in the QUIC application close frame
            // itself, so the encoded payload message is intentionally not
            // sent as a data frame first.
            let reason = payload
                .map(|(reason, _)| reason)
                .unwrap_or_else(|| "Disconnecting from remote".to_string());
            ctx.disconnect(close_code::CloseCode::Graceful, reason);
        }
        Box::pin(async {})
    }

    fn send(&self, remote_url: Url, data: Bytes) -> BoxFut<'_, K2Result<()>> {
        let connections = self.connections.clone();
        let connection_locks = self.connection_locks.clone();

        Box::pin(async move {
            let remote = match endpoint_from_url(&remote_url) {
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
                    let ctx = self
                        .create_connection_and_context(
                            remote,
                            remote_url.clone(),
                        )
                        .await?;

                    // Now that the preflight has been sent successfully, register
                    // the connection. This resolves any simultaneous-open race
                    // with an inbound connection from the same peer: if our dial
                    // lost the deterministic tie-break, close it and send over the
                    // connection that won instead.
                    if ctx.register_as_active(&connections, &remote_url) {
                        ctx
                    } else {
                        // Our dial lost the tie-break; discard it (its reader
                        // then exits quietly) and use the connection that won.
                        ctx.close_quietly();
                        connections
                            .read()
                            .expect("poisoned")
                            .get(&remote_url)
                            .cloned()
                            .unwrap_or(ctx)
                    }
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

    fn configure_for_space(
        &self,
        space_id: SpaceId,
        config: &Config,
    ) -> BoxFut<'_, K2Result<()>> {
        let per_space_config: Option<IrohTransportModConfig> =
            config.get_module_config().ok();

        let per_space = per_space_config.map(|c| c.iroh_transport);

        let space_relay = per_space_relay(
            per_space.as_ref().and_then(|c| c.relay_url.as_deref()),
            per_space
                .as_ref()
                .and_then(|c| c.auth_material_relay_base64.as_deref()),
            self.config.relay_url.as_deref(),
            self.config.auth_material_relay_base64.as_deref(),
        );

        let Some(SpaceRelay {
            url,
            auth_material_base64,
        }) = space_relay
        else {
            return Box::pin(async { Ok(()) });
        };
        let auth_material = auth_material_base64.and_then(|b64| {
            use ::base64::Engine;
            let decoded = ::base64::engine::general_purpose::STANDARD
                .decode(&b64)
                .ok();
            if decoded.is_none() {
                tracing::warn!(
                    ?space_id,
                    "Ignoring per-space relay auth material that is not valid \
                     base64; the relay will be used unauthenticated"
                );
            }
            decoded
        });
        let prepared =
            match Self::prepare_space_relay(&self.endpoint, url, auth_material)
            {
                Ok(prepared) => prepared,
                Err(error) => {
                    return Box::pin(async move { Err(error) });
                }
            };
        self.space_relays.write().expect("poisoned").insert(
            space_id.clone(),
            SpaceRelayState {
                relay_url: prepared.relay_url.clone(),
                local_url: None,
                installed: false,
            },
        );
        let unavailable = self
            .handler
            .transport_url_changed(TransportUrl::Unavailable, Some(&space_id));
        let task = Self::spawn_space_relay_lifecycle_task(
            self.endpoint.clone(),
            space_id.clone(),
            prepared,
            self.space_relays.clone(),
            self.space_relay_state_changed.clone(),
            Duration::from_secs(self.config.relay_keepalive_interval_s as u64),
        );
        if let Some(previous_task) = self
            .space_relay_tasks
            .lock()
            .expect("poisoned")
            .insert(space_id, task)
        {
            previous_task.abort();
        }
        Box::pin(async move {
            unavailable.await;
            Ok(())
        })
    }

    fn unconfigure_for_space(
        &self,
        space_id: SpaceId,
    ) -> BoxFut<'_, K2Result<()>> {
        let handler = self.handler.clone();
        let relay_task = self
            .space_relay_tasks
            .lock()
            .expect("poisoned")
            .remove(&space_id);

        Box::pin(async move {
            if let Some(relay_task) = relay_task {
                relay_task.abort();
            }
            let removed = self
                .space_relays
                .write()
                .expect("poisoned")
                .remove(&space_id);
            handler.use_global_transport_url_for_space(&space_id).await;

            if let Some(removed) = removed {
                let relay_url = removed.relay_url;
                let still_used = self
                    .space_relays
                    .read()
                    .expect("poisoned")
                    .values()
                    .any(|state| state.relay_url == relay_url);

                if !still_used {
                    // A space may have inserted the transport's own relay to
                    // present auth material of its own. It does not own it, so
                    // releasing the space must not take the relay with it.
                    if is_transport_own_relay(
                        relay_url.as_str(),
                        self.config.relay_url.as_deref(),
                    ) {
                        tracing::debug!(
                            ?space_id,
                            %relay_url,
                            "Keeping the transport's own relay after releasing the space"
                        );
                    } else {
                        self.endpoint.remove_relay(&relay_url).await;
                        tracing::info!(
                            ?space_id,
                            %relay_url,
                            "Removed per-space relay from endpoint"
                        );
                    }
                }
            }

            Ok(())
        })
    }
}
