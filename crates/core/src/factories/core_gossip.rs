use kitsune2_api::builder::Builder;
use kitsune2_api::config::Config;
use kitsune2_api::fetch::DynFetch;
use kitsune2_api::peer_store::DynPeerStore;
use kitsune2_api::transport::{DynTransport, TxBaseHandler, TxModuleHandler};
use kitsune2_api::{
    BoxFut, DynGossip, DynGossipFactory, DynLocalAgentStore, DynOpStore,
    DynPeerMetaStore, Gossip, GossipFactory, K2Result, SpaceId,
};
use std::sync::Arc;

#[cfg(test)]
mod test;

/// Factory for creating core gossip instances.
///
/// This factory returns stub gossip instances that do nothing.
#[derive(Debug)]
pub struct CoreGossipStubFactory;

impl CoreGossipStubFactory {
    /// Construct a new CoreGossipFactory.
    pub fn create() -> DynGossipFactory {
        Arc::new(CoreGossipStubFactory)
    }
}

impl GossipFactory for CoreGossipStubFactory {
    fn default_config(&self, _config: &mut Config) -> K2Result<()> {
        Ok(())
    }

    fn create(
        &self,
        _builder: Arc<Builder>,
        _space_id: SpaceId,
        _peer_store: DynPeerStore,
        _local_agent_store: DynLocalAgentStore,
        _peer_meta_store: DynPeerMetaStore,
        _op_store: DynOpStore,
        _transport: DynTransport,
        _fetch: DynFetch,
    ) -> BoxFut<'static, K2Result<DynGossip>> {
        let out: DynGossip = Arc::new(CoreGossipStub);
        Box::pin(async move { Ok(out) })
    }
}

/// A stub gossip implementation that does nothing.
///
/// This is useful for constructing a Kitsune2 instance that does not require gossip, such as for
/// testing.
#[derive(Debug, Clone)]
pub struct CoreGossipStub;

impl Gossip for CoreGossipStub {}

impl TxBaseHandler for CoreGossipStub {}
impl TxModuleHandler for CoreGossipStub {}
