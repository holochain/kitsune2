use crate::{builder, config, AgentId, BoxFut, K2Result, SpaceId};
use futures::future::BoxFuture;
use std::sync::Arc;

/// A store for peer metadata.
///
/// This is expected to be backed by a key-value store that keys by space, agent_id and key.
pub trait PeerMetaStore: 'static + Send + Sync + std::fmt::Debug {
    /// Store a key-value pair for a given space and agent.
    fn put(
        &self,
        space: SpaceId,
        agent: AgentId,
        key: String,
        value: Vec<u8>,
    ) -> BoxFuture<'_, K2Result<()>>;

    /// Get a value by key for a given space and agent.
    fn get(
        &self,
        space: SpaceId,
        agent: AgentId,
        key: String,
    ) -> BoxFuture<'_, K2Result<Option<Vec<u8>>>>;

    /// Delete a key-value pair for a given space and agent.
    fn delete(
        &self,
        space: SpaceId,
        agent: AgentId,
        key: String,
    ) -> BoxFuture<'_, K2Result<()>>;
}

/// Trait-object version of kitsune2 [PeerMetaStore].
pub type DynPeerMetaStore = Arc<dyn PeerMetaStore>;

/// A factory for constructing [PeerMetaStore] instances.
pub trait PeerMetaStoreFactory:
    'static + Send + Sync + std::fmt::Debug
{
    /// Help the builder construct a default config from the chosen
    /// module factories.
    fn default_config(&self, config: &mut config::Config) -> K2Result<()>;

    /// Construct a meta store instance.
    fn create(
        &self,
        builder: Arc<builder::Builder>,
    ) -> BoxFut<'static, K2Result<DynPeerMetaStore>>;
}

/// Trait-object [PeerMetaStoreFactory].
pub type DynPeerMetaStoreFactory = Arc<dyn PeerMetaStoreFactory>;
