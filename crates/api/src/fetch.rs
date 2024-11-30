//! Kitsune2 fetch queue types.

use std::sync::Arc;

use crate::{AgentId, BoxFut, K2Result, OpId};

/// Trait for implementing a fetch queue for managing ops
/// to be fetched from other agents.
pub trait FetchQueue: 'static + Send + Sync {
    /// Add op ids to be fetched to the queue.
    fn add_ops(&mut self, op_list: Vec<OpId>, source: AgentId);
}

/// Trait object [FetchQueue].
pub type DynFetchQueue = Arc<dyn FetchQueue>;

/// A factory for creating FetchQueue instances.
pub trait FetchQueueFactory: 'static + Send + Sync + std::fmt::Debug {
    /// Construct a fetch queue instance.
    fn create(&self) -> BoxFut<'static, K2Result<DynFetchQueue>>;
}

/// Trait object [FetchQueueFactory].
pub type DynFetchQueueFactory = Arc<dyn FetchQueueFactory>;
