use kitsune2_api::K2Error;
use std::sync::Arc;

#[derive(Debug, thiserror::Error)]
pub(crate) enum K2GossipError {
    /// An error caused by peer behavior.
    #[error("Rejected peer behavior - {ctx}")]
    PeerBehaviorError {
        ctx: Arc<str>,
    },

    /// A Kitsune2 error.
    #[error("K2Error - {0}")]
    K2Error(#[from] K2Error),
}

impl K2GossipError {
    pub(crate) fn peer_behavior(ctx: impl Into<Arc<str>>) -> Self {
        Self::PeerBehaviorError { ctx: ctx.into() }
    }
}

pub(crate) type K2GossipResult<T> = Result<T, K2GossipError>;
