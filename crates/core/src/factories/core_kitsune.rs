//! The core kitsune implementation provided by Kitsune2.

use kitsune2_api::*;
use std::collections::HashMap;
use std::sync::Arc;

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
        handler: DynKitsuneHandler,
    ) -> BoxFut<'static, K2Result<DynKitsune>> {
        Box::pin(async move {
            let tx = builder
                .transport
                .create(
                    builder.clone(),
                    Arc::new(TxHandlerTranslator(handler.clone())),
                )
                .await?;
            let out: DynKitsune =
                Arc::new(CoreKitsune::new(builder.clone(), handler, tx));
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
    ) -> K2Result<bytes::Bytes> {
        self.0.preflight_gather_outgoing(peer_url)
    }

    fn preflight_validate_incoming(
        &self,
        peer_url: Url,
        data: bytes::Bytes,
    ) -> K2Result<()> {
        self.0.preflight_validate_incoming(peer_url, data)
    }
}

type SpaceFut = futures::future::Shared<BoxFut<'static, K2Result<DynSpace>>>;
type Map = HashMap<SpaceId, SpaceFut>;

#[derive(Debug)]
struct CoreKitsune {
    builder: Arc<Builder>,
    handler: DynKitsuneHandler,
    map: std::sync::Mutex<Map>,
    tx: DynTransport,
}

impl CoreKitsune {
    pub fn new(
        builder: Arc<Builder>,
        handler: DynKitsuneHandler,
        tx: DynTransport,
    ) -> Self {
        Self {
            builder,
            handler,
            map: std::sync::Mutex::new(HashMap::new()),
            tx,
        }
    }
}

impl Kitsune for CoreKitsune {
    fn space(&self, space: SpaceId) -> BoxFut<'_, K2Result<DynSpace>> {
        Box::pin(async move {
            use std::collections::hash_map::Entry;

            // This is quick, we don't hold the lock very long,
            // because we're just constructing the future here,
            // not awaiting it.
            let fut = match self.map.lock().unwrap().entry(space.clone()) {
                Entry::Occupied(e) => e.get().clone(),
                Entry::Vacant(e) => {
                    let builder = self.builder.clone();
                    let handler = self.handler.clone();
                    let tx = self.tx.clone();
                    e.insert(futures::future::FutureExt::shared(Box::pin(
                        async move {
                            let sh =
                                handler.create_space(space.clone()).await?;
                            let s = builder
                                .space
                                .create(builder.clone(), sh, space, tx)
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
                _to_agent: AgentId,
                _from_agent: AgentId,
                _space: SpaceId,
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
                _space: SpaceId,
            ) -> BoxFut<'_, K2Result<DynSpaceHandler>> {
                Box::pin(async move {
                    let s: DynSpaceHandler = Arc::new(S);
                    Ok(s)
                })
            }
        }

        let k: DynKitsuneHandler = Arc::new(K);

        let k = crate::default_test_builder()
            .with_default_config()
            .unwrap()
            .build(k)
            .await
            .unwrap();

        k.space(TEST_SPACE_ID).await.unwrap();
    }
}
