//! Kitsune2 fetch queue types.

use crate::{AgentId, OpId};

/// Trait for implementing a fetch queue that fetches ops
/// from other agents.
pub trait FetchQueueT: 'static + Send + Sync {
    /// Add op ids to be fetched to the queue.
    fn add_ops(&mut self, op_list: Vec<OpId>, source: AgentId);
}

/// Trait-object fetch queue instance.
pub type DynFetchQueue = dyn FetchQueueT;
