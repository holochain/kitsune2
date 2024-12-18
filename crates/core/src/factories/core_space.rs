//! The core space implementation provided by Kitsune2.

use kitsune2_api::{config::*, space::*, *};
use std::sync::Arc;

/// The core space implementation provided by Kitsune2.
/// You probably will have no reason to use something other than this.
/// This abstraction is mainly here for testing purposes.
#[derive(Debug)]
pub struct CoreSpaceFactory {}

impl CoreSpaceFactory {
    /// Construct a new CoreSpaceFactory.
    pub fn create() -> DynSpaceFactory {
        let out: DynSpaceFactory = Arc::new(CoreSpaceFactory {});
        out
    }
}

impl SpaceFactory for CoreSpaceFactory {
    fn default_config(&self, _config: &mut Config) -> K2Result<()> {
        Ok(())
    }

    fn create(
        &self,
        builder: Arc<builder::Builder>,
        _handler: DynSpaceHandler,
        _space: SpaceId,
    ) -> BoxFut<'static, K2Result<DynSpace>> {
        Box::pin(async move {
            let peer_store = builder.peer_store.create(builder.clone()).await?;
            let out: DynSpace = Arc::new(CoreSpace::new(peer_store));
            Ok(out)
        })
    }
}

#[derive(Debug)]
struct CoreSpace {
    peer_store: peer_store::DynPeerStore,
}

impl CoreSpace {
    pub fn new(peer_store: peer_store::DynPeerStore) -> Self {
        Self { peer_store }
    }
}

impl Space for CoreSpace {
    fn peer_store(&self) -> &peer_store::DynPeerStore {
        &self.peer_store
    }

    fn local_agent_join(
        &self,
        _local_agent: agent::DynLocalAgent,
    ) -> BoxFut<'_, K2Result<()>> {
        Box::pin(async move { todo!() })
    }

    fn local_agent_leave(&self, _local_agent: id::AgentId) -> BoxFut<'_, ()> {
        Box::pin(async move { todo!() })
    }

    fn send_message(
        &self,
        _peer: AgentId,
        _data: bytes::Bytes,
    ) -> BoxFut<'_, ()> {
        Box::pin(async move { todo!() })
    }
}
