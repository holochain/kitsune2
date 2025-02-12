#[cfg(test)]
pub(crate) mod test_utils {
    use crate::factories::MemoryOp;
    use kitsune2_api::{OpId, Timestamp, Url};
    use kitsune2_test_utils::id::random_op_id;
    use rand::RngCore;

    pub fn random_peer_url() -> Url {
        let id = rand::thread_rng().next_u32();
        Url::from_str(format!("ws://test:80/{id}")).unwrap()
    }

    pub fn create_op_id_list(num_ops: u16) -> Vec<OpId> {
        let mut ops = Vec::with_capacity(num_ops as usize);
        for _ in 0..num_ops {
            let op_id = random_op_id();
            ops.push(op_id);
        }
        ops
    }

    pub fn make_op(data: Vec<u8>) -> MemoryOp {
        MemoryOp::new(Timestamp::now(), data)
    }
}
