//! Gossip related types.

use crate::fetch::DynFetch;
use crate::peer_store::DynPeerStore;
use crate::transport::DynTransport;
use crate::{
    builder, config, BoxFut, DhtArc, DynLocalAgentStore, DynOpStore,
    DynPeerMetaStore, K2Result, SpaceId, StoredOp, Timestamp, Url,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
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
    ) -> BoxFut<'_, K2Result<GossipStateSummary>>;
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

/// DHT segment state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DhtSegmentState {
    /// The top hash of the DHT ring segment.
    pub disc_top_hash: bytes::Bytes,
    /// The boundary timestamp of the DHT ring segment.
    pub disc_boundary: Timestamp,
    /// The top hashes of each DHT ring segment.
    pub ring_top_hashes: Vec<bytes::Bytes>,
}

/// Peer metadata dump.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerMeta {
    /// The timestamp of the last gossip round.
    pub last_gossip_timestamp: Option<Timestamp>,
    /// The bookmark of the last op bookmark received.
    pub new_ops_bookmark: Option<Timestamp>,
    /// The number of behavior errors observed.
    pub peer_behavior_errors: Option<u32>,
    /// The number of local errors.
    pub local_errors: Option<u32>,
    /// The number of busy peer errors.
    pub peer_busy: Option<u32>,
    /// The number of terminated rounds.
    ///
    /// Note that termination is not necessarily an error.
    pub peer_terminated: Option<u32>,
    /// The timestamp when the peer was marked unresponsive.
    pub peer_unresponsive: Option<Timestamp>,
    /// The number of completed rounds.
    pub completed_rounds: Option<u32>,
    /// The number of peer timeouts.
    pub peer_timeouts: Option<u32>,
    /// Whether this peer has declared itself as offline, and no longer reachable, with a tombstone.
    pub is_tombstone: bool,
    /// The storage arc that this peer is declaring.
    pub storage_arc: DhtArc,
}

/// Gossip round state summary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GossipRoundStateSummary {
    /// The URL of the peer with which the round is initiated.
    pub session_with_peer: Url,
}

/// Gossip state summary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GossipStateSummary {
    /// The current initiated round summary.
    pub initiated_round: Option<GossipRoundStateSummary>,
    /// The list of accepted round summaries.
    pub accepted_rounds: Vec<GossipRoundStateSummary>,
    /// DHT summary.
    pub dht_summary: HashMap<String, DhtSegmentState>,
    /// Peer metadata dump for each agent in this space.
    pub peer_meta: HashMap<Url, PeerMeta>,
}
