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
            (1, u32::MAX - 1, Some((u32::MAX, 1))),
            (0, 0, Some((u32::MAX, 1))),
        ];

        for (dist, loc, arc) in F.iter() {
            assert_eq!(*dist, calc_dist(*loc, *arc));
        }
    }

    #[test]
    fn arcs_overlap_edge_cases() {
        type DoOverlap = bool;
        const F: &[(DoOverlap, BasicArc, BasicArc)] = &[
            (false, Some((0, 0)), Some((1, 1))),
            (false, Some((0, 0)), Some((u32::MAX, u32::MAX))),
            (true, Some((0, 0)), Some((0, 0))),
            (true, Some((u32::MAX, u32::MAX)), Some((u32::MAX, u32::MAX))),
            (true, Some((u32::MAX, 0)), Some((0, 0))),
            (true, Some((u32::MAX, 0)), Some((u32::MAX, u32::MAX))),
            (true, Some((u32::MAX, 0)), Some((u32::MAX, u32::MAX))),
            (true, Some((0, 3)), Some((1, 2))),
            (true, Some((1, 2)), Some((0, 3))),
            (true, Some((1, 3)), Some((2, 4))),
            (true, Some((2, 4)), Some((1, 3))),
            (true, Some((u32::MAX - 1, 1)), Some((u32::MAX, 0))),
            (true, Some((u32::MAX, 0)), Some((u32::MAX - 1, 1))),
            (true, Some((u32::MAX - 1, 0)), Some((u32::MAX, 1))),
            (true, Some((u32::MAX, 1)), Some((u32::MAX - 1, 0))),
        ];

        for (do_overlap, a, b) in F.iter() {
            assert_eq!(*do_overlap, arcs_overlap(*a, *b));
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn empty_store() {
        let b = crate::default_builder().build();
        let s = b.peer_store.create(b.clone()).await.unwrap();

        assert_eq!(0, s.get_all().await.unwrap().len());
    }
}
