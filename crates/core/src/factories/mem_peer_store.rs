//! A production-ready memory-based peer store.

use kitsune2_api::*;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

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
    ) -> BoxFut<'static, K2Result<DynPeerStore>> {
        Box::pin(async move {
            let config: MemPeerStoreModConfig =
                builder.config.get_module_config()?;
            let out: DynPeerStore =
                Arc::new(MemPeerStore::new(config.mem_peer_store));
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
    pub fn new(config: MemPeerStoreConfig) -> Self {
        Self(Mutex::new(Inner::new(config, std::time::Instant::now())))
    }
}

impl PeerStore for MemPeerStore {
    fn insert(
        &self,
        agent_list: Vec<Arc<AgentInfoSigned>>,
    ) -> BoxFut<'_, K2Result<()>> {
        self.0.lock().unwrap().insert(agent_list);
        Box::pin(async move { Ok(()) })
    }

    fn remove(&self, agent_id: AgentId) -> BoxFut<'_, K2Result<()>> {
        self.0.lock().unwrap().remove(&agent_id);

        Box::pin(async { Ok(()) })
    }

    fn get(
        &self,
        agent: AgentId,
    ) -> BoxFut<'_, K2Result<Option<Arc<AgentInfoSigned>>>> {
        let r = self.0.lock().unwrap().get(agent);
        Box::pin(async move { Ok(r) })
    }

    fn get_all(&self) -> BoxFut<'_, K2Result<Vec<Arc<AgentInfoSigned>>>> {
        let r = self.0.lock().unwrap().get_all();
        Box::pin(async move { Ok(r) })
    }

    fn get_by_overlapping_storage_arc(
        &self,
        arc: DhtArc,
    ) -> BoxFut<'_, K2Result<Vec<Arc<AgentInfoSigned>>>> {
        let r = self.0.lock().unwrap().get_by_overlapping_storage_arc(arc);
        Box::pin(async move { Ok(r) })
    }

    fn get_near_location(
        &self,
        loc: u32,
        limit: usize,
    ) -> BoxFut<'_, K2Result<Vec<Arc<AgentInfoSigned>>>> {
        let r = self.0.lock().unwrap().get_near_location(loc, limit);
        Box::pin(async move { Ok(r) })
    }
}

struct Inner {
    config: MemPeerStoreConfig,
    store: HashMap<AgentId, Arc<AgentInfoSigned>>,
    no_prune_until: std::time::Instant,
}

impl Inner {
    pub fn new(
        config: MemPeerStoreConfig,
        now_inst: std::time::Instant,
    ) -> Self {
        let no_prune_until = now_inst + config.prune_interval();
        Self {
            config,
            store: HashMap::new(),
            no_prune_until,
        }
    }

    fn do_prune(&mut self, now_inst: std::time::Instant, now_ts: Timestamp) {
        self.store.retain(|_, v| v.expires_at > now_ts);

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

    pub fn insert(&mut self, agent_list: Vec<Arc<AgentInfoSigned>>) {
        self.check_prune();

        let now = Timestamp::now();

        for agent in agent_list {
            // Don't insert expired infos.
            if agent.expires_at < now {
                continue;
            }

            if let Some(a) = self.store.get(&agent.agent) {
                // If we already have a newer (or equal) one, abort.
                if a.created_at >= agent.created_at {
                    continue;
                }
            }

            self.store.insert(agent.agent.clone(), agent);
        }
    }

    pub fn remove(&mut self, agent_id: &AgentId) {
        self.store.remove(agent_id);
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
}

#[cfg(test)]
mod test;
