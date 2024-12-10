use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use bytes::Bytes;
use kitsune2_api::{fetch::Fetch, id::Id, AgentId, K2Error, OpId};
use rand::Rng;
use tokio::sync::Mutex;

use super::{CoreFetch, CoreFetchConfig, Inner, Transport};

#[derive(Debug)]
pub struct MockTransport {
    requests_sent: Vec<(OpId, AgentId)>,
    fail: bool,
}

type DynMockTransport = Arc<Mutex<MockTransport>>;

impl MockTransport {
    fn new(fail: bool) -> DynMockTransport {
        Arc::new(Mutex::new(Self {
            requests_sent: Vec::new(),
            fail,
        }))
    }
}

impl Transport for MockTransport {
    fn send_op_request(
        &mut self,
        op_id: OpId,
        dest: AgentId,
    ) -> kitsune2_api::BoxFut<'static, kitsune2_api::K2Result<()>> {
        self.requests_sent.push((op_id, dest));
        if self.fail {
            Box::pin(async move { Err(K2Error::other("connection timed out")) })
        } else {
            Box::pin(async move { Ok(()) })
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn fetch_queue() {
    let config = CoreFetchConfig::default();
    let mock_transport = MockTransport::new(false);
    let mut fetch = CoreFetch::new(config.clone(), mock_transport.clone());

    let op_id = random_op_id();
    let op_list = vec![op_id.clone()];
    let agent_id = random_agent_id();

    let requests_sent = mock_transport.lock().await.requests_sent.clone();
    assert!(requests_sent.is_empty());

    // Add 1 op.
    fetch.add_ops(op_list, agent_id.clone()).await.unwrap();

    // Let the fetch request be sent multiple times. As only 1 op was added to the queue,
    // this proves that it is being re-added to the queue after sending a request for it.
    tokio::time::timeout(Duration::from_millis(10), async {
        loop {
            if mock_transport.lock().await.requests_sent.len() >= 3 {
                break;
            }
        }
    })
    .await
    .unwrap();

    // Clear set of ops to fetch to stop sending requests.
    fetch.0.state.lock().await.ops.clear();

    let mut num_requests_sent = mock_transport.lock().await.requests_sent.len();

    // Wait for tasks to settle all requests.
    tokio::time::timeout(Duration::from_millis(10), async {
        loop {
            tokio::time::sleep(Duration::from_millis(1)).await;
            let current_num_requests_sent =
                mock_transport.lock().await.requests_sent.len();
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
        .lock()
        .await
        .requests_sent
        .iter()
        .all(|request| request == &(op_id.clone(), agent_id.clone())));

    // Give time for more requests to be sent, which shouldn't happen now that the set of
    // ops to fetch is cleared.
    tokio::time::sleep(Duration::from_millis(10)).await;

    // No more requests should have been sent.
    // Ideally it were possible to check that no more fetch request have been passed back into
    // the internal channel, but that would require a custom wrapper around the channel.
    let requests_sent = mock_transport.lock().await.requests_sent.clone();
    assert_eq!(requests_sent.len(), num_requests_sent);
}

#[tokio::test(flavor = "multi_thread")]
async fn happy_multi_op_fetch_from_single_agent() {
    let config = CoreFetchConfig::default();
    let mock_transport = MockTransport::new(false);
    let mut fetch = CoreFetch::new(config.clone(), mock_transport.clone());

    let num_ops: usize = 50;
    let op_list = create_op_list(num_ops as u16);
    let agent = random_agent_id();

    let mut expected_ops = Vec::new();
    op_list
        .clone()
        .into_iter()
        .for_each(|op_id| expected_ops.push((op_id, agent.clone())));

    fetch.add_ops(op_list.clone(), agent.clone()).await.unwrap();

    // Check that at least one request was sent to the agent for each op.
    tokio::time::timeout(Duration::from_millis(100), async {
        loop {
            let requests_sent =
                mock_transport.lock().await.requests_sent.clone();
            if requests_sent.len() >= num_ops {
                op_list.clone().into_iter().all(|op_id| {
                    requests_sent.contains(&(op_id, agent.clone()))
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
    let config = CoreFetchConfig {
        parallel_request_count: 5,
        ..Default::default()
    };
    let mock_transport = MockTransport::new(false);
    let mut fetch = CoreFetch::new(config.clone(), mock_transport.clone());

    let op_list_1 = create_op_list(10);
    let agent_1 = random_agent_id();
    let op_list_2 = create_op_list(20);
    let agent_2 = random_agent_id();
    let op_list_3 = create_op_list(30);
    let agent_3 = random_agent_id();
    let total_ops = op_list_1.len() + op_list_2.len() + op_list_3.len();

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
            let requests_sent =
                mock_transport.lock().await.requests_sent.clone();
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
    let config = CoreFetchConfig::default();
    let mock_transport = MockTransport::new(true);
    let mut fetch = CoreFetch::new(config.clone(), mock_transport.clone());

    let op_list = create_op_list(1);
    let agent = random_agent_id();

    fetch.add_ops(op_list, agent.clone()).await.unwrap();

    tokio::time::timeout(Duration::from_millis(10), async {
        loop {
            if !mock_transport.lock().await.requests_sent.is_empty() {
                break;
            }
        }
    })
    .await
    .unwrap();

    let cool_down_list = fetch.0.state.lock().await.cool_down_list.clone();
    assert!(cool_down_list.contains_key(&agent));
}

#[tokio::test(flavor = "multi_thread")]
async fn agent_cooling_down_is_removed_from_list() {
    let config = CoreFetchConfig {
        cool_down_interval_ms: 10,
        ..Default::default()
    };
    let mock_transport = MockTransport::new(false);
    let fetch = CoreFetch::new(config.clone(), mock_transport.clone());
    let agent_id = random_agent_id();
    let now = Instant::now();

    fetch
        .0
        .state
        .lock()
        .await
        .cool_down_list
        .insert(agent_id.clone(), now);

    assert!(Inner::is_agent_cooling_down(
        &agent_id,
        &mut fetch.0.state.lock().await.cool_down_list,
        config.cool_down_interval_ms
    ));

    // Wait for the cool-down interval + 1 ms to avoid flakiness.
    tokio::time::sleep(Duration::from_millis(config.cool_down_interval_ms + 1))
        .await;

    assert!(!Inner::is_agent_cooling_down(
        &agent_id,
        &mut fetch.0.state.lock().await.cool_down_list,
        config.cool_down_interval_ms
    ));
}

#[tokio::test(flavor = "multi_thread")]
async fn multi_op_fetch_from_multiple_unresponsive_agents() {
    let config = CoreFetchConfig::default();
    let mock_transport = MockTransport::new(true);
    let mut fetch = CoreFetch::new(config.clone(), mock_transport.clone());

    let op_list_1 = create_op_list(10);
    let agent_1 = random_agent_id();
    let op_list_2 = create_op_list(20);
    let agent_2 = random_agent_id();
    let op_list_3 = create_op_list(30);
    let agent_3 = random_agent_id();

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
            let requests_sent =
                mock_transport.lock().await.requests_sent.clone();
            let request_destinations = requests_sent
                .iter()
                .map(|(_, agent_id)| agent_id)
                .collect::<Vec<_>>();
            if expected_agents
                .iter()
                .all(|agent| request_destinations.contains(&agent))
            {
                break;
            }
        }
    })
    .await
    .unwrap();

    // Check all agents are on cool_down_list.
    let cool_down_list = &fetch.0.state.lock().await.cool_down_list;
    assert!(expected_agents
        .iter()
        .all(|agent| cool_down_list.contains_key(agent)));
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