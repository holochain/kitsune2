use kitsune2_api::Timestamp;
use std::collections::{HashMap, HashSet};

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

#[cfg(test)]
mod tests {
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
