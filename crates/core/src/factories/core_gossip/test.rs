use super::*;
use crate::default_test_builder;
use kitsune2_api::space::SpaceHandler;
use kitsune2_api::transport::{TxBaseHandler, TxHandler};
use std::sync::Arc;

#[derive(Debug)]
struct NoopTxHandler;
impl TxBaseHandler for NoopTxHandler {}
impl TxHandler for NoopTxHandler {}

#[tokio::test]
async fn create_gossip_instance() {
    let factory = CoreGossipStubFactory::create();

    #[derive(Debug)]
    struct NoopHandler;
    impl SpaceHandler for NoopHandler {}

    let builder =
        Arc::new(default_test_builder().with_default_config().unwrap());
    let space_id = SpaceId::from(bytes::Bytes::from_static(b"test"));
    let tx = builder
        .transport
        .create(builder.clone(), Arc::new(NoopTxHandler))
        .await
        .unwrap();
    let peer_store = builder.peer_store.create(builder.clone()).await.unwrap();
    let op_store = builder
        .op_store
        .create(builder.clone(), space_id.clone())
        .await
        .unwrap();
    factory
        .create(
            builder.clone(),
            space_id.clone(),
            builder
                .space
                .create(
                    builder.clone(),
                    Arc::new(NoopHandler),
                    space_id.clone(),
                    tx.clone(),
                )
                .await
                .unwrap(),
            peer_store.clone(),
            builder
                .peer_meta_store
                .create(builder.clone())
                .await
                .unwrap(),
            op_store.clone(),
            tx.clone(),
            builder
                .fetch
                .create(
                    builder.clone(),
                    space_id.clone(),
                    op_store.clone(),
                    tx.clone(),
                )
                .await
                .unwrap(),
        )
        .await
        .unwrap();
}
