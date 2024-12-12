use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use bytes::Bytes;
use kitsune2_api::{
    fetch::Fetch, id::Id, transport::Transport, AgentId, K2Error, OpId,
    SpaceId, Url,
};
use rand::Rng;

use crate::{default_builder, factories::test_utils::AgentBuild};

use super::{CoreFetch, CoreFetchConfig};

#[derive(Debug)]
pub struct MockTransport {
    requests_sent: Arc<Mutex<Vec<(OpId, AgentId)>>>,
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
        _peer: kitsune2_api::Url,
        _space: kitsune2_api::SpaceId,
        _module: String,
        mut data: bytes::Bytes,
    ) -> kitsune2_api::BoxFut<'_, kitsune2_api::K2Result<()>> {
        Box::pin(async move {
            let op_id_bytes = data.split_to(data.len() / 2);
            let agent_id_bytes = data;
            let op_id = OpId::from(op_id_bytes);
            let agent_id = AgentId::from(agent_id_bytes);
            self.requests_sent.lock().unwrap().push((op_id, agent_id));

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
    let builder = Arc::new(default_builder());
    let peer_store = builder.peer_store.create(builder.clone()).await.unwrap();
    let mock_transport = MockTransport::new(false);
    let config = CoreFetchConfig::default();

    let op_id = random_op_id();
    let op_list = vec![op_id.clone()];
    let agent_id = random_agent_id();
    let agent_info = AgentBuild {
        agent: Some(agent_id.clone()),
        url: Some(Some(Url::from_str("wss://127.0.0.1:8888").unwrap())),
        ..Default::default()
    }
    .build();
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
    fetch.state.lock().unwrap().ops.clear();

    let mut num_requests_sent =
        mock_transport.requests_sent.lock().unwrap().len();

    // Wait for tasks to settle all requests.
    tokio::time::timeout(Duration::from_millis(10), async {
        loop {
            tokio::task::yield_now().await;
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

    // CHeck that all requests have been made for the 1 op to the agent.
    assert!(mock_transport
        .requests_sent
        .lock()
        .unwrap()
        .iter()
        .all(|request| request == &(op_id.clone(), agent_id.clone())));

    // Give time for more requests to be sent, which shouldn't happen now that the set of
    // ops to fetch is cleared.
    tokio::time::sleep(Duration::from_millis(20)).await;

    // No more requests should have been sent.
    // Ideally it were possible to check that no more fetch request have been passed back into
    // the internal channel, but that would require a custom wrapper around the channel.
    let requests_sent = mock_transport.requests_sent.lock().unwrap().clone();
    assert_eq!(requests_sent.len(), num_requests_sent);
}

#[tokio::test(flavor = "multi_thread")]
async fn happy_multi_op_fetch_from_single_agent() {
    let builder = Arc::new(default_builder());
    let peer_store = builder.peer_store.create(builder.clone()).await.unwrap();
    let config = CoreFetchConfig::default();
    let mock_transport = MockTransport::new(false);

    let num_ops: usize = 50;
    let op_list = create_op_list(num_ops as u16);
    let agent_id = random_agent_id();
    let agent_info = AgentBuild {
        agent: Some(agent_id.clone()),
        url: Some(Some(Url::from_str("wss://127.0.0.1:8888").unwrap())),
        ..Default::default()
    }
    .build();
    peer_store.insert(vec![agent_info.clone()]).await.unwrap();

    let fetch = CoreFetch::new(
        config.clone(),
        agent_info.space.clone(),
        peer_store.clone(),
        mock_transport.clone(),
    );

    let mut expected_ops = Vec::new();
    op_list
        .clone()
        .into_iter()
        .for_each(|op_id| expected_ops.push((op_id, agent_id.clone())));

    fetch
        .add_ops(op_list.clone(), agent_id.clone())
        .await
        .unwrap();

    // Check that at least one request was sent to the agent for each op.
    tokio::time::timeout(Duration::from_millis(100), async {
        loop {
            tokio::task::yield_now().await;
            let requests_sent =
                mock_transport.requests_sent.lock().unwrap().clone();
            if requests_sent.len() >= num_ops {
                op_list.clone().into_iter().all(|op_id| {
                    requests_sent.contains(&(op_id, agent_id.clone()))
                });
                break;
            }
        }
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn happy_multi_op_fetch_from_multiple_agents() {
    let builder = Arc::new(default_builder());
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

    let agent_info_1 = AgentBuild {
        agent: Some(agent_1.clone()),
        url: Some(Some(Url::from_str("wss://127.0.0.1:8888").unwrap())),
        space: Some(space_id.clone()),
        ..Default::default()
    }
    .build();
    let agent_info_2 = AgentBuild {
        agent: Some(agent_2.clone()),
        url: Some(Some(Url::from_str("wss://127.0.0.1:8888").unwrap())),
        space: Some(space_id.clone()),
        ..Default::default()
    }
    .build();
    let agent_info_3 = AgentBuild {
        agent: Some(agent_3.clone()),
        url: Some(Some(Url::from_str("wss://127.0.0.1:8888").unwrap())),
        space: Some(space_id.clone()),
        ..Default::default()
    }
    .build();
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

    fetch
        .add_ops(op_list_1.clone(), agent_1.clone())
        .await
        .unwrap();
    fetch
        .add_ops(op_list_2.clone(), agent_2.clone())
        .await
        .unwrap();
    fetch
        .add_ops(op_list_3.clone(), agent_3.clone())
        .await
        .unwrap();

    // Check that at least one request was sent for each op.
    tokio::time::timeout(Duration::from_millis(100), async {
        loop {
            tokio::task::yield_now().await;
            let requests_sent =
                mock_transport.requests_sent.lock().unwrap().clone();
            if requests_sent.len() >= total_ops
                && expected_ops
                    .iter()
                    .all(|expected_op| requests_sent.contains(expected_op))
            {
                break;
            }
        }
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn unresponsive_agents_are_put_on_cool_down_list() {
    let builder = Arc::new(default_builder());
    let peer_store = builder.peer_store.create(builder.clone()).await.unwrap();
    let config = CoreFetchConfig::default();
    let mock_transport = MockTransport::new(true);

    let op_list = create_op_list(1);
    let agent_id = random_agent_id();
    let agent_info = AgentBuild {
        agent: Some(agent_id.clone()),
        url: Some(Some(Url::from_str("wss://127.0.0.1:8888").unwrap())),
        ..Default::default()
    }
    .build();
    peer_store.insert(vec![agent_info.clone()]).await.unwrap();

    let fetch = CoreFetch::new(
        config.clone(),
        agent_info.space.clone(),
        peer_store.clone(),
        mock_transport.clone(),
    );

    fetch.add_ops(op_list, agent_id.clone()).await.unwrap();

    tokio::time::timeout(Duration::from_millis(10), async {
        loop {
            tokio::task::yield_now().await;
            if !mock_transport.requests_sent.lock().unwrap().is_empty()
                && fetch
                    .state
                    .lock()
                    .unwrap()
                    .cool_down_list
                    .is_agent_cooling_down(&agent_id)
            {
                break;
            }
        }
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn agent_cooling_down_is_removed_from_list() {
    let builder = Arc::new(default_builder());
    let peer_store = builder.peer_store.create(builder.clone()).await.unwrap();
    let config = CoreFetchConfig {
        cool_down_interval_ms: 10,
        ..Default::default()
    };
    let mock_transport = MockTransport::new(false);
    let space_id = SpaceId::from(bytes::Bytes::from_static(b"space_1"));

    let fetch = CoreFetch::new(
        config.clone(),
        space_id,
        peer_store,
        mock_transport.clone(),
    );
    let agent_id = random_agent_id();

    fetch
        .state
        .lock()
        .unwrap()
        .cool_down_list
        .add_agent(agent_id.clone());

    assert!(fetch
        .state
        .lock()
        .unwrap()
        .cool_down_list
        .is_agent_cooling_down(&agent_id));

    // Wait for the cool-down interval + 1 ms to avoid flakiness.
    tokio::time::sleep(Duration::from_millis(config.cool_down_interval_ms + 1))
        .await;

    assert!(!fetch
        .state
        .lock()
        .unwrap()
        .cool_down_list
        .is_agent_cooling_down(&agent_id));
}

#[tokio::test(flavor = "multi_thread")]
async fn multi_op_fetch_from_multiple_unresponsive_agents() {
    let builder = Arc::new(default_builder());
    let peer_store = builder.peer_store.create(builder.clone()).await.unwrap();
    let config = CoreFetchConfig::default();
    let mock_transport = MockTransport::new(true);
    let space_id = SpaceId::from(bytes::Bytes::from_static(b"space_1"));

    let op_list_1 = create_op_list(10);
    let agent_1 = random_agent_id();
    let op_list_2 = create_op_list(20);
    let agent_2 = random_agent_id();
    let op_list_3 = create_op_list(30);
    let agent_3 = random_agent_id();

    let agent_info_1 = AgentBuild {
        agent: Some(agent_1.clone()),
        url: Some(Some(Url::from_str("wss://127.0.0.1:8888").unwrap())),
        space: Some(space_id.clone()),
        ..Default::default()
    }
    .build();
    let agent_info_2 = AgentBuild {
        agent: Some(agent_2.clone()),
        url: Some(Some(Url::from_str("wss://127.0.0.1:8888").unwrap())),
        space: Some(space_id.clone()),
        ..Default::default()
    }
    .build();
    let agent_info_3 = AgentBuild {
        agent: Some(agent_3.clone()),
        url: Some(Some(Url::from_str("wss://127.0.0.1:8888").unwrap())),
        space: Some(space_id.clone()),
        ..Default::default()
    }
    .build();
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

    // Add all ops to the queue.
    fetch
        .add_ops(op_list_1.clone(), agent_1.clone())
        .await
        .unwrap();
    fetch
        .add_ops(op_list_2.clone(), agent_2.clone())
        .await
        .unwrap();
    fetch
        .add_ops(op_list_3.clone(), agent_3.clone())
        .await
        .unwrap();

    // Wait for one request for each agent.
    let expected_agents = [agent_1, agent_2, agent_3];
    tokio::time::timeout(Duration::from_millis(100), async {
        loop {
            tokio::time::sleep(Duration::from_millis(1)).await;
            let requests_sent =
                mock_transport.requests_sent.lock().unwrap().clone();
            let request_destinations = requests_sent
                .iter()
                .map(|(_, agent_id)| agent_id)
                .collect::<Vec<_>>();
            if expected_agents
                .iter()
                .all(|agent| request_destinations.contains(&agent))
            {
                // Check all agents are on cool_down_list.
                let cool_down_list =
                    &mut fetch.state.lock().unwrap().cool_down_list;
                if expected_agents
                    .iter()
                    .all(|agent| cool_down_list.is_agent_cooling_down(agent))
                {
                    break;
                }
            }
        }
    })
    .await
    .unwrap();
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