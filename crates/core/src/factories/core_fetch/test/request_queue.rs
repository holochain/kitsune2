use crate::{
    default_test_builder,
    factories::{
        core_fetch::{CoreFetch, CoreFetchConfig},
        MemOpStoreFactory,
    },
};
use kitsune2_api::{
    fetch::{deserialize_op_ids, Fetch},
    id::Id,
    transport::MockTransport,
    K2Error, OpId, SpaceId, Url,
};
use kitsune2_test_utils::agent::{AgentBuilder, TestLocalAgent};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use super::utils::{create_op_list, random_agent_id, random_op_id};

const SPACE_ID: SpaceId = SpaceId(Id(bytes::Bytes::from_static(b"space_1")));

fn make_mock_transport(
    requests_sent: Arc<Mutex<Vec<(OpId, Url)>>>,
    faulty_connection: bool,
) -> Arc<MockTransport> {
    let mut mock_transport = MockTransport::new();
    mock_transport.expect_send_module().returning({
        let requests_sent = requests_sent.clone();
        move |peer, space, module, data| {
            assert_eq!(space, SPACE_ID);
            assert_eq!(module, crate::factories::core_fetch::MOD_NAME);
            Box::pin({
                let value = requests_sent.clone();
                async move {
                    let op_ids = deserialize_op_ids(data).unwrap();
                    let mut lock = value.lock().unwrap();
                    op_ids.into_iter().for_each(|op_id| {
                        lock.push((op_id, peer.clone()));
                    });

                    if faulty_connection {
                        Err(K2Error::other("connection timed out"))
                    } else {
                        Ok(())
                    }
                }
            })
        }
    });
    mock_transport
        .expect_register_module_handler()
        .returning(|_, _, _| ());
    Arc::new(mock_transport)
}

#[tokio::test(flavor = "multi_thread")]
async fn fetch_queue() {
    let builder =
        Arc::new(default_test_builder().with_default_config().unwrap());
    let peer_store = builder.peer_store.create(builder.clone()).await.unwrap();
    let op_store = MemOpStoreFactory::create()
        .create(builder.clone(), SPACE_ID)
        .await
        .unwrap();
    let requests_sent = Arc::new(Mutex::new(Vec::new()));
    let mock_transport = make_mock_transport(requests_sent.clone(), false);
    let config = CoreFetchConfig::default();

    let op_id = random_op_id();
    let op_list = vec![op_id.clone()];
    let agent_id = random_agent_id();
    let agent_info = AgentBuilder {
        agent: Some(agent_id.clone()),
        space: Some(SPACE_ID),
        url: Some(Some(Url::from_str("wss://127.0.0.1:1").unwrap())),
        ..Default::default()
    }
    .build(TestLocalAgent::default());
    let agent_url = agent_info.url.clone().unwrap();
    peer_store.insert(vec![agent_info.clone()]).await.unwrap();

    let fetch = CoreFetch::new(
        config.clone(),
        SPACE_ID,
        peer_store.clone(),
        op_store,
        mock_transport.clone(),
    );

    assert!(requests_sent.lock().unwrap().is_empty());

    // Add 1 op.
    fetch.request_ops(op_list, agent_id.clone()).await.unwrap();

    // Let the fetch request be sent multiple times. As only 1 op was added to the queue,
    // this proves that it is being re-added to the queue after sending a request for it.
    tokio::time::timeout(Duration::from_millis(20), async {
        loop {
            tokio::task::yield_now().await;
            if requests_sent.lock().unwrap().len() >= 3 {
                break;
            }
        }
    })
    .await
    .unwrap();

    // Clear set of ops to fetch to stop sending requests.
    fetch.state.lock().unwrap().requests.clear();

    let mut num_requests_sent = requests_sent.lock().unwrap().len();

    // Wait for tasks to settle all requests.
    tokio::time::timeout(Duration::from_millis(20), async {
        loop {
            tokio::time::sleep(Duration::from_millis(1)).await;
            let current_num_requests_sent = requests_sent.lock().unwrap().len();
            if current_num_requests_sent == num_requests_sent {
                break;
            } else {
                num_requests_sent = current_num_requests_sent;
            }
        }
    })
    .await
    .unwrap();

    // Check that all requests have been made for the 1 op to the agent.
    assert!(requests_sent
        .lock()
        .unwrap()
        .iter()
        .all(|request| request == &(op_id.clone(), agent_url.clone())));

    // Give time for more requests to be sent, which shouldn't happen now that the set of
    // ops to fetch is cleared.
    tokio::time::sleep(Duration::from_millis(20)).await;

    // No more requests should have been sent.
    // Ideally it were possible to check that no more fetch request have been passed back into
    // the internal channel, but that would require a custom wrapper around the channel.
    let requests_sent = requests_sent.lock().unwrap().clone();
    assert_eq!(requests_sent.len(), num_requests_sent);
}

#[tokio::test(flavor = "multi_thread")]
async fn happy_op_fetch_from_multiple_agents() {
    let builder =
        Arc::new(default_test_builder().with_default_config().unwrap());
    let peer_store = builder.peer_store.create(builder.clone()).await.unwrap();
    let requests_sent = Arc::new(Mutex::new(Vec::new()));
    let mock_transport = make_mock_transport(requests_sent.clone(), false);
    let op_store = MemOpStoreFactory::create()
        .create(builder.clone(), SPACE_ID)
        .await
        .unwrap();
    let config = CoreFetchConfig {
        parallel_request_count: 5,
        ..Default::default()
    };

    let op_list_1 = create_op_list(10);
    let agent_1 = random_agent_id();
    let op_list_2 = create_op_list(20);
    let agent_2 = random_agent_id();
    let op_list_3 = create_op_list(30);
    let agent_3 = random_agent_id();
    let total_ops = op_list_1.len() + op_list_2.len() + op_list_3.len();

    let agent_info_1 = AgentBuilder {
        agent: Some(agent_1.clone()),
        url: Some(Some(Url::from_str("wss://127.0.0.1:1").unwrap())),
        space: Some(SPACE_ID),
        ..Default::default()
    }
    .build(TestLocalAgent::default());
    let agent_info_2 = AgentBuilder {
        agent: Some(agent_2.clone()),
        url: Some(Some(Url::from_str("wss://127.0.0.1:2").unwrap())),
        space: Some(SPACE_ID),
        ..Default::default()
    }
    .build(TestLocalAgent::default());
    let agent_info_3 = AgentBuilder {
        agent: Some(agent_3.clone()),
        url: Some(Some(Url::from_str("wss://127.0.0.1:3").unwrap())),
        space: Some(SPACE_ID),
        ..Default::default()
    }
    .build(TestLocalAgent::default());
    let agent_url_1 = agent_info_1.url.clone().unwrap();
    let agent_url_2 = agent_info_2.url.clone().unwrap();
    let agent_url_3 = agent_info_3.url.clone().unwrap();
    peer_store
        .insert(vec![agent_info_1, agent_info_2, agent_info_3])
        .await
        .unwrap();
    let fetch = CoreFetch::new(
        config.clone(),
        SPACE_ID,
        peer_store.clone(),
        op_store,
        mock_transport.clone(),
    );

    let mut expected_requests = Vec::new();
    op_list_1
        .clone()
        .into_iter()
        .for_each(|op_id| expected_requests.push((op_id, agent_url_1.clone())));
    op_list_2
        .clone()
        .into_iter()
        .for_each(|op_id| expected_requests.push((op_id, agent_url_2.clone())));
    op_list_3
        .clone()
        .into_iter()
        .for_each(|op_id| expected_requests.push((op_id, agent_url_3.clone())));
    let mut expected_ops = Vec::new();
    op_list_1
        .clone()
        .into_iter()
        .for_each(|op_id| expected_ops.push((op_id, agent_1.clone())));
    op_list_2
        .clone()
        .into_iter()
        .for_each(|op_id| expected_ops.push((op_id, agent_2.clone())));
    op_list_3
        .clone()
        .into_iter()
        .for_each(|op_id| expected_ops.push((op_id, agent_3.clone())));

    futures::future::join_all([
        fetch.request_ops(op_list_1.clone(), agent_1.clone()),
        fetch.request_ops(op_list_2.clone(), agent_2.clone()),
        fetch.request_ops(op_list_3.clone(), agent_3.clone()),
    ])
    .await;

    // Check that at least one request was sent for each op.
    tokio::time::timeout(Duration::from_millis(20), async {
        loop {
            tokio::task::yield_now().await;
            let requests_sent = requests_sent.lock().unwrap().clone();
            if requests_sent.len() >= total_ops
                && expected_requests
                    .iter()
                    .all(|expected_op| requests_sent.contains(expected_op))
            {
                break;
            }
        }
    })
    .await
    .unwrap();

    // Check that op ids are still part of ops to fetch.
    let lock = fetch.state.lock().unwrap();
    assert!(expected_ops.iter().all(|v| lock.requests.contains(v)));
}

#[tokio::test(flavor = "multi_thread")]
async fn ops_are_cleared_when_agent_not_in_peer_store() {
    let builder =
        Arc::new(default_test_builder().with_default_config().unwrap());
    let peer_store = builder.peer_store.create(builder.clone()).await.unwrap();
    let op_store = MemOpStoreFactory::create()
        .create(builder.clone(), SPACE_ID)
        .await
        .unwrap();
    let requests_sent = Arc::new(Mutex::new(Vec::new()));
    let mock_transport = make_mock_transport(requests_sent.clone(), false);
    let config = CoreFetchConfig::default();

    let op_list = create_op_list(2);
    let agent_id = random_agent_id();
    let agent_info = AgentBuilder {
        agent: Some(agent_id.clone()),
        url: Some(Some(Url::from_str("wss://127.0.0.1:1").unwrap())),
        ..Default::default()
    }
    .build(TestLocalAgent::default());

    let fetch = CoreFetch::new(
        config.clone(),
        agent_info.space.clone(),
        peer_store.clone(),
        op_store,
        mock_transport.clone(),
    );

    fetch.request_ops(op_list, agent_id).await.unwrap();

    // Wait for agent to be looked up in peer store.
    tokio::time::sleep(Duration::from_millis(20)).await;

    // Check that all op ids for agent have been removed from ops set.
    assert!(fetch.state.lock().unwrap().requests.is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn unresponsive_agents_are_put_on_back_off_list() {
    let builder =
        Arc::new(default_test_builder().with_default_config().unwrap());
    let peer_store = builder.peer_store.create(builder.clone()).await.unwrap();
    let op_store = MemOpStoreFactory::create()
        .create(builder.clone(), SPACE_ID)
        .await
        .unwrap();
    let requests_sent = Arc::new(Mutex::new(Vec::new()));
    let mock_transport = make_mock_transport(requests_sent.clone(), true);
    let config = CoreFetchConfig::default();

    let op_list_1 = create_op_list(5);
    let agent_1 = random_agent_id();
    let op_list_2 = create_op_list(5);
    let agent_2 = random_agent_id();

    let agent_info_1 = AgentBuilder {
        agent: Some(agent_1.clone()),
        url: Some(Some(Url::from_str("wss://127.0.0.1:1").unwrap())),
        space: Some(SPACE_ID),
        ..Default::default()
    }
    .build(TestLocalAgent::default());
    let agent_info_2 = AgentBuilder {
        agent: Some(agent_2.clone()),
        url: Some(Some(Url::from_str("wss://127.0.0.1:2").unwrap())),
        space: Some(SPACE_ID),
        ..Default::default()
    }
    .build(TestLocalAgent::default());
    let agent_url_1 = agent_info_1.url.clone().unwrap();
    let agent_url_2 = agent_info_2.url.clone().unwrap();
    peer_store
        .insert(vec![agent_info_1, agent_info_2])
        .await
        .unwrap();

    let fetch = CoreFetch::new(
        config.clone(),
        SPACE_ID,
        peer_store,
        op_store,
        mock_transport.clone(),
    );

    // Add all ops to the queue.
    futures::future::join_all([
        fetch.request_ops(op_list_1.clone(), agent_1.clone()),
        fetch.request_ops(op_list_2.clone(), agent_2.clone()),
    ])
    .await;

    // Wait for one request for each agent.
    let expected_agent_url = [agent_url_1, agent_url_2];
    let expected_agents = [agent_1, agent_2];
    tokio::time::timeout(Duration::from_millis(100), async {
        loop {
            tokio::time::sleep(Duration::from_millis(1)).await;
            let requests_sent = requests_sent.lock().unwrap();
            let request_destinations = requests_sent
                .iter()
                .map(|(_, agent_id)| agent_id)
                .collect::<Vec<_>>();
            if expected_agent_url
                .iter()
                .all(|agent| request_destinations.contains(&agent))
            {
                // Check all agents are on back off list.
                let back_off_list =
                    &mut fetch.state.lock().unwrap().back_off_list;
                if expected_agents
                    .iter()
                    .all(|agent| back_off_list.is_agent_on_back_off(agent))
                {
                    break;
                }
            }
        }
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn agent_on_back_off_is_removed_from_list_after_successful_send() {
    let builder =
        Arc::new(default_test_builder().with_default_config().unwrap());
    let peer_store = builder.peer_store.create(builder.clone()).await.unwrap();
    let op_store = MemOpStoreFactory::create()
        .create(builder.clone(), SPACE_ID)
        .await
        .unwrap();
    let requests_sent = Arc::new(Mutex::new(Vec::new()));
    let mock_transport = make_mock_transport(requests_sent.clone(), false);
    let config = CoreFetchConfig {
        first_back_off_interval: Duration::from_millis(10),
        ..Default::default()
    };

    let op_list = create_op_list(1);
    let agent_id = random_agent_id();
    let agent_info = AgentBuilder {
        agent: Some(agent_id.clone()),
        space: Some(SPACE_ID),
        url: Some(Some(Url::from_str("wss://127.0.0.1:1").unwrap())),
        ..Default::default()
    }
    .build(TestLocalAgent::default());
    peer_store.insert(vec![agent_info.clone()]).await.unwrap();

    let fetch = CoreFetch::new(
        config.clone(),
        SPACE_ID,
        peer_store,
        op_store,
        mock_transport.clone(),
    );

    let first_back_off_interval = {
        let mut lock = fetch.state.lock().unwrap();
        lock.back_off_list.back_off_agent(&agent_id);

        assert!(lock.back_off_list.is_agent_on_back_off(&agent_id));

        lock.back_off_list.first_back_off_interval
    };

    tokio::time::sleep(first_back_off_interval).await;

    fetch.request_ops(op_list, agent_id.clone()).await.unwrap();

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

    assert!(fetch.state.lock().unwrap().back_off_list.state.is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn requests_are_dropped_when_max_back_off_expired() {
    let builder =
        Arc::new(default_test_builder().with_default_config().unwrap());
    let peer_store = builder.peer_store.create(builder.clone()).await.unwrap();
    let op_store = MemOpStoreFactory::create()
        .create(builder.clone(), SPACE_ID)
        .await
        .unwrap();
    let requests_sent = Arc::new(Mutex::new(Vec::new()));
    let mock_transport = make_mock_transport(requests_sent.clone(), true);
    let config = CoreFetchConfig {
        first_back_off_interval: Duration::from_millis(10),
        last_back_off_interval: Duration::from_millis(10),
        ..Default::default()
    };

    let op_list_1 = create_op_list(2);
    let agent_id_1 = random_agent_id();
    let agent_info_1 = AgentBuilder {
        agent: Some(agent_id_1.clone()),
        url: Some(Some(Url::from_str("wss://127.0.0.1:1").unwrap())),
        ..Default::default()
    }
    .build(TestLocalAgent::default());
    let agent_url_1 = agent_info_1.url.clone().unwrap();
    peer_store.insert(vec![agent_info_1.clone()]).await.unwrap();

    // Create a second agent to later check that their ops have not been removed.
    let op_list_2 = create_op_list(2);
    let agent_id_2 = random_agent_id();
    let agent_info_2 = AgentBuilder {
        agent: Some(agent_id_2.clone()),
        url: Some(Some(Url::from_str("wss://127.0.0.1:2").unwrap())),
        ..Default::default()
    }
    .build(TestLocalAgent::default());
    peer_store.insert(vec![agent_info_2.clone()]).await.unwrap();

    let fetch = CoreFetch::new(
        config.clone(),
        SPACE_ID,
        peer_store,
        op_store,
        mock_transport.clone(),
    );

    fetch
        .request_ops(op_list_1.clone(), agent_id_1.clone())
        .await
        .unwrap();

    // Wait for one request to fail, so agent is put on back off list.
    tokio::time::timeout(Duration::from_millis(20), async {
        loop {
            tokio::time::sleep(Duration::from_millis(1)).await;
            if !requests_sent.lock().unwrap().clone().is_empty() {
                break;
            }
        }
    })
    .await
    .unwrap();

    let current_number_of_requests_to_agent_1 = requests_sent
        .lock()
        .unwrap()
        .iter()
        .filter(|(_, a)| *a != agent_url_1)
        .count();

    // Back off agent the maximum possible number of times.
    let last_back_off_interval = {
        let mut lock = fetch.state.lock().unwrap();
        assert!(op_list_1.iter().all(|op_id| lock
            .requests
            .contains(&(op_id.clone(), agent_id_1.clone()))));
        for _ in 0..config.num_back_off_intervals {
            lock.back_off_list.back_off_agent(&agent_id_1);
        }

        lock.back_off_list.last_back_off_interval
    };

    // Wait for back off interval to expire. Afterwards the request should fail again and all
    // of the agent's requests should be removed from the set.
    tokio::time::sleep(last_back_off_interval).await;

    assert!(fetch
        .state
        .lock()
        .unwrap()
        .back_off_list
        .has_last_back_off_expired(&agent_id_1));

    // Add control agent's ops to set.
    fetch
        .request_ops(op_list_2, agent_id_2.clone())
        .await
        .unwrap();

    // Wait for another request attempt to agent 1, which should remove all of their requests
    // from the set.
    tokio::time::timeout(Duration::from_millis(20), async {
        loop {
            if requests_sent
                .lock()
                .unwrap()
                .iter()
                .filter(|(_, a)| *a != agent_url_1)
                .count()
                > current_number_of_requests_to_agent_1
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await
    .unwrap();

    assert!(fetch
        .state
        .lock()
        .unwrap()
        .requests
        .iter()
        .all(|(_, agent_id)| *agent_id != agent_id_1),);
}
