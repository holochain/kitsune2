//! The core known-peers index implementation.

use kitsune2_api::*;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

/// The core known-peers factory.
///
/// This stores agent ID → URL mappings in memory.  Entries are never
/// removed – the whole point of this store is to retain the URL → agent
/// mapping even after an agent has been blocked and removed from the
/// peer store.
#[derive(Debug)]
pub struct CoreKnownPeersFactory {}

impl CoreKnownPeersFactory {
    /// Construct a new `CoreKnownPeersFactory`.
    pub fn create() -> DynKnownPeersFactory {
        let out: DynKnownPeersFactory = Arc::new(Self {});
        out
    }
}

impl KnownPeersFactory for CoreKnownPeersFactory {
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
            let out: DynKnownPeers = Arc::new(CoreKnownPeers::default());
            Ok(out)
        })
    }
}

/// The core implementation of the [`KnownPeers`] trait.
#[derive(Default)]
pub struct CoreKnownPeers(Mutex<Inner>);

impl std::fmt::Debug for CoreKnownPeers {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CoreKnownPeers").finish()
    }
}

impl KnownPeers for CoreKnownPeers {
    fn record(
        &self,
        agent_infos: Vec<Arc<AgentInfoSigned>>,
    ) -> BoxFut<'_, K2Result<()>> {
        Box::pin(async move {
            self.0.lock().await.record(agent_infos);
            Ok(())
        })
    }

    fn get_by_url(&self, url: Url) -> BoxFut<'_, K2Result<Vec<AgentId>>> {
        Box::pin(async move { Ok(self.0.lock().await.get_by_url(&url)) })
    }
}

#[derive(Default)]
struct Inner {
    /// Agent ID → most recently seen URL for that agent.
    store: HashMap<AgentId, Option<Url>>,
}

impl Inner {
    fn record(&mut self, agent_infos: Vec<Arc<AgentInfoSigned>>) {
        for agent_info in agent_infos {
            self.store
                .insert(agent_info.agent.clone(), agent_info.url.clone());
        }
    }

    fn get_by_url(&self, url: &Url) -> Vec<AgentId> {
        self.store
            .iter()
            .filter(|(_, stored_url)| stored_url.as_ref() == Some(url))
            .map(|(agent_id, _)| agent_id.clone())
            .collect()
    }
}
