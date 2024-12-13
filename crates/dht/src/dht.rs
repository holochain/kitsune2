//! Top-level DHT model.

use crate::arc_set::ArcSet;
use crate::PartitionedHashes;
use kitsune2_api::{DynOpStore, K2Result, StoredOp, Timestamp};

pub struct Dht {
    partition: PartitionedHashes,
}

#[derive(Debug, Eq, PartialEq)]
pub enum DhtSnapshot {
    Minimal {
        disc_top_hash: bytes::Bytes,
        disc_boundary: Timestamp,
        // There should be at most 2 * (time_factor - 1) of these. Check!
        ring_top_hashes: Vec<bytes::Bytes>,
    },
}

#[derive(Debug)]
#[cfg_attr(test, derive(Eq, PartialEq))]
pub enum DhtSnapshotCompareOutcome {
    Identical,
    CannotCompare,
    DiscMismatch,
    RingMismatch(Vec<u32>),
}

impl DhtSnapshot {
    pub fn compare(&self, other: &Self) -> DhtSnapshotCompareOutcome {
        // Check if they match exactly, before doing further work to check how they differ.
        if self == other {
            return DhtSnapshotCompareOutcome::Identical;
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
                    return DhtSnapshotCompareOutcome::CannotCompare;
                }

                // If the disc hash mismatches, then there is a historical mismatch.
                // This shouldn't be common, but we sync forwards through time, so if we
                // find a historical mismatch then focus on fixing that first.
                if our_disc_top_hash != other_disc_top_hash {
                    return DhtSnapshotCompareOutcome::DiscMismatch;
                }

                // This is more common, it can happen if we're close to a UNIT_TIME boundary
                // and there is a small clock difference or just one node calculated this snapshot
                // before the other did. Still, we'll have to wait until we next compare to
                // compare our DHT state.
                if our_ring_top_hashes.len() != other_ring_top_hashes.len() {
                    return DhtSnapshotCompareOutcome::CannotCompare;
                }

                let mismatched_rings = our_ring_top_hashes.iter().enumerate().zip(other_ring_top_hashes.iter()).filter_map(
                    |((idx, our_ring_top_hash), other_ring_top_hash)| {
                        if our_ring_top_hash != other_ring_top_hash {
                            // Cast is safe if the size of the number of rings has been checked.
                            Some(idx as u32)
                        } else {
                            None
                        }
                    },
                ).collect::<Vec<_>>();

                // There should always be at least one mismatched ring, otherwise the snapshots
                // are identical.
                DhtSnapshotCompareOutcome::RingMismatch(mismatched_rings)
            },
        }
    }
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
        let (disc_top_hash, disc_boundary) =
            self.partition.full_time_slice_disc(arc_set, store).await?;

        Ok(DhtSnapshot::Minimal {
            disc_top_hash,
            disc_boundary,
            ring_top_hashes: self.partition.partial_time_rings(arc_set),
        })
    }
}

#[cfg(test)]
mod snapshot_tests {
    use std::time::Duration;
    use super::*;

    #[test]
    fn minimal_self_identical() {
        let snapshot = DhtSnapshot::Minimal {
            disc_top_hash: bytes::Bytes::from(vec![1; 32]),
            disc_boundary: Timestamp::now(),
            ring_top_hashes: vec![bytes::Bytes::from(vec![2; 32])],
        };

        assert_eq!(DhtSnapshotCompareOutcome::Identical, snapshot.compare(&snapshot));
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

        assert_eq!(snapshot_1.compare(&snapshot_2), DhtSnapshotCompareOutcome::DiscMismatch);
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

        assert_eq!(snapshot_1.compare(&snapshot_2), DhtSnapshotCompareOutcome::CannotCompare);
    }
}

#[cfg(test)]
mod dht_tests {
    use super::*;
    use kitsune2_api::{DhtArc, OpId, OpStore, UNIX_TIMESTAMP};
    use kitsune2_memory::{Kitsune2MemoryOp, Kitsune2MemoryOpStore};
    use std::sync::Arc;

    const SECTOR_SIZE: u32 = 1u32 << 23;

    #[tokio::test]
    async fn from_store_empty() {
        let store = Arc::new(Kitsune2MemoryOpStore::default());
        Dht::try_from_store(UNIX_TIMESTAMP, store).await.unwrap();
    }

    #[tokio::test]
    async fn take_minimal_snapshot() {
        let store = Arc::new(Kitsune2MemoryOpStore::default());
        store.process_incoming_ops(vec![Kitsune2MemoryOp::new(
            OpId::from(bytes::Bytes::from(vec![7; 32])),
            UNIX_TIMESTAMP,
            vec![],
        ).try_into().unwrap()]).await.unwrap();

        let dht = Dht::try_from_store(Timestamp::now(), store.clone()).await.unwrap();

        let arc_set = ArcSet::new(SECTOR_SIZE, vec![DhtArc::FULL]).unwrap();

        let mut snapshot = dht.snapshot_minimal(&arc_set, store.clone()).await.unwrap();
        match snapshot {
            DhtSnapshot::Minimal {
                disc_boundary,
                disc_top_hash,
                ring_top_hashes,
            } => {
                assert_eq!(dht.partition.full_slice_end_timestamp(), disc_boundary);
                assert_eq!(bytes::Bytes::from(vec![7; 32]), disc_top_hash);
                assert!(!ring_top_hashes.is_empty());
            },
        }
    }
}
