use super::*;
use crate::default_test_builder;
use kitsune2_api::transport::{TxBaseHandler, TxHandler};
use std::sync::Arc;

#[derive(Debug)]
struct NoopTxHandler;
impl TxBaseHandler for NoopTxHandler {}
impl TxHandler for NoopTxHandler {}

#[tokio::test]
async fn create_gossip_instance() {
    let factory = CoreGossipStubFactory::create();

    let builder =
        Arc::new(default_test_builder().with_default_config().unwrap());
    let space = SpaceId::from(bytes::Bytes::from_static(b"test"));
    factory
        .create(
            builder.clone(),
            space.clone(),
            builder.peer_store.create(builder.clone()).await.unwrap(),
            builder
                .op_store
                .create(builder.clone(), space)
                .await
                .unwrap(),
            builder
                .transport
                .create(builder.clone(), Arc::new(NoopTxHandler))
                .await
                .unwrap(),
        )
        .await
        .unwrap();
}
