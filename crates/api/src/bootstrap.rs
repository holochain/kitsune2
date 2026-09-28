//! Kitsune2 bootstrap related types.

use crate::*;
use std::sync::Arc;

/// Method for bootstrapping peer discovery.
///
/// The internal implementation will take care of whatever polling
/// or managing of message queues is required to be notified of
/// remote peers both on initialization and over runtime.
pub trait Bootstrap: 'static + Send + Sync + std::fmt::Debug {
    /// Put an agent info onto a bootstrap server.
    ///
    /// This method takes responsibility for retrying the send operation in the case
    /// of server error until such time as:
    /// - the Put succeeds
    /// - we receive a new info that supersedes the previous
    /// - or the info expires
    fn put(&self, info: Arc<AgentInfoSigned>);

    /// Notify bootstrap whether a local transport URL can be advertised.
    ///
    /// Implementations must not publish stale local advertisements while the
    /// URL is unavailable. Discovery work that does not expose a local address
    /// may continue.
    fn set_transport_url_available(&self, _is_transport_url_available: bool) {}
}

/// Trait-object [Bootstrap].
pub type DynBootstrap = Arc<dyn Bootstrap>;

/// A factory for constructing Bootstrap instances.
pub trait BootstrapFactory: 'static + Send + Sync + std::fmt::Debug {
    /// Help the builder construct a default config from the chosen
    /// module factories.
    fn default_config(&self, config: &mut Config) -> K2Result<()>;

    /// Validate configuration.
    fn validate_config(&self, config: &Config) -> K2Result<()>;

    /// Construct a bootstrap instance.
    fn create(
        &self,
        builder: Arc<Builder>,
        peer_store: DynPeerStore,
        space_id: SpaceId,
    ) -> BoxFut<'static, K2Result<DynBootstrap>>;
}

/// Trait-object [BootstrapFactory].
pub type DynBootstrapFactory = Arc<dyn BootstrapFactory>;
