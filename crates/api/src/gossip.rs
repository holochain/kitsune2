//! Gossip related types.

use crate::peer_store::DynPeerStore;
use crate::transport::{DynTransport, DynTxModuleHandler};
use crate::{builder, config, BoxFut, DynOpStore, K2Result, SpaceId};
use std::sync::Arc;

/// Represents the ability to sync DHT data with other agents through background communication.
pub trait Gossip: 'static + Send + Sync + std::fmt::Debug {
    /// Get the tx module handler for this gossip instance.
    fn tx_module_handler(&self) -> DynTxModuleHandler;
}

/// Trait-object [Gossip].
pub type DynGossip = Arc<dyn Gossip>;

/// A factory for constructing [Gossip] instances.
pub trait GossipFactory: 'static + Send + Sync + std::fmt::Debug {
    /// Help the builder construct a default config from the chosen
    /// module factories.
    fn default_config(&self, config: &mut config::Config) -> K2Result<()>;

    /// Construct a gossip instance.
    fn create(
        &self,
        builder: Arc<builder::Builder>,
        space: SpaceId,
        peer_store: DynPeerStore,
        op_store: DynOpStore,
        transport: DynTransport,
    ) -> BoxFut<'static, K2Result<DynGossip>>;
}

/// Trait-object [GossipFactory].
pub type DynGossipFactory = Arc<dyn GossipFactory>;
