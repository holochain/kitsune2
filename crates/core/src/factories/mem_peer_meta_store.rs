use futures::future::BoxFuture;
use kitsune2_api::{
    AgentId, DynPeerMetaStore, DynPeerMetaStoreFactory, K2Result,
    PeerMetaStore, PeerMetaStoreFactory, SpaceId,
};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

#[cfg(test)]
mod test;

type MemPeerMetaInner = HashMap<(SpaceId, AgentId), HashMap<String, Vec<u8>>>;

/// An in-memory implementation of the [PeerMetaStore].
///
/// This is useful for testing but peer metadata is supposed to be persistent in a real deployment.
#[derive(Debug)]
pub struct MemPeerMetaStore {
    inner: Arc<Mutex<MemPeerMetaInner>>,
}

impl MemPeerMetaStore {
    /// Create a new [MemPeerMetaStore].
    pub fn create() -> DynPeerMetaStore {
        let inner = Arc::new(Mutex::new(HashMap::new()));
        Arc::new(MemPeerMetaStore { inner })
    }
}

impl PeerMetaStore for MemPeerMetaStore {
    fn put(
        &self,
        space: SpaceId,
        agent: AgentId,
        key: String,
        value: Vec<u8>,
    ) -> BoxFuture<'_, K2Result<()>> {
        let inner = self.inner.clone();
        Box::pin(async move {
            let mut inner = inner.lock().await;
            let entry =
                inner.entry((space, agent)).or_insert_with(HashMap::new);
            entry.insert(key, value);
            Ok(())
        })
    }

    fn get(
        &self,
        space: SpaceId,
        agent: AgentId,
        key: String,
    ) -> BoxFuture<'_, K2Result<Option<Vec<u8>>>> {
        let inner = self.inner.clone();
        Box::pin(async move {
            let inner = inner.lock().await;
            Ok(inner
                .get(&(space, agent))
                .and_then(|entry| entry.get(&key).cloned()))
        })
    }

    fn delete(
        &self,
        space: SpaceId,
        agent: AgentId,
        key: String,
    ) -> BoxFuture<'_, K2Result<()>> {
        let inner = self.inner.clone();
        Box::pin(async move {
            let mut inner = inner.lock().await;
            if let Some(entry) = inner.get_mut(&(space, agent)) {
                entry.remove(&key);
            }
            Ok(())
        })
    }
}

/// A factor for creating [MemPeerMetaStore] instances.
#[derive(Debug)]
pub struct MemPeerMetaStoreFactory;

impl MemPeerMetaStoreFactory {
    /// Construct a new [MemPeerMetaStoreFactory].
    pub fn create() -> DynPeerMetaStoreFactory {
        Arc::new(MemPeerMetaStoreFactory)
    }
}

impl PeerMetaStoreFactory for MemPeerMetaStoreFactory {
    fn default_config(
        &self,
        _config: &mut crate::config::Config,
    ) -> K2Result<()> {
        Ok(())
    }

    fn create(
        &self,
        _builder: Arc<crate::builder::Builder>,
    ) -> BoxFuture<'static, K2Result<DynPeerMetaStore>> {
        Box::pin(async move { Ok(MemPeerMetaStore::create()) })
    }
}
