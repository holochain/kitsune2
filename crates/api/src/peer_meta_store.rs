//! Allows Kitsune2 to store metadata about peers.
//!
//! This enables Kitsune2 to keep track of peer state and uptime etc. More docs to come :)

use crate::{AgentId, Timestamp};
use futures::future::BoxFuture;
use std::sync::Arc;

/// Metadata about a peer.
#[derive(Debug, Clone)]
pub struct PeerMeta {
    /// The agent id of the peer.
    pub agent_id: AgentId,

    /// The last time we received an op from this peer.
    ///
    /// Note that this is not a timestamp calculated by ourselves. It is based on the timestamp of
    /// the last op we received from this peer. That means we should always be able to use this as
    /// a bookmark for requesting more ops from this peer in the future.
    pub last_gossip_timestamp: Option<Timestamp>,
}

impl PeerMeta {
    /// Create a new peer meta.
    pub fn new(agent_id: AgentId) -> Self {
        Self {
            agent_id,
            last_gossip_timestamp: None,
        }
    }
}

/// The API that a kitsune2 host must implement to provide peer metadata persistence for kitsune2.
///
/// Peer metadata is used between gossip rounds to keep track of peer state and uptime etc.
pub trait PeerMetaStore: 'static + Send + Sync + std::fmt::Debug {
    /// The error type for this op store.
    type Error: std::error::Error;

    /// Get the metadata for a peer.
    ///
    /// If we haven't seen this peer before, return None.
    /// Otherwise, return the latest state stored for this peer.
    fn get_peer_meta(
        &self,
        agent_id: AgentId,
    ) -> BoxFuture<'_, Result<Option<PeerMeta>, Self::Error>>;

    /// Store metadata for a peer.
    ///
    /// This should overwrite existing metadata for this peer.
    fn store_peer_meta(
        &self,
        peer_meta: PeerMeta,
    ) -> BoxFuture<'_, Result<(), Self::Error>>;
}

/// Trait-object version of the kitsune2 peer meta store.
pub type DynPeerMetaStore<Error> = Arc<dyn PeerMetaStore<Error = Error>>;
