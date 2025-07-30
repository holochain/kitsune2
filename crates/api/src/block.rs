//! A module that defines the [`Block`] trait that must be implemented by the Host to allow
//! blocking of [`BlockTarget`]s.
//!
//! If the Host wishes to not support the blocking of [`BlockTarget`]s then a simple implementation
//! can be created to make [`Block::is_blocked`] and [`Block::are_all_blocked`] always return
//! `false` and have [`Block::block`] be a no-op that returns `Ok`.

use crate::{AgentId, BoxFut, K2Result};

/// A selection of targets to be blocked.
///
/// Marked as `non_exhaustive` as other targets might be added later.
#[non_exhaustive]
#[derive(Debug, PartialEq, Eq)]
pub enum BlockTarget {
    /// Block an agent by its [`AgentId`].
    Agent(AgentId),
}

/// Implemented by the Host to signal that a target must be blocked.
pub trait Block: 'static + Send + Sync + std::fmt::Debug {
    /// Used by the Host to block a target.
    ///
    /// After blocking a [`BlockTarget`] with this method, the Host **must** also remove the peer
    /// from the [`crate::PeerStore`] by calling [`crate::PeerStore::remove`].
    ///
    /// Note: This is a convenience method that is not used by `kitsune2` but allows the Host to
    /// more easily block targets.
    fn block(&self, target: BlockTarget) -> BoxFut<'static, K2Result<()>>;

    /// Check individual targets to see if they are blocked.
    fn is_blocked(
        &self,
        target: &BlockTarget,
    ) -> BoxFut<'static, K2Result<bool>>;

    /// Check a collection of targets and return `Ok(true)` if **all** targets are blocked.
    ///
    /// Note: If a single target is not blocked then return `Ok(false)`.
    fn are_all_blocked(
        &self,
        targets: &[BlockTarget],
    ) -> BoxFut<'static, K2Result<bool>>;
}
