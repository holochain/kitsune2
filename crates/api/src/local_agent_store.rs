use crate::agent::DynLocalAgent;
use crate::{BoxFut, K2Result, builder, config, id};
use std::sync::Arc;

/// A store for local agents.
///
/// These are the agents that are running on the current Kitsune2 instance.
pub trait LocalAgentStore: 'static + Send + Sync + std::fmt::Debug {
    /// Add a local agent to the store.
    fn add(&self, local_agent: DynLocalAgent) -> BoxFut<'_, K2Result<()>>;

    /// Remove a local agent from the store.
    fn remove(
        &self,
        local_agent: id::AgentId,
    ) -> BoxFut<'_, Option<DynLocalAgent>>;

    /// Get a list of all local agents currently in the store.
    fn get_all(&self) -> BoxFut<'_, K2Result<Vec<DynLocalAgent>>>;
}

/// Trait-object version of kitsune2 [LocalAgentStore].
pub type DynLocalAgentStore = Arc<dyn LocalAgentStore>;

/// A factory for constructing [LocalAgentStore] instances.
pub trait LocalAgentStoreFactory:
    'static + Send + Sync + std::fmt::Debug
{
    /// Help the builder construct a default config from the chosen
    /// module factories.
    fn default_config(&self, config: &mut config::Config) -> K2Result<()>;

    /// Validate configuration.
    fn validate_config(&self, config: &config::Config) -> K2Result<()>;

    /// Construct a local agent store instance.
    fn create(
        &self,
        builder: Arc<builder::Builder>,
    ) -> BoxFut<'static, K2Result<DynLocalAgentStore>>;
}

/// Trait-object [LocalAgentStoreFactory].
pub type DynLocalAgentStoreFactory = Arc<dyn LocalAgentStoreFactory>;
