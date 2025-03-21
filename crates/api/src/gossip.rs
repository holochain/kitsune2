//! Gossip related types.

use crate::fetch::DynFetch;
use crate::peer_store::DynPeerStore;
use crate::transport::DynTransport;
use crate::{
    builder, config, BoxFut, DynLocalAgentStore, DynOpStore, DynPeerMetaStore,
    K2Result, SpaceId, StoredOp,
};
use std::sync::Arc;

/// Request for a gossip state summary.
#[derive(Debug, Clone)]
pub struct GossipStateSummaryRequest {
    /// Include DHT summary in the response.
    pub include_dht_summary: bool,
}

/// Represents the ability to sync DHT data with other agents through background communication.
pub trait Gossip: 'static + Send + Sync + std::fmt::Debug {
    /// Inform the gossip module that a set of ops have been stored.
    ///
    /// This is not expected to be called directly. It is intended to be used by the
    /// space that owns this gossip module. See [crate::space::Space::inform_ops_stored].
    fn inform_ops_stored(&self, ops: Vec<StoredOp>)
        -> BoxFut<'_, K2Result<()>>;

    /// Get a state summary from the gossip module.
    fn get_state_summary(
        &self,
        request: GossipStateSummaryRequest,
    ) -> BoxFut<'_, K2Result<serde_json::Value>>;
}

/// Trait-object [Gossip].
pub type DynGossip = Arc<dyn Gossip>;

/// A factory for constructing [Gossip] instances.
pub trait GossipFactory: 'static + Send + Sync + std::fmt::Debug {
    /// Help the builder construct a default config from the chosen
    /// module factories.
    fn default_config(&self, config: &mut config::Config) -> K2Result<()>;

    /// Validate configuration.
    fn validate_config(&self, config: &config::Config) -> K2Result<()>;

    /// Construct a gossip instance.
    #[allow(clippy::too_many_arguments)]
    fn create(
        &self,
        builder: Arc<builder::Builder>,
        space_id: SpaceId,
        peer_store: DynPeerStore,
        local_agent_store: DynLocalAgentStore,
        peer_meta_store: DynPeerMetaStore,
        op_store: DynOpStore,
        transport: DynTransport,
        fetch: DynFetch,
    ) -> BoxFut<'static, K2Result<DynGossip>>;
}

/// Trait-object [GossipFactory].
pub type DynGossipFactory = Arc<dyn GossipFactory>;
