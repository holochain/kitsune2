use super::*;
use crate::default_builder;
use kitsune2_api::transport::{TxBaseHandler, TxHandler};
use std::sync::Arc;

#[derive(Debug)]
struct NoopTxHandler;
impl TxBaseHandler for NoopTxHandler {}
impl TxHandler for NoopTxHandler {}

#[tokio::test]
async fn create_gossip_instance() {
    let factory = CoreGossipFactory::create();

    let builder = Arc::new(default_builder().with_default_config().unwrap());
    factory
        .create(
            builder.clone(),
            builder.peer_store.create(builder.clone()).await.unwrap(),
            builder
                .transport
                .create(builder.clone(), Arc::new(NoopTxHandler))
                .await
                .unwrap(),
        )
        .await
        .unwrap();
}
