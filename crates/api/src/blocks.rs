//! A module that defines the [`Blocks`] trait that must be implemented by the Host to allow
//! blocking of [`BlockTarget`]s.
//!
//! If the Host wishes to not support the blocking of [`BlockTarget`]s then a simple implementation
//! can be created to make [`Blocks::is_blocked`] and [`Blocks::are_all_blocked`] always return
//! `false` and have [`Blocks::block`] be a no-op that returns `Ok`.

use std::sync::Arc;

use crate::{AgentId, BoxFut, Builder, Config, K2Result, SpaceId};

/// Trait-object version of kitsune2 [`Blocks`] trait.
pub type DynBlocks = Arc<dyn Blocks>;

/// A selection of targets to be blocked.
///
/// Marked as `non_exhaustive` as other targets might be added later.
#[non_exhaustive]
#[derive(Debug, PartialEq, Eq, Hash, Clone)]
pub enum BlockTarget {
    /// Block an agent by its [`AgentId`].
    Agent(AgentId),
}

/// Implemented by the Host to signal that a target must be blocked.
pub trait Blocks: 'static + Send + Sync + std::fmt::Debug {
    /// Used by the Host to block a target.
    ///
    /// After blocking a [`BlockTarget`] with this method, the Host **must** also remove the peer
    /// from the [`crate::PeerStore`] by calling [`crate::PeerStore::remove`].
    ///
    /// Note: This function is not called by Kitsune2 but is used as a suggestion to the host that
    /// they should implement functionality to store blocks. It also makes working with blocks
    /// simpler once the implementation is a trait-object.
    fn block(&self, target: BlockTarget) -> BoxFut<'static, K2Result<()>>;

    /// Check an individual target to see if they are blocked.
    fn is_blocked(
        &self,
        target: BlockTarget,
    ) -> BoxFut<'static, K2Result<bool>>;

    /// Check a collection of targets and return `Ok(true)` if **all** targets are blocked.
    ///
    /// Note: If a single target is not blocked then return `Ok(false)`.
    fn are_all_blocked(
        &self,
        targets: Vec<BlockTarget>,
    ) -> BoxFut<'static, K2Result<bool>>;
}

/// Trait-object version of kitsune2 [`BlocksFactory`] trait.
pub type DynBlocksFactory = Arc<dyn BlocksFactory>;

/// A factory for constructing [`Blocks`] instances.
pub trait BlocksFactory: 'static + Send + Sync + std::fmt::Debug {
    /// Help the builder construct a default config from the chosen module factories.
    fn default_config(&self, config: &mut Config) -> K2Result<()>;

    /// Validate configuration.
    fn validate_config(&self, config: &Config) -> K2Result<()>;

    /// Construct a [`Blocks`] instance.
    fn create(
        &self,
        builder: Arc<Builder>,
        space_id: SpaceId,
    ) -> BoxFut<'static, K2Result<DynBlocks>>;
}
