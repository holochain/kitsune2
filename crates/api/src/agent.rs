//! Types dealing with agent metadata.

use crate::*;

/// Agent metadata.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct AgentInfoMetadata {
    /// The agent id.
    agent: AgentId,

    /// The space id.
    space: SpaceId,
}
