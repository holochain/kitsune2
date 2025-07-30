//! Peer-store related types.

use crate::*;
use std::sync::Arc;

/// Represents the ability to store and query agents.
pub trait PeerStore: 'static + Send + Sync + std::fmt::Debug {
    /// Insert agents into the store.
    fn insert(
        &self,
        agent_list: Vec<Arc<AgentInfoSigned>>,
    ) -> BoxFut<'_, K2Result<()>>;

    /// Remove agent from the store with the passed [`AgentId`].
    fn remove(&self, agent_id: AgentId) -> BoxFut<'_, K2Result<()>>;

    /// Get an agent from the store.
    fn get(
        &self,
        agent: AgentId,
    ) -> BoxFut<'_, K2Result<Option<Arc<AgentInfoSigned>>>>;

    /// Get all agents from the store.
    fn get_all(&self) -> BoxFut<'_, K2Result<Vec<Arc<AgentInfoSigned>>>>;

    /// Get the complete list of agents we know about that
    /// claim storage_arcs that overlap the provided storage arc.
    /// This function, provided with the storage arc of a currently active
    /// local agent, will return a list of agents with whom
    /// to gossip (and also the local agent). If there are multiple
    /// local agents in this space, you'll need to call this function
    /// multiple times and union the results.
    fn get_by_overlapping_storage_arc(
        &self,
        arc: DhtArc,
    ) -> BoxFut<'_, K2Result<Vec<Arc<AgentInfoSigned>>>>;

    /// Get a list of agents sorted by nearness to a target basis location.
    /// Offline (tombstoned) agents, and agents with zero arcs are not
    /// included in the returned list.
    fn get_near_location(
        &self,
        loc: u32,
        limit: usize,
    ) -> BoxFut<'_, K2Result<Vec<Arc<AgentInfoSigned>>>>;
}

/// Trait-object [PeerStore].
pub type DynPeerStore = Arc<dyn PeerStore>;

/// A factory for constructing [PeerStore] instances.
pub trait PeerStoreFactory: 'static + Send + Sync + std::fmt::Debug {
    /// Help the builder construct a default config from the chosen
    /// module factories.
    fn default_config(&self, config: &mut Config) -> K2Result<()>;

    /// Validate configuration.
    fn validate_config(&self, config: &Config) -> K2Result<()>;

    /// Construct a peer store instance.
    fn create(
        &self,
        builder: Arc<Builder>,
        space_id: SpaceId,
    ) -> BoxFut<'static, K2Result<DynPeerStore>>;
}

/// Trait-object [PeerStoreFactory].
pub type DynPeerStoreFactory = Arc<dyn PeerStoreFactory>;
