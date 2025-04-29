#![deny(missing_docs)]
//! kitsune2 tx5 transport module.

use base64::Engine;
use kitsune2_api::*;
use std::sync::Arc;

trait PeerUrlExt {
    fn to_kitsune(&self) -> K2Result<Url>;
}

impl PeerUrlExt for tx5::PeerUrl {
    fn to_kitsune(&self) -> K2Result<Url> {
        Url::from_str(self.as_ref())
    }
}

trait UrlExt {
    fn to_peer_url(&self) -> K2Result<tx5::PeerUrl>;
}

impl UrlExt for Url {
    fn to_peer_url(&self) -> K2Result<tx5::PeerUrl> {
        tx5::PeerUrl::parse(self).map_err(|e| {
            K2Error::other_src("converting kitsune url to tx5 PeerUrl", e)
        })
    }
}

trait SigUrlExt {
    fn to_sig_url(&self) -> K2Result<tx5::SigUrl>;
}

impl SigUrlExt for &str {
    fn to_sig_url(&self) -> K2Result<tx5::SigUrl> {
        tx5::SigUrl::parse(self)
            .map_err(|e| K2Error::other_src("parsing tx5 sig url", e))
    }
}

/// Tx5Transport configuration types.
pub mod config {
    use tx5::WebRtcConfig;

    /// Configuration parameters for [Tx5TransportFactory](super::Tx5TransportFactory).
    #[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
    #[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
    #[serde(rename_all = "camelCase")]
    pub struct Tx5TransportConfig {
        /// Allow connecting to plaintext (ws) signal server
        /// instead of the default requiring TLS (wss).
        ///
        /// Default: false.
        #[cfg_attr(feature = "schema", schemars(default))]
        pub signal_allow_plain_text: bool,

        /// The url of the sbd signal server. E.g. `wss://sbd.kitsu.ne`.
        pub server_url: String,

        /// The internal time in seconds to use as a maximum for operations,
        /// connecting, and idleing.
        ///
        /// Default: 60s.
        #[cfg_attr(feature = "schema", schemars(default))]
        pub timeout_s: u32,

        /// WebRTC peer connection config.
        ///
        /// Configuration passed to the selected networking implementation.
        ///
        /// Although the default configuration for this is empty, and that is a valid configuration,
        /// it is recommended to provide ICE servers.
        ///
        /// Default: empty configuration
        #[cfg_attr(feature = "schema", schemars(default))]
        pub webrtc_config: WebRtcConfig,

        /// If true, tracing logs from the backend webrtc library will be
        /// included.
        #[cfg_attr(feature = "schema", schemars(default))]
        pub tracing_enabled: bool,

        /// The minimum ephemeral udp port to bind.
        #[cfg_attr(feature = "schema", schemars(default))]
        pub ephemeral_udp_port_min: Option<u16>,

        /// The maximum ephemeral udp port to bind.
        #[cfg_attr(feature = "schema", schemars(default))]
        pub ephemeral_udp_port_max: Option<u16>,
    }

    impl Default for Tx5TransportConfig {
        fn default() -> Self {
            Self {
                signal_allow_plain_text: false,
                server_url: "<wss://your.sbd.url>".into(),
                timeout_s: 60,
                webrtc_config: WebRtcConfig {
                    ice_servers: vec![],
                    ice_transport_policy: Default::default(),
                },
                tracing_enabled: false,
                ephemeral_udp_port_min: None,
                ephemeral_udp_port_max: None,
            }
        }
    }

    /// Module-level configuration for Tx5Transport.
    #[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
    #[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
    #[serde(rename_all = "camelCase")]
    pub struct Tx5TransportModConfig {
        /// Tx5Transport configuration.
        pub tx5_transport: Tx5TransportConfig,
    }
}

pub use config::*;
pub use tx5::{IceServers, WebRtcConfig};

/// Provides a Kitsune2 transport module based on the Tx5 crate.
#[derive(Debug)]
pub struct Tx5TransportFactory {}

impl Tx5TransportFactory {
    /// Construct a new Tx5TransportFactory.
    pub fn create() -> DynTransportFactory {
        let out: DynTransportFactory = Arc::new(Tx5TransportFactory {});
        out
    }
}

impl TransportFactory for Tx5TransportFactory {
    fn default_config(&self, config: &mut Config) -> K2Result<()> {
        config.set_module_config(&Tx5TransportModConfig::default())
    }

    fn validate_config(&self, config: &Config) -> K2Result<()> {
        let config: Tx5TransportModConfig = config.get_module_config()?;

        // make sure our signal server url is parse-able.
        let sig = config.tx5_transport.server_url.as_str().to_sig_url()?;
        let sig = url::Url::parse(&sig)
            .map_err(|err| K2Error::other_src("invalid tx5 server url", err))?;
        let uses_tls = sig.scheme() == "wss";

        if !uses_tls && !config.tx5_transport.signal_allow_plain_text {
            return Err(K2Error::other(format!(
                "disallowed plaintext signal url, either specify wss or set signal_allow_plain_text to true: {}",
                sig,
            )));
        }

        Ok(())
    }

    fn create(
        &self,
        builder: Arc<Builder>,
        handler: DynTxHandler,
    ) -> BoxFut<'static, K2Result<DynTransport>> {
        Box::pin(async move {
            let config: Tx5TransportModConfig =
                builder.config.get_module_config()?;

            let mut tx5_init_config = tx5_core::Tx5InitConfig {
                tracing_enabled: config.tx5_transport.tracing_enabled,
                ..Default::default()
            };

            if let Some(port) = config.tx5_transport.ephemeral_udp_port_min {
                tx5_init_config.ephemeral_udp_port_min = port;
            }

            if let Some(port) = config.tx5_transport.ephemeral_udp_port_max {
                tx5_init_config.ephemeral_udp_port_max = port;
            }

            // Ignore errors. Only the first call of this can succeed.
            let _ = tx5_init_config.set_as_global_default();

            let handler = TxImpHnd::new(handler);
            let imp =
                Tx5Transport::create(config.tx5_transport, handler.clone())
                    .await?;
            Ok(DefaultTransport::create(&handler, imp))
        })
    }
}

#[derive(Debug)]
struct Tx5Transport {
    ep: Arc<tx5::Endpoint>,
    pre_task: tokio::task::AbortHandle,
    evt_task: tokio::task::AbortHandle,
}

impl Drop for Tx5Transport {
    fn drop(&mut self) {
        self.pre_task.abort();
        self.evt_task.abort();
    }
}

type PreCheckResp = tokio::sync::oneshot::Sender<std::io::Result<()>>;
type PreCheck = (tx5::PeerUrl, Vec<u8>, PreCheckResp);
type PreCheckRecv = tokio::sync::mpsc::Receiver<PreCheck>;

impl Tx5Transport {
    pub async fn create(
        config: Tx5TransportConfig,
        handler: Arc<TxImpHnd>,
    ) -> K2Result<DynTxImp> {
        let (pre_send, pre_recv) = tokio::sync::mpsc::channel::<PreCheck>(1024);

        let preflight_send_handler = handler.clone();
        let tx5_config = Arc::new(tx5::Config {
            signal_allow_plain_text: config.signal_allow_plain_text,
            preflight: Some((
                Arc::new(move |peer_url| {
                    // gather any preflight data, and send to remote
                    let handler = preflight_send_handler.clone();
                    let peer_url = peer_url.to_kitsune();
                    Box::pin(async move {
                        let peer_url =
                            peer_url.map_err(std::io::Error::other)?;
                        let data = handler
                            .peer_connect(peer_url)
                            .map_err(std::io::Error::other)?;
                        Ok(data.to_vec())
                    })
                }),
                Arc::new(move |peer_url, data| {
                    // process sent preflight data
                    let peer_url = peer_url.clone();
                    let pre_send = pre_send.clone();
                    Box::pin(async move {
                        let (s, r) = tokio::sync::oneshot::channel();
                        // kitsune2 expects this to be sent in the normal
                        // "recv_data" handler, so we need another task
                        // to forward that.
                        //
                        // If the app is too slow processing incoming
                        // preflights, reject it to close the connection.
                        pre_send.try_send((peer_url, data, s)).map_err(
                            |_| std::io::Error::other("app overloaded"),
                        )?;
                        r.await.map_err(|_| {
                            std::io::Error::other("channel closed")
                        })?
                    })
                }),
            )),
            timeout: std::time::Duration::from_secs(config.timeout_s as u64),
            backend_module: tx5::backend::BackendModule::LibDataChannel,
            backend_module_config: Some(
                tx5::backend::BackendModule::LibDataChannel.default_config(),
            ),
            initial_webrtc_config: config.webrtc_config,
            ..Default::default()
        });

        let (ep, ep_recv) = tx5::Endpoint::new(tx5_config);
        let ep = Arc::new(ep);

        if let Some(local_url) =
            ep.listen(config.server_url.as_str().to_sig_url()?).await
        {
            handler
                .new_listening_address(Url::from_str(local_url.as_ref())?)
                .await;
        }

        let pre_task = tokio::task::spawn(pre_task(handler.clone(), pre_recv))
            .abort_handle();

        let evt_task =
            tokio::task::spawn(evt_task(handler, ep.clone(), ep_recv))
                .abort_handle();

        let out: DynTxImp = Arc::new(Self {
            ep,
            pre_task,
            evt_task,
        });

        Ok(out)
    }
}

impl TxImp for Tx5Transport {
    fn url(&self) -> Option<Url> {
        self.ep
            .get_listening_addresses()
            .first()
            .and_then(|u| Url::from_str(u.as_ref()).ok())
    }

    fn disconnect(
        &self,
        peer: Url,
        _payload: Option<(String, bytes::Bytes)>,
    ) -> BoxFut<'_, ()> {
        Box::pin(async move {
            if let Ok(peer) = tx5::PeerUrl::parse(&peer) {
                self.ep.close(&peer);
            }
        })
    }

    fn send(&self, peer: Url, data: bytes::Bytes) -> BoxFut<'_, K2Result<()>> {
        Box::pin(async move {
            let peer = peer.to_peer_url()?;
            // this would be more efficient if we retool tx5 to use bytes
            self.ep
                .send(peer, data.to_vec())
                .await
                .map_err(|e| K2Error::other_src("tx5 send error", e))
        })
    }

    fn dump_network_stats(&self) -> BoxFut<'_, K2Result<TransportStats>> {
        Box::pin(async move {
            let tx5_stats = self.ep.get_stats();

            Ok(TransportStats {
                backend: format!("{:?}", tx5_stats.backend),
                peer_urls: tx5_stats
                    .peer_url_list
                    .into_iter()
                    .map(|url| url.to_kitsune())
                    .collect::<K2Result<Vec<_>>>()?,
                connections: tx5_stats
                    .connection_list
                    .into_iter()
                    .map(|conn| TransportConnectionStats {
                        pub_key: base64::prelude::BASE64_URL_SAFE_NO_PAD
                            .encode(conn.pub_key),
                        send_message_count: conn.send_message_count,
                        send_bytes: conn.send_bytes,
                        recv_message_count: conn.recv_message_count,
                        recv_bytes: conn.recv_bytes,
                        opened_at_s: conn.opened_at_s,
                        is_webrtc: conn.is_webrtc,
                    })
                    .collect(),
            })
        })
    }
}

fn handle_msg(
    handler: &TxImpHnd,
    peer_url: tx5::PeerUrl,
    message: Vec<u8>,
) -> K2Result<()> {
    let peer_url = match peer_url.to_kitsune() {
        Ok(peer_url) => peer_url,
        Err(err) => {
            return Err(K2Error::other_src("malformed peer url", err));
        }
    };
    // this would be more efficient if we retool tx5 to use bytes internally
    let message = bytes::BytesMut::from(message.as_slice()).freeze();
    if let Err(err) = handler.recv_data(peer_url, message) {
        return Err(K2Error::other_src("error in recv data handler", err));
    }
    Ok(())
}

struct TaskDrop(&'static str);

impl Drop for TaskDrop {
    fn drop(&mut self) {
        tracing::error!(task = %self.0, "Task Ended");
    }
}

async fn pre_task(handler: Arc<TxImpHnd>, mut pre_recv: PreCheckRecv) {
    let _drop = TaskDrop("pre_task");
    while let Some((peer_url, message, resp)) = pre_recv.recv().await {
        let _ = resp.send(
            handle_msg(&handler, peer_url, message)
                .map_err(std::io::Error::other),
        );
    }
}

async fn evt_task(
    handler: Arc<TxImpHnd>,
    ep: Arc<tx5::Endpoint>,
    mut ep_recv: tx5::EndpointRecv,
) {
    let _drop = TaskDrop("evt_task");
    use tx5::EndpointEvent::*;
    while let Some(evt) = ep_recv.recv().await {
        match evt {
            ListeningAddressOpen { local_url } => {
                let local_url = match local_url.to_kitsune() {
                    Ok(local_url) => local_url,
                    Err(err) => {
                        tracing::debug!(?err, "ignoring malformed local url");
                        continue;
                    }
                };
                handler.new_listening_address(local_url).await;
            }
            ListeningAddressClosed { local_url: _ } => {
                // MAYBE trigger tombstone of our bootstrap entry here
            }
            Connected { peer_url: _ } => {
                // This is handled in our preflight hook,
                // we can safely ignore this event.
            }
            Disconnected { peer_url } => {
                let peer_url = match peer_url.to_kitsune() {
                    Ok(peer_url) => peer_url,
                    Err(err) => {
                        tracing::debug!(?err, "ignoring malformed peer url");
                        continue;
                    }
                };
                handler.peer_disconnect(peer_url, None);
            }
            Message { peer_url, message } => {
                if let Err(err) =
                    handle_msg(&handler, peer_url.clone(), message)
                {
                    ep.close(&peer_url);
                    tracing::debug!(?err);
                }
            }
        }
    }
}

#[cfg(test)]
mod test;
