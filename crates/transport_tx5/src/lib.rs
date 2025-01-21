//! The core stub transport implementation provided by Kitsune2.

use kitsune2_api::{config::*, transport::*, *};
use std::sync::Arc;

trait PeerUrlExt {
    fn to_k(&self) -> K2Result<Url>;
}

impl PeerUrlExt for tx5::PeerUrl {
    fn to_k(&self) -> K2Result<Url> {
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

/// Tx5Transport configuration types.
pub mod config {
    /// Configuration parameters for [Tx5TransportFactory](super::Tx5TransportFactory).
    #[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct Tx5TransportConfig {
        /// The url of the sbd signal server. E.g. `wss://sbd.kitsu.ne`.
        pub server_url: String,
    }

    impl Default for Tx5TransportConfig {
        fn default() -> Self {
            Self {
                server_url: "<wss://your.sbd.url>".into(),
            }
        }
    }

    /// Module-level configuration for Tx5Transport.
    #[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct Tx5TransportModConfig {
        /// Tx5Transport configuration.
        pub tx5_transport: Tx5TransportConfig,
    }
}

use config::*;

/// The core stub transport implementation provided by Kitsune2.
/// This is NOT a production module. It is for testing only.
/// It will only establish "connections" within the same process.
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

    fn create(
        &self,
        builder: Arc<builder::Builder>,
        handler: DynTxHandler,
    ) -> BoxFut<'static, K2Result<DynTransport>> {
        Box::pin(async move {
            let config: Tx5TransportModConfig =
                builder.config.get_module_config()?;
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
    pre_task: tokio::task::JoinHandle<()>,
    evt_task: tokio::task::JoinHandle<()>,
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
            signal_allow_plain_text: true,
            preflight: Some((
                Arc::new(move |peer_url| {
                    let handler = preflight_send_handler.clone();
                    let peer_url = peer_url.to_k();
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
                    let peer_url = peer_url.clone();
                    let pre_send = pre_send.clone();
                    Box::pin(async move {
                        let (s, r) = tokio::sync::oneshot::channel();
                        pre_send
                            .try_send((peer_url, data, s))
                            .map_err(std::io::Error::other)?;
                        r.await.map_err(|_| {
                            std::io::Error::other("channel closed")
                        })?
                    })
                }),
            )),
            backend_module: tx5::backend::BackendModule::LibDataChannel,
            backend_module_config: Some(
                tx5::backend::BackendModule::LibDataChannel.default_config(),
            ),
            ..Default::default()
        });

        let (ep, ep_recv) = tx5::Endpoint::new(tx5_config);
        let ep = Arc::new(ep);

        if let Some(local_url) = ep
            .listen(
                tx5::SigUrl::parse(&config.server_url).map_err(|e| {
                    K2Error::other_src("parsing tx5 sig url", e)
                })?,
            )
            .await
        {
            handler.new_listening_address(Url::from_str(local_url.as_ref())?);
        }

        let pre_task = tokio::task::spawn(pre_task(handler.clone(), pre_recv));

        let evt_task = tokio::task::spawn(evt_task(handler, ep_recv));

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
}

fn handle_msg(
    handler: &TxImpHnd,
    peer_url: tx5::PeerUrl,
    message: Vec<u8>,
) -> K2Result<()> {
    let peer_url = match peer_url.to_k() {
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

async fn pre_task(handler: Arc<TxImpHnd>, mut pre_recv: PreCheckRecv) {
    while let Some((peer_url, message, resp)) = pre_recv.recv().await {
        let _ = resp.send(
            handle_msg(&handler, peer_url, message)
                .map_err(std::io::Error::other),
        );
    }
}

async fn evt_task(handler: Arc<TxImpHnd>, mut ep_recv: tx5::EndpointRecv) {
    use tx5::EndpointEvent::*;
    while let Some(evt) = ep_recv.recv().await {
        match evt {
            ListeningAddressOpen { local_url } => {
                let local_url = match local_url.to_k() {
                    Ok(local_url) => local_url,
                    Err(err) => {
                        tracing::debug!(?err, "ignoring malformed local url");
                        continue;
                    }
                };
                handler.new_listening_address(local_url);
            }
            ListeningAddressClosed { local_url: _ } => {
                // MABYE trigger tombstone of our bootstrap entry here
            }
            Connected { peer_url: _ } => {
                // This is handled in our preflight hook,
                // we can safely ignore this event.
            }
            Disconnected { peer_url } => {
                let peer_url = match peer_url.to_k() {
                    Ok(peer_url) => peer_url,
                    Err(err) => {
                        tracing::debug!(?err, "ignoring malformed peer url");
                        continue;
                    }
                };
                handler.peer_disconnect(peer_url, None);
            }
            Message { peer_url, message } => {
                if let Err(err) = handle_msg(&handler, peer_url, message) {
                    // TODO - ban the connection
                    tracing::debug!(?err);
                }
            }
        }
    }
}
