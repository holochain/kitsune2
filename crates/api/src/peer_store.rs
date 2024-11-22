//! Peer-store related types.

use crate::*;
use std::sync::Arc;

/// Represents the ability to store and query agents.
pub trait PeerStore: 'static + Send + Sync + std::fmt::Debug {
    /* Do we need this??
     *
     * /// Clear (delete) the entire store.
     * /// Leave it ready to accept insertion of new entries.
     * fn clear(&self) -> BoxFut<'_, Result<()>>;
     */

    /// Insert agents into the store.
    fn insert(
        &self,
        agent_list: Vec<Arc<agent::AgentInfoSigned>>,
    ) -> BoxFut<'_, K2Result<()>>;

    /// Get an agent from the store.
    fn get(
        &self,
        agent: id::AgentId,
    ) -> BoxFut<'_, K2Result<Option<Arc<agent::AgentInfoSigned>>>>;

    /// Get all agents from the store.
    fn get_all(&self)
        -> BoxFut<'_, K2Result<Vec<Arc<agent::AgentInfoSigned>>>>;

    /* Do we need this??
     *
     * /// Get multiple agents from the store.
     * fn get_many(&self, agent_list: Vec<id::AgentId>) -> BoxFut<'_, K2Result<Vec<agent::AgentInfoSigned>>>;
     */

    /// Query the peer store by time and arc bounds.
    fn query_by_time_and_arq(
        &self,
        since: Timestamp,
        until: Timestamp,
        arc: agent::BasicArc,
    ) -> BoxFut<'_, K2Result<Vec<Arc<agent::AgentInfoSigned>>>>;

    /// Query the peer store by location nearness.
    fn query_by_location(
        &self,
        loc: u32,
        limit: usize,
    ) -> BoxFut<'_, K2Result<Vec<Arc<agent::AgentInfoSigned>>>>;
}

/// Trait-object [PeerStore].
pub type DynPeerStore = Arc<dyn PeerStore>;

/// A factory for constructing PeerStore instances.
pub trait PeerStoreFactory: 'static + Send + Sync + std::fmt::Debug {
    /// Help the builder construct a default config from the chosen
    /// module factories.
    fn default_config(&self, config: &mut config::Config);

    /// Construct a peer store instance.
    fn create(
        &self,
        builder: Arc<builder::Builder>,
    ) -> BoxFut<'static, K2Result<DynPeerStore>>;
}

/// Trait-object [PeerStoreFactory].
pub type DynPeerStoreFactory = Arc<dyn PeerStoreFactory>;
