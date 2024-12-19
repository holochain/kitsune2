//! The core kitsune implementation provided by Kitsune2.

use kitsune2_api::{config::*, kitsune::*, *};
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

    fn create(
        &self,
        builder: Arc<builder::Builder>,
        handler: DynKitsuneHandler,
    ) -> BoxFut<'static, K2Result<DynKitsune>> {
        Box::pin(async move {
            let out: DynKitsune =
                Arc::new(CoreKitsune::new(builder.clone(), handler));
            Ok(out)
        })
    }
}

type SpaceFut =
    futures::future::Shared<BoxFut<'static, K2Result<space::DynSpace>>>;
type Map = HashMap<SpaceId, SpaceFut>;

#[derive(Debug)]
struct CoreKitsune {
    builder: Arc<builder::Builder>,
    handler: DynKitsuneHandler,
    map: std::sync::Mutex<Map>,
}

impl CoreKitsune {
    pub fn new(
        builder: Arc<builder::Builder>,
        handler: DynKitsuneHandler,
    ) -> Self {
        Self {
            builder,
            handler,
            map: std::sync::Mutex::new(HashMap::new()),
        }
    }
}

impl Kitsune for CoreKitsune {
    fn space(&self, space: SpaceId) -> BoxFut<'_, K2Result<space::DynSpace>> {
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
                    e.insert(futures::future::FutureExt::shared(Box::pin(
                        async move {
                            let sh =
                                handler.create_space(space.clone()).await?;
                            let s = builder
                                .space
                                .create(builder.clone(), sh, space)
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
    #[tokio::test(flavor = "multi_thread")]
    async fn happy_space_construct() {
        use kitsune2_api::{kitsune::*, space::*, *};
        use std::sync::Arc;

        #[derive(Debug)]
        struct S;

        impl SpaceHandler for S {
            fn incoming_message(
                &self,
                _peer: AgentId,
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
            ) -> BoxFut<'_, K2Result<space::DynSpaceHandler>> {
                Box::pin(async move {
                    let s: DynSpaceHandler = Arc::new(S);
                    Ok(s)
                })
            }
        }

        let k: DynKitsuneHandler = Arc::new(K);

        let k = crate::default_builder()
            .with_default_config()
            .unwrap()
            .build(k)
            .await
            .unwrap();

        k.space(bytes::Bytes::from_static(b"space1").into())
            .await
            .unwrap();
    }
}
