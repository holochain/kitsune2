use std::sync::Arc;

use kitsune2_api::{
    DynOpStore, Publish, SpaceHandler, Timestamp, TxBaseHandler, TxHandler,
    TxSpaceHandler, Url,
};
use kitsune2_test_utils::{
    enable_tracing, id::create_op_id_list, space::TEST_SPACE_ID,
};

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

    let ops = tokio::time::timeout(
        std::time::Duration::from_millis(1000),
        async move {
            loop {
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
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
        },
    )
    .await
    .unwrap();

    assert!(ops.len() == 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn publish_to_invalid_url_does_not_error() {
    enable_tracing();

    let (core_publish_1, _, _) =
        setup_test(&CorePublishConfig::default()).await;

    let op_ids = create_op_id_list(2);

    core_publish_1
        .publish_ops(op_ids, Url::from_str("ws://notanexistingurl:80").unwrap())
        .await
        .unwrap();
}
