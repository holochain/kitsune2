use super::utils::random_op_id;
use crate::{
    default_builder,
    factories::{
        core_fetch::{CoreFetch, CoreFetchConfig},
        Kitsune2MemoryOp, MemOpStoreFactory,
    },
};
use bytes::Bytes;
use kitsune2_api::{
    fetch::{serialize_op_ids, Ops},
    id::Id,
    transport::MockTransport,
    K2Error, MetaOp, OpId, SpaceId, Timestamp, Url,
};
use kitsune2_test_utils::enable_tracing;
use prost::Message;
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

type ResponsesSent = Vec<(Vec<Bytes>, Url)>;

const SPACE_ID: SpaceId = SpaceId(Id(Bytes::from_static(b"space_id")));

fn hash_op(input: &bytes::Bytes) -> OpId {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(input);
    let result = hasher.finalize();
    let hash_bytes = bytes::Bytes::from(result.to_vec());
    hash_bytes.into()
}

fn make_op(data: Vec<u8>) -> Kitsune2MemoryOp {
    let op_id = hash_op(&data.clone().into());
    Kitsune2MemoryOp::new(op_id, Timestamp::now(), data)
}

fn make_mock_transport(
    responses_sent: Arc<Mutex<ResponsesSent>>,
) -> Arc<MockTransport> {
    let mut mock_transport = MockTransport::new();
    mock_transport
        .expect_register_module_handler()
        .returning(|_, _, _| {});
    mock_transport.expect_send_module().returning({
        let responses_sent = responses_sent.clone();
        move |peer, space, module, data| {
            assert_eq!(space, SPACE_ID);
            assert_eq!(module, crate::factories::core_fetch::MOD_NAME);
            let ops = Ops::decode(data)
                .unwrap()
                .op_list
                .into_iter()
                .map(|op| op.data)
                .collect::<Vec<_>>();
            responses_sent.lock().unwrap().push((ops, peer));
            Box::pin(async move { Ok(()) })
        }
    });
    Arc::new(mock_transport)
}

#[tokio::test(flavor = "multi_thread")]
async fn respond_to_multiple_requests() {
    enable_tracing();
    let builder = Arc::new(default_builder().with_default_config().unwrap());
    let peer_store = builder.peer_store.create(builder.clone()).await.unwrap();
    let op_store = MemOpStoreFactory::create()
        .create(builder.clone(), SPACE_ID)
        .await
        .unwrap();
    let responses_sent = Arc::new(Mutex::new(Vec::new()));
    let mock_transport = make_mock_transport(responses_sent.clone());
    let config = CoreFetchConfig::default();

    let agent_url_1 = Url::from_str("wss://127.0.0.1:1").unwrap();
    let agent_url_2 = Url::from_str("wss://127.0.0.1:2").unwrap();

    // Insert ops to be read and sent into op store.
    let op_1 = make_op(vec![1; 128]);
    let op_2 = make_op(vec![2; 128]);
    let op_3 = make_op(vec![3; 128]);
    // Insert op 1, 2, 3 into op store. Op 4 will not be returned in the response.
    let stored_ops = vec![
        op_1.clone().into(),
        op_2.clone().into(),
        op_3.clone().into(),
    ];
    op_store.process_incoming_ops(stored_ops).await.unwrap();

    let fetch = CoreFetch::new(
        config.clone(),
        SPACE_ID,
        peer_store.clone(),
        op_store.clone(),
        mock_transport.clone(),
    );

    let requested_op_ids_1 =
        serialize_op_ids(vec![op_1.op_id.clone(), op_2.op_id.clone()]);
    let requested_op_ids_2 =
        serialize_op_ids(vec![op_3.op_id.clone(), random_op_id()]);
    fetch
        .response_handler
        .recv_module_msg(
            agent_url_1.clone(),
            SPACE_ID,
            crate::factories::core_fetch::MOD_NAME.to_string(),
            requested_op_ids_1.clone(),
        )
        .unwrap();
    fetch
        .response_handler
        .recv_module_msg(
            agent_url_2.clone(),
            SPACE_ID,
            crate::factories::core_fetch::MOD_NAME.to_string(),
            requested_op_ids_2.clone(),
        )
        .unwrap();

    tokio::time::timeout(Duration::from_millis(10), async {
        loop {
            tokio::time::sleep(Duration::from_millis(1)).await;
            if responses_sent.lock().unwrap().len() == 2 {
                break;
            }
        }
    })
    .await
    .unwrap();

    assert!(responses_sent
        .lock()
        .unwrap()
        .contains(&(vec![op_1.into(), op_2.into()], agent_url_1)));
    // Only op 3 is in op store.
    assert!(responses_sent
        .lock()
        .unwrap()
        .contains(&(vec![op_3.into()], agent_url_2)));
}

#[tokio::test(flavor = "multi_thread")]
async fn no_response_sent_when_no_ops_found() {
    let builder = Arc::new(default_builder().with_default_config().unwrap());
    let peer_store = builder.peer_store.create(builder.clone()).await.unwrap();
    let op_store = MemOpStoreFactory::create()
        .create(builder.clone(), SPACE_ID)
        .await
        .unwrap();
    let responses_sent = Arc::new(Mutex::new(Vec::new()));
    let mock_transport = make_mock_transport(responses_sent.clone());
    let config = CoreFetchConfig::default();

    let op_id_1 = random_op_id();
    let op_id_2 = random_op_id();
    let agent_url = Url::from_str("wss://127.0.0.1:1").unwrap();

    let fetch = CoreFetch::new(
        config.clone(),
        SPACE_ID,
        peer_store.clone(),
        op_store,
        mock_transport,
    );

    // Handle op request.
    let data = serialize_op_ids(vec![op_id_1, op_id_2]);
    fetch
        .response_handler
        .recv_module_msg(
            agent_url,
            SPACE_ID,
            crate::factories::core_fetch::MOD_NAME.to_string(),
            data,
        )
        .unwrap();

    tokio::time::sleep(Duration::from_millis(20)).await;

    assert!(responses_sent.lock().unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn fail_to_respond_once_then_succeed() {
    let builder = Arc::new(default_builder().with_default_config().unwrap());
    let peer_store = builder.peer_store.create(builder.clone()).await.unwrap();
    let op_store = MemOpStoreFactory::create()
        .create(builder.clone(), SPACE_ID)
        .await
        .unwrap();
    let responses_sent = Arc::new(Mutex::new(Vec::new()));
    let mut mock_transport = MockTransport::new();
    mock_transport
        .expect_register_module_handler()
        .returning(|_, _, _| ());
    mock_transport.expect_send_module().once().returning({
        move |_peer, space, module, _data| {
            assert_eq!(space, SPACE_ID);
            assert_eq!(module, crate::factories::core_fetch::MOD_NAME);
            Box::pin(async move { Err(K2Error::other("could not send ops")) })
        }
    });
    mock_transport.expect_send_module().once().returning({
        let responses_sent = responses_sent.clone();
        move |peer, space, module, data| {
            assert_eq!(space, SPACE_ID);
            assert_eq!(module, crate::factories::core_fetch::MOD_NAME);
            let ops = Ops::decode(data).unwrap();
            let ops = ops
                .op_list
                .into_iter()
                .map(|op| {
                    let op_data =
                        serde_json::from_slice::<Kitsune2MemoryOp>(&op.data)
                            .unwrap();
                    let op_id = hash_op(&bytes::Bytes::from(op_data.payload));
                    MetaOp {
                        op_id,
                        op_data: op.data.into(),
                    }
                })
                .collect::<Vec<_>>();
            responses_sent.lock().unwrap().push((ops, peer));
            Box::pin(async move { Ok(()) })
        }
    });
    let mock_transport = Arc::new(mock_transport);
    let config = CoreFetchConfig::default();

    let op = make_op(vec![1; 128]);
    op_store
        .process_incoming_ops(vec![op.clone().into()])
        .await
        .unwrap();
    let agent_url = Url::from_str("wss://127.0.0.1:1").unwrap();

    let fetch = CoreFetch::new(
        config.clone(),
        SPACE_ID,
        peer_store.clone(),
        op_store,
        mock_transport,
    );

    // Handle op request.
    let data = serialize_op_ids(vec![op.op_id]);
    fetch
        .response_handler
        .recv_module_msg(
            agent_url.clone(),
            SPACE_ID,
            crate::factories::core_fetch::MOD_NAME.to_string(),
            data.clone(),
        )
        .unwrap();

    tokio::time::sleep(Duration::from_millis(20)).await;

    // Send response should have failed.
    assert!(responses_sent.lock().unwrap().is_empty());

    // Request same op again.
    fetch
        .response_handler
        .recv_module_msg(
            agent_url.clone(),
            SPACE_ID,
            crate::factories::core_fetch::MOD_NAME.to_string(),
            data.clone(),
        )
        .unwrap();

    // Send should succeed after it failed at first.
    tokio::time::timeout(Duration::from_millis(20), async {
        loop {
            tokio::time::sleep(Duration::from_millis(1)).await;
            if !responses_sent.lock().unwrap().is_empty() {
                break;
            }
        }
    })
    .await
    .unwrap();
}
