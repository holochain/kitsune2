use futures::future::BoxFuture;
use futures::FutureExt;
use kitsune2_api::{AgentId, K2Result, PeerMeta, PeerMetaStore};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Default)]
pub struct Kitsune2MemoryPeerMetaStore(
    pub Arc<RwLock<Kitsune2MemoryPeerMetaStoreInner>>,
);

impl std::ops::Deref for Kitsune2MemoryPeerMetaStore {
    type Target = Arc<RwLock<Kitsune2MemoryPeerMetaStoreInner>>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Debug, Default)]
pub struct Kitsune2MemoryPeerMetaStoreInner {
    peer_meta: HashMap<AgentId, PeerMeta>,
}

impl PeerMetaStore for Kitsune2MemoryPeerMetaStore {
    fn get_peer_meta(
        &self,
        agent_id: AgentId,
    ) -> BoxFuture<'_, K2Result<Option<PeerMeta>>> {
        async move { Ok(self.read().await.peer_meta.get(&agent_id).cloned()) }
            .boxed()
    }

    fn store_peer_meta(
        &self,
        peer_meta: PeerMeta,
    ) -> BoxFuture<'_, K2Result<()>> {
        async move {
            self.write()
                .await
                .peer_meta
                .insert(peer_meta.agent_id.clone(), peer_meta);
            Ok(())
        }
        .boxed()
    }
}
