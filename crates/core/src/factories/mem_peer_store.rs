//! A production-ready memory-based peer store.

use kitsune2_api::*;
use std::collections::HashMap;
use std::fmt::Debug;
use std::sync::Arc;
use tokio::sync::Mutex;

/// MemPeerStore configuration types.
mod config {
    /// Configuration parameters for [MemPeerStoreFactory](super::MemPeerStoreFactory).
    #[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
    #[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
    #[serde(rename_all = "camelCase")]
    pub struct MemPeerStoreConfig {
        /// The interval in seconds at which expired infos will be pruned.
        ///
        /// Default: 10s.
        #[cfg_attr(feature = "schema", schemars(default))]
        pub prune_interval_s: u32,
    }

    impl Default for MemPeerStoreConfig {
        fn default() -> Self {
            Self {
                prune_interval_s: 10,
            }
        }
    }

    impl MemPeerStoreConfig {
        /// Get the prune interval as a [std::time::Duration].
        pub fn prune_interval(&self) -> std::time::Duration {
            std::time::Duration::from_secs(self.prune_interval_s as u64)
        }
    }

    /// Module-level configuration for MemPeerStore.
    #[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
    #[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
    #[serde(rename_all = "camelCase")]
    pub struct MemPeerStoreModConfig {
        /// MemPeerStore configuration.
        pub mem_peer_store: MemPeerStoreConfig,
    }
}

pub use config::*;

/// A production-ready memory-based peer store factory.
///
/// This stores peer info in an in-memory hash map by [AgentId].
/// The more complex `get_*` functions do aditional filtering at call time.
//
// Legacy Holochain/Kitsune stored peer info in a database, but the frequency
// with which it was queried resulted in the need for a memory cache anyways.
// For Kitsune2 we're doing away with the persistance step to start with,
// and just keeping the peer store in memory. Since the infos expire after
// a matter of minutes anyways, there isn't often any use to persisting.
#[derive(Debug)]
pub struct MemPeerStoreFactory {}

impl MemPeerStoreFactory {
    /// Construct a new MemPeerStoreFactory
    pub fn create() -> DynPeerStoreFactory {
        let out: DynPeerStoreFactory = Arc::new(Self {});
        out
    }
}

impl PeerStoreFactory for MemPeerStoreFactory {
    fn default_config(&self, config: &mut Config) -> K2Result<()> {
        config.set_module_config(&MemPeerStoreModConfig::default())
    }

    fn validate_config(&self, _config: &Config) -> K2Result<()> {
        Ok(())
    }

    fn create(
        &self,
        builder: Arc<Builder>,
        _space_id: SpaceId,
        blocks: DynBlocks,
    ) -> BoxFut<'static, K2Result<DynPeerStore>> {
        Box::pin(async move {
            let config: MemPeerStoreModConfig =
                builder.config.get_module_config()?;
            let out: DynPeerStore =
                Arc::new(MemPeerStore::new(config.mem_peer_store, blocks));
            Ok(out)
        })
    }
}

struct MemPeerStore(Mutex<Inner>);

impl std::fmt::Debug for MemPeerStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MemPeerStore").finish()
    }
}

impl MemPeerStore {
    pub fn new(config: MemPeerStoreConfig, blocks: DynBlocks) -> Self {
        Self(Mutex::new(Inner::new(
            config,
            std::time::Instant::now(),
            blocks,
        )))
    }
}

impl PeerStore for MemPeerStore {
    fn insert(
        &self,
        agent_list: Vec<Arc<AgentInfoSigned>>,
    ) -> BoxFut<'_, K2Result<()>> {
        Box::pin(async move {
            let mut guard = self.0.lock().await;
            let inserted = guard.insert(agent_list.clone()).await?;

            if !inserted.is_empty() && !guard.listeners.is_empty() {
                let listeners = guard.listeners.to_vec();
                drop(guard);

                for agent in inserted {
                    for listener in &listeners {
                        listener(agent.clone()).await;
                    }
                }
            }

            Ok(())
        })
    }

    fn remove(&self, agent_id: AgentId) -> BoxFut<'_, K2Result<()>> {
        Box::pin(async move {
            let mut guard = self.0.lock().await;
            let removed = guard.remove(&agent_id);

            if let Some(removed) = removed {
                let listeners = guard.listeners.to_vec();
                drop(guard);

                for listener in &listeners {
                    listener(removed.clone()).await;
                }
            }

            Ok(())
        })
    }

    fn get(
        &self,
        agent: AgentId,
    ) -> BoxFut<'_, K2Result<Option<Arc<AgentInfoSigned>>>> {
        Box::pin(async move { Ok(self.0.lock().await.get(agent)) })
    }

    fn get_all(&self) -> BoxFut<'_, K2Result<Vec<Arc<AgentInfoSigned>>>> {
        Box::pin(async move { Ok(self.0.lock().await.get_all()) })
    }

    fn get_by_overlapping_storage_arc(
        &self,
        arc: DhtArc,
    ) -> BoxFut<'_, K2Result<Vec<Arc<AgentInfoSigned>>>> {
        Box::pin(async move {
            Ok(self.0.lock().await.get_by_overlapping_storage_arc(arc))
        })
    }

    fn get_near_location(
        &self,
        loc: u32,
        limit: usize,
    ) -> BoxFut<'_, K2Result<Vec<Arc<AgentInfoSigned>>>> {
        Box::pin(async move {
            Ok(self.0.lock().await.get_near_location(loc, limit))
        })
    }

    fn get_by_url(
        &self,
        peer_url: Url,
    ) -> BoxFut<'_, K2Result<Vec<Arc<AgentInfoSigned>>>> {
        Box::pin(async move { Ok(self.0.lock().await.get_by_url(&peer_url)) })
    }

    fn register_peer_update_listener(
        &self,
        listener: Arc<
            dyn (Fn(Arc<AgentInfoSigned>) -> BoxFut<'static, ()>) + Send + Sync,
        >,
    ) -> K2Result<()> {
        let mut inner = self.0.try_lock().map_err(|e| {
            K2Error::other_src("Unable to lock MemPeerStore", e)
        })?;
        inner.listeners.push(listener);
        Ok(())
    }
}

type Listener =
    Arc<dyn (Fn(Arc<AgentInfoSigned>) -> BoxFut<'static, ()>) + Send + Sync>;

struct Inner {
    config: MemPeerStoreConfig,
    store: HashMap<AgentId, Arc<AgentInfoSigned>>,
    no_prune_until: std::time::Instant,
    listeners: Vec<Listener>,
    blocks: DynBlocks,
}

impl Inner {
    pub fn new(
        config: MemPeerStoreConfig,
        now_inst: std::time::Instant,
        blocks: DynBlocks,
    ) -> Self {
        let no_prune_until = now_inst + config.prune_interval();
        Self {
            config,
            store: HashMap::new(),
            no_prune_until,
            listeners: Vec::new(),
            blocks,
        }
    }

    fn do_prune(&mut self, now_inst: std::time::Instant, now_ts: Timestamp) {
        self.store.retain(|_, v| {
            if v.expires_at > now_ts {
                true
            } else {
                tracing::debug!(?v.agent, ?v.space, "Pruning expired agent info");
                false
            }
        });

        // we only care about not looping on the order of tight cpu cycles
        // even a couple seconds gets us away from this.
        self.no_prune_until = now_inst + self.config.prune_interval()
    }

    fn check_prune(&mut self) {
        // use an instant here even though we have to create a Timestamp::now()
        // below, because it's faster to query than SystemTime if we're aborting
        let now_inst = std::time::Instant::now();
        if self.no_prune_until > now_inst {
            return;
        }

        self.do_prune(now_inst, Timestamp::now());
    }

    pub async fn insert(
        &mut self,
        agent_list: Vec<Arc<AgentInfoSigned>>,
    ) -> K2Result<Vec<Arc<AgentInfoSigned>>> {
        self.check_prune();

        let now = Timestamp::now();

        let mut inserted = Vec::new();
        for agent in agent_list {
            // Don't insert blocked agents.
            if self
                .blocks
                .is_blocked(BlockTarget::Agent(agent.agent.clone()))
                .await?
            {
                tracing::debug!(
                    ?agent,
                    "Refusing to insert agent as it is currently blocked",
                );
                continue;
            }

            // Don't insert expired infos.
            if agent.expires_at < now {
                tracing::debug!(?agent.agent, ?agent.space, "Ignoring insert for expired agent info");
                continue;
            }

            if let Some(a) = self.store.get(&agent.agent) {
                // If we already have a newer (or equal) one, abort.
                if a.created_at >= agent.created_at {
                    // Don't want to log anything if we got the same agent info again. That's a
                    // normal part of operation to rediscover existing agents.
                    if a.created_at > agent.created_at {
                        tracing::debug!(?agent.agent, ?agent.space, "Ignoring insert for older agent info");
                    }
                    continue;
                }
            }

            self.store.insert(agent.agent.clone(), agent.clone());
            inserted.push(agent);
        }

        Ok(inserted)
    }

    pub fn remove(
        &mut self,
        agent_id: &AgentId,
    ) -> Option<Arc<AgentInfoSigned>> {
        self.store.remove(agent_id)
    }

    pub fn get(&mut self, agent: AgentId) -> Option<Arc<AgentInfoSigned>> {
        self.check_prune();

        self.store.get(&agent).cloned()
    }

    pub fn get_all(&mut self) -> Vec<Arc<AgentInfoSigned>> {
        self.check_prune();

        self.store.values().cloned().collect()
    }

    pub fn get_by_overlapping_storage_arc(
        &mut self,
        arc: DhtArc,
    ) -> Vec<Arc<AgentInfoSigned>> {
        self.check_prune();

        self.store
            .values()
            .filter_map(|info| {
                if info.is_tombstone {
                    return None;
                }

                if !arc.overlaps(&info.storage_arc) {
                    return None;
                }

                Some(info.clone())
            })
            .collect()
    }

    pub fn get_near_location(
        &mut self,
        loc: u32,
        limit: usize,
    ) -> Vec<Arc<AgentInfoSigned>> {
        self.check_prune();

        let mut out: Vec<(u32, &Arc<AgentInfoSigned>)> = self
            .store
            .values()
            .filter_map(|v| {
                if !v.is_tombstone {
                    if v.storage_arc == DhtArc::Empty {
                        // filter out empty arcs, they can't help us
                        None
                    } else {
                        Some((v.storage_arc.dist(loc), v))
                    }
                } else {
                    None
                }
            })
            .collect();

        out.sort_by(|a, b| a.0.cmp(&b.0));

        out.into_iter()
            .take(limit)
            .map(|(_, v)| v.clone())
            .collect()
    }

    pub fn get_by_url(&mut self, peer_url: &Url) -> Vec<Arc<AgentInfoSigned>> {
        self.check_prune();

        self.store
            .values()
            .filter(|agent| agent.url.as_ref() == Some(peer_url))
            .cloned()
            .collect()
    }
}

#[cfg(test)]
mod test;
