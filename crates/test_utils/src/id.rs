//! Test utilities associated with ids.

use bytes::Bytes;
use kitsune2_api::{id::Id, AgentId, OpId};

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
