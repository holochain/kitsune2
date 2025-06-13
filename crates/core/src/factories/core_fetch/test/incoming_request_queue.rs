use crate::{
    default_test_builder,
    factories::{
        core_fetch::{test::test_utils::make_op, CoreFetch, CoreFetchConfig},
        MemOpStoreFactory, MemoryOp,
    },
};
use bytes::Bytes;
use kitsune2_api::*;
use kitsune2_test_utils::{
    enable_tracing, id::random_op_id, iter_check, space::TEST_SPACE_ID,
};
use prost::Message;
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

type ResponsesSent = Vec<(Vec<Bytes>, Url)>;

struct TestCase {
    fetch: CoreFetch,
    op_store: DynOpStore,
    responses_sent: Arc<Mutex<ResponsesSent>>,
    _transport: DynTransport,
}

async fn setup_test() -> TestCase {
    let builder =
        Arc::new(default_test_builder().with_default_config().unwrap());
    let op_store = MemOpStoreFactory::create()
        .create(builder.clone(), TEST_SPACE_ID)
        .await
        .unwrap();
    let peer_meta_store = builder
        .peer_meta_store
        .create(builder.clone(), TEST_SPACE_ID)
        .await
        .unwrap();
    let responses_sent = Arc::new(Mutex::new(Vec::new()));
    let mock_transport = make_mock_transport(responses_sent.clone());
    let config = CoreFetchConfig::default();

    let fetch = CoreFetch::new(
        config.clone(),
        TEST_SPACE_ID,
        op_store.clone(),
        peer_meta_store,
        mock_transport.clone(),
    );

    TestCase {
        fetch,
        op_store,
        responses_sent,
        _transport: mock_transport,
    }
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
            assert_eq!(space, TEST_SPACE_ID);
            assert_eq!(module, crate::factories::core_fetch::MOD_NAME);
            let fetch_message = K2FetchMessage::decode(data).unwrap();
            let response = match fetch_message.fetch_message_type() {
                FetchMessageType::Response => {
                    FetchResponse::decode(fetch_message.data).unwrap()
                }
                _ => panic!("unexpected fetch message"),
            };
            let ops = response.ops.into_iter().map(|op| op.data).collect();
            responses_sent.lock().unwrap().push((ops, peer));
            Box::pin(async move { Ok(()) })
        }
    });
    Arc::new(mock_transport)
}

#[tokio::test(flavor = "multi_thread")]
async fn respond_to_multiple_requests() {
    enable_tracing();
    let TestCase {
        fetch,
        op_store,
        responses_sent,
        _transport,
        ..
    } = setup_test().await;

    let peer_url_1 = Url::from_str("wss://127.0.0.1:1").unwrap();
    let peer_url_2 = Url::from_str("wss://127.0.0.1:2").unwrap();

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

    // Receive incoming requests.
    let requested_op_ids_1 = serialize_request_message(vec![
        op_1.compute_op_id(),
        op_2.compute_op_id(),
    ]);
    let requested_op_ids_2 =
        serialize_request_message(vec![op_3.compute_op_id(), random_op_id()]);
    fetch
        .message_handler
        .recv_module_msg(
            peer_url_1.clone(),
            TEST_SPACE_ID,
            crate::factories::core_fetch::MOD_NAME.to_string(),
            requested_op_ids_1.clone(),
        )
        .unwrap();
    fetch
        .message_handler
        .recv_module_msg(
            peer_url_2.clone(),
            TEST_SPACE_ID,
            crate::factories::core_fetch::MOD_NAME.to_string(),
            requested_op_ids_2.clone(),
        )
        .unwrap();

    // Wait for responses to happen.
    iter_check!({
        if responses_sent.lock().unwrap().len() == 2 {
            break;
        }
    });

    assert!(responses_sent
        .lock()
        .unwrap()
        .contains(&(vec![op_1.into(), op_2.into()], peer_url_1)));
    // Only op 3 is in op store.
    assert!(responses_sent
        .lock()
        .unwrap()
        .contains(&(vec![op_3.into()], peer_url_2)));
}

#[tokio::test(flavor = "multi_thread")]
async fn no_response_sent_when_no_ops_found() {
    let TestCase {
        fetch,
        responses_sent,
        ..
    } = setup_test().await;

    let op_id_1 = random_op_id();
    let op_id_2 = random_op_id();
    let peer_url = Url::from_str("wss://127.0.0.1:1").unwrap();

    // Receive incoming request.
    let data = serialize_request_message(vec![op_id_1, op_id_2]);
    fetch
        .message_handler
        .recv_module_msg(
            peer_url,
            TEST_SPACE_ID,
            crate::factories::core_fetch::MOD_NAME.to_string(),
            data,
        )
        .unwrap();

    tokio::time::sleep(Duration::from_millis(100)).await;

    assert!(responses_sent.lock().unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn fail_to_respond_once_then_succeed() {
    enable_tracing();

    let builder =
        Arc::new(default_test_builder().with_default_config().unwrap());
    let op_store = builder
        .op_store
        .create(builder.clone(), TEST_SPACE_ID)
        .await
        .unwrap();
    let peer_meta_store = builder
        .peer_meta_store
        .create(builder.clone(), TEST_SPACE_ID)
        .await
        .unwrap();
    let responses_sent = Arc::new(Mutex::new(Vec::new()));
    let mut mock_transport = MockTransport::new();
    mock_transport
        .expect_register_module_handler()
        .returning(|_, _, _| ());
    mock_transport.expect_send_module().once().returning({
        move |_peer, space, module, _data| {
            assert_eq!(space, TEST_SPACE_ID);
            assert_eq!(module, crate::factories::core_fetch::MOD_NAME);
            Box::pin(async move { Err(K2Error::other("could not send ops")) })
        }
    });
    mock_transport.expect_send_module().once().returning({
        let responses_sent = responses_sent.clone();
        move |peer, space, module, data| {
            assert_eq!(space, TEST_SPACE_ID);
            assert_eq!(module, crate::factories::core_fetch::MOD_NAME);
            let response = FetchResponse::decode(
                K2FetchMessage::decode(data).unwrap().data,
            )
            .unwrap();
            let ops: Vec<MemoryOp> =
                response.ops.into_iter().map(Into::into).collect::<Vec<_>>();
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
    let peer_url = Url::from_str("wss://127.0.0.1:1").unwrap();

    let fetch = CoreFetch::new(
        config.clone(),
        TEST_SPACE_ID,
        op_store,
        peer_meta_store,
        mock_transport.clone(),
    );

    // Receive incoming request.
    let data = serialize_request_message(vec![op.compute_op_id()]);
    fetch
        .message_handler
        .recv_module_msg(
            peer_url.clone(),
            TEST_SPACE_ID,
            crate::factories::core_fetch::MOD_NAME.to_string(),
            data.clone(),
        )
        .unwrap();

    tokio::time::sleep(Duration::from_millis(30)).await;

    // Send response should have failed.
    assert!(responses_sent.lock().unwrap().is_empty());

    // Request same op again.
    fetch
        .message_handler
        .recv_module_msg(
            peer_url.clone(),
            TEST_SPACE_ID,
            crate::factories::core_fetch::MOD_NAME.to_string(),
            data.clone(),
        )
        .unwrap();

    // Send should succeed after it failed at first.
    iter_check!({
        if !responses_sent.lock().unwrap().is_empty() {
            break;
        }
    });
}
