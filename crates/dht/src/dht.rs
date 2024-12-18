//! Top-level DHT model.

use crate::arc_set::ArcSet;
use crate::PartitionedHashes;
use kitsune2_api::{DynOpStore, K2Result, OpId, StoredOp, Timestamp};
use std::collections::{HashMap, HashSet};

pub struct Dht {
    partition: PartitionedHashes,
}

#[derive(Debug, Eq, PartialEq)]
pub enum DhtSnapshot {
    /// The default, smallest snapshot type.
    ///
    /// It contains enough information to make further decisions about where mismatches might be
    /// but does its best to compress historical information. The assumption being that the more
    /// recent data is more likely to contains mismatches than older data.
    ///
    /// Requires 4 bytes for the timestamp. It then requires at most
    /// 2 * (`time_factor` - 1) * `HASH_SIZE` + 1 bytes for the disc hash and the ring hashes.
    /// Where `HASH_SIZE` is a host implementation detail. Assuming 4-byte hashes and a
    /// `time_factor` of 14, this would be 4 + 2 * (14 - 1) * 4 + 1 = 109 bytes.
    ///
    /// Note that the calculation above is a maximum. The snapshot only contains the hashes that
    /// are relevant to the pair of nodes that are comparing snapshots. Also, some sectors may be
    /// empty and will be sent as an empty hash.
    Minimal {
        disc_top_hash: bytes::Bytes,
        disc_boundary: Timestamp,
        ring_top_hashes: Vec<bytes::Bytes>,
    },
    /// A snapshot to be used when there is a [DhtSnapshot::Minimal] mismatch in the disc top hash.
    DiscSectors {
        disc_sector_top_hashes: HashMap<u32, bytes::Bytes>,
        disc_boundary: Timestamp,
    },
    /// A snapshot to be used when there is a [DhtSnapshot::DiscSectors] mismatch.
    ///
    /// For each mismatched disc sector, the snapshot will contain the sector id and all the hashes
    /// for that sector.
    DiscSectorDetails {
        disc_sector_hashes: HashMap<u32, HashMap<u64, bytes::Bytes>>,
        disc_boundary: Timestamp,
    },
    /// A snapshot to be used when there is a [DhtSnapshot::Minimal] mismatch in the ring top
    /// hashes.
    RingSectorDetails {
        ring_sector_hashes: HashMap<u32, HashMap<u32, bytes::Bytes>>,
        disc_boundary: Timestamp,
    },
}

#[derive(Debug)]
#[cfg_attr(test, derive(Eq, PartialEq))]
pub enum SnapshotDiff {
    Identical,
    CannotCompare,
    // Historical mismatch
    DiscMismatch,
    DiscSectorMismatches(Vec<u32>),
    DiscSectorSliceMismatches(HashMap<u32, Vec<u64>>),
    // Recent mismatch
    RingMismatches(Vec<u32>),
    RingSectorMismatches(HashMap<u32, Vec<u32>>),
}

impl DhtSnapshot {
    pub fn compare(&self, other: &Self) -> SnapshotDiff {
        // Check if they match exactly, before doing further work to check how they differ.
        if self == other {
            return SnapshotDiff::Identical;
        }

        match (self, other) {
            (
                DhtSnapshot::Minimal {
                    disc_top_hash: our_disc_top_hash,
                    disc_boundary: our_disc_boundary,
                    ring_top_hashes: our_ring_top_hashes,
                },
                DhtSnapshot::Minimal {
                    disc_top_hash: other_disc_top_hash,
                    disc_boundary: other_disc_boundary,
                    ring_top_hashes: other_ring_top_hashes,
                },
            ) => {
                // If the historical time boundary doesn't match, we can't compare.
                // This won't happen very often so it's okay to just fail this match.
                if our_disc_boundary != other_disc_boundary {
                    return SnapshotDiff::CannotCompare;
                }

                // If the disc hash mismatches, then there is a historical mismatch.
                // This shouldn't be common, but we sync forwards through time, so if we
                // find a historical mismatch then focus on fixing that first.
                if our_disc_top_hash != other_disc_top_hash {
                    return SnapshotDiff::DiscMismatch;
                }

                // This is more common, it can happen if we're close to a UNIT_TIME boundary
                // and there is a small clock difference or just one node calculated this snapshot
                // before the other did. Still, we'll have to wait until we next compare to
                // compare our DHT state.
                if our_ring_top_hashes.len() != other_ring_top_hashes.len() {
                    return SnapshotDiff::CannotCompare;
                }

                // There should always be at least one mismatched ring, otherwise the snapshots
                // would have been identical which has already been checked.
                SnapshotDiff::RingMismatches(hash_mismatch_indices(
                    our_ring_top_hashes,
                    other_ring_top_hashes,
                ))
            }
            (
                DhtSnapshot::DiscSectors {
                    disc_sector_top_hashes: our_disc_sector_top_hashes,
                    disc_boundary: our_disc_boundary,
                },
                DhtSnapshot::DiscSectors {
                    disc_sector_top_hashes: other_disc_sector_top_hashes,
                    disc_boundary: other_disc_boundary,
                },
            ) => {
                if our_disc_boundary != other_disc_boundary {
                    // TODO Don't expect the boundary to move during a comparison so treat this as
                    //      an error at this point? We should have stopped before now.
                    return SnapshotDiff::CannotCompare;
                }

                // If one side has a hash for a sector and the other doesn't then that is a mismatch
                let our_ids =
                    our_disc_sector_top_hashes.keys().collect::<HashSet<_>>();
                let other_ids =
                    other_disc_sector_top_hashes.keys().collect::<HashSet<_>>();
                let mut mismatched_sector_ids = our_ids
                    .symmetric_difference(&other_ids)
                    .map(|id| **id)
                    .collect::<Vec<_>>();

                // Then for any common sectors, check if the hashes match
                let common_ids =
                    our_ids.intersection(&other_ids).collect::<HashSet<_>>();
                for id in common_ids {
                    if our_disc_sector_top_hashes[id]
                        != other_disc_sector_top_hashes[id]
                    {
                        // We found a mismatched sector, store it
                        mismatched_sector_ids.push(**id);
                    }
                }

                // If the number of sectors is the same, then we can compare the hashes.
                // There will be at least one mismatched sector, otherwise the snapshots would be
                // identical which has already been checked.
                SnapshotDiff::DiscSectorMismatches(mismatched_sector_ids)
            }
            (
                DhtSnapshot::DiscSectorDetails {
                    disc_sector_hashes: our_disc_sector_hashes,
                    disc_boundary: our_disc_boundary,
                },
                DhtSnapshot::DiscSectorDetails {
                    disc_sector_hashes: other_disc_sector_hashes,
                    disc_boundary: other_disc_boundary,
                },
            ) => {
                if our_disc_boundary != other_disc_boundary {
                    // TODO Don't expect the boundary to move during a comparison so treat this as
                    //      an error at this point? We should have stopped before now.
                    return SnapshotDiff::CannotCompare;
                }

                let our_ids =
                    our_disc_sector_hashes.keys().collect::<HashSet<_>>();
                let other_ids =
                    other_disc_sector_hashes.keys().collect::<HashSet<_>>();

                // If one side has a sector and the other doesn't then that is a mismatch
                let mut mismatched_sector_ids = our_ids
                    .symmetric_difference(&other_ids)
                    .map(|id| (**id, Vec::new()))
                    .collect::<HashMap<_, _>>();

                // Then for any common sectors, check if the hashes match
                let common_sector_ids =
                    our_ids.intersection(&other_ids).collect::<HashSet<_>>();
                for sector_id in common_sector_ids {
                    let our_slice_ids = &our_disc_sector_hashes[sector_id]
                        .keys()
                        .collect::<HashSet<_>>();
                    let other_slice_ids = &other_disc_sector_hashes[sector_id]
                        .keys()
                        .collect::<HashSet<_>>();

                    let mut mismatched_slice_ids = our_slice_ids
                        .symmetric_difference(other_slice_ids)
                        .map(|id| **id)
                        .collect::<Vec<_>>();

                    let common_slice_ids = our_slice_ids
                        .intersection(other_slice_ids)
                        .collect::<HashSet<_>>();

                    for slice_id in common_slice_ids {
                        if our_disc_sector_hashes[sector_id][slice_id]
                            != other_disc_sector_hashes[sector_id][slice_id]
                        {
                            mismatched_slice_ids.push(**slice_id);
                        }
                    }

                    mismatched_sector_ids
                        .entry(**sector_id)
                        .or_insert_with(Vec::new)
                        .extend(mismatched_slice_ids);
                }

                SnapshotDiff::DiscSectorSliceMismatches(mismatched_sector_ids)
            }
            (
                DhtSnapshot::RingSectorDetails {
                    ring_sector_hashes: our_ring_sector_hashes,
                    disc_boundary: our_disc_boundary,
                },
                DhtSnapshot::RingSectorDetails {
                    ring_sector_hashes: other_ring_sector_hashes,
                    disc_boundary: other_disc_boundary,
                },
            ) => {
                if our_disc_boundary != other_disc_boundary {
                    // TODO Don't expect the boundary to move during a comparison so treat this as
                    //      an error at this point? We should have stopped before now.
                    return SnapshotDiff::CannotCompare;
                }

                let our_ids =
                    our_ring_sector_hashes.keys().collect::<HashSet<_>>();
                let other_ids =
                    other_ring_sector_hashes.keys().collect::<HashSet<_>>();

                // If one side has a hash in a given ring and the other doesn't then that is a
                // mismatch
                let mut mismatched_ring_sectors = our_ids
                    .symmetric_difference(&other_ids)
                    .map(|id| (**id, Vec::new()))
                    .collect::<HashMap<_, _>>();

                // Then for any common rings, check if the hashes match
                let common_ring_ids =
                    our_ids.intersection(&other_ids).collect::<HashSet<_>>();
                for ring_id in common_ring_ids {
                    let our_sector_ids = &our_ring_sector_hashes[ring_id]
                        .keys()
                        .collect::<HashSet<_>>();
                    let other_sector_ids = &other_ring_sector_hashes[ring_id]
                        .keys()
                        .collect::<HashSet<_>>();

                    let mut mismatched_sector_ids = our_sector_ids
                        .symmetric_difference(other_sector_ids)
                        .map(|id| **id)
                        .collect::<Vec<_>>();

                    let common_sector_ids = our_sector_ids
                        .intersection(other_sector_ids)
                        .collect::<HashSet<_>>();

                    for sector_id in common_sector_ids {
                        if our_ring_sector_hashes[ring_id][sector_id]
                            != other_ring_sector_hashes[ring_id][sector_id]
                        {
                            mismatched_sector_ids.push(**sector_id);
                        }
                    }

                    mismatched_ring_sectors
                        .insert(**ring_id, mismatched_sector_ids);
                }

                SnapshotDiff::RingSectorMismatches(mismatched_ring_sectors)
            }
            (theirs, other) => {
                tracing::error!(
                    "Mismatched snapshot types: ours: {:?}, theirs: {:?}",
                    theirs,
                    other
                );
                SnapshotDiff::CannotCompare
            }
        }
    }
}

fn hash_mismatch_indices(
    left: &[bytes::Bytes],
    right: &[bytes::Bytes],
) -> Vec<u32> {
    left.iter()
        .enumerate()
        .zip(right.iter())
        .filter_map(|((idx, left), right)| {
            if left != right {
                Some(idx as u32)
            } else {
                None
            }
        })
        .collect()
}

#[derive(Debug)]
pub enum DhtSnapshotCompareOutcome {
    Identical,
    CannotCompare,
    NewSnapshot(DhtSnapshot),
    HashList(DhtSnapshot, Vec<OpId>),
}

impl Dht {
    pub async fn try_from_store(
        current_time: Timestamp,
        store: DynOpStore,
    ) -> K2Result<Dht> {
        Ok(Dht {
            partition: PartitionedHashes::try_from_store(
                14,
                current_time,
                store,
            )
            .await?,
        })
    }

    pub fn next_update_at(&self) -> Timestamp {
        self.partition.next_update_at()
    }

    pub async fn update(
        &mut self,
        current_time: Timestamp,
        store: DynOpStore,
    ) -> K2Result<()> {
        self.partition.update(store, current_time).await
    }

    pub async fn inform_ops_stored(
        &mut self,
        store: DynOpStore,
        stored_ops: Vec<StoredOp>,
    ) -> K2Result<()> {
        self.partition.inform_ops_stored(store, stored_ops).await
    }

    pub async fn snapshot_minimal(
        &self,
        arc_set: &ArcSet,
        store: DynOpStore,
    ) -> K2Result<DhtSnapshot> {
        let (disc_top_hash, disc_boundary) = self
            .partition
            .full_time_slice_disc_top_hash(arc_set, store)
            .await?;

        Ok(DhtSnapshot::Minimal {
            disc_top_hash,
            disc_boundary,
            ring_top_hashes: self.partition.partial_time_rings(arc_set),
        })
    }

    pub async fn handle_snapshot(
        &self,
        their_snapshot: DhtSnapshot,
        arc_set: &ArcSet,
        store: DynOpStore,
    ) -> K2Result<DhtSnapshotCompareOutcome> {
        let our_snapshot = match &their_snapshot {
            DhtSnapshot::Minimal { .. } => {
                self.snapshot_minimal(arc_set, store.clone()).await?
            }
            DhtSnapshot::DiscSectors { .. } => {
                self.snapshot_disc_sectors(arc_set, store.clone()).await?
            }
            DhtSnapshot::DiscSectorDetails {
                disc_sector_hashes, ..
            } => {
                self.snapshot_disc_sector_details(
                    disc_sector_hashes.keys().cloned().collect(),
                    arc_set,
                    store.clone(),
                )
                .await?
            }
            DhtSnapshot::RingSectorDetails {
                ring_sector_hashes, ..
            } => self.snapshot_ring_sector_details(
                ring_sector_hashes.keys().cloned().collect(),
                arc_set,
            )?,
        };

        println!("Our snapshot: {:?}", our_snapshot);

        match our_snapshot.compare(&their_snapshot) {
            SnapshotDiff::Identical => Ok(DhtSnapshotCompareOutcome::Identical),
            SnapshotDiff::CannotCompare => {
                Ok(DhtSnapshotCompareOutcome::CannotCompare)
            }
            SnapshotDiff::DiscMismatch => {
                Ok(DhtSnapshotCompareOutcome::NewSnapshot(
                    self.snapshot_disc_sectors(arc_set, store).await?,
                ))
            }
            SnapshotDiff::DiscSectorMismatches(mismatched_sectors) => {
                Ok(DhtSnapshotCompareOutcome::NewSnapshot(
                    self.snapshot_disc_sector_details(
                        mismatched_sectors,
                        arc_set,
                        store,
                    )
                    .await?,
                ))
            }
            SnapshotDiff::DiscSectorSliceMismatches(mismatched_slice_ids) => {
                println!(
                    "Creating response for slice mismatches: {:?}",
                    mismatched_slice_ids
                );
                let mut out = Vec::new();
                for (sector_id, missing_slices) in mismatched_slice_ids {
                    let Ok(arc) =
                        self.partition.dht_arc_for_sector_id(sector_id)
                    else {
                        tracing::error!(
                            "Sector id {} out of bounds, ignoring",
                            sector_id
                        );
                        continue;
                    };

                    println!("Using arc: {:?}", arc);

                    // TODO handle an empty list of missing slices here?
                    //      if that's empty then we actually need to send the whole sector?

                    for missing_slice in missing_slices {
                        let Ok((start, end)) = self
                            .partition
                            .time_bounds_for_full_slice_id(missing_slice)
                        else {
                            tracing::error!(
                                "Missing slice {} out of bounds, ignoring",
                                missing_slice
                            );
                            continue;
                        };

                        println!("In time range: {:?} - {:?}", start, end);

                        out.extend(
                            store
                                .retrieve_op_hashes_in_time_slice(
                                    arc, start, end,
                                )
                                .await?,
                        );
                        println!("Out is now: {:?}", out);
                    }
                }

                Ok(DhtSnapshotCompareOutcome::HashList(our_snapshot, out))
            }
            SnapshotDiff::RingMismatches(mismatched_rings) => {
                println!("Mismatched rings: {:?}", mismatched_rings);
                Ok(DhtSnapshotCompareOutcome::NewSnapshot(
                    self.snapshot_ring_sector_details(
                        mismatched_rings,
                        arc_set,
                    )?,
                ))
            }
            SnapshotDiff::RingSectorMismatches(mismatched_sectors) => {
                let mut out = Vec::new();

                for (ring_id, missing_sectors) in mismatched_sectors {
                    for sector_id in missing_sectors {
                        let Ok(arc) =
                            self.partition.dht_arc_for_sector_id(sector_id)
                        else {
                            tracing::error!(
                                "Sector id {} out of bounds, ignoring",
                                sector_id
                            );
                            continue;
                        };

                        let Ok((start, end)) = self
                            .partition
                            .time_bounds_for_partial_slice_id(ring_id)
                        else {
                            tracing::error!(
                                "Partial slice id {} out of bounds, ignoring",
                                ring_id
                            );
                            continue;
                        };

                        out.extend(
                            store
                                .retrieve_op_hashes_in_time_slice(
                                    arc, start, end,
                                )
                                .await?,
                        );
                    }
                }

                Ok(DhtSnapshotCompareOutcome::HashList(our_snapshot, out))
            }
        }
    }

    async fn snapshot_disc_sectors(
        &self,
        arc_set: &ArcSet,
        store: DynOpStore,
    ) -> K2Result<DhtSnapshot> {
        let (disc_sector_top_hashes, disc_boundary) = self
            .partition
            .full_time_slice_sector_hashes(arc_set, store)
            .await?;

        Ok(DhtSnapshot::DiscSectors {
            disc_sector_top_hashes,
            disc_boundary,
        })
    }

    async fn snapshot_disc_sector_details(
        &self,
        mismatched_sector_ids: Vec<u32>,
        arc_set: &ArcSet,
        store: DynOpStore,
    ) -> K2Result<DhtSnapshot> {
        let (disc_sector_hashes, disc_boundary) = self
            .partition
            .full_time_slice_sector_details(
                mismatched_sector_ids,
                arc_set,
                store,
            )
            .await?;

        Ok(DhtSnapshot::DiscSectorDetails {
            disc_sector_hashes,
            disc_boundary,
        })
    }

    fn snapshot_ring_sector_details(
        &self,
        mismatched_rings: Vec<u32>,
        arc_set: &ArcSet,
    ) -> K2Result<DhtSnapshot> {
        let (ring_sector_hashes, disc_boundary) = self
            .partition
            .partial_time_slice_details(mismatched_rings, arc_set)?;

        Ok(DhtSnapshot::RingSectorDetails {
            ring_sector_hashes,
            disc_boundary,
        })
    }
}

#[cfg(test)]
mod snapshot_tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn minimal_self_identical() {
        let snapshot = DhtSnapshot::Minimal {
            disc_top_hash: bytes::Bytes::from(vec![1; 32]),
            disc_boundary: Timestamp::now(),
            ring_top_hashes: vec![bytes::Bytes::from(vec![2; 32])],
        };

        assert_eq!(SnapshotDiff::Identical, snapshot.compare(&snapshot));
    }

    #[test]
    fn minimal_disc_hash_mismatch() {
        let timestamp = Timestamp::now();
        let snapshot_1 = DhtSnapshot::Minimal {
            disc_boundary: timestamp,
            disc_top_hash: bytes::Bytes::from(vec![1; 32]),
            ring_top_hashes: vec![],
        };

        let snapshot_2 = DhtSnapshot::Minimal {
            disc_boundary: timestamp,
            disc_top_hash: bytes::Bytes::from(vec![2; 32]),
            ring_top_hashes: vec![],
        };

        assert_eq!(snapshot_1.compare(&snapshot_2), SnapshotDiff::DiscMismatch);
    }

    #[test]
    fn minimal_disc_boundary_mismatch() {
        let snapshot_1 = DhtSnapshot::Minimal {
            disc_boundary: Timestamp::now(),
            disc_top_hash: bytes::Bytes::from(vec![1; 32]),
            ring_top_hashes: vec![],
        };

        let snapshot_2 = DhtSnapshot::Minimal {
            // Just to be sure, we're using `::now()` twice but it can return the same value.
            disc_boundary: Timestamp::now() + Duration::from_secs(1),
            disc_top_hash: bytes::Bytes::from(vec![1; 32]),
            ring_top_hashes: vec![],
        };

        assert_eq!(
            snapshot_1.compare(&snapshot_2),
            SnapshotDiff::CannotCompare
        );
    }
}

#[cfg(test)]
mod dht_tests {
    use super::*;
    use kitsune2_api::{AgentId, DhtArc, OpId, OpStore, UNIX_TIMESTAMP};
    use kitsune2_memory::{Kitsune2MemoryOp, Kitsune2MemoryOpStore};
    use rand::RngCore;
    use std::sync::Arc;
    use std::time::Duration;

    const SECTOR_SIZE: u32 = 1u32 << 23;

    async fn transfer_ops(
        source: DynOpStore,
        target: DynOpStore,
        target_dht: &mut Dht,
        requested_ops: Vec<OpId>,
    ) -> K2Result<()> {
        let selected = source.retrieve_ops(requested_ops).await?;
        target.process_incoming_ops(selected.clone()).await?;

        let stored_ops = selected
            .into_iter()
            .map(|op| Kitsune2MemoryOp::try_from(op).unwrap().into())
            .collect::<Vec<StoredOp>>();
        target_dht
            .inform_ops_stored(target.clone(), stored_ops)
            .await?;

        Ok(())
    }

    /// Intended to represent a single agent in a network, which knows how to sync with
    /// some other agent.
    struct DhtSyncHarness {
        store: Arc<Kitsune2MemoryOpStore>,
        dht: Dht,
        arc: DhtArc,
        agent_id: AgentId,
        discovered_ops: HashMap<AgentId, Vec<OpId>>,
    }

    enum SyncWithOutcome {
        InSync,
        CannotCompare,
        SyncedDisc,
        SyncedRings,
        DiscoveredInSync,
    }

    impl DhtSyncHarness {
        async fn new(current_time: Timestamp, arc: DhtArc) -> Self {
            let store = Arc::new(Kitsune2MemoryOpStore::default());
            let dht = Dht::try_from_store(current_time, store.clone())
                .await
                .unwrap();

            let mut bytes = [0; 32];
            rand::thread_rng().fill_bytes(&mut bytes);
            let agent_id = AgentId::from(bytes::Bytes::copy_from_slice(&bytes));

            Self {
                store,
                dht,
                arc,
                agent_id,
                discovered_ops: HashMap::new(),
            }
        }

        async fn inject_ops(
            &mut self,
            op_list: Vec<Kitsune2MemoryOp>,
        ) -> K2Result<()> {
            self.store
                .process_incoming_ops(
                    op_list
                        .iter()
                        .map(|op| op.clone().try_into().unwrap())
                        .collect(),
                )
                .await?;

            self.dht
                .inform_ops_stored(
                    self.store.clone(),
                    op_list.into_iter().map(|op| op.into()).collect(),
                )
                .await?;

            Ok(())
        }

        async fn apply_op_sync(&mut self, other: &mut Self) -> K2Result<()> {
            // Sync from other to self, using op IDs we've discovered from other
            if let Some(ops) = self
                .discovered_ops
                .get_mut(&other.agent_id)
                .map(std::mem::take)
            {
                transfer_ops(
                    other.store.clone(),
                    self.store.clone(),
                    &mut self.dht,
                    ops,
                )
                .await?;
            }

            // Sync from self to other, using op that the other has discovered from us
            if let Some(ops) = other
                .discovered_ops
                .get_mut(&self.agent_id)
                .map(std::mem::take)
            {
                transfer_ops(
                    self.store.clone(),
                    other.store.clone(),
                    &mut other.dht,
                    ops,
                )
                .await?;
            }

            Ok(())
        }

        async fn is_in_sync_with(&self, other: &Self) -> K2Result<bool> {
            let arc_set_1 = ArcSet::new(SECTOR_SIZE, vec![self.arc])?;
            let arc_set_2 = ArcSet::new(SECTOR_SIZE, vec![other.arc])?;
            let arc_set = arc_set_1.intersection(&arc_set_2);
            let initial_snapshot = self
                .dht
                .snapshot_minimal(&arc_set, self.store.clone())
                .await?;
            match other
                .dht
                .handle_snapshot(
                    initial_snapshot,
                    &arc_set,
                    other.store.clone(),
                )
                .await?
            {
                DhtSnapshotCompareOutcome::Identical => Ok(true),
                _ => Ok(false),
            }
        }

        async fn sync_with(
            &mut self,
            other: &mut Self,
        ) -> K2Result<SyncWithOutcome> {
            let arc_set_1 = ArcSet::new(SECTOR_SIZE, vec![self.arc])?;
            let arc_set_2 = ArcSet::new(SECTOR_SIZE, vec![other.arc])?;
            let arc_set = arc_set_1.intersection(&arc_set_2);

            // Create the initial snapshot locally
            let initial_snapshot = self
                .dht
                .snapshot_minimal(&arc_set, self.store.clone())
                .await?;

            // Send it to the other agent and have them diff against it
            let outcome = other
                .dht
                .handle_snapshot(
                    initial_snapshot,
                    &arc_set,
                    other.store.clone(),
                )
                .await?;

            match outcome {
                DhtSnapshotCompareOutcome::Identical => {
                    // Nothing to do, the agents are in sync
                    Ok(SyncWithOutcome::InSync)
                }
                DhtSnapshotCompareOutcome::CannotCompare => {
                    // Permit this for testing purposes, it would be treated as an error in
                    // real usage
                    Ok(SyncWithOutcome::CannotCompare)
                }
                DhtSnapshotCompareOutcome::NewSnapshot(new_snapshot) => {
                    match new_snapshot {
                        DhtSnapshot::Minimal { .. } => {
                            panic!("A minimal snapshot cannot be produced from a minimal snapshot");
                        }
                        DhtSnapshot::DiscSectors { .. } => {
                            // This means there's a historical mismatch, so we need to continue
                            // down this path another step. Do that in another function!
                            self.sync_disc_with(other, &arc_set, new_snapshot)
                                .await
                        }
                        DhtSnapshot::DiscSectorDetails { .. } => {
                            panic!("A sector details snapshot cannot be produced from a minimal snapshot");
                        }
                        DhtSnapshot::RingSectorDetails { .. } => {
                            // This means there's a recent mismatch in the partial time slices.
                            // Similar to above, continue in another function.
                            self.sync_rings_with(other, &arc_set, new_snapshot)
                                .await
                        }
                    }
                }
                DhtSnapshotCompareOutcome::HashList(_, _) => {
                    panic!("A hash list cannot be produced from a minimal snapshot");
                }
            }
        }

        async fn sync_disc_with(
            &mut self,
            other: &mut Self,
            arc_set: &ArcSet,
            snapshot: DhtSnapshot,
        ) -> K2Result<SyncWithOutcome> {
            match snapshot {
                DhtSnapshot::DiscSectors { .. } => {}
                _ => panic!("Expected a DiscSectors snapshot"),
            }

            // We expect the sync to have been initiated by self, so the disc snapshot should be
            // coming back to us
            let outcome = self
                .dht
                .handle_snapshot(snapshot, arc_set, self.store.clone())
                .await?;

            let snapshot = match outcome {
                DhtSnapshotCompareOutcome::NewSnapshot(new_snapshot) => {
                    new_snapshot
                }
                DhtSnapshotCompareOutcome::Identical => {
                    // This can't happen in tests but in a real implementation it's possible that
                    // missing ops might show up while we're syncing so this isn't an error, just
                    // a shortcut and we can stop syncing
                    return Ok(SyncWithOutcome::DiscoveredInSync);
                }
                DhtSnapshotCompareOutcome::HashList(_, _)
                | DhtSnapshotCompareOutcome::CannotCompare => {
                    // A real implementation would treat these as errors because they are logic
                    // errors
                    panic!("Unexpected outcome: {:?}", outcome);
                }
            };

            // At this point, the snapshot must be a disc details snapshot
            match snapshot {
                DhtSnapshot::DiscSectorDetails { .. } => {}
                _ => panic!("Expected a DiscSectorDetails snapshot"),
            }

            // Now we need to ask the other agent to diff against this details snapshot
            let outcome = other
                .dht
                .handle_snapshot(snapshot, arc_set, other.store.clone())
                .await?;

            let (snapshot, hash_list_from_other) = match outcome {
                DhtSnapshotCompareOutcome::HashList(
                    new_snapshot,
                    hash_list,
                ) => (new_snapshot, hash_list),
                DhtSnapshotCompareOutcome::Identical => {
                    // Nothing to do, the agents are in sync
                    return Ok(SyncWithOutcome::InSync);
                }
                DhtSnapshotCompareOutcome::NewSnapshot(_)
                | DhtSnapshotCompareOutcome::CannotCompare => {
                    // A real implementation would treat these as errors because they are logic
                    // errors
                    panic!("Unexpected outcome: {:?}", outcome);
                }
            };

            // This snapshot must also be a disc details snapshot
            match snapshot {
                DhtSnapshot::DiscSectorDetails { .. } => {}
                _ => panic!("Expected a DiscSectorDetails snapshot"),
            }

            // Finally, we need to receive the details snapshot from the other agent and send them
            // back our ops
            let outcome = self
                .dht
                .handle_snapshot(snapshot, arc_set, self.store.clone())
                .await?;

            println!("Our final outcome: {:?}", outcome);

            // TODO leaving duplicated code here, there's some inefficiency here that actually needs
            //      resolving
            let hash_list_from_self = match outcome {
                DhtSnapshotCompareOutcome::HashList(_, hash_list) => hash_list,
                DhtSnapshotCompareOutcome::Identical => {
                    // Nothing to do, the agents are in sync
                    return Ok(SyncWithOutcome::InSync);
                }
                DhtSnapshotCompareOutcome::NewSnapshot(_)
                | DhtSnapshotCompareOutcome::CannotCompare => {
                    // A real implementation would treat these as errors because they are logic
                    // errors
                    panic!("Unexpected outcome: {:?}", outcome);
                }
            };

            // Capture the discovered ops, but don't actually transfer them yet.
            // Let the test decide when to do that.
            if !hash_list_from_other.is_empty() {
                self.discovered_ops
                    .entry(other.agent_id.clone())
                    .or_default()
                    .extend(hash_list_from_other);
            }
            if !hash_list_from_self.is_empty() {
                other
                    .discovered_ops
                    .entry(self.agent_id.clone())
                    .or_default()
                    .extend(hash_list_from_self);
            }

            Ok(SyncWithOutcome::SyncedDisc)
        }

        async fn sync_rings_with(
            &mut self,
            other: &mut Self,
            arc_set: &ArcSet,
            snapshot: DhtSnapshot,
        ) -> K2Result<SyncWithOutcome> {
            match snapshot {
                DhtSnapshot::RingSectorDetails { .. } => {}
                _ => panic!("Expected a RingSectorDetails snapshot"),
            }

            // We expect the sync to have been initiated by self, so the ring sector details should
            // have been sent to us
            let outcome = self
                .dht
                .handle_snapshot(snapshot, arc_set, self.store.clone())
                .await?;

            let (snapshot, hash_list_from_self) = match outcome {
                DhtSnapshotCompareOutcome::Identical => {
                    // Nothing to do, the agents are in sync
                    return Ok(SyncWithOutcome::InSync);
                }
                DhtSnapshotCompareOutcome::HashList(
                    new_snapshot,
                    hash_list,
                ) => (new_snapshot, hash_list),
                DhtSnapshotCompareOutcome::CannotCompare
                | DhtSnapshotCompareOutcome::NewSnapshot(_) => {
                    panic!("Unexpected outcome: {:?}", outcome);
                }
            };

            // This snapshot must also be a ring sector details snapshot
            match snapshot {
                DhtSnapshot::RingSectorDetails { .. } => {}
                _ => panic!("Expected a RingSectorDetails snapshot"),
            }

            // Finally, we need to send the ring sector details back to the other agent so they can
            // produce a hash list for us
            let outcome = other
                .dht
                .handle_snapshot(snapshot, arc_set, other.store.clone())
                .await?;

            // TODO same problem here, we don't need to have re-computed the snapshot
            let hash_list_from_other = match outcome {
                DhtSnapshotCompareOutcome::Identical => {
                    // Nothing to do, the agents are in sync
                    return Ok(SyncWithOutcome::InSync);
                }
                DhtSnapshotCompareOutcome::HashList(_, hash_list) => hash_list,
                DhtSnapshotCompareOutcome::CannotCompare
                | DhtSnapshotCompareOutcome::NewSnapshot(_) => {
                    panic!("Unexpected outcome: {:?}", outcome);
                }
            };

            // Capture the discovered ops, but don't actually transfer them yet.
            // Let the test decide when to do that.
            if !hash_list_from_other.is_empty() {
                self.discovered_ops
                    .entry(other.agent_id.clone())
                    .or_default()
                    .extend(hash_list_from_other);
            }
            if !hash_list_from_self.is_empty() {
                other
                    .discovered_ops
                    .entry(self.agent_id.clone())
                    .or_default()
                    .extend(hash_list_from_self);
            }

            Ok(SyncWithOutcome::SyncedRings)
        }
    }

    #[tokio::test]
    async fn from_store_empty() {
        let store = Arc::new(Kitsune2MemoryOpStore::default());
        Dht::try_from_store(UNIX_TIMESTAMP, store).await.unwrap();
    }

    #[tokio::test]
    async fn take_minimal_snapshot() {
        let store = Arc::new(Kitsune2MemoryOpStore::default());
        store
            .process_incoming_ops(vec![Kitsune2MemoryOp::new(
                OpId::from(bytes::Bytes::from(vec![7; 32])),
                UNIX_TIMESTAMP,
                vec![],
            )
            .try_into()
            .unwrap()])
            .await
            .unwrap();

        let dht = Dht::try_from_store(Timestamp::now(), store.clone())
            .await
            .unwrap();

        let arc_set = ArcSet::new(SECTOR_SIZE, vec![DhtArc::FULL]).unwrap();

        let snapshot =
            dht.snapshot_minimal(&arc_set, store.clone()).await.unwrap();
        match snapshot {
            DhtSnapshot::Minimal {
                disc_boundary,
                disc_top_hash,
                ring_top_hashes,
            } => {
                assert_eq!(
                    dht.partition.full_slice_end_timestamp(),
                    disc_boundary
                );
                assert_eq!(bytes::Bytes::from(vec![7; 32]), disc_top_hash);
                assert!(!ring_top_hashes.is_empty());
            }
            s => panic!("Unexpected snapshot type: {:?}", s),
        }
    }

    #[tokio::test]
    async fn empty_dht_is_in_sync_with_empty() {
        let current_time = Timestamp::now();
        let dht1 = DhtSyncHarness::new(current_time, DhtArc::FULL).await;
        let dht2 = DhtSyncHarness::new(current_time, DhtArc::FULL).await;

        assert!(dht1.is_in_sync_with(&dht2).await.unwrap());
        assert!(dht2.is_in_sync_with(&dht1).await.unwrap());
    }

    #[tokio::test]
    async fn one_way_disc_sync_from_initiator() {
        let current_time = Timestamp::now();
        let mut dht1 = DhtSyncHarness::new(current_time, DhtArc::FULL).await;
        let mut dht2 = DhtSyncHarness::new(current_time, DhtArc::FULL).await;

        // Put historical data in the first DHT
        dht1.inject_ops(vec![Kitsune2MemoryOp::new(
            OpId::from(bytes::Bytes::from(vec![41; 32])),
            UNIX_TIMESTAMP,
            vec![],
        )])
        .await
        .unwrap();

        // Try a sync
        let outcome = dht1.sync_with(&mut dht2).await.unwrap();
        assert!(matches!(outcome, SyncWithOutcome::SyncedDisc));

        // We shouldn't learn about any ops
        assert!(dht1.discovered_ops.is_empty());

        // The other agent should have learned about the op
        assert_eq!(1, dht2.discovered_ops.len());
        assert_eq!(1, dht2.discovered_ops[&dht1.agent_id].len());

        // Move any discovered ops between the two DHTs
        dht1.apply_op_sync(&mut dht2).await.unwrap();

        // Now the two DHTs should be in sync
        assert!(dht1.is_in_sync_with(&dht2).await.unwrap());
    }

    #[tokio::test]
    async fn one_way_disc_sync_from_acceptor() {
        let current_time = Timestamp::now();
        let mut dht1 = DhtSyncHarness::new(current_time, DhtArc::FULL).await;
        let mut dht2 = DhtSyncHarness::new(current_time, DhtArc::FULL).await;

        // Put historical data in the second DHT
        dht2.inject_ops(vec![Kitsune2MemoryOp::new(
            OpId::from(bytes::Bytes::from(vec![41; 32])),
            UNIX_TIMESTAMP,
            vec![],
        )])
        .await
        .unwrap();

        // Try a sync initiated by the first agent
        let outcome = dht1.sync_with(&mut dht2).await.unwrap();
        assert!(matches!(outcome, SyncWithOutcome::SyncedDisc));

        // They shouldn't learn about any ops
        assert!(dht2.discovered_ops.is_empty());

        // We should have learned about the op
        assert_eq!(1, dht1.discovered_ops.len());
        assert_eq!(1, dht1.discovered_ops[&dht2.agent_id].len());

        // Move any discovered ops between the two DHTs
        dht1.apply_op_sync(&mut dht2).await.unwrap();

        // Now the two DHTs should be in sync
        assert!(dht1.is_in_sync_with(&dht2).await.unwrap());
    }

    #[tokio::test]
    async fn two_way_disc_sync() {
        let current_time = Timestamp::now();
        let mut dht1 = DhtSyncHarness::new(current_time, DhtArc::FULL).await;
        let mut dht2 = DhtSyncHarness::new(current_time, DhtArc::FULL).await;

        // Put historical data in both DHTs
        dht1.inject_ops(vec![Kitsune2MemoryOp::new(
            OpId::from(bytes::Bytes::from(vec![7; 32])),
            UNIX_TIMESTAMP,
            vec![],
        )])
        .await
        .unwrap();
        dht2.inject_ops(vec![Kitsune2MemoryOp::new(
            OpId::from(bytes::Bytes::from(vec![43; 32])),
            // Two weeks later
            UNIX_TIMESTAMP + Duration::from_secs(14 * 24 * 60 * 60),
            vec![],
        )])
        .await
        .unwrap();

        // Try a sync initiated by the first agent
        let outcome = dht1.sync_with(&mut dht2).await.unwrap();
        assert!(matches!(outcome, SyncWithOutcome::SyncedDisc));

        // Each learns about one op
        assert_eq!(1, dht1.discovered_ops.len());
        assert_eq!(1, dht1.discovered_ops[&dht2.agent_id].len());
        assert_eq!(1, dht2.discovered_ops.len());
        assert_eq!(1, dht2.discovered_ops[&dht1.agent_id].len());

        // Move any discovered ops between the two DHTs
        dht1.apply_op_sync(&mut dht2).await.unwrap();

        // Now the two DHTs should be in sync
        assert!(dht1.is_in_sync_with(&dht2).await.unwrap());
    }

    #[tokio::test]
    async fn one_way_ring_sync_from_initiator() {
        let current_time = Timestamp::now();
        let mut dht1 = DhtSyncHarness::new(current_time, DhtArc::FULL).await;
        let mut dht2 = DhtSyncHarness::new(current_time, DhtArc::FULL).await;

        // Put recent data in the first ring of the first DHT
        dht1.inject_ops(vec![Kitsune2MemoryOp::new(
            OpId::from(bytes::Bytes::from(vec![41; 32])),
            dht1.dht.partition.full_slice_end_timestamp(),
            vec![],
        )])
        .await
        .unwrap();

        // Try a sync
        let outcome = dht1.sync_with(&mut dht2).await.unwrap();
        assert!(matches!(outcome, SyncWithOutcome::SyncedRings));

        // We shouldn't learn about any ops
        assert!(dht1.discovered_ops.is_empty());

        // The other agent should have learned about the op
        assert_eq!(1, dht2.discovered_ops.len());
        assert_eq!(1, dht2.discovered_ops[&dht1.agent_id].len());

        // Move any discovered ops between the two DHTs
        dht1.apply_op_sync(&mut dht2).await.unwrap();

        // Now the two DHTs should be in sync
        assert!(dht1.is_in_sync_with(&dht2).await.unwrap());
    }

    #[tokio::test]
    async fn one_way_ring_sync_from_acceptor() {
        let current_time = Timestamp::now();
        let mut dht1 = DhtSyncHarness::new(current_time, DhtArc::FULL).await;
        let mut dht2 = DhtSyncHarness::new(current_time, DhtArc::FULL).await;

        // Put recent data in the first ring of the second DHT
        dht2.inject_ops(vec![Kitsune2MemoryOp::new(
            OpId::from(bytes::Bytes::from(vec![41; 32])),
            dht1.dht.partition.full_slice_end_timestamp(),
            vec![],
        )])
        .await
        .unwrap();

        // Try a sync initiated by the first agent
        let outcome = dht1.sync_with(&mut dht2).await.unwrap();
        assert!(matches!(outcome, SyncWithOutcome::SyncedRings));

        // They shouldn't learn about any ops
        assert!(dht2.discovered_ops.is_empty());

        // We should have learned about the op
        assert_eq!(1, dht1.discovered_ops.len());
        assert_eq!(1, dht1.discovered_ops[&dht2.agent_id].len());

        // Move any discovered ops between the two DHTs
        dht1.apply_op_sync(&mut dht2).await.unwrap();

        // Now the two DHTs should be in sync
        assert!(dht1.is_in_sync_with(&dht2).await.unwrap());
    }

    #[tokio::test]
    async fn two_way_ring_sync() {
        let current_time = Timestamp::now();
        let mut dht1 = DhtSyncHarness::new(current_time, DhtArc::FULL).await;
        let mut dht2 = DhtSyncHarness::new(current_time, DhtArc::FULL).await;

        // Put recent data in the first ring of both DHTs
        dht1.inject_ops(vec![Kitsune2MemoryOp::new(
            OpId::from(bytes::Bytes::from(vec![7; 32])),
            dht1.dht.partition.full_slice_end_timestamp(),
            vec![],
        )])
        .await
        .unwrap();
        dht2.inject_ops(vec![Kitsune2MemoryOp::new(
            OpId::from(bytes::Bytes::from(vec![43; 32])),
            // Two weeks later
            dht1.dht.partition.full_slice_end_timestamp(),
            vec![],
        )])
        .await
        .unwrap();

        // Try a sync initiated by the first agent
        let outcome = dht1.sync_with(&mut dht2).await.unwrap();
        assert!(matches!(outcome, SyncWithOutcome::SyncedRings));

        // Each learns about one op
        assert_eq!(1, dht1.discovered_ops.len());
        assert_eq!(1, dht1.discovered_ops[&dht2.agent_id].len());
        assert_eq!(1, dht2.discovered_ops.len());
        assert_eq!(1, dht2.discovered_ops[&dht1.agent_id].len());

        // Move any discovered ops between the two DHTs
        dht1.apply_op_sync(&mut dht2).await.unwrap();

        // Now the two DHTs should be in sync
        assert!(dht1.is_in_sync_with(&dht2).await.unwrap());
    }

    #[tokio::test]
    async fn ring_sync_with_matching_disc() {
        let current_time = Timestamp::now();
        let mut dht1 = DhtSyncHarness::new(current_time, DhtArc::FULL).await;
        let mut dht2 = DhtSyncHarness::new(current_time, DhtArc::FULL).await;

        // Put historical data in both DHTs
        let historical_ops = vec![
            Kitsune2MemoryOp::new(
                OpId::from(bytes::Bytes::from(vec![7; 32])),
                UNIX_TIMESTAMP,
                vec![],
            ),
            Kitsune2MemoryOp::new(
                OpId::from(bytes::Bytes::from(
                    (u32::MAX / 2).to_le_bytes().to_vec(),
                )),
                UNIX_TIMESTAMP + Duration::from_secs(14 * 24 * 60 * 60),
                vec![],
            ),
        ];
        dht1.inject_ops(historical_ops.clone()).await.unwrap();
        dht2.inject_ops(historical_ops).await.unwrap();

        // Put recent data in the first ring of both DHTs
        dht1.inject_ops(vec![Kitsune2MemoryOp::new(
            OpId::from(bytes::Bytes::from(vec![7; 32])),
            dht1.dht.partition.full_slice_end_timestamp(),
            vec![],
        )])
        .await
        .unwrap();

        dht2.inject_ops(vec![Kitsune2MemoryOp::new(
            OpId::from(bytes::Bytes::from(vec![13; 32])),
            dht1.dht.partition.full_slice_end_timestamp(),
            vec![],
        )])
        .await
        .unwrap();

        // Try a sync initiated by the first agent
        let outcome = dht1.sync_with(&mut dht2).await.unwrap();
        assert!(matches!(outcome, SyncWithOutcome::SyncedRings));

        // Each learns about one op
        assert_eq!(1, dht1.discovered_ops.len());
        assert_eq!(1, dht1.discovered_ops[&dht2.agent_id].len());
        assert_eq!(1, dht2.discovered_ops.len());
        assert_eq!(1, dht2.discovered_ops[&dht1.agent_id].len());

        // Move any discovered ops between the two DHTs
        dht1.apply_op_sync(&mut dht2).await.unwrap();

        // Now the two DHTs should be in sync
        assert!(dht1.is_in_sync_with(&dht2).await.unwrap());
    }

    #[tokio::test]
    async fn two_stage_sync_with_symmetry() {
        let current_time = Timestamp::now();
        let mut dht1 = DhtSyncHarness::new(current_time, DhtArc::FULL).await;
        let mut dht2 = DhtSyncHarness::new(current_time, DhtArc::FULL).await;

        // Put mismatched historical data in both DHTs
        dht1.inject_ops(vec![Kitsune2MemoryOp::new(
            OpId::from(bytes::Bytes::from(vec![7; 32])),
            UNIX_TIMESTAMP,
            vec![],
        )])
        .await
        .unwrap();
        dht2.inject_ops(vec![Kitsune2MemoryOp::new(
            OpId::from(bytes::Bytes::from(vec![13; 32])),
            UNIX_TIMESTAMP,
            vec![],
        )])
        .await
        .unwrap();

        // Put mismatched recent data in the first ring of both DHTs
        dht1.inject_ops(vec![Kitsune2MemoryOp::new(
            OpId::from(bytes::Bytes::from(vec![11; 32])),
            dht1.dht.partition.full_slice_end_timestamp(),
            vec![],
        )])
        .await
        .unwrap();
        dht2.inject_ops(vec![Kitsune2MemoryOp::new(
            OpId::from(bytes::Bytes::from(vec![17; 32])),
            dht1.dht.partition.full_slice_end_timestamp(),
            vec![],
        )])
        .await
        .unwrap();

        // Try a sync initiated by the first agent
        let outcome = dht1.sync_with(&mut dht2).await.unwrap();
        assert!(matches!(outcome, SyncWithOutcome::SyncedDisc));

        let learned1 = dht1.discovered_ops.clone();
        let learned2 = dht2.discovered_ops.clone();
        dht1.discovered_ops.clear();
        dht2.discovered_ops.clear();

        // Try a sync initiated by the second agent
        let outcome = dht2.sync_with(&mut dht1).await.unwrap();
        assert!(matches!(outcome, SyncWithOutcome::SyncedDisc));

        // The outcome should be identical, regardless of who initiated the sync
        assert_eq!(learned1, dht1.discovered_ops);
        assert_eq!(learned2, dht2.discovered_ops);

        // Move any discovered ops between the two DHTs
        dht1.apply_op_sync(&mut dht2).await.unwrap();

        // That's the disc sync done, now we need to check the rings
        let outcome = dht1.sync_with(&mut dht2).await.unwrap();
        assert!(matches!(outcome, SyncWithOutcome::SyncedRings));

        let learned1 = dht1.discovered_ops.clone();
        let learned2 = dht2.discovered_ops.clone();
        dht1.discovered_ops.clear();
        dht2.discovered_ops.clear();

        // Try a sync initiated by the second agent
        let outcome = dht2.sync_with(&mut dht1).await.unwrap();
        assert!(matches!(outcome, SyncWithOutcome::SyncedRings));

        // The outcome should be identical, regardless of who initiated the sync
        assert_eq!(learned1, dht1.discovered_ops);
        assert_eq!(learned2, dht2.discovered_ops);

        // Move any discovered ops between the two DHTs
        dht1.apply_op_sync(&mut dht2).await.unwrap();

        // Now the two DHTs should be in sync
        assert!(dht1.is_in_sync_with(&dht2).await.unwrap());
    }

    #[tokio::test]
    async fn disc_sync_respects_arc() {
        let current_time = Timestamp::now();
        let mut dht1 = DhtSyncHarness::new(
            current_time,
            DhtArc::Arc(0, 3 * SECTOR_SIZE - 1),
        )
        .await;
        let mut dht2 = DhtSyncHarness::new(
            current_time,
            DhtArc::Arc(2 * SECTOR_SIZE, 4 * SECTOR_SIZE - 1),
        )
        .await;

        // Put mismatched historical data in both DHTs, but in sectors that don't overlap
        dht1.inject_ops(vec![Kitsune2MemoryOp::new(
            OpId::from(bytes::Bytes::from(SECTOR_SIZE.to_le_bytes().to_vec())),
            UNIX_TIMESTAMP,
            vec![],
        )])
        .await
        .unwrap();
        dht2.inject_ops(vec![Kitsune2MemoryOp::new(
            OpId::from(bytes::Bytes::from(
                (3 * SECTOR_SIZE).to_le_bytes().to_vec(),
            )),
            UNIX_TIMESTAMP,
            vec![],
        )])
        .await
        .unwrap();

        // At this point, the DHTs should be in sync, because the mismatch is not in common sectors
        assert!(dht1.is_in_sync_with(&dht2).await.unwrap());

        // Now put mismatched data in the common sector
        dht1.inject_ops(vec![Kitsune2MemoryOp::new(
            OpId::from(bytes::Bytes::from(
                (2 * SECTOR_SIZE).to_le_bytes().to_vec(),
            )),
            UNIX_TIMESTAMP,
            vec![],
        )])
        .await
        .unwrap();
        dht2.inject_ops(vec![Kitsune2MemoryOp::new(
            OpId::from(bytes::Bytes::from(
                (2 * SECTOR_SIZE + 1).to_le_bytes().to_vec(),
            )),
            UNIX_TIMESTAMP,
            vec![],
        )])
        .await
        .unwrap();

        // Try to sync the DHTs
        let outcome = dht1.sync_with(&mut dht2).await.unwrap();
        assert!(matches!(outcome, SyncWithOutcome::SyncedDisc));

        // Sync the ops
        dht1.apply_op_sync(&mut dht2).await.unwrap();

        // Now the DHTs should be in sync
        assert!(dht1.is_in_sync_with(&dht2).await.unwrap());
    }

    #[tokio::test]
    async fn ring_sync_respects_arc() {
        let current_time = Timestamp::now();
        let mut dht1 = DhtSyncHarness::new(
            current_time,
            DhtArc::Arc(0, 3 * SECTOR_SIZE - 1),
        )
        .await;
        let mut dht2 = DhtSyncHarness::new(
            current_time,
            DhtArc::Arc(2 * SECTOR_SIZE, 4 * SECTOR_SIZE - 1),
        )
        .await;

        // Put mismatched recent data in both DHTs, but in sectors that don't overlap
        dht1.inject_ops(vec![Kitsune2MemoryOp::new(
            OpId::from(bytes::Bytes::from(SECTOR_SIZE.to_le_bytes().to_vec())),
            dht1.dht.partition.full_slice_end_timestamp(),
            vec![],
        )])
        .await
        .unwrap();
        dht2.inject_ops(vec![Kitsune2MemoryOp::new(
            OpId::from(bytes::Bytes::from(
                (3 * SECTOR_SIZE).to_le_bytes().to_vec(),
            )),
            dht1.dht.partition.full_slice_end_timestamp(),
            vec![],
        )])
        .await
        .unwrap();

        // At this point, the DHTs should be in sync, because the mismatch is not in common sectors
        assert!(dht1.is_in_sync_with(&dht2).await.unwrap());

        // Now put mismatched data in the common sector
        dht1.inject_ops(vec![Kitsune2MemoryOp::new(
            OpId::from(bytes::Bytes::from(
                (2 * SECTOR_SIZE).to_le_bytes().to_vec(),
            )),
            dht1.dht.partition.full_slice_end_timestamp(),
            vec![],
        )])
        .await
        .unwrap();
        dht2.inject_ops(vec![Kitsune2MemoryOp::new(
            OpId::from(bytes::Bytes::from(
                (2 * SECTOR_SIZE + 1).to_le_bytes().to_vec(),
            )),
            dht1.dht.partition.full_slice_end_timestamp(),
            vec![],
        )])
        .await
        .unwrap();

        // Try to sync the DHTs
        let outcome = dht1.sync_with(&mut dht2).await.unwrap();
        assert!(matches!(outcome, SyncWithOutcome::SyncedRings));

        // Sync the ops
        dht1.apply_op_sync(&mut dht2).await.unwrap();

        // Now the DHTs should be in sync
        assert!(dht1.is_in_sync_with(&dht2).await.unwrap());
    }
}
