use std::{sync::Arc, time::Duration};

use bytes::Bytes;
use kitsune2_api::{fetch::Fetch, id::Id, AgentId, OpId};
use rand::Rng;
use tokio::sync::Mutex;

use super::{CoreFetch, CoreFetchConfig, Transport};

#[derive(Debug)]
pub struct MockTransport {
    requests_sent: Vec<(OpId, AgentId)>,
}

type DynMockTransport = Arc<Mutex<MockTransport>>;

impl MockTransport {
    fn new() -> DynMockTransport {
        Arc::new(Mutex::new(Self {
            requests_sent: Vec::new(),
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
        Box::pin(async move { Ok(()) })
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn parallel_request_count_is_not_exceeded() {
    let config = CoreFetchConfig {
        parallel_request_count: 1,
        parallel_request_pause: 10000, // 10 seconds to be certain that no other request is sent during the test
        ..Default::default()
    };
    let mock_tx = MockTransport::new();
    let mut fetch = CoreFetch::new(config.clone(), mock_tx.clone());

    let op_list = create_op_list(2);
    let source = random_agent_id();
    fetch.add_ops(op_list, source).await.unwrap();

    // Wait until some request has been sent.
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let requests_sent = mock_tx.lock().await.requests_sent.clone();
            if requests_sent.is_empty() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            } else {
                break;
            }
        }
    })
    .await
    .unwrap();

    let requests_sent = mock_tx.lock().await.requests_sent.clone();
    assert_eq!(
        requests_sent.len(),
        config.parallel_request_count() as usize
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn happy_multi_op_fetch_from_one_agent() {
    let config = CoreFetchConfig::default();
    let mock_tx = MockTransport::new();
    let mut fetch = CoreFetch::new(config.clone(), mock_tx.clone());

    let num_ops: u8 = 50;
    let op_list = create_op_list(num_ops as u16);
    let source = random_agent_id();
    fetch
        .add_ops(op_list.clone(), source.clone())
        .await
        .unwrap();

    // Check that at least one request was sent to the source for each op.
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let requests_sent = mock_tx.lock().await.requests_sent.clone();
            if requests_sent.len() < num_ops as usize {
                tokio::time::sleep(Duration::from_millis(100)).await;
            } else {
                assert!(requests_sent.len() >= num_ops as usize);
                op_list.clone().into_iter().for_each(|op_id| {
                    assert!(requests_sent.contains(&(op_id, source.clone(),)));
                });
                break;
            }
        }
    })
    .await
    .unwrap();

    // Check that the ops that are being fetched are appended to the end of
    // the queue with the correct source.
    let ops = fetch.0.ops.lock().await;
    op_list
        .clone()
        .into_iter()
        .for_each(|op_id| assert!(ops.contains_key(&(op_id, source.clone()))));
}

#[tokio::test(flavor = "multi_thread")]
async fn happy_multi_op_fetch_from_multiple_agents() {
    let config = CoreFetchConfig {
        parallel_request_count: 5,
        ..Default::default()
    };
    let mock_tx = MockTransport::new();
    let mut fetch = CoreFetch::new(config.clone(), mock_tx.clone());

    let op_list_1 = create_op_list(10);
    let source_1 = random_agent_id();
    let op_list_2 = create_op_list(20);
    let source_2 = random_agent_id();
    let op_list_3 = create_op_list(30);
    let source_3 = random_agent_id();
    let total_ops = op_list_1.len() + op_list_2.len() + op_list_3.len();

    fetch
        .add_ops(op_list_1.clone(), source_1.clone())
        .await
        .unwrap();
    fetch
        .add_ops(op_list_2.clone(), source_2.clone())
        .await
        .unwrap();
    fetch
        .add_ops(op_list_3.clone(), source_3.clone())
        .await
        .unwrap();

    // Check that at least one request was sent for each op.
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let requests_sent = mock_tx.lock().await.requests_sent.clone();
            if requests_sent.len() < total_ops {
                tokio::time::sleep(Duration::from_millis(100)).await;
            } else {
                assert!(requests_sent.len() >= total_ops);
                op_list_1.clone().into_iter().for_each(|op_id| {
                    assert!(requests_sent.contains(&(op_id, source_1.clone(),)));
                });
                op_list_2.clone().into_iter().for_each(|op_id| {
                    assert!(requests_sent.contains(&(op_id, source_2.clone(),)));
                });
                op_list_3.clone().into_iter().for_each(|op_id| {
                    assert!(requests_sent.contains(&(op_id, source_3.clone(),)));
                });
                break;
            }
        }
    })
    .await
    .unwrap();

    // Check that the ops that are being fetched are appended to the end of
    // the queue with the correct source.
    let ops = fetch.0.ops.lock().await;
    assert_eq!(ops.len(), total_ops);
    op_list_1.clone().into_iter().for_each(|op_id| {
        assert!(ops.contains_key(&(op_id, source_1.clone())))
    });
    op_list_2.clone().into_iter().for_each(|op_id| {
        assert!(ops.contains_key(&(op_id, source_2.clone())))
    });
    op_list_3.clone().into_iter().for_each(|op_id| {
        assert!(ops.contains_key(&(op_id, source_3.clone())))
    });
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
