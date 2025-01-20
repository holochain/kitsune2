//! The core stub transport implementation provided by Kitsune2.

use kitsune2_api::{config::*, transport::*, *};
use std::sync::Arc;

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
    fn default_config(&self, _config: &mut Config) -> K2Result<()> {
        Ok(())
    }

    fn create(
        &self,
        _builder: Arc<builder::Builder>,
        handler: DynTxHandler,
    ) -> BoxFut<'static, K2Result<DynTransport>> {
        Box::pin(async move {
            let handler = TxImpHnd::new(handler);
            let imp = Tx5Transport::create(handler.clone()).await?;
            Ok(DefaultTransport::create(&handler, imp))
        })
    }
}

#[derive(Debug)]
struct Tx5Transport {
    ep: tx5::Endpoint,
    evt_task: tokio::task::JoinHandle<()>,
}

impl Drop for Tx5Transport {
    fn drop(&mut self) {
        self.evt_task.abort();
    }
}

impl Tx5Transport {
    pub async fn create(handler: Arc<TxImpHnd>) -> K2Result<DynTxImp> {
        let config = Arc::new(tx5::Config {
            signal_allow_plain_text: true,
            backend_module: tx5::backend::BackendModule::LibDataChannel,
            backend_module_config: Some(
                tx5::backend::BackendModule::LibDataChannel.default_config(),
            ),
            ..Default::default()
        });

        let (ep, ep_recv) = tx5::Endpoint::new(config);

        if let Some(local_url) = ep
            .listen(
                // TODO from config
                tx5::SigUrl::parse("wss://sbd.holo.host").map_err(|e| {
                    K2Error::other_src("parsing tx5 sig url", e)
                })?,
            )
            .await
        {
            handler.new_listening_address(Url::from_str(local_url.as_ref())?);
        }

        let evt_task = tokio::task::spawn(evt_task(handler, ep_recv));

        let out: DynTxImp = Arc::new(Self { ep, evt_task });

        Ok(out)
    }
}

impl TxImp for Tx5Transport {
    fn url(&self) -> Option<Url> {
        self.ep
            .get_listening_addresses()
            .get(0)
            .map(|u| Url::from_str(u.as_ref()).ok())
            .flatten()
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
            let peer = tx5::PeerUrl::parse(&peer).map_err(|e| {
                K2Error::other_src("parsing peer url in tx5_transport send", e)
            })?;
            // TODO use bytes internally in tx5
            self.ep
                .send(peer, data.to_vec())
                .await
                .map_err(|e| K2Error::other_src("tx5 send error", e))
        })
    }
}

async fn evt_task(handler: Arc<TxImpHnd>, mut ep_recv: tx5::EndpointRecv) {
    use tx5::EndpointEvent::*;
    while let Some(evt) = ep_recv.recv().await {
        match evt {
            ListeningAddressOpen { local_url } => {
                let local_url = match Url::from_str(local_url.as_ref()) {
                    Ok(local_url) => local_url,
                    Err(err) => {
                        tracing::debug!(?err, "ignoring malformed local url");
                        continue;
                    }
                };
                handler.new_listening_address(local_url);
            }
            ListeningAddressClosed { local_url: _ } => {
                // we should probably tombstone our bootstrap entry here
            }
            Connected { peer_url: _ } => {
                // this is handled in our preflight hook, not here
            }
            Disconnected { peer_url } => {
                let peer_url = match Url::from_str(peer_url.as_ref()) {
                    Ok(peer_url) => peer_url,
                    Err(err) => {
                        tracing::debug!(?err, "ignoring malformed peer url");
                        continue;
                    }
                };
                handler.peer_disconnect(peer_url, None);
            }
            Message { peer_url, message } => {
                let peer_url = match Url::from_str(peer_url.as_ref()) {
                    Ok(peer_url) => peer_url,
                    Err(err) => {
                        // TODO - ban this connection
                        tracing::debug!(?err, "ignoring malformed peer url");
                        continue;
                    }
                };
                // TODO - retool tx5 to use bytes internally
                let message =
                    bytes::BytesMut::from(message.as_slice()).freeze();
                if let Err(err) = handler.recv_data(peer_url, message) {
                    // TODO - ban this connection
                    tracing::debug!(?err, "error in recv data handler");
                    continue;
                }
            }
        }
    }
}
