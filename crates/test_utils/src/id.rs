//! Test utilities associated with ids.

use bytes::Bytes;
use kitsune2_api::*;

use crate::random_bytes;

/// Create a random id.
pub fn random_id() -> Id {
    Id(Bytes::from(random_bytes(32)))
}

/// Create a random agent id.
pub fn random_agent_id() -> AgentId {
    AgentId(random_id())
}

/// Create a random op id.
pub fn random_op_id() -> OpId {
    OpId(random_id())
}

/// Creates a Vec of random ops
pub fn create_op_id_list(num_ops: u16) -> Vec<OpId> {
    let mut ops = Vec::with_capacity(num_ops as usize);
    for _ in 0..num_ops {
        let op_id = random_op_id();
        ops.push(op_id);
    }
    ops
}
