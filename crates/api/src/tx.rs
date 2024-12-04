//! Kitsune2 transport module.

use crate::{AgentId, BoxFut, K2Result, OpId};

/// Trait for implementing a transport module for exchanging messages
/// between agents.
pub trait Transport: 'static + Send + Sync + std::fmt::Debug {
    /// Send a request for ops to a peer.
    fn send_op_request(
        &mut self,
        op_id: OpId,
        dest: AgentId,
    ) -> BoxFut<'static, K2Result<()>>;
}
