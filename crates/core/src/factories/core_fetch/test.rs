use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use bytes::Bytes;
use kitsune2_api::{
    fetch::{deserialize_op_ids, Fetch},
    id::Id,
    transport::Transport,
    AgentId, K2Error, OpId, SpaceId, Url,
};
use rand::Rng;

use crate::{default_builder, factories::test_utils::AgentBuilder};

use super::{BackOffList, CoreFetch, CoreFetchConfig};

#[derive(Debug)]
pub struct MockTransport {
    requests_sent: Arc<Mutex<Vec<(OpId, Url)>>>,
    fail: bool,
}

impl MockTransport {
    fn new(fail: bool) -> Arc<MockTransport> {
        Arc::new(Self {
            requests_sent: Arc::new(Mutex::new(Vec::new())),
            fail,
        })
    }
}

impl Transport for MockTransport {
    fn send_module(
        &self,
        peer: kitsune2_api::Url,
        _space: kitsune2_api::SpaceId,
        _module: String,
        data: bytes::Bytes,
    ) -> kitsune2_api::BoxFut<'_, kitsune2_api::K2Result<()>> {
        Box::pin(async move {
            let op_ids = deserialize_op_ids(data).unwrap();
            let mut lock = self.requests_sent.lock().unwrap();
            op_ids.into_iter().for_each(|op_id| {
                lock.push((op_id, peer.clone()));
            });

            if self.fail {
                Err(K2Error::other("connection timed out"))
            } else {
                Ok(())
            }
        })
    }

    fn disconnect(
        &self,
        _peer: Url,
        _reason: Option<String>,
    ) -> kitsune2_api::BoxFut<'_, ()> {
        unimplemented!()
    }

    fn register_module_handler(
        &self,
        _space: SpaceId,
        _module: String,
        _handler: kitsune2_api::transport::DynTxModuleHandler,
    ) {
        unimplemented!()
    }

    fn register_space_handler(
        &self,
        _space: SpaceId,
        _handler: kitsune2_api::transport::DynTxSpaceHandler,
    ) {
        unimplemented!()
    }

    fn send_space_notify(
        &self,
        _peer: Url,
        _space: SpaceId,
        _data: bytes::Bytes,
    ) -> kitsune2_api::BoxFut<'_, kitsune2_api::K2Result<()>> {
        unimplemented!()
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn fetch_queue() {
    let builder = Arc::new(default_builder().with_default_config().unwrap());
    let peer_store = builder.peer_store.create(builder.clone()).await.unwrap();
    let mock_transport = MockTransport::new(false);
    let config = CoreFetchConfig::default();

    let op_id = random_op_id();
    let op_list = vec![op_id.clone()];
    let agent_id = random_agent_id();
    let agent_info = AgentBuilder {
        agent: Some(agent_id.clone()),
        url: Some(Some(Url::from_str("wss://127.0.0.1:1").unwrap())),
        ..Default::default()
    }
    .build();
    let agent_url = agent_info.url.clone().unwrap();
    peer_store.insert(vec![agent_info.clone()]).await.unwrap();

    let fetch = CoreFetch::new(
        config.clone(),
        agent_info.space.clone(),
        peer_store.clone(),
        mock_transport.clone(),
    );

    let requests_sent = mock_transport.requests_sent.lock().unwrap().clone();
    assert!(requests_sent.is_empty());

    // Add 1 op.
    fetch.add_ops(op_list, agent_id.clone()).await.unwrap();

    // Let the fetch request be sent multiple times. As only 1 op was added to the queue,
    // this proves that it is being re-added to the queue after sending a request for it.
    tokio::time::timeout(Duration::from_millis(10), async {
        loop {
            tokio::task::yield_now().await;
            if mock_transport.requests_sent.lock().unwrap().len() >= 3 {
                break;
            }
        }
    })
    .await
    .unwrap();

    // Clear set of ops to fetch to stop sending requests.
    fetch.state.lock().unwrap().requests.clear();

    let mut num_requests_sent =
        mock_transport.requests_sent.lock().unwrap().len();

    // Wait for tasks to settle all requests.
    tokio::time::timeout(Duration::from_millis(20), async {
        loop {
            tokio::time::sleep(Duration::from_millis(1)).await;
            let current_num_requests_sent =
                mock_transport.requests_sent.lock().unwrap().len();
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
    assert!(mock_transport
        .requests_sent
        .lock()
        .unwrap()
        .iter()
        .all(|request| request == &(op_id.clone(), agent_url.clone())));

    // Give time for more requests to be sent, which shouldn't happen now that the set of
    // ops to fetch is cleared.
    tokio::time::sleep(Duration::from_millis(10)).await;

    // No more requests should have been sent.
    // Ideally it were possible to check that no more fetch request have been passed back into
    // the internal channel, but that would require a custom wrapper around the channel.
    let requests_sent = mock_transport.requests_sent.lock().unwrap().clone();
    assert_eq!(requests_sent.len(), num_requests_sent);
}

#[tokio::test(flavor = "multi_thread")]
async fn happy_op_fetch_from_multiple_agents() {
    let builder = Arc::new(default_builder().with_default_config().unwrap());
    let peer_store = builder.peer_store.create(builder.clone()).await.unwrap();
    let config = CoreFetchConfig {
        parallel_request_count: 5,
        ..Default::default()
    };
    let mock_transport = MockTransport::new(false);
    let space_id = SpaceId::from(bytes::Bytes::from_static(b"space_1"));

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
        space: Some(space_id.clone()),
        ..Default::default()
    }
    .build();
    let agent_info_2 = AgentBuilder {
        agent: Some(agent_2.clone()),
        url: Some(Some(Url::from_str("wss://127.0.0.1:2").unwrap())),
        space: Some(space_id.clone()),
        ..Default::default()
    }
    .build();
    let agent_info_3 = AgentBuilder {
        agent: Some(agent_3.clone()),
        url: Some(Some(Url::from_str("wss://127.0.0.1:3").unwrap())),
        space: Some(space_id.clone()),
        ..Default::default()
    }
    .build();
    let agent_url_1 = agent_info_1.url.clone().unwrap();
    let agent_url_2 = agent_info_2.url.clone().unwrap();
    let agent_url_3 = agent_info_3.url.clone().unwrap();
    peer_store
        .insert(vec![agent_info_1, agent_info_2, agent_info_3])
        .await
        .unwrap();
    let fetch = CoreFetch::new(
        config.clone(),
        space_id.clone(),
        peer_store.clone(),
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
        fetch.add_ops(op_list_1.clone(), agent_1.clone()),
        fetch.add_ops(op_list_2.clone(), agent_2.clone()),
        fetch.add_ops(op_list_3.clone(), agent_3.clone()),
    ])
    .await;

    // Check that at least one request was sent for each op.
    tokio::time::timeout(Duration::from_millis(20), async {
        loop {
            tokio::task::yield_now().await;
            let requests_sent =
                mock_transport.requests_sent.lock().unwrap().clone();
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
    let builder = Arc::new(default_builder().with_default_config().unwrap());
    let peer_store = builder.peer_store.create(builder.clone()).await.unwrap();
    let config = CoreFetchConfig::default();
    let mock_transport = MockTransport::new(false);

    let op_list = create_op_list(2);
    let agent_id = random_agent_id();
    let agent_info = AgentBuilder {
        agent: Some(agent_id.clone()),
        url: Some(Some(Url::from_str("wss://127.0.0.1:1").unwrap())),
        ..Default::default()
    }
    .build();

    let fetch = CoreFetch::new(
        config.clone(),
        agent_info.space.clone(),
        peer_store.clone(),
        mock_transport.clone(),
    );

    fetch.add_ops(op_list, agent_id).await.unwrap();

    // Wait for agent to be looked up in peer store.
    tokio::time::sleep(Duration::from_millis(10)).await;

    // Check that all op ids for agent have been removed from ops set.
    assert!(fetch.state.lock().unwrap().requests.is_empty());
}

#[test]
fn back_off() {
    let mut back_off_list = BackOffList::new(
        Duration::from_millis(10),
        Duration::from_millis(10),
        2,
    );
    let agent_id = random_agent_id();
    back_off_list.back_off_agent(&agent_id);
    assert!(back_off_list.is_agent_on_back_off(&agent_id));

    std::thread::sleep(
        back_off_list.state.get(&agent_id).unwrap().current_interval,
    );

    assert!(!back_off_list.is_agent_on_back_off(&agent_id));

    back_off_list.back_off_agent(&agent_id);
    assert!(back_off_list.is_agent_on_back_off(&agent_id));

    std::thread::sleep(
        back_off_list.state.get(&agent_id).unwrap().current_interval,
    );

    assert!(!back_off_list.is_agent_on_back_off(&agent_id));
}

#[tokio::test(flavor = "multi_thread")]
async fn unresponsive_agents_are_put_on_back_off_list() {
    let builder = Arc::new(default_builder().with_default_config().unwrap());
    let peer_store = builder.peer_store.create(builder.clone()).await.unwrap();
    let config = CoreFetchConfig::default();
    let mock_transport = MockTransport::new(true);
    let space_id = SpaceId::from(bytes::Bytes::from_static(b"space_1"));

    let op_list_1 = create_op_list(5);
    let agent_1 = random_agent_id();
    let op_list_2 = create_op_list(5);
    let agent_2 = random_agent_id();

    let agent_info_1 = AgentBuilder {
        agent: Some(agent_1.clone()),
        url: Some(Some(Url::from_str("wss://127.0.0.1:1").unwrap())),
        space: Some(space_id.clone()),
        ..Default::default()
    }
    .build();
    let agent_info_2 = AgentBuilder {
        agent: Some(agent_2.clone()),
        url: Some(Some(Url::from_str("wss://127.0.0.1:2").unwrap())),
        space: Some(space_id.clone()),
        ..Default::default()
    }
    .build();
    let agent_url_1 = agent_info_1.url.clone().unwrap();
    let agent_url_2 = agent_info_2.url.clone().unwrap();
    peer_store
        .insert(vec![agent_info_1, agent_info_2])
        .await
        .unwrap();

    let fetch = CoreFetch::new(
        config.clone(),
        space_id.clone(),
        peer_store.clone(),
        mock_transport.clone(),
    );

    // Add all ops to the queue.
    futures::future::join_all([
        fetch.add_ops(op_list_1.clone(), agent_1.clone()),
        fetch.add_ops(op_list_2.clone(), agent_2.clone()),
    ])
    .await;

    // Wait for one request for each agent.
    let expected_agent_url = [agent_url_1, agent_url_2];
    let expected_agents = [agent_1, agent_2];
    tokio::time::timeout(Duration::from_millis(100), async {
        loop {
            tokio::time::sleep(Duration::from_millis(1)).await;
            let requests_sent =
                mock_transport.requests_sent.lock().unwrap().clone();
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
    let builder = Arc::new(default_builder().with_default_config().unwrap());
    let peer_store = builder.peer_store.create(builder.clone()).await.unwrap();
    let config = CoreFetchConfig {
        first_back_off_interval: Duration::from_millis(10),
        ..Default::default()
    };
    let mock_transport = MockTransport::new(false);

    let op_list = create_op_list(1);
    let agent_id = random_agent_id();
    let agent_info = AgentBuilder {
        agent: Some(agent_id.clone()),
        url: Some(Some(Url::from_str("wss://127.0.0.1:1").unwrap())),
        ..Default::default()
    }
    .build();
    peer_store.insert(vec![agent_info.clone()]).await.unwrap();

    let fetch = CoreFetch::new(
        config.clone(),
        agent_info.space.clone(),
        peer_store,
        mock_transport.clone(),
    );

    let first_back_off_interval = {
        let mut lock = fetch.state.lock().unwrap();
        lock.back_off_list.back_off_agent(&agent_id);

        assert!(lock.back_off_list.is_agent_on_back_off(&agent_id));

        lock.back_off_list.first_back_off_interval
    };

    tokio::time::sleep(first_back_off_interval).await;

    fetch.add_ops(op_list, agent_id.clone()).await.unwrap();

    tokio::time::timeout(Duration::from_millis(10), async {
        loop {
            tokio::time::sleep(Duration::from_millis(1)).await;
            if !mock_transport.requests_sent.lock().unwrap().is_empty() {
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
    let builder = Arc::new(default_builder().with_default_config().unwrap());
    let peer_store = builder.peer_store.create(builder.clone()).await.unwrap();
    let config = CoreFetchConfig {
        first_back_off_interval: Duration::from_millis(10),
        last_back_off_interval: Duration::from_millis(10),
        ..Default::default()
    };
    let mock_transport = MockTransport::new(true);
    let space_id = SpaceId::from(bytes::Bytes::from_static(b"space_1"));

    let op_list_1 = create_op_list(2);
    let agent_id_1 = random_agent_id();
    let agent_info_1 = AgentBuilder {
        agent: Some(agent_id_1.clone()),
        url: Some(Some(Url::from_str("wss://127.0.0.1:1").unwrap())),
        ..Default::default()
    }
    .build();
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
    .build();
    peer_store.insert(vec![agent_info_2.clone()]).await.unwrap();

    let fetch = CoreFetch::new(
        config.clone(),
        space_id.clone(),
        peer_store,
        mock_transport.clone(),
    );

    fetch
        .add_ops(op_list_1.clone(), agent_id_1.clone())
        .await
        .unwrap();

    // Wait for one request to fail, so agent is put on back off list.
    tokio::time::timeout(Duration::from_millis(10), async {
        loop {
            tokio::time::sleep(Duration::from_millis(1)).await;
            if !mock_transport
                .requests_sent
                .lock()
                .unwrap()
                .clone()
                .is_empty()
            {
                break;
            }
        }
    })
    .await
    .unwrap();

    let current_number_of_requests_to_agent_1 = mock_transport
        .requests_sent
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
    fetch.add_ops(op_list_2, agent_id_2.clone()).await.unwrap();

    // Wait for another request attempt to agent 1, which should remove all of their requests
    // from the set.
    tokio::time::timeout(Duration::from_millis(10), async {
        loop {
            if mock_transport
                .requests_sent
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

fn random_id() -> Id {
    let mut rng = rand::thread_rng();
    let mut bytes = [0u8; 32];
    rng.fill(&mut bytes);
    let bytes = Bytes::from(bytes.to_vec());
    Id(bytes)
}

fn random_op_id() -> OpId {
    OpId(random_id())
}

fn random_agent_id() -> AgentId {
    AgentId(random_id())
}

fn create_op_list(num_ops: u16) -> Vec<OpId> {
    let mut ops = Vec::new();
    for _ in 0..num_ops {
        let op = random_op_id();
        ops.push(op.clone());
    }
    ops
}
