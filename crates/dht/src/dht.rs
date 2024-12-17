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
    /// A snapshot to be used when there is a disc top hash mismatch in the minimal snapshot.
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
}

#[derive(Debug)]
#[cfg_attr(test, derive(Eq, PartialEq))]
pub enum DhtSnapshotCompareOutcomeInner {
    Identical,
    CannotCompare,
    DiscMismatch,
    RingMismatches(Vec<u32>),
    DiscSectorMismatches(Vec<u32>),
    DiscSectorSliceMismatches(HashMap<u32, Vec<u64>>),
}

impl DhtSnapshot {
    pub fn compare(&self, other: &Self) -> DhtSnapshotCompareOutcomeInner {
        // Check if they match exactly, before doing further work to check how they differ.
        if self == other {
            return DhtSnapshotCompareOutcomeInner::Identical;
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
                    return DhtSnapshotCompareOutcomeInner::CannotCompare;
                }

                // If the disc hash mismatches, then there is a historical mismatch.
                // This shouldn't be common, but we sync forwards through time, so if we
                // find a historical mismatch then focus on fixing that first.
                if our_disc_top_hash != other_disc_top_hash {
                    return DhtSnapshotCompareOutcomeInner::DiscMismatch;
                }

                // This is more common, it can happen if we're close to a UNIT_TIME boundary
                // and there is a small clock difference or just one node calculated this snapshot
                // before the other did. Still, we'll have to wait until we next compare to
                // compare our DHT state.
                if our_ring_top_hashes.len() != other_ring_top_hashes.len() {
                    return DhtSnapshotCompareOutcomeInner::CannotCompare;
                }

                // There should always be at least one mismatched ring, otherwise the snapshots
                // would have been identical which has already been checked.
                DhtSnapshotCompareOutcomeInner::RingMismatches(
                    hash_mismatch_indices(
                        our_ring_top_hashes,
                        other_ring_top_hashes,
                    ),
                )
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
                    return DhtSnapshotCompareOutcomeInner::CannotCompare;
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
                DhtSnapshotCompareOutcomeInner::DiscSectorMismatches(
                    mismatched_sector_ids,
                )
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
                    return DhtSnapshotCompareOutcomeInner::CannotCompare;
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

                DhtSnapshotCompareOutcomeInner::DiscSectorSliceMismatches(
                    mismatched_sector_ids,
                )
            }
            (theirs, other) => {
                tracing::error!(
                    "Mismatched snapshot types: ours: {:?}, theirs: {:?}",
                    theirs,
                    other
                );
                DhtSnapshotCompareOutcomeInner::CannotCompare
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
        };

        println!("Our snapshot: {:?}", our_snapshot);

        match our_snapshot.compare(&their_snapshot) {
            DhtSnapshotCompareOutcomeInner::Identical => {
                Ok(DhtSnapshotCompareOutcome::Identical)
            }
            DhtSnapshotCompareOutcomeInner::CannotCompare => {
                Ok(DhtSnapshotCompareOutcome::CannotCompare)
            }
            DhtSnapshotCompareOutcomeInner::DiscMismatch => {
                Ok(DhtSnapshotCompareOutcome::NewSnapshot(
                    self.snapshot_disc_sectors(arc_set, store).await?,
                ))
            }
            DhtSnapshotCompareOutcomeInner::DiscSectorMismatches(
                mismatched_sectors,
            ) => Ok(DhtSnapshotCompareOutcome::NewSnapshot(
                self.snapshot_disc_sector_details(
                    mismatched_sectors,
                    arc_set,
                    store,
                )
                .await?,
            )),
            DhtSnapshotCompareOutcomeInner::DiscSectorSliceMismatches(
                mismatched_slice_ids,
            ) => {
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
            DhtSnapshotCompareOutcomeInner::RingMismatches(
                _mismatched_rings,
            ) => {
                unimplemented!("Ring mismatches are not yet supported")
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

        assert_eq!(
            DhtSnapshotCompareOutcomeInner::Identical,
            snapshot.compare(&snapshot)
        );
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

        assert_eq!(
            snapshot_1.compare(&snapshot_2),
            DhtSnapshotCompareOutcomeInner::DiscMismatch
        );
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
            DhtSnapshotCompareOutcomeInner::CannotCompare
        );
    }
}

#[cfg(test)]
mod dht_tests {
    use super::*;
    use kitsune2_api::{DhtArc, OpId, OpStore, UNIX_TIMESTAMP};
    use kitsune2_memory::{Kitsune2MemoryOp, Kitsune2MemoryOpStore};
    use std::sync::Arc;

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
    async fn sync_ops_in_disc() {
        let store1 = Arc::new(Kitsune2MemoryOpStore::default());
        store1
            .process_incoming_ops(vec![Kitsune2MemoryOp::new(
                OpId::from(bytes::Bytes::from(vec![41; 32])),
                UNIX_TIMESTAMP,
                vec![],
            )
            .try_into()
            .unwrap()])
            .await
            .unwrap();
        let store2 = Arc::new(Kitsune2MemoryOpStore::default());

        let current_time = Timestamp::now();
        let mut dht1 = Dht::try_from_store(current_time, store1.clone())
            .await
            .unwrap();
        let mut dht2 = Dht::try_from_store(current_time, store2.clone())
            .await
            .unwrap();

        let arc_set = ArcSet::new(SECTOR_SIZE, vec![DhtArc::FULL]).unwrap();

        let snapshot = dht1
            .snapshot_minimal(&arc_set, store1.clone())
            .await
            .unwrap();

        println!("Initial snapshot: {:?}", snapshot);

        let outcome = dht2
            .handle_snapshot(snapshot, &arc_set, store2.clone())
            .await
            .unwrap();
        let snapshot = match outcome {
            DhtSnapshotCompareOutcome::NewSnapshot(new_snapshot) => {
                match new_snapshot {
                    DhtSnapshot::DiscSectors { .. } => new_snapshot,
                    s => panic!("Unexpected snapshot type: {:?}", s),
                }
            }
            s => panic!("Unexpected outcome: {:?}", s),
        };

        println!("Sectors snapshot {:?}", snapshot);

        let outcome = dht1
            .handle_snapshot(snapshot, &arc_set, store1.clone())
            .await
            .unwrap();
        let snapshot = match outcome {
            DhtSnapshotCompareOutcome::NewSnapshot(new_snapshot) => {
                match new_snapshot {
                    DhtSnapshot::DiscSectorDetails { .. } => new_snapshot,
                    s => panic!("Unexpected snapshot type: {:?}", s),
                }
            }
            s => panic!("Unexpected outcome: {:?}", s),
        };

        println!("Sector details snapshot {:?}", snapshot);

        let outcome = dht2
            .handle_snapshot(snapshot, &arc_set, store2.clone())
            .await
            .unwrap();
        let (snapshot, hash_list) = match outcome {
            DhtSnapshotCompareOutcome::HashList(new_snapshot, hash_list) => {
                assert_eq!(0, hash_list.len());

                (new_snapshot, hash_list)
            }
            s => panic!("Unexpected outcome: {:?}", s),
        };

        println!("Sector details snapshot 2 {:?}", snapshot);

        // Test only behaviour, transfer the ops directly to the other store and
        // update the DHT model
        transfer_ops(store2.clone(), store1.clone(), &mut dht1, hash_list)
            .await
            .unwrap();

        // The final snapshot needs to be checked against by the other peer too
        let outcome = dht1
            .handle_snapshot(snapshot, &arc_set, store1.clone())
            .await
            .unwrap();
        let hash_list = match outcome {
            DhtSnapshotCompareOutcome::HashList(_, hash_list) => {
                assert_eq!(1, hash_list.len());
                assert_eq!(
                    OpId::from(bytes::Bytes::from(vec![41; 32])),
                    hash_list[0]
                );

                hash_list
            }
            s => panic!("Unexpected outcome: {:?}", s),
        };

        // Test only behaviour, transfer the ops directly to the other store and
        // update the DHT model
        transfer_ops(store1.clone(), store2.clone(), &mut dht2, hash_list)
            .await
            .unwrap();

        let snapshot = dht2
            .snapshot_minimal(&arc_set, store2.clone())
            .await
            .unwrap();
        let outcome = dht1
            .handle_snapshot(snapshot, &arc_set, store1.clone())
            .await
            .unwrap();
        match outcome {
            DhtSnapshotCompareOutcome::Identical => {}
            s => panic!("Unexpected outcome: {:?}", s),
        }
    }
}
