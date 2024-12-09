//! Kitsune2 fetch types.

use std::sync::Arc;

use crate::{builder, config, AgentId, BoxFut, K2Result, OpId};

/// Trait for implementing a fetch module to fetch ops from other agents.
pub trait Fetch: 'static + Send + Sync + std::fmt::Debug {
    /// Add op ids to be fetched.
    fn add_ops(
        &mut self,
        op_list: Vec<OpId>,
        source: AgentId,
    ) -> BoxFut<'_, K2Result<()>>;
}

/// Trait object [Fetch].
pub type DynFetch = Arc<dyn Fetch>;

/// A factory for creating Fetch instances.
pub trait FetchFactory: 'static + Send + Sync + std::fmt::Debug {
    /// Help the builder construct a default config from the chosen
    /// module factories.
    fn default_config(&self, config: &mut config::Config) -> K2Result<()>;

    /// Construct a Fetch instance.
    fn create(
        &self,
        builder: Arc<builder::Builder>,
    ) -> BoxFut<'static, K2Result<DynFetch>>;
}

/// Trait object [FetchFactory].
pub type DynFetchFactory = Arc<dyn FetchFactory>;
