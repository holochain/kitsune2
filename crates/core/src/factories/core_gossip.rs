use kitsune2_api::builder::Builder;
use kitsune2_api::config::Config;
use kitsune2_api::peer_store::DynPeerStore;
use kitsune2_api::transport::DynTransport;
use kitsune2_api::{
    BoxFut, DynGossip, DynGossipFactory, GossipFactory, K2Result,
};
use kitsune2_gossip::K2Gossip;
use std::sync::Arc;

#[cfg(test)]
mod test;

/// The core gossip implementation provided by Kitsune2.
#[derive(Debug)]
pub struct CoreGossipFactory {}

impl CoreGossipFactory {
    /// Construct a new CoreGossipFactory.
    pub fn create() -> DynGossipFactory {
        Arc::new(CoreGossipFactory {})
    }
}

impl GossipFactory for CoreGossipFactory {
    fn default_config(&self, _config: &mut Config) -> K2Result<()> {
        // TODO configuration
        Ok(())
    }

    fn create(
        &self,
        _builder: Arc<Builder>,
        peer_store: DynPeerStore,
        transport: DynTransport,
    ) -> BoxFut<'static, K2Result<DynGossip>> {
        Box::pin(async move { Ok(K2Gossip::create(peer_store, transport)) })
    }
}
