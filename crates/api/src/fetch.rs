//! Kitsune2 fetch queue types.

use std::sync::Arc;

use crate::{AgentId, OpId};

/// Trait for implementing a fetch queue for managing ops
/// to be fetched from other agents.
pub trait FetchQueueT: 'static + Send + Sync {
    /// Add op ids to be fetched to the queue.
    fn add_ops(&mut self, op_list: Vec<OpId>, source: AgentId);

    /// Get a list of op ids to be fetched.
    fn get_ops_to_fetch(&self) -> Vec<OpId>;

    /// Get a random agent that holds the ops to be fetched.
    fn get_random_source(&self) -> Option<AgentId>;
}

/// Trait-object fetch queue instance.
pub type DynFetchQueue = Arc<dyn FetchQueueT>;

/// Trait for implementing a fetch task that periodically attempts to
/// send op fetch requests to another agent.
pub trait FetchTaskT {
    /// Spawn a new fetch task, backed by a fetch queue.
    fn spawn(&self, fetch_queue: DynFetchQueue);
}

/// Configuration for [`FetchTaskT`].
pub struct FetchTaskConfig {
    /// How long to pause after sending a fetch request, before the next attempt.
    pub pause_between_runs: u64, // in ms
}

impl FetchTaskConfig {
    /// Default fetch task config.
    pub fn default() -> Self {
        Self {
            pause_between_runs: 1000 * 5, // 5 seconds
        }
    }
}
