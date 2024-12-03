use std::{sync::Arc, time::Duration};

use bytes::Bytes;
use kitsune2_api::{fetch::FetchQueue, id::Id, AgentId, OpId};
use rand::Rng;
use tokio::sync::Mutex;

use super::{QConfig, Tx, Q};

pub struct MockTx {
    requests_sent: Vec<(AgentId, Vec<OpId>, u32)>,
}

type DynMockTx = Arc<Mutex<MockTx>>;

impl MockTx {
    fn new() -> DynMockTx {
        Arc::new(Mutex::new(Self {
            requests_sent: Vec::new(),
        }))
    }
}

impl Tx for MockTx {
    fn send_op_request(
        &mut self,
        source: AgentId,
        op_list: Vec<OpId>,
        max_byte_count: u32,
    ) -> kitsune2_api::BoxFut<'static, kitsune2_api::K2Result<()>> {
        self.requests_sent.push((source, op_list, max_byte_count));
        Box::pin(async move { Ok(()) })
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn chunked_op_requests() {
    let config = QConfig::default();
    let mock_tx = MockTx::new();
    let mut q = Q::new(config.clone(), mock_tx.clone());

    // let num_ops = config.max_hash_count + 1;
    let num_ops: u8 = 2;
    let op_list = create_op_list(num_ops as u16);
    let source = random_agent_id();
    q.add_ops(op_list.clone(), source.clone()).await.unwrap();

    let expected_num_requests = num_ops.div_ceil(config.max_hash_count);

    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let requests_sent = mock_tx.lock().await.requests_sent.clone();
            if requests_sent.len() < expected_num_requests as usize {
                tokio::time::sleep(Duration::from_millis(10)).await;
            } else {
                assert_eq!(requests_sent.len(), expected_num_requests as usize);
                op_list
                    .clone()
                    .chunks(config.max_hash_count as usize)
                    .enumerate()
                    .for_each(|(index, op_ids)| {
                        assert_eq!(requests_sent[index].0, source);
                        assert_eq!(requests_sent[index].1, op_ids);
                        assert_eq!(
                            requests_sent[index].2,
                            QConfig::default().max_byte_count
                        );
                    });
                break;
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
