use futures::future::BoxFuture;
use kitsune2_api::*;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

#[cfg(test)]
mod test;

type MemPeerMetaInner = HashMap<Url, HashMap<String, bytes::Bytes>>;

/// An in-memory implementation of the [PeerMetaStore].
///
/// This is useful for testing, but peer metadata is supposed to be persistent in a real deployment.
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
        peer: Url,
        key: String,
        value: bytes::Bytes,
        _expiry: Option<Timestamp>,
    ) -> BoxFuture<'_, K2Result<()>> {
        let inner = self.inner.clone();
        Box::pin(async move {
            let mut inner = inner.lock().await;
            let entry = inner.entry(peer).or_insert_with(HashMap::new);
            entry.insert(key, value);
            Ok(())
        })
    }

    fn get(
        &self,
        peer: Url,
        key: String,
    ) -> BoxFuture<'_, K2Result<Option<bytes::Bytes>>> {
        let inner = self.inner.clone();
        Box::pin(async move {
            let inner = inner.lock().await;
            Ok(inner.get(&peer).and_then(|entry| entry.get(&key).cloned()))
        })
    }

    fn delete(&self, peer: Url, key: String) -> BoxFuture<'_, K2Result<()>> {
        let inner = self.inner.clone();
        Box::pin(async move {
            let mut inner = inner.lock().await;
            if let Some(entry) = inner.get_mut(&peer) {
                entry.remove(&key);
            }
            Ok(())
        })
    }
}

/// A factory for creating [MemPeerMetaStore] instances.
#[derive(Debug)]
pub struct MemPeerMetaStoreFactory;

impl MemPeerMetaStoreFactory {
    /// Construct a new [MemPeerMetaStoreFactory].
    pub fn create() -> DynPeerMetaStoreFactory {
        Arc::new(MemPeerMetaStoreFactory)
    }
}

impl PeerMetaStoreFactory for MemPeerMetaStoreFactory {
    fn default_config(&self, _config: &mut Config) -> K2Result<()> {
        Ok(())
    }

    fn validate_config(&self, _config: &Config) -> K2Result<()> {
        Ok(())
    }

    fn create(
        &self,
        _builder: Arc<Builder>,
        _space: SpaceId,
    ) -> BoxFuture<'static, K2Result<DynPeerMetaStore>> {
        Box::pin(async move { Ok(MemPeerMetaStore::create()) })
    }
}
