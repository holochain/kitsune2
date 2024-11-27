//! A production-ready memory-based peer store.

use kitsune2_api::{agent::*, config::*, peer_store::*, *};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

const MOD_NAME: &str = "MemPeerStore";

/// Configuration parameters for [MemPeerStoreFactory]
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemPeerStoreConfig {
    /// The interval in seconds at which expired infos will be pruned.
    /// Default: 10s.
    pub prune_interval_secs: u32,
}

impl Default for MemPeerStoreConfig {
    fn default() -> Self {
        Self {
            prune_interval_secs: 10,
        }
    }
}

impl ModConfig for MemPeerStoreConfig {}

impl MemPeerStoreConfig {
    fn prune_interval(&self) -> std::time::Duration {
        std::time::Duration::from_secs(self.prune_interval_secs as u64)
    }
}

/// A production-ready memory-based peer store factory.
///
/// This stores peer info in an in-memory hash map by [kitsune2_api::AgentId].
/// The more complex `get_*` functions do aditional filtering at call time.
///
/// Legacy Holochain/Kitsune stored peer info in a database, but the frequency
/// with which it was queried resulted in the need for a memory cache anyways.
/// For Kitsune2 we're doing away with the persistance step to start with,
/// and just keeping the peer store in memory. Since the infos expire after
/// a matter of minutes anyways, there isn't often any use to persisting.
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
        config
            .add_default_module_config::<MemPeerStoreConfig>(MOD_NAME.into())?;
        Ok(())
    }

    fn create(
        &self,
        builder: Arc<builder::Builder>,
    ) -> BoxFut<'static, K2Result<DynPeerStore>> {
        Box::pin(async move {
            let config = builder
                .config
                .get_module_config::<MemPeerStoreConfig>(MOD_NAME)?;
            let out: DynPeerStore = Arc::new(MemPeerStore::new(config));
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

impl peer_store::PeerStore for MemPeerStore {
    fn insert(
        &self,
        agent_list: Vec<Arc<AgentInfoSigned>>,
    ) -> BoxFut<'_, K2Result<()>> {
        self.0.lock().unwrap().insert(agent_list);
        Box::pin(async move { Ok(()) })
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

    fn get_overlapping_storage_arc(
        &self,
        arc: BasicArc,
    ) -> BoxFut<'_, K2Result<Vec<Arc<AgentInfoSigned>>>> {
        let r = self.0.lock().unwrap().get_overlapping_storage_arc(arc);
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

        for agent in agent_list {
            // Don't insert expired infos.
            if agent.expires_at < Timestamp::now() {
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

    pub fn get(&mut self, agent: AgentId) -> Option<Arc<AgentInfoSigned>> {
        self.check_prune();

        self.store.get(&agent).cloned()
    }

    pub fn get_all(&mut self) -> Vec<Arc<AgentInfoSigned>> {
        self.check_prune();

        self.store.values().cloned().collect()
    }

    pub fn get_overlapping_storage_arc(
        &mut self,
        arc: BasicArc,
    ) -> Vec<Arc<AgentInfoSigned>> {
        self.check_prune();

        self.store
            .values()
            .filter_map(|info| {
                if info.is_tombstone {
                    return None;
                }

                if !arcs_overlap(arc, info.storage_arc) {
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
                    if v.storage_arc.is_none() {
                        // filter out zero arcs, they can't help us
                        None
                    } else {
                        Some((calc_dist(loc, v.storage_arc), v))
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

/// Get the min distance from a location to an arc in a wrapping u32 space.
/// This function will only return 0 if the location is covered by the arc.
/// This function will return u32::MAX if the arc is not set.
fn calc_dist(loc: u32, arc: BasicArc) -> u32 {
    match arc {
        None => u32::MAX,
        Some((arc_start, arc_end)) => {
            let (d1, d2) = if arc_start > arc_end {
                // this arc wraps around the end of u32::MAX

                if loc >= arc_start || loc <= arc_end {
                    return 0;
                } else {
                    (loc - arc_end, arc_start - loc)
                }
            } else {
                // this arc does not wrap

                if loc >= arc_start && loc <= arc_end {
                    return 0;
                } else if loc < arc_start {
                    (arc_start - loc, u32::MAX - arc_end + loc + 1)
                } else {
                    (loc - arc_end, u32::MAX - loc + arc_start + 1)
                }
            };
            std::cmp::min(d1, d2)
        }
    }
}

/// Determine if any part of two arcs overlap.
fn arcs_overlap(a: BasicArc, b: BasicArc) -> bool {
    match (a, b) {
        (None, _) | (_, None) => false,
        (Some((a_beg, a_end)), Some((b_beg, b_end))) => {
            // The only way for there to be overlap is if
            // either of a's start or end points are within b
            // or either of b's start or end points are within a
            calc_dist(a_beg, Some((b_beg, b_end))) == 0
                || calc_dist(a_end, Some((b_beg, b_end))) == 0
                || calc_dist(b_beg, Some((a_beg, a_end))) == 0
                || calc_dist(b_end, Some((a_beg, a_end))) == 0
        }
    }
}

#[cfg(test)]
mod test;
