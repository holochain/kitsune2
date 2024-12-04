use std::{sync::Arc, time::Duration};

use bytes::Bytes;
use kitsune2_api::{fetch::Fetch, id::Id, AgentId, OpId};
use rand::Rng;
use tokio::sync::Mutex;

use super::{Kitsune2Fetch, Kitsune2FetchConfig, Transport};

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
async fn multi_op_fetch_from_one_agent() {
    let config = Kitsune2FetchConfig::default();
    let mock_tx = MockTransport::new();
    let mut fetch = Kitsune2Fetch::new(config.clone(), mock_tx.clone());

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
