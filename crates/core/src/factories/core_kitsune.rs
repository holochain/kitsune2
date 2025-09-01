//! The core kitsune implementation provided by Kitsune2.

use kitsune2_api::*;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};

/// The core kitsune implementation provided by Kitsune2.
/// You probably will have no reason to use something other than this.
/// This abstraction is mainly here for testing purposes.
#[derive(Debug)]
pub struct CoreKitsuneFactory {}

impl CoreKitsuneFactory {
    /// Construct a new CoreKitsuneFactory.
    pub fn create() -> DynKitsuneFactory {
        let out: DynKitsuneFactory = Arc::new(CoreKitsuneFactory {});
        out
    }
}

impl KitsuneFactory for CoreKitsuneFactory {
    fn default_config(&self, _config: &mut Config) -> K2Result<()> {
        Ok(())
    }

    fn validate_config(&self, _config: &Config) -> K2Result<()> {
        Ok(())
    }

    fn create(
        &self,
        builder: Arc<Builder>,
    ) -> BoxFut<'static, K2Result<DynKitsune>> {
        Box::pin(async move {
            let out: DynKitsune = Arc::new(CoreKitsune::new(builder));
            Ok(out)
        })
    }
}

#[derive(Debug)]
struct TxHandlerTranslator(DynKitsuneHandler);

impl TxBaseHandler for TxHandlerTranslator {
    fn new_listening_address(&self, this_url: Url) -> BoxFut<'static, ()> {
        self.0.new_listening_address(this_url)
    }

    fn peer_disconnect(&self, peer: Url, reason: Option<String>) {
        self.0.peer_disconnect(peer, reason);
    }
}

impl TxHandler for TxHandlerTranslator {
    fn preflight_gather_outgoing(
        &self,
        peer_url: Url,
    ) -> BoxFut<'_, K2Result<bytes::Bytes>> {
        self.0.preflight_gather_outgoing(peer_url)
    }

    fn preflight_validate_incoming(
        &self,
        peer_url: Url,
        data: bytes::Bytes,
    ) -> BoxFut<'_, K2Result<()>> {
        self.0.preflight_validate_incoming(peer_url, data)
    }
}

type SpaceFut = futures::future::Shared<BoxFut<'static, K2Result<DynSpace>>>;
type Map = HashMap<SpaceId, SpaceFut>;

#[derive(Debug)]
struct CoreKitsune {
    builder: Arc<Builder>,
    handler: std::sync::OnceLock<DynKitsuneHandler>,
    map: std::sync::Mutex<Map>,
    tx: std::sync::OnceLock<DynTransport>,
}

impl CoreKitsune {
    pub fn new(builder: Arc<Builder>) -> Self {
        Self {
            builder,
            handler: std::sync::OnceLock::new(),
            map: std::sync::Mutex::new(HashMap::new()),
            tx: std::sync::OnceLock::new(),
        }
    }
}

impl Kitsune for CoreKitsune {
    fn register_handler(
        &self,
        handler: DynKitsuneHandler,
    ) -> BoxFut<'_, K2Result<()>> {
        Box::pin(async move {
            const ERR: &str = "handler already registered";

            if self.handler.get().is_some() {
                return Err(K2Error::other(ERR));
            }

            let tx = self
                .builder
                .transport
                .create(
                    self.builder.clone(),
                    Arc::new(TxHandlerTranslator(handler.clone())),
                )
                .await?;

            self.handler.set(handler).map_err(|_| K2Error::other(ERR))?;
            self.tx.set(tx).map_err(|_| K2Error::other(ERR))?;

            Ok(())
        })
    }

    fn list_spaces(&self) -> Vec<SpaceId> {
        self.map.lock().unwrap().keys().cloned().collect()
    }

    fn space(&self, space_id: SpaceId) -> BoxFut<'_, K2Result<DynSpace>> {
        Box::pin(async move {
            const ERR: &str = "handler not registered";
            use std::collections::hash_map::Entry;

            // This is quick, we don't hold the lock very long,
            // because we're just constructing the future here,
            // not awaiting it.
            let fut = match self.map.lock().unwrap().entry(space_id.clone()) {
                Entry::Occupied(e) => e.get().clone(),
                Entry::Vacant(e) => {
                    let builder = self.builder.clone();
                    let handler = self
                        .handler
                        .get()
                        .ok_or_else(|| K2Error::other(ERR))?
                        .clone();
                    let tx = self
                        .tx
                        .get()
                        .ok_or_else(|| K2Error::other(ERR))?
                        .clone();
                    e.insert(futures::future::FutureExt::shared(Box::pin(
                        async move {
                            let sh =
                                handler.create_space(space_id.clone()).await?;
                            let s = builder
                                .space
                                .create(builder.clone(), sh, space_id, tx)
                                .await?;
                            Ok(s)
                        },
                    )))
                    .clone()
                }
            };

            fut.await
        })
    }

    fn space_if_exists(
        &self,
        space_id: SpaceId,
    ) -> BoxFut<'_, Option<DynSpace>> {
        Box::pin(async move {
            let fut = self.map.lock().unwrap().get(&space_id)?.clone();
            fut.await.ok()
        })
    }

    fn remove_space(&self, space_id: SpaceId) -> BoxFut<'_, K2Result<()>> {
        Box::pin(async move {
            let fut = self.map.lock().unwrap().get(&space_id).cloned();
            if let Some(fut) = fut {
                if let Ok(s) = fut.await {
                    // Just a point in time check, try to help the user out if they accidentally
                    // try to remove a space with active local agents.
                    // Those agents need to leave the space before it should be removed.
                    if !s.local_agent_store().get_all().await?.is_empty() {
                        return Err(K2Error::other(
                            "Cannot remove space with local agents",
                        ));
                    }

                    // Checks passed, remove our reference to the space.
                    self.map.lock().unwrap().remove(&space_id);

                    // Unregister the space and its module handlers from the transport.
                    self.transport().await?.unregister_space(space_id).await;

                    // Get all the peer URLs of connected peers.
                    let connected_peer_urls = Arc::new(RwLock::new(
                        self.transport()
                            .await?
                            .get_connected_peers()
                            .await?
                            .into_iter()
                            .collect::<HashSet<_>>(),
                    ));

                    // Remove any peer URLs that are connected for another space.
                    let spaces = self
                        .map
                        .lock()
                        .unwrap()
                        .iter()
                        .map(|s| s.1.clone())
                        .collect::<Vec<_>>();
                    futures::future::join_all(spaces.into_iter().map(
                        |fut| -> BoxFut<'_, K2Result<()>> {
                            let connected_peer_urls =
                                connected_peer_urls.clone();
                            Box::pin(async move {
                                if let Ok(s) = fut.await {
                                    let check_our_peers = connected_peer_urls
                                        .read()
                                        .unwrap()
                                        .clone();
                                    let space_agent_infos =
                                        s.peer_store().get_all().await?;

                                    let to_remove = space_agent_infos
                                        .into_iter()
                                        .filter_map(|a| {
                                            if let Some(url) = &a.url {
                                                check_our_peers
                                                    .contains(url)
                                                    .then_some(url.clone())
                                            } else {
                                                None
                                            }
                                        })
                                        .collect::<HashSet<_>>();

                                    connected_peer_urls
                                        .write()
                                        .unwrap()
                                        .retain(|u| !to_remove.contains(u));
                                }

                                Ok(())
                            })
                        },
                    ))
                    .await;

                    // Disconnect from any remaining peers. I.e. those that aren't in use by
                    // another space.
                    let transport = self.transport().await?;
                    let to_remove = connected_peer_urls.read().unwrap().clone();
                    for url in to_remove {
                        tracing::info!(
                            "Disconnecting peer {url} from transport"
                        );
                        transport
                            .disconnect(
                                url.clone(),
                                Some("Space is being removed".to_string()),
                            )
                            .await;
                    }
                } else {
                    tracing::warn!(
                        "Space exists, but failed to get a handle to it"
                    );
                }
            } else {
                tracing::info!("Space with id {space_id:?} not found");
            }
            Ok(())
        })
    }

    fn transport(&self) -> BoxFut<'_, K2Result<DynTransport>> {
        Box::pin(async move {
            self.tx
                .get()
                .cloned()
                .ok_or_else(|| K2Error::other("Transport not registered yet"))
        })
    }
}

#[cfg(test)]
mod test {
    use kitsune2_test_utils::space::TEST_SPACE_ID;

    #[tokio::test(flavor = "multi_thread")]
    async fn happy_space_construct() {
        use kitsune2_api::*;
        use std::sync::Arc;

        #[derive(Debug)]
        struct S;

        impl SpaceHandler for S {
            fn recv_notify(
                &self,
                _peer: Url,
                _space_id: SpaceId,
                _data: bytes::Bytes,
            ) -> K2Result<()> {
                // this test is a bit of a stub for now until we have the
                // transport module implemented and can send/receive messages.
                Ok(())
            }
        }

        #[derive(Debug)]
        struct K;

        impl KitsuneHandler for K {
            fn create_space(
                &self,
                _space_id: SpaceId,
            ) -> BoxFut<'_, K2Result<DynSpaceHandler>> {
                Box::pin(async move {
                    let s: DynSpaceHandler = Arc::new(S);
                    Ok(s)
                })
            }
        }

        let h: DynKitsuneHandler = Arc::new(K);

        let k = crate::default_test_builder()
            .with_default_config()
            .unwrap()
            .build()
            .await
            .unwrap();

        k.register_handler(h).await.unwrap();

        k.space(TEST_SPACE_ID).await.unwrap();
    }
}
