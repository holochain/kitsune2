//! A production-ready memory-based known-peers index.

use kitsune2_api::*;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

/// A production-ready memory-based known-peers factory.
///
/// This stores agent infos in an in-memory hash map by [`AgentId`], indexed
/// by [`Url`].  Entries are never removed – the whole point of this store is
/// to retain the URL → agent mapping even after an agent has been blocked and
/// removed from the [`crate::factories::MemPeerStoreFactory`].
#[derive(Debug)]
pub struct MemKnownPeersFactory {}

impl MemKnownPeersFactory {
    /// Construct a new `MemKnownPeersFactory`.
    pub fn create() -> DynKnownPeersFactory {
        let out: DynKnownPeersFactory = Arc::new(Self {});
        out
    }
}

impl KnownPeersFactory for MemKnownPeersFactory {
    fn default_config(&self, _config: &mut Config) -> K2Result<()> {
        Ok(())
    }

    fn validate_config(&self, _config: &Config) -> K2Result<()> {
        Ok(())
    }

    fn create(
        &self,
        _builder: Arc<Builder>,
        _space_id: SpaceId,
    ) -> BoxFut<'static, K2Result<DynKnownPeers>> {
        Box::pin(async move {
            let out: DynKnownPeers = Arc::new(MemKnownPeers::default());
            Ok(out)
        })
    }
}

/// A simple, in-memory implementation of the [`KnownPeers`] trait.
#[derive(Default)]
pub struct MemKnownPeers(Mutex<Inner>);

impl std::fmt::Debug for MemKnownPeers {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MemKnownPeers").finish()
    }
}

impl KnownPeers for MemKnownPeers {
    fn record(
        &self,
        agent_infos: Vec<Arc<AgentInfoSigned>>,
    ) -> BoxFut<'_, K2Result<()>> {
        Box::pin(async move {
            self.0.lock().await.record(agent_infos);
            Ok(())
        })
    }

    fn get_by_url(
        &self,
        url: Url,
    ) -> BoxFut<'_, K2Result<Vec<Arc<AgentInfoSigned>>>> {
        Box::pin(async move { Ok(self.0.lock().await.get_by_url(&url)) })
    }
}

#[derive(Default)]
struct Inner {
    /// All known agent infos indexed by agent id.
    store: HashMap<AgentId, Arc<AgentInfoSigned>>,
}

impl Inner {
    fn record(&mut self, agent_infos: Vec<Arc<AgentInfoSigned>>) {
        for agent_info in agent_infos {
            // Only keep the most recent info for each agent.
            if self.store.get(&agent_info.agent).is_some_and(|existing| {
                existing.created_at >= agent_info.created_at
            }) {
                continue;
            }
            self.store.insert(agent_info.agent.clone(), agent_info);
        }
    }

    fn get_by_url(&self, url: &Url) -> Vec<Arc<AgentInfoSigned>> {
        self.store
            .values()
            .filter(|info| info.url.as_ref() == Some(url))
            .cloned()
            .collect()
    }
}
