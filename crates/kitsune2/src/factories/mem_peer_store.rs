//! A production-ready memory-based peer store.

use kitsune2_api::{agent::*, config::*, peer_store::*, *};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Configuration parameters for [MemPeerStoreFactory]
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemPeerStoreConfig {}

impl ModConfig for MemPeerStoreConfig {}

/// A production-ready memory-based peer store factory.
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
    fn default_config(&self, _config: &mut Config) {}

    fn create(
        &self,
        _builder: Arc<builder::Builder>,
    ) -> BoxFut<'static, K2Result<DynPeerStore>> {
        Box::pin(async move {
            let out: DynPeerStore = Arc::new(MemPeerStore::new());
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
    pub fn new() -> Self {
        Self(Mutex::new(Inner::new()))
    }
}

impl peer_store::PeerStore for MemPeerStore {
    /*
    fn clear(&self) -> BoxFut<'_, K2Result<()>> {
        self.0.lock().unwrap().clear();
        Box::pin(async move { Ok(()) })
    }
    */

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

    /*
    fn get_many(&self, agent_list: Vec<AgentId>) -> BoxFut<'_, K2Result<Vec<Arc<AgentInfoSigned>>>> {
        let r = self.0.lock().unwrap().get_many(agent_list);
        Box::pin(async move { Ok(r) })
    }
    */

    fn query_by_time_and_arq(
        &self,
        since: Timestamp,
        until: Timestamp,
        arc: BasicArc,
    ) -> BoxFut<'_, K2Result<Vec<Arc<AgentInfoSigned>>>> {
        let r = self
            .0
            .lock()
            .unwrap()
            .query_by_time_and_arc(since, until, arc);
        Box::pin(async move { Ok(r) })
    }

    fn query_by_location(
        &self,
        loc: u32,
        limit: usize,
    ) -> BoxFut<'_, K2Result<Vec<Arc<AgentInfoSigned>>>> {
        let r = self.0.lock().unwrap().query_by_location(loc, limit);
        Box::pin(async move { Ok(r) })
    }
}

struct Inner {
    store: HashMap<AgentId, Arc<AgentInfoSigned>>,
    no_prune_until: std::time::Instant,
}

impl Inner {
    pub fn new() -> Self {
        Self {
            store: HashMap::new(),
            no_prune_until: std::time::Instant::now()
                + std::time::Duration::from_secs(10),
        }
    }

    fn check_prune(&mut self) {
        // use an instant here even though we have to create a Timestamp::now()
        // below, because it's faster to query than SystemTime if we're aborting
        let inst_now = std::time::Instant::now();
        if self.no_prune_until > inst_now {
            return;
        }

        let now = Timestamp::now();

        self.store.retain(|_, v| v.expires_at > now);

        // we only care about not looping on the order of tight cpu cycles
        // even a couple seconds gets us away from this.
        self.no_prune_until = inst_now + std::time::Duration::from_secs(10)
    }

    /*
    pub fn clear(&mut self) {
        self.store.clear();
        self.no_prune_until = std::time::Instant::now() + std::time::Duration::from_secs(10)
    }
    */

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

    /*
    fn get_many(&mut self, agent_list: Vec<AgentId>) -> Vec<Arc<AgentInfoSigned>> {
        self.check_prune();

        agent_list
            .into_iter()
            .filter_map(|agent| self.store.get(&agent).cloned())
            .collect()
    }
    */

    pub fn query_by_time_and_arc(
        &mut self,
        since: Timestamp,
        until: Timestamp,
        _arc: BasicArc,
    ) -> Vec<Arc<AgentInfoSigned>> {
        self.check_prune();

        self.store
            .values()
            .filter_map(|info| {
                if info.is_tombstone {
                    return None;
                }

                if info.created_at < since {
                    return None;
                }

                if info.created_at > until {
                    return None;
                }

                // TODO - fixme
                /*
                if !overlaps(arc, info.storage_arc) {
                    return None;
                }
                */

                Some(info.clone())
            })
            .collect()
    }

    pub fn query_by_location(
        &mut self,
        basis: u32,
        limit: usize,
    ) -> Vec<Arc<AgentInfoSigned>> {
        self.check_prune();

        let mut out: Vec<(u32, &Arc<AgentInfoSigned>)> = self
            .store
            .values()
            .filter_map(|v| {
                if !v.is_tombstone {
                    Some((calc_dist(basis, v.storage_arc), v))
                } else {
                    None
                }
            })
            .collect();

        if out.len() > 1 {
            out.sort_by(|a, b| a.0.cmp(&b.0));
        }

        out.into_iter()
            .filter(|(dist, _)| *dist != u32::MAX) // Filter out Zero arcs
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
            let loc = loc as u64;
            let arc_start = arc_start as u64;
            let mut arc_end = arc_end as u64;
            if arc_end < arc_start {
                arc_end += u32::MAX as u64 + 1;
            }
            let (d1, d2) = if loc >= arc_start && loc <= arc_end {
                return 0;
            } else if loc < arc_start {
                (arc_start - loc, loc + u32::MAX as u64 + 1 - arc_end)
            } else {
                // loc > arc_end
                (loc - arc_end, u32::MAX as u64 + 1 - loc + arc_start)
            };
            if d1 < d2 {
                d1 as u32
            } else {
                d2 as u32
            }
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn dist_edge_cases() {
        type Dist = u32;
        type Loc = u32;
        const F: &[(Dist, Loc, BasicArc)] = &[
            (u32::MAX, 0, None),
            (0, 0, Some((0, 1))),
            (0, u32::MAX, Some((u32::MAX - 1, u32::MAX))),
            (1, 0, Some((1, 2))),
            (1, u32::MAX, Some((0, 1))),
            (1, 0, Some((u32::MAX - 1, u32::MAX))),
            (0, 0, Some((u32::MAX, 0))),
            (1, 1, Some((u32::MAX, 0))),
            (1, u32::MAX - 1, Some((u32::MAX, 0))),
            (1, 0, Some((u32::MAX, u32::MAX))),
            (1, u32::MAX, Some((0, 0))),
            (u32::MAX / 2, u32::MAX / 2, Some((0, 0))),
            (u32::MAX / 2 + 1, u32::MAX / 2, Some((u32::MAX, u32::MAX))),
        ];

        for (dist, loc, arc) in F.iter() {
            assert_eq!(*dist, calc_dist(*loc, *arc));
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn empty_store() {
        let b = crate::default_builder().build();
        let s = b.peer_store.create(b.clone()).await.unwrap();

        assert_eq!(0, s.get_all().await.unwrap().len());
    }
}
