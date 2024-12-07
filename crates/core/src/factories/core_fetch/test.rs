use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use bytes::Bytes;
use kitsune2_api::{fetch::Fetch, id::Id, AgentId, K2Error, OpId};
use rand::Rng;
use tokio::sync::Mutex;

use super::{CoreFetch, CoreFetchConfig, Transport};

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
            Box::pin(async move {
                tokio::time::sleep(Duration::from_millis(10)).await;
                Err(K2Error::other("connection timed out"))
            })
        } else {
            Box::pin(async move { Ok(()) })
        }
    }
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
    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            let requests_sent =
                mock_transport.lock().await.requests_sent.clone();
            if requests_sent.len() >= num_ops {
                op_list.clone().into_iter().all(|op_id| {
                    requests_sent.contains(&(op_id, agent.clone()))
                });
                break;
            } else {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }
    })
    .await
    .unwrap();

    // Assert that not an excessive amount of redundant requests was sent.
    let requests_sent = mock_transport.lock().await.requests_sent.clone();
    assert!(
        requests_sent.len() < num_ops + 10,
        "sent {} requests",
        requests_sent.len()
    );

    let ops = fetch.0.ops.lock().await;
    expected_ops
        .into_iter()
        .all(|(op_id, agent_id)| ops.contains(&(op_id, agent_id)));
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
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let requests_sent =
                mock_transport.lock().await.requests_sent.clone();
            if requests_sent.len() >= total_ops
                && expected_ops
                    .iter()
                    .all(|expected_op| requests_sent.contains(expected_op))
            {
                break;
            } else {
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
    })
    .await
    .unwrap();

    // Assert that not an excessive amount of redundant requests was sent.
    let requests_sent = mock_transport.lock().await.requests_sent.clone();
    assert!(
        requests_sent.len() < total_ops * 2,
        "sent {} requests",
        requests_sent.len()
    );

    // Leave time for all request threads to complete and re-insert op ids into the data object.
    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            let ops = fetch.0.ops.lock().await.clone();
            if expected_ops
                .iter()
                .all(|expected_op| ops.contains(expected_op))
            {
                break;
            } else {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn unresponsive_agent_is_put_on_cool_down_list() {
    let config = CoreFetchConfig::default();
    let mock_transport = MockTransport::new(true);
    let mut fetch = CoreFetch::new(config.clone(), mock_transport.clone());

    let op_list = create_op_list(1);
    let agent = random_agent_id();

    fetch.add_ops(op_list, agent.clone()).await.unwrap();

    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            let requests_sent =
                mock_transport.lock().await.requests_sent.clone();
            if !requests_sent.is_empty() {
                break;
            } else {
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
    })
    .await
    .unwrap();

    let cool_down_list = fetch.0.cool_down_list.lock().await.clone();
    assert!(cool_down_list.contains_key(&agent));
}

#[tokio::test(flavor = "multi_thread")]
async fn purge_cool_down_list() {
    let config = CoreFetchConfig::default();
    let mock_transport = MockTransport::new(false);
    let fetch = CoreFetch::new(config.clone(), mock_transport.clone());
    let agent = random_agent_id();

    // Add agent to cool-down list.
    fetch
        .0
        .cool_down_list
        .lock()
        .await
        .insert(agent.clone(), Instant::now());

    // Purge list 10 ms after cool down interval has passed.
    fetch
        .0
        .purge_cool_down_list(
            Instant::now()
                + Duration::from_millis(config.cool_down_interval + 1),
        )
        .await;

    assert!(fetch.0.cool_down_list.lock().await.is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn unresponsive_agent_is_removed_from_cool_down_list_after_interval() {
    let config = CoreFetchConfig {
        // Set short cool down interval for testing.
        cool_down_interval: 100,
        ..Default::default()
    };
    let mock_transport = MockTransport::new(false);
    let mut fetch = CoreFetch::new(config.clone(), mock_transport.clone());

    let op_list = create_op_list(1);
    let responsive_agent = random_agent_id();
    let unresponsive_agent = random_agent_id();

    // Add unresponsive agent to cool-down list.
    fetch
        .0
        .cool_down_list
        .lock()
        .await
        .insert(unresponsive_agent.clone(), Instant::now());

    // Add ops from unresponsive agent first.
    fetch
        .add_ops(op_list.clone(), unresponsive_agent.clone())
        .await
        .unwrap();
    fetch
        .add_ops(op_list, responsive_agent.clone())
        .await
        .unwrap();

    // Wait until requests have been sent.
    tokio::time::timeout(Duration::from_millis(100), async {
        loop {
            let requests_sent =
                mock_transport.lock().await.requests_sent.clone();
            if !requests_sent.is_empty() {
                break;
            } else {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }
    })
    .await
    .unwrap();

    // Check that no requests have been sent to unresponsive agent.
    let requests_sent = mock_transport.lock().await.requests_sent.clone();
    assert!(requests_sent
        .iter()
        .all(|(_, dest)| *dest != unresponsive_agent));

    // Wait for cool down interval to pass.
    tokio::time::sleep(Duration::from_millis(config.cool_down_interval)).await;

    // Check that the agent has been removed from the cool-down list.
    let cool_down_list = fetch.0.cool_down_list.lock().await.clone();
    assert!(cool_down_list.is_empty());
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

    let expected_agents = [agent_1, agent_2, agent_3];
    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            let request_destinations =
                mock_transport.lock().await.requests_sent.clone();
            let request_destinations = request_destinations
                .iter()
                .map(|(_, agent_id)| agent_id)
                .collect::<Vec<_>>();
            if expected_agents
                .iter()
                .all(|agent| request_destinations.contains(&agent))
            {
                break;
            } else {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }
    })
    .await
    .unwrap();

    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            let cool_down_list = fetch.0.cool_down_list.lock().await;
            if expected_agents
                .iter()
                .all(|agent| cool_down_list.contains_key(agent))
            {
                break;
            } else {
                tokio::time::sleep(Duration::from_millis(10)).await;
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
