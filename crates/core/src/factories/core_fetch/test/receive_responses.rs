use super::utils::{random_agent_id, random_op_id};
use crate::{
    default_builder,
    factories::{
        core_fetch::{CoreFetch, CoreFetchConfig},
        test_utils::AgentBuilder,
    },
};
use kitsune2_api::{
    fetch::{deserialize_op_ids, Fetch},
    id,
    transport::{DynTransport, MockTransport},
    K2Error, MetaOp, MockOpStore, OpId, SpaceId, Timestamp, Url,
};
use kitsune2_memory::{Kitsune2MemoryOp, Kitsune2MemoryOpStore};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

const SPACE_ID: SpaceId = SpaceId(id::Id(bytes::Bytes::from_static(b"space1")));

fn create_mock_transport(
    requests_sent: Arc<Mutex<Vec<(OpId, Url)>>>,
) -> DynTransport {
    let mut mock_transport = MockTransport::new();
    mock_transport.expect_send_module().returning({
        move |peer, space, module, data| {
            assert_eq!(space, SPACE_ID);
            assert_eq!(module, crate::factories::core_fetch::MOD_NAME);
            let op_ids = deserialize_op_ids(data).unwrap();
            let mut lock = requests_sent.lock().unwrap();
            op_ids.into_iter().for_each(|op_id| {
                lock.push((op_id, peer.clone()));
            });
            Box::pin(async { Ok(()) })
        }
    });
    Arc::new(mock_transport)
}

#[tokio::test(flavor = "multi_thread")]
async fn no_more_requests_sent_for_removed_ops() {
    let builder = Arc::new(default_builder().with_default_config().unwrap());
    let peer_store = builder.peer_store.create(builder.clone()).await.unwrap();
    let requests_sent = Arc::new(Mutex::new(Vec::new()));
    let mock_transport = create_mock_transport(requests_sent.clone());
    let op_store = Arc::new(Kitsune2MemoryOpStore::default());

    let agent_id = random_agent_id();
    let agent_info = AgentBuilder {
        agent: Some(agent_id.clone()),
        url: Some(Some(Url::from_str("wss://127.0.0.1:1").unwrap())),
        ..Default::default()
    }
    .build();
    let another_agent_id = random_agent_id();
    let another_agent_info = AgentBuilder {
        agent: Some(another_agent_id.clone()),
        url: Some(Some(Url::from_str("wss://127.0.0.1:2").unwrap())),
        ..Default::default()
    }
    .build();
    peer_store
        .insert(vec![agent_info, another_agent_info])
        .await
        .unwrap();

    let fetch = CoreFetch::new(
        CoreFetchConfig::default(),
        SPACE_ID.clone(),
        peer_store,
        op_store,
        mock_transport,
    );

    // Add 1 op that'll be removed and another op that won't be.
    let incoming_op_id = random_op_id();
    let incoming_op = MetaOp {
        op_id: incoming_op_id.clone(),
        op_data: serde_json::to_vec(&Kitsune2MemoryOp::new(
            incoming_op_id.clone(),
            Timestamp::now(),
            vec![1],
        ))
        .unwrap(),
    };
    let another_op_id = random_op_id();

    futures::future::join_all([
        fetch.add_ops(
            vec![incoming_op_id.clone(), another_op_id.clone()],
            agent_id.clone(),
        ),
        fetch.add_ops(vec![incoming_op_id.clone()], another_agent_id),
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

    // Capture how many requests have been sent so far for the op that will be incoming.
    let requests_lock = requests_sent.lock().unwrap();
    let requests_sent_for_fetched_op = requests_lock
        .iter()
        .filter(|(op_id, _)| *op_id == incoming_op_id)
        .count();

    fetch
        .handle_incoming_ops(vec![incoming_op.clone()])
        .await
        .unwrap();

    drop(requests_lock);

    {
        let fetch_state = fetch.state.lock().unwrap();
        assert!(!fetch_state
            .requests
            .contains(&(incoming_op_id.clone(), agent_id.clone())));
        // Only 1 request of another op should remain.
        assert!(fetch_state.requests.contains(&(another_op_id, agent_id)));
        assert_eq!(fetch_state.requests.len(), 1);
    }

    // Let some time pass for more requests to be sent.
    tokio::time::sleep(Duration::from_millis(1)).await;

    // Check that no further requests have been sent for the removed op id.
    // Adding 1 because of a race condition of the lock on requests_sent.
    assert!(
        requests_sent
            .lock()
            .unwrap()
            .iter()
            .filter(|(op_id, _)| *op_id == incoming_op_id)
            .count()
            <= requests_sent_for_fetched_op + 1
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn op_ids_are_not_removed_when_storing_op_failed() {
    let builder = Arc::new(default_builder().with_default_config().unwrap());
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
        SPACE_ID.clone(),
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
    .build();
    peer_store.insert(vec![agent_info.clone()]).await.unwrap();
    let op_id = random_op_id();
    let op = MetaOp {
        op_id: op_id.clone(),
        op_data: serde_json::to_vec(&Kitsune2MemoryOp::new(
            op_id.clone(),
            Timestamp::now(),
            vec![1],
        ))
        .unwrap(),
    };

    fetch
        .add_ops(vec![op_id.clone()], agent_id.clone())
        .await
        .unwrap();

    assert_eq!(fetch.state.lock().unwrap().requests.len(), 1);

    fetch.handle_incoming_ops(vec![op]).await.unwrap();

    // Op id should not have been removed from requests.
    assert_eq!(fetch.state.lock().unwrap().requests.len(), 1);
}
