mod incoming_request_queue;
mod outgoing_request_queue;

#[cfg(test)]
pub(crate) mod utils {
    use crate::factories::MemoryOp;
    use bytes::Bytes;
    use kitsune2_api::{id::Id, AgentId, OpId, Timestamp};
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

    pub fn make_op(data: Vec<u8>) -> MemoryOp {
        MemoryOp::new(Timestamp::now(), data)
    }
}
