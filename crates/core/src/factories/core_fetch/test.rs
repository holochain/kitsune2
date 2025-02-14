mod incoming_request_queue;
mod incoming_response_queue;
mod outgoing_request_queue;

#[cfg(test)]
pub(crate) mod test_utils {
    use crate::factories::MemoryOp;
    use kitsune2_api::{Timestamp, Url};
    use rand::RngCore;

    pub fn random_peer_url() -> Url {
        let id = rand::thread_rng().next_u32();
        Url::from_str(format!("ws://test:80/{id}")).unwrap()
    }

    pub fn make_op(data: Vec<u8>) -> MemoryOp {
        MemoryOp::new(Timestamp::now(), data)
    }
}
