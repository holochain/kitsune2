#![deny(missing_docs)]
//! Kitsune2 transport implementation backed by iroh.

use bytes::Bytes;
use iroh::{
    endpoint::Connection, Endpoint, EndpointAddr, EndpointId, RelayMap,
    RelayMode, RelayUrl, TransportAddr, Watcher,
};
use kitsune2_api::*;
use std::{
    collections::HashMap,
    fmt,
    str::FromStr,
    sync::{Arc, Mutex, RwLock},
};
use tokio::task::AbortHandle;
use tracing::{error, warn};

#[cfg(test)]
mod test;

const ALPN: &[u8] = b"kitsune2/iroh/0";
const FRAME_HEADER_LEN: usize = 5;
const MAX_FRAME_BYTES: usize = 1024 * 1024;

/// IrohTransport configuration types
pub mod config {
    /// Configuration for the [`IrohTransportFactory`](super::IrohTransportFactory).
    #[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
    #[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
    #[serde(rename_all = "camelCase")]
    pub struct IrohTransportConfig {
        // /// Which relay mode should be used.
        // #[cfg_attr(feature = "schema", schemars(default))]
        // pub relay_mode: IrohRelayMode,
        /// Optional explicit relay URL to advertise to peers if the relay mode
        /// cannot provide one.
        #[cfg_attr(feature = "schema", schemars(default))]
        pub relay_url: Option<String>,
        /// Allow connecting to plaintext (http) relay server
        /// instead of the default requiring TLS (https).
        ///
        /// Default: false.
        #[cfg_attr(feature = "schema", schemars(default))]
        pub relay_allow_plain_text: bool,
        // /// Optional base32-encoded secret key to use for the local endpoint.
        // #[cfg_attr(feature = "schema", schemars(default))]
        // pub secret_key: Option<String>,
    }

    impl Default for IrohTransportConfig {
        fn default() -> Self {
            Self {
                // relay_mode: IrohRelayMode::default(),
                relay_url: None,
                relay_allow_plain_text: false,
                // secret_key: None,
            }
        }
    }

    /// Iroh relay mode selection
    // #[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
    // #[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
    // #[serde(rename_all = "camelCase")]
    // pub enum IrohRelayMode {
    //     /// Use the default production relays bundled with iroh.
    //     Default,
    //     /// Use the staging relays exposed by iroh.
    //     Staging,
    //     /// Do not use relays.
    //     Disabled,
    // }

    // impl Default for IrohRelayMode {
    //     fn default() -> Self {
    //         Self::Default
    //     }
    // }

    // impl IrohRelayMode {
    //     fn to_iroh_mode(&self) -> K2Result<RelayMode> {
    //         Ok(match self {
    //             IrohRelayMode::Default => endpoint::default_relay_mode(),
    //             IrohRelayMode::Staging => RelayMode::Staging,
    //             IrohRelayMode::Disabled => RelayMode::Disabled,
    //         })
    //     }
    // }

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
            let relay_url = url::Url::parse(relay)
                .map_err(|err| K2Error::other_src("invalid relay URL", err))?;
            let uses_tls = relay_url.scheme() == "https";
            if !&config.iroh_transport.relay_allow_plain_text && !uses_tls {
                return Err(K2Error::other(format!(
                    "disallowed plaintext relay url, either specify https or set relay_allow_plain_text to true: {relay_url}"
                )));
            }
        }

        // if let Some(secret) = &config.iroh_transport.secret_key {
        //     SecretKey::from_str(secret)
        //         .map_err(|err| K2Error::other_src("invalid secret key", err))?;
        // }
        // config.iroh_transport.relay_mode.to_iroh_mode()?;
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

#[derive(Debug)]
struct IrohTransport {
    endpoint: Arc<Endpoint>,
    handler: Arc<TxImpHnd>,
    _local_url: Arc<RwLock<Option<Url>>>,
    _connections: Arc<Mutex<HashMap<Url, Arc<ConnectionContext>>>>,
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
        println!("relay url config {:?}", config.relay_url);
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

        // if let Some(secret) = &config.secret_key {
        //     let secret = SecretKey::from_str(secret).map_err(|err| {
        //         K2Error::other_src("invalid iroh secret key", err)
        //     })?;
        //     builder = builder.secret_key(secret);
        // }

        let endpoint = builder.bind().await.map_err(|err| {
            K2Error::other_src("failed to bind iroh endpoint", err)
        })?;

        let endpoint = Arc::new(endpoint);
        let _local_url = Arc::new(RwLock::new(None));
        let _connections = Arc::new(Mutex::new(HashMap::new()));

        let watch_addr_task = Self::spawn_watch_addr_task(
            endpoint.clone(),
            handler.clone(),
            _local_url.clone(),
        );

        let accept_task = Self::spawn_accept_task(
            endpoint.clone(),
            handler.clone(),
            _connections.clone(),
        );

        let out: DynTxImp = Arc::new(Self {
            endpoint,
            handler,
            _local_url,
            _connections,
            watch_addr_task,
            accept_task,
        });
        Ok(out)
    }

    fn spawn_watch_addr_task(
        endpoint: Arc<Endpoint>,
        handler: Arc<TxImpHnd>,
        // override_relay: Option<String>,
        local_url: Arc<RwLock<Option<Url>>>,
        // net_stats: Arc<Mutex<TransportStats>>,
    ) -> AbortHandle {
        let mut watcher = endpoint.watch_addr();
        tokio::spawn(async move {
            loop {
                let addr = watcher.get();
                println!("watcher task ran {addr:?}");
                if let Some(url) = convert_endpoint_addr(&addr) {
                    println!("got new listening address {url:?}");
                    {
                        let mut guard = local_url.write().unwrap();
                        if guard.as_ref() != Some(&url) {
                            *guard = Some(url.clone());
                        }
                    }
                    handler.new_listening_address(url.clone()).await;
                }
                if watcher.updated().await.is_err() {
                    break;
                }
            }
        })
        .abort_handle()
    }

    fn spawn_accept_task(
        endpoint: Arc<Endpoint>,
        handler: Arc<TxImpHnd>,
        connections: Arc<Mutex<HashMap<Url, Arc<ConnectionContext>>>>,
    ) -> AbortHandle {
        tokio::spawn(async move {
            loop {
                match endpoint.accept().await {
                    Some(incoming) => match incoming.await {
                        Ok(conn) => {
                            println!(
                                "incoming connection from {:?}",
                                conn.remote_id()
                            );
                            let conn = Arc::new(conn);
                            let ctx = Arc::new(ConnectionContext::new(
                                handler.clone(),
                                conn.clone(),
                                true,
                                connections.clone(),
                            ));
                            spawn_connection_reader(conn, ctx);
                        }
                        Err(err) => {
                            warn!(?err, "iroh incoming connection failed");
                        }
                    },
                    None => break,
                }
                println!("spawn accept task");
            }
        })
        .abort_handle()
    }
}

impl TxImp for IrohTransport {
    fn url(&self) -> Option<Url> {
        None
        // self.local_url
        //     .try_read()
        //     .ok()
        //     .and_then(|guard| guard.clone())
    }

    fn disconnect(
        &self,
        _peer: Url,
        _payload: Option<(String, Bytes)>,
    ) -> BoxFut<'_, ()> {
        Box::pin(async {})
    }

    fn send(&self, peer: Url, data: Bytes) -> BoxFut<'_, K2Result<()>> {
        Box::pin(async move {
            let target = parse_peer_url(&peer).ok_or_else(|| {
                K2Error::other(format!("invalid peer url: {peer}"))
            })?;
            let conn = self
                .endpoint
                .connect(target.addr.clone(), ALPN)
                .await
                .map_err(|err| {
                K2Error::other_src("iroh connect failed", err)
            })?;
            let conn = Arc::new(conn);
            let ctx = Arc::new(ConnectionContext::with_remote(
                self.handler.clone(),
                conn.clone(),
                peer.clone(),
            ));
            // spawn_connection_reader(
            //     conn.clone(),
            //     ctx.clone(),
            //     self.local_url.clone(),
            // );
            // if let Some(local_url) = self.local_url.read().await.clone() {
            //     let payload =
            //         Bytes::copy_from_slice(local_url.as_str().as_bytes());
            //     send_frame(&conn, FrameType::PeerUrl, payload).await?;
            // }
            // let preflight = self.handler.peer_connect(peer.clone()).await?;
            // send_frame(&conn, FrameType::Payload, preflight).await?;
            // send_frame(&conn, FrameType::Payload, data).await?;
            // conn.close(0u32.into(), b"done");
            Ok(())
        })
    }

    fn get_connected_peers(&self) -> BoxFut<'_, K2Result<Vec<Url>>> {
        Box::pin(async { Ok(vec![]) })
    }

    fn dump_network_stats(&self) -> BoxFut<'_, K2Result<TransportStats>> {
        Box::pin(async move {
            Ok(TransportStats {
                backend: "iroh".to_string(),
                peer_urls: vec![],
                connections: vec![],
            })
        })
    }
}

fn convert_endpoint_addr(endpoint_addr: &EndpointAddr) -> Option<Url> {
    endpoint_addr.addrs.iter().find_map(|addr| match addr {
        TransportAddr::Relay(relay_url) => {
            Url::from_str(format!("https://{relay_url}/{}", endpoint_addr.id))
                .ok()
        }
        _ => None,
    })
}

fn relay_to_peer_url(relay: &RelayUrl, peer: EndpointId) -> Option<Url> {
    let relay: url::Url = relay.clone().into();
    let host = relay.host_str()?.trim_end_matches('.');
    let port = relay.port_or_known_default()?;
    let url = format!("https://{host}:{port}/{peer}");
    Url::from_str(&url).ok()
}

fn parse_peer_url(url: &Url) -> Option<ParsedPeer> {
    let raw = url::Url::parse(url.as_str()).ok()?;
    let peer_id = raw.path().trim_start_matches('/');
    if peer_id.is_empty() {
        return None;
    }
    let endpoint_id = EndpointId::from_str(peer_id).ok()?;
    let relay = url::Url::parse(&format!(
        "https://{}:{}/",
        raw.host_str()?,
        raw.port_or_known_default()?
    ))
    .ok()?;
    let relay = RelayUrl::from(relay);
    let addr =
        EndpointAddr::from_parts(endpoint_id, [TransportAddr::Relay(relay)]);
    Some(ParsedPeer { addr })
}

struct ParsedPeer {
    addr: EndpointAddr,
}

struct ConnectionContext {
    handler: Arc<TxImpHnd>,
    connection: Arc<Connection>,
    remote_url: RwLock<Option<Url>>,
    auto_preflight: bool,
    preflight_done: Mutex<bool>,
    connections: Arc<Mutex<HashMap<Url, Arc<ConnectionContext>>>>,
}

impl fmt::Debug for ConnectionContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ConnectionContext").finish()
    }
}

impl ConnectionContext {
    fn new(
        handler: Arc<TxImpHnd>,
        connection: Arc<Connection>,
        auto_preflight: bool,
        connections: Arc<Mutex<HashMap<Url, Arc<ConnectionContext>>>>,
    ) -> Self {
        Self {
            handler,
            connection,
            remote_url: RwLock::new(None),
            auto_preflight,
            preflight_done: Mutex::new(false),
            connections,
        }
    }

    fn with_remote(
        handler: Arc<TxImpHnd>,
        connection: Arc<Connection>,
        remote: Url,
    ) -> Self {
        Self {
            handler,
            connection,
            remote_url: RwLock::new(Some(remote)),
            auto_preflight: false,
            connections: Arc::new(Mutex::new(HashMap::new())),
            preflight_done: Mutex::new(false),
        }
    }

    async fn handle_peer_url(&self, peer: Url) -> K2Result<()> {
        {
            let mut lock = self.remote_url.write().unwrap();
            if lock.as_ref() == Some(&peer) {
                return Ok(());
            }
            *lock = Some(peer.clone());
        }
        // Add to connections map when we learn the peer URL
        {
            let mut conns = self.connections.lock().unwrap();
            if !conns.contains_key(&peer) {
                conns.insert(
                    peer.clone(),
                    Arc::new(Self {
                        handler: self.handler.clone(),
                        connection: self.connection.clone(),
                        remote_url: RwLock::new(Some(peer.clone())),
                        auto_preflight: self.auto_preflight,
                        preflight_done: Mutex::new(
                            *self.preflight_done.lock().unwrap(),
                        ),
                        connections: self.connections.clone(),
                    }),
                );
            }
        }
        if self.auto_preflight && !*self.preflight_done.lock().unwrap() {
            let preflight = self.handler.peer_connect(peer.clone()).await?;
            send_frame(&self.connection, FrameType::Payload, preflight).await?;
            *self.preflight_done.lock().unwrap() = true;
        }
        Ok(())
    }

    async fn remote(&self) -> Option<Url> {
        self.remote_url.read().unwrap().clone()
    }

    fn handler(&self) -> Arc<TxImpHnd> {
        self.handler.clone()
    }

    async fn notify_disconnect(&self) {
        if let Some(peer) = self.remote().await {
            self.handler.peer_disconnect(peer, None);
        }
    }
}

fn spawn_connection_reader(conn: Arc<Connection>, ctx: Arc<ConnectionContext>) {
    tokio::spawn(async move {
        // if let Some(url) = local_url.read().await.clone() {
        //     let payload = Bytes::copy_from_slice(url.as_str().as_bytes());
        //     let _ = send_frame(&conn, FrameType::PeerUrl, payload).await;
        // }
        loop {
            match conn.accept_uni().await {
                Ok(mut stream) => {
                    let ctx = ctx.clone();
                    tokio::spawn(async move {
                        let res: K2Result<()> = async {
                            let data = stream
                                .read_to_end(MAX_FRAME_BYTES)
                                .await
                                .map_err(|err| {
                                    K2Error::other_src(
                                        "failed to read iroh frame",
                                        err,
                                    )
                                })?;
                            if data.len() < FRAME_HEADER_LEN {
                                return Err(K2Error::other(
                                    "iroh frame shorter than header"
                                        .to_string(),
                                ));
                            }
                            let kind = FrameType::try_from(data[0])?;
                            let len = u32::from_be_bytes([
                                data[1], data[2], data[3], data[4],
                            ]) as usize;
                            if data.len() - FRAME_HEADER_LEN != len {
                                return Err(K2Error::other(
                                    "iroh frame payload length mismatch"
                                        .to_string(),
                                ));
                            }
                            let payload = &data[FRAME_HEADER_LEN..];
                            match kind {
                                FrameType::PeerUrl => {
                                    let url = Url::from_str(
                                        std::str::from_utf8(payload)
                                            .map_err(|err| {
                                                K2Error::other_src(
                                                    "invalid peer url payload",
                                                    err,
                                                )
                                            })?,
                                    )?;
                                    ctx.handle_peer_url(url).await
                                }
                                FrameType::Payload => {
                                    let peer =
                                        ctx.remote().await.ok_or_else(|| {
                                            K2Error::other(
                                                "received payload before peer url"
                                                    .to_string(),
                                            )
                                        })?;
                                    ctx.handler()
                                        .recv_data(
                                            peer,
                                            Bytes::copy_from_slice(payload),
                                        )
                                        .await
                                }
                            }
                        }
                        .await;
                        if let Err(err) = res {
                            warn!(?err, "iroh stream error");
                            if let Some(peer) = ctx.remote().await {
                                let _ = ctx
                                    .handler()
                                    .set_unresponsive(
                                        peer.clone(),
                                        Timestamp::now(),
                                    )
                                    .await;
                                ctx.handler().peer_disconnect(peer, None);
                            }
                        }
                    });
                }
                Err(err) => {
                    error!(?err, "iroh connection closed");
                    ctx.notify_disconnect().await;
                    break;
                }
            }
        }
    });
}

#[derive(Clone, Copy)]
enum FrameType {
    PeerUrl = 0,
    Payload = 1,
}

impl TryFrom<u8> for FrameType {
    type Error = K2Error;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(FrameType::PeerUrl),
            1 => Ok(FrameType::Payload),
            _ => {
                Err(K2Error::other(format!("unknown iroh frame type: {value}")))
            }
        }
    }
}

async fn send_frame(
    conn: &Arc<Connection>,
    ty: FrameType,
    data: Bytes,
) -> K2Result<()> {
    let mut stream = conn.open_uni().await.map_err(|err| {
        K2Error::other_src("failed to open iroh send stream", err)
    })?;
    if data.len() > MAX_FRAME_BYTES {
        return Err(K2Error::other("frame too large"));
    }
    let mut header = [0u8; FRAME_HEADER_LEN];
    header[0] = ty as u8;
    header[1..5].copy_from_slice(&(data.len() as u32).to_be_bytes());
    stream.write_all(&header).await.map_err(|err| {
        K2Error::other_src("failed to send frame header", err)
    })?;
    stream.write_all(&data).await.map_err(|err| {
        K2Error::other_src("failed to send frame payload", err)
    })?;
    stream.finish().map_err(|err| {
        K2Error::other_src("failed to finish iroh stream", err)
    })?;
    Ok(())
}
