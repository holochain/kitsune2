use crate::{K2Result, Timestamp, Url};

/// The decision made about access for a peer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccessDecision {
    /// Access is granted to the peer.
    Granted,
    /// Access is blocked for the peer.
    Blocked,
}

/// The access information for a peer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerAccess {
    /// The access decision for the peer.
    pub decision: AccessDecision,

    /// The timestamp when the decision was made.
    pub decided_at: Timestamp,
}

/// Trait for tracking access state of peers.
pub trait PeerAccessState: 'static + Send + Sync + std::fmt::Debug {
    /// Get a previously made access decision for a peer at the given URL.
    fn get_access_decision(
        &self,
        peer_url: Url,
    ) -> K2Result<Option<PeerAccess>>;
}

/// Trait-object version of kitsune2 [`PeerAccessState`] trait.
pub type DynPeerAccessState = std::sync::Arc<dyn PeerAccessState>;
