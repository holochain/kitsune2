//! Kitsune2 transport module.

use crate::{AgentId, BoxFut, K2Result, OpId};

/// Trait for implementing a transport module for exchanging messages
/// between agents.
pub trait Tx: 'static + Send + Sync + std::fmt::Debug {
    /// Send a request for ops to a peer.
    fn send_op_request(
        &mut self,
        source: AgentId,
        op_list: Vec<OpId>,
        max_byte_count: u32,
    ) -> BoxFut<'static, K2Result<()>>;
}
