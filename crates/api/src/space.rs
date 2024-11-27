//! Kitsune2 space related types.

use crate::*;
use std::sync::Arc;

/// Represents a unique dht space within which to communicate with peers.
pub trait Space: 'static + Send + Sync + std::fmt::Debug {
    /// Get a reference to the peer store being used by this space.
    /// This could allow you to inject peer info from some source other
    /// than gossip or bootstrap, or to query the store directly if
    /// you have a need to determine peers for direct messaging.
    fn peer_store(&self) -> &peer_store::DynPeerStore;

    /// Indicate that an agent is now online, and should begin receiving
    /// messages and exchanging dht information.
    fn local_agent_join(
        &self,
        local_agent: agent::DynLocalAgent,
    ) -> BoxFut<'_, K2Result<()>>;

    /// Indicate that an agent is no longer online, and should stop
    /// receiving messages and exchanging dht information.
    /// Before the agent is actually removed a tombstone agent info will
    /// be generated and sent to the bootstrap server. A best effort will
    /// be made to publish this tomstone to peers in the space as well.
    fn local_agent_leave(&self, local_agent: id::AgentId) -> BoxFut<'_, ()>;
}

/// Trait-object [Space].
pub type DynSpace = Arc<dyn Space>;

/// A factory for constructing Space instances.
pub trait SpaceFactory: 'static + Send + Sync + std::fmt::Debug {
    /// Help the builder construct a default config from the chosen
    /// module factories.
    fn default_config(&self, config: &mut config::Config) -> K2Result<()>;

    /// Construct a peer store instance.
    fn create(
        &self,
        builder: Arc<builder::Builder>,
    ) -> BoxFut<'static, K2Result<DynSpace>>;
}

/// Trait-object [SpaceFactory].
pub type DynSpaceFactory = Arc<dyn SpaceFactory>;
