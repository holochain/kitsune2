mod incoming_request_queue;
mod outgoing_request_queue;

#[cfg(test)]
pub(crate) mod utils {
    use crate::factories::Kitsune2MemoryOp;
    use bytes::Bytes;
    use kitsune2_api::{id::Id, AgentId, MetaOp, OpId, Timestamp};
    use rand::Rng;

    pub fn random_id() -> Id {
        let mut rng = rand::thread_rng();
        let mut bytes = [0u8; 32];
        rng.fill(&mut bytes);
        let bytes = Bytes::from(bytes.to_vec());
        Id(bytes)
    }

    pub fn random_op_id() -> OpId {
        OpId(random_id())
    }

    pub fn random_agent_id() -> AgentId {
        AgentId(random_id())
    }

    pub fn create_op_list(num_ops: u16) -> Vec<OpId> {
        let mut ops = Vec::new();
        for _ in 0..num_ops {
            let op = random_op_id();
            ops.push(op.clone());
        }
        ops
    }

    pub fn hash_op(input: &bytes::Bytes) -> OpId {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(input);
        let result = hasher.finalize();
        let hash_bytes = bytes::Bytes::from(result.to_vec());
        hash_bytes.into()
    }

    pub fn make_op(data: Vec<u8>) -> MetaOp {
        let op_id = hash_op(&data.clone().into());
        MetaOp {
            op_id: op_id.clone(),
            op_data: serde_json::to_vec(&Kitsune2MemoryOp::new(
                op_id,
                Timestamp::now(),
                data,
            ))
            .unwrap(),
        }
    }
}
