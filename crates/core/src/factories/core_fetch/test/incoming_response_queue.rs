use super::utils::{random_agent_id, random_op_id};
use crate::{
    default_test_builder,
    factories::{
        core_fetch::{CoreFetch, CoreFetchConfig},
        MemOpStoreFactory, MemoryOp,
    },
};
use kitsune2_api::{
    fetch::{
        k2_fetch_message::FetchMessageType, serialize_response_message, Fetch,
        FetchRequest, K2FetchMessage,
    },
    peer_store::DynPeerStore,
    transport::{DynTransport, MockTransport},
    DynOpStore, K2Error, MetaOp, MockOpStore, OpId, Timestamp, Url,
};
use kitsune2_test_utils::{
    agent::{AgentBuilder, TestLocalAgent},
    space::TEST_SPACE_ID,
};
use prost::Message;
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

type RequestsSent = Vec<(OpId, Url)>;

struct TestCase {
    fetch: CoreFetch,
    peer_store: DynPeerStore,
    op_store: DynOpStore,
    requests_sent: Arc<Mutex<RequestsSent>>,
}

async fn setup_test() -> TestCase {
    let builder =
        Arc::new(default_test_builder().with_default_config().unwrap());
    let peer_store = builder.peer_store.create(builder.clone()).await.unwrap();
    let requests_sent = Arc::new(Mutex::new(Vec::new()));
    let mock_transport = create_mock_transport(requests_sent.clone());
    let op_store = MemOpStoreFactory::create()
        .create(builder, TEST_SPACE_ID)
        .await
        .unwrap();

    let fetch = CoreFetch::new(
        CoreFetchConfig::default(),
        TEST_SPACE_ID.clone(),
        peer_store.clone(),
        op_store.clone(),
        mock_transport,
    );

    TestCase {
        fetch,
        peer_store,
        op_store,
        requests_sent,
    }
}

fn create_mock_transport(
    requests_sent: Arc<Mutex<RequestsSent>>,
) -> DynTransport {
    let mut mock_transport = MockTransport::new();
    mock_transport
        .expect_register_module_handler()
        .returning(|_, _, _| ());
    mock_transport.expect_send_module().returning({
        move |peer, space, module, data| {
            assert_eq!(space, TEST_SPACE_ID);
            assert_eq!(module, crate::factories::core_fetch::MOD_NAME);
            let fetch_message = K2FetchMessage::decode(data).unwrap();
            match FetchMessageType::try_from(fetch_message.fetch_message_type) {
                Ok(FetchMessageType::Request) => {
                    let request =
                        FetchRequest::decode(fetch_message.data).unwrap();
                    let mut lock = requests_sent.lock().unwrap();
                    request.op_ids.into_iter().for_each(|op_id| {
                        lock.push((OpId::from(op_id), peer.clone()))
                    });
                }
                _ => unimplemented!(),
            }

            Box::pin(async { Ok(()) })
        }
    });
    Arc::new(mock_transport)
}

#[tokio::test(flavor = "multi_thread")]
async fn incoming_ops_are_written_to_op_store() {
    let TestCase {
        fetch, op_store, ..
    } = setup_test().await;

    let incoming_op_1 = MemoryOp::new(Timestamp::now(), vec![1]);
    let incoming_op_id_1 = incoming_op_1.compute_op_id();
    let incoming_op_2 = MemoryOp::new(Timestamp::now(), vec![2]);
    let incoming_op_id_2 = incoming_op_2.compute_op_id();

    let incoming_op_ids =
        vec![incoming_op_id_1.clone(), incoming_op_id_2.clone()];
    let stored_ops = op_store
        .retrieve_ops(incoming_op_ids.clone())
        .await
        .unwrap();
    assert!(stored_ops.is_empty());

    // Receive incoming ops.
    let fetch_message = serialize_response_message(vec![
        incoming_op_1.clone().into(),
        incoming_op_2.clone().into(),
    ]);

    fetch
        .message_handler
        .recv_module_msg(
            Url::from_str("ws://127.0.0.1:1").unwrap(),
            TEST_SPACE_ID,
            crate::factories::core_fetch::MOD_NAME.to_string(),
            fetch_message,
        )
        .unwrap();

    let expected_stored_ops = vec![incoming_op_1, incoming_op_2]
        .into_iter()
        .map(|op| MetaOp {
            op_id: op.compute_op_id(),
            op_data: op.into(),
        })
        .collect::<Vec<_>>();
    tokio::time::timeout(Duration::from_millis(30), async {
        loop {
            tokio::time::sleep(Duration::from_millis(1)).await;
            let stored_ops = op_store
                .retrieve_ops(incoming_op_ids.clone())
                .await
                .unwrap();
            if stored_ops.len() == 2 {
                assert_eq!(expected_stored_ops, stored_ops);
                break;
            }
        }
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn requests_for_received_ops_are_removed_from_state() {
    let TestCase {
        fetch,
        peer_store,
        requests_sent,
        ..
    } = setup_test().await;

    let agent_id = random_agent_id();
    let agent_info = AgentBuilder {
        agent: Some(agent_id.clone()),
        url: Some(Some(Url::from_str("wss://127.0.0.1:1").unwrap())),
        ..Default::default()
    }
    .build(TestLocalAgent::default());
    let agent_url = agent_info.url.clone().unwrap();
    let another_agent_id = random_agent_id();
    let another_agent_info = AgentBuilder {
        agent: Some(another_agent_id.clone()),
        url: Some(Some(Url::from_str("wss://127.0.0.1:2").unwrap())),
        ..Default::default()
    }
    .build(TestLocalAgent::default());
    peer_store
        .insert(vec![agent_info, another_agent_info])
        .await
        .unwrap();

    // Add 1 op that'll be removed and another op that won't be.
    let incoming_op = MemoryOp::new(Timestamp::now(), vec![1]);
    let incoming_op_id = incoming_op.compute_op_id();
    let another_op_id = random_op_id();

    futures::future::join_all([
        fetch.request_ops(
            vec![incoming_op_id.clone(), another_op_id.clone()],
            agent_id.clone(),
        ),
        // Add the same op twice with different agent ids.
        fetch.request_ops(vec![incoming_op_id.clone()], another_agent_id),
    ])
    .await;

    assert_eq!(fetch.state.lock().unwrap().requests.len(), 3);

    // Wait for a request to be sent.
    tokio::time::timeout(Duration::from_millis(20), async {
        loop {
            tokio::time::sleep(Duration::from_millis(1)).await;
            if !requests_sent.lock().unwrap().is_empty() {
                break;
            }
        }
    })
    .await
    .unwrap();

    // Receive incoming ops.
    let fetch_message =
        serialize_response_message(vec![incoming_op.clone().into()]);
    fetch
        .message_handler
        .recv_module_msg(
            agent_url,
            TEST_SPACE_ID,
            crate::factories::core_fetch::MOD_NAME.to_string(),
            fetch_message,
        )
        .unwrap();

    // Wait for op ids that were processed by the op store to be removed from state.
    tokio::time::timeout(Duration::from_millis(30), async {
        loop {
            tokio::time::sleep(Duration::from_millis(1)).await;
            let fetch_state = fetch.state.lock().unwrap();
            if !fetch_state
                .requests
                .contains(&(incoming_op_id.clone(), agent_id.clone()))
            {
                // Only 1 request of another op should remain.
                assert_eq!(fetch_state.requests.len(), 1);
                assert!(fetch_state
                    .requests
                    .contains(&(another_op_id, agent_id)));
                break;
            }
        }
    })
    .await
    .unwrap();

    // Record how many requests have been sent so far for the incoming op.
    let requests_sent_for_fetched_op = requests_sent
        .lock()
        .unwrap()
        .iter()
        .filter(|(op_id, _)| *op_id == incoming_op_id)
        .count();

    // Let some time pass for more requests to be sent.
    tokio::time::sleep(Duration::from_millis(10)).await;

    // Check that no further requests have been sent for the removed op id.
    assert_eq!(
        requests_sent
            .lock()
            .unwrap()
            .iter()
            .filter(|(op_id, _)| *op_id == incoming_op_id)
            .count(),
        requests_sent_for_fetched_op
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn op_ids_are_not_removed_when_storing_op_failed() {
    let builder =
        Arc::new(default_test_builder().with_default_config().unwrap());
    let peer_store = builder.peer_store.create(builder.clone()).await.unwrap();
    let requests_sent = Arc::new(Mutex::new(Vec::new()));
    let mock_transport = create_mock_transport(requests_sent.clone());
    let mut op_store = MockOpStore::new();
    op_store.expect_process_incoming_ops().returning(|_| {
        Box::pin(async { Err(K2Error::other("couldn't store ops")) })
    });
    let op_store = Arc::new(op_store);

    let fetch = CoreFetch::new(
        CoreFetchConfig::default(),
        TEST_SPACE_ID.clone(),
        peer_store.clone(),
        op_store,
        mock_transport,
    );

    let agent_id = random_agent_id();
    let agent_info = AgentBuilder {
        agent: Some(agent_id.clone()),
        url: Some(Some(Url::from_str("wss://127.0.0.1:1").unwrap())),
        ..Default::default()
    }
    .build(TestLocalAgent::default());
    let agent_url = agent_info.url.clone().unwrap();
    peer_store.insert(vec![agent_info.clone()]).await.unwrap();

    let incoming_op = MemoryOp::new(Timestamp::now(), vec![1]);
    let incoming_op_id = incoming_op.compute_op_id();

    fetch
        .request_ops(vec![incoming_op_id.clone()], agent_id.clone())
        .await
        .unwrap();

    assert_eq!(fetch.state.lock().unwrap().requests.len(), 1);

    // Receive incoming ops will fail.
    let fetch_message = serialize_response_message(vec![incoming_op.into()]);
    fetch
        .message_handler
        .recv_module_msg(
            agent_url,
            TEST_SPACE_ID,
            crate::factories::core_fetch::MOD_NAME.to_string(),
            fetch_message,
        )
        .unwrap();

    tokio::time::sleep(Duration::from_millis(10)).await;

    // Op id should not have been removed from requests.
    assert_eq!(fetch.state.lock().unwrap().requests.len(), 1);
}
