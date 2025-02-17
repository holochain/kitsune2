use std::sync::Arc;

use kitsune2_api::{
    DynOpStore, Publish, SpaceHandler, Timestamp, TxBaseHandler, TxHandler,
    TxSpaceHandler, Url,
};
use kitsune2_test_utils::{enable_tracing, iter_check, space::TEST_SPACE_ID};

use crate::{
    default_test_builder,
    factories::{core_publish::CorePublish, MemoryOp},
};

use super::CorePublishConfig;

async fn setup_test(
    config: &CorePublishConfig,
) -> (CorePublish, DynOpStore, Url) {
    let builder =
        Arc::new(default_test_builder().with_default_config().unwrap());

    let op_store = builder
        .op_store
        .create(builder.clone(), TEST_SPACE_ID)
        .await
        .unwrap();

    #[derive(Debug)]
    struct NoopHandler;
    impl TxBaseHandler for NoopHandler {}
    impl TxHandler for NoopHandler {}
    impl TxSpaceHandler for NoopHandler {}
    impl SpaceHandler for NoopHandler {}

    let transport = builder
        .transport
        .create(builder.clone(), Arc::new(NoopHandler))
        .await
        .unwrap();

    let fetch = builder
        .fetch
        .create(
            builder.clone(),
            TEST_SPACE_ID,
            op_store.clone(),
            transport.clone(),
        )
        .await
        .unwrap();

    let url =
        transport.register_space_handler(TEST_SPACE_ID, Arc::new(NoopHandler));

    (
        CorePublish::new(config.clone(), TEST_SPACE_ID, fetch, transport),
        op_store,
        url.unwrap(),
    )
}

#[tokio::test(flavor = "multi_thread")]
async fn published_ops_can_be_retrieved() {
    enable_tracing();

    let (core_publish_1, op_store_1, _) =
        setup_test(&CorePublishConfig::default()).await;
    let (_core_publish2, op_store_2, url_2) =
        setup_test(&CorePublishConfig::default()).await;

    let incoming_op_1 = MemoryOp::new(Timestamp::now(), vec![1]);
    let incoming_op_id_1 = incoming_op_1.compute_op_id();
    let incoming_op_2 = MemoryOp::new(Timestamp::now(), vec![2]);
    let incoming_op_id_2 = incoming_op_2.compute_op_id();

    op_store_1
        .process_incoming_ops(vec![incoming_op_1.into(), incoming_op_2.into()])
        .await
        .unwrap();

    core_publish_1
        .publish_ops(
            vec![incoming_op_id_1.clone(), incoming_op_id_2.clone()],
            url_2,
        )
        .await
        .unwrap();

    let ops = iter_check!(1000, {
        let ops = op_store_2
            .retrieve_ops(vec![
                incoming_op_id_1.clone(),
                incoming_op_id_2.clone(),
            ])
            .await
            .unwrap();

        if ops.len() == 2 {
            return ops;
        }
    });

    assert!(ops.len() == 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn publish_to_invalid_url_does_not_impede_subsequent_publishes() {
    enable_tracing();

    let (core_publish_1, op_store_1, _) =
        setup_test(&CorePublishConfig::default()).await;
    let (_core_publish2, op_store_2, url_2) =
        setup_test(&CorePublishConfig::default()).await;

    let incoming_op_1 = MemoryOp::new(Timestamp::now(), vec![1]);
    let incoming_op_id_1 = incoming_op_1.compute_op_id();
    let incoming_op_2 = MemoryOp::new(Timestamp::now(), vec![2]);
    let incoming_op_id_2 = incoming_op_2.compute_op_id();

    op_store_1
        .process_incoming_ops(vec![incoming_op_1.into(), incoming_op_2.into()])
        .await
        .unwrap();

    let op_ids = vec![incoming_op_id_1.clone(), incoming_op_id_2.clone()];

    // Publish to a non-existing Url
    core_publish_1
        .publish_ops(
            op_ids.clone(),
            Url::from_str("ws://notanexistingurl:80").unwrap(),
        )
        .await
        .unwrap();

    // Publish to an existing Url to verify that the prior publishing
    // to a non-existing Url did not cause an error that would
    // break the publishing flow
    core_publish_1.publish_ops(op_ids, url_2).await.unwrap();

    let ops = iter_check!(1000, {
        let ops = op_store_2
            .retrieve_ops(vec![
                incoming_op_id_1.clone(),
                incoming_op_id_2.clone(),
            ])
            .await
            .unwrap();

        if ops.len() == 2 {
            return ops;
        }
    });

    assert!(ops.len() == 2);
}
