//! A snapshot of the DHT state at a given point in time.
//!
//! This module is public because its types need to be communicated between DHT instances, but it is
//! largely opaque to the user. See [crate::dht::DhtApi::snapshot_minimal] and
//! [crate::dht::DhtApi::handle_snapshot] for more information about using this module.

use kitsune2_api::Timestamp;
use std::collections::{HashMap, HashSet};

/// A snapshot of the DHT state at a given point in time.
///
/// This is largely opaque to the user of the [crate::dht::Dht] model. It is intended to be sent
/// between nodes to compare their DHT states and compared with [DhtSnapshot::compare].
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum DhtSnapshot {
    /// The default, smallest snapshot type.
    ///
    /// It contains enough information to make further decisions about where mismatches might be
    /// but does its best to compress historical information. The assumption being that the more
    /// recent data is more likely to contain mismatches than older data.
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
        /// Disc top hash, representing the combined hash of the full time slice top hashes.
        disc_top_hash: bytes::Bytes,
        /// The end timestamp of the most recent full time slice.
        disc_boundary: Timestamp,
        /// Ring top hashes, representing the combined hashes of the partial time slices.
        ring_top_hashes: Vec<bytes::Bytes>,
    },
    /// A snapshot to be used when there is a [DhtSnapshot::Minimal] mismatch in the disc top hash.
    DiscSectors {
        /// Similar to the `disc_top_hash` except the sector hashes are not combined.
        disc_sector_top_hashes: HashMap<u32, bytes::Bytes>,
        /// The end timestamp of the most recent full time slice.
        disc_boundary: Timestamp,
    },
    /// A snapshot to be used when there is a [DhtSnapshot::DiscSectors] mismatch.
    ///
    /// For each mismatched disc sector, the snapshot will contain the sector index and all the
    /// hashes for that sector.
    DiscSectorDetails {
        /// Similar to the `disc_sector_top_hashes` except the full time slice hashes are not
        /// combined.
        disc_sector_hashes: HashMap<u32, HashMap<u64, bytes::Bytes>>,
        /// The end timestamp of the most recent full time slice.
        disc_boundary: Timestamp,
    },
    /// A snapshot to be used when there is a [DhtSnapshot::Minimal] mismatch in the ring top
    /// hashes.
    RingSectorDetails {
        /// Similar to the `ring_top_hashes` except the sector hashes are not combined.
        ///
        /// Organized by ring index in the first HashMap, then sector index in the second HashMap.
        ring_sector_hashes: HashMap<u32, HashMap<u32, bytes::Bytes>>,
        /// The end timestamp of the most recent full time slice.
        disc_boundary: Timestamp,
    },
}

impl DhtSnapshot {
    /// Compare two snapshots to determine how they differ.
    ///
    /// Produces a [SnapshotDiff] that describes the differences between the two snapshots.
    /// This should not be use directly, please see [crate::dht::DhtApi::handle_snapshot].
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
                // our DHT state.
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
                    return SnapshotDiff::CannotCompare;
                }

                // If one side has a hash for a sector and the other doesn't then that is a mismatch
                let our_indices =
                    our_disc_sector_top_hashes.keys().collect::<HashSet<_>>();
                let other_indices =
                    other_disc_sector_top_hashes.keys().collect::<HashSet<_>>();
                let mut mismatched_sector_indices = our_indices
                    .symmetric_difference(&other_indices)
                    .map(|index| **index)
                    .collect::<Vec<_>>();

                // Then for any common sectors, check if the hashes match
                let common_indices = our_indices
                    .intersection(&other_indices)
                    .collect::<HashSet<_>>();
                for index in common_indices {
                    if our_disc_sector_top_hashes[index]
                        != other_disc_sector_top_hashes[index]
                    {
                        // We found a mismatched sector, store it
                        mismatched_sector_indices.push(**index);
                    }
                }

                SnapshotDiff::DiscSectorMismatches(mismatched_sector_indices)
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
                    return SnapshotDiff::CannotCompare;
                }

                let our_indices =
                    our_disc_sector_hashes.keys().collect::<HashSet<_>>();
                let other_indices =
                    other_disc_sector_hashes.keys().collect::<HashSet<_>>();

                // If one side has a sector and the other doesn't then that is a mismatch
                let mut mismatched_sector_indices = our_indices
                    .symmetric_difference(&other_indices)
                    .map(|index| {
                        (
                            **index,
                            our_disc_sector_hashes
                                .get(*index)
                                .unwrap_or_else(|| {
                                    &other_disc_sector_hashes[*index]
                                })
                                .keys()
                                .copied()
                                .collect::<Vec<_>>(),
                        )
                    })
                    .collect::<HashMap<_, _>>();

                // Then for any common sectors, check if the hashes match
                let common_sector_indices = our_indices
                    .intersection(&other_indices)
                    .collect::<HashSet<_>>();
                for sector_index in common_sector_indices {
                    let our_slice_indices = &our_disc_sector_hashes
                        [sector_index]
                        .keys()
                        .collect::<HashSet<_>>();
                    let other_slice_indices = &other_disc_sector_hashes
                        [sector_index]
                        .keys()
                        .collect::<HashSet<_>>();

                    let mut mismatched_slice_indices = our_slice_indices
                        .symmetric_difference(other_slice_indices)
                        .map(|index| **index)
                        .collect::<Vec<_>>();

                    let common_slice_indices = our_slice_indices
                        .intersection(other_slice_indices)
                        .collect::<HashSet<_>>();

                    for slice_index in common_slice_indices {
                        if our_disc_sector_hashes[sector_index][slice_index]
                            != other_disc_sector_hashes[sector_index]
                                [slice_index]
                        {
                            mismatched_slice_indices.push(**slice_index);
                        }
                    }

                    if !mismatched_slice_indices.is_empty() {
                        mismatched_sector_indices
                            .entry(**sector_index)
                            .or_insert_with(Vec::new)
                            .extend(mismatched_slice_indices);
                    }
                }

                SnapshotDiff::DiscSectorSliceMismatches(
                    mismatched_sector_indices,
                )
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
                    return SnapshotDiff::CannotCompare;
                }

                let our_indices =
                    our_ring_sector_hashes.keys().collect::<HashSet<_>>();
                let other_indices =
                    other_ring_sector_hashes.keys().collect::<HashSet<_>>();

                // The mismatched rings should have been figured out from the minimal snapshot
                // or from the ring mismatch in the previous step. They should be identical
                // regardless of which side computed this snapshot.
                if our_indices.len() != other_indices.len()
                    || our_indices != other_indices
                {
                    return SnapshotDiff::CannotCompare;
                }

                // Then for any common rings, check if the hashes match
                let common_ring_indices = our_indices
                    .intersection(&other_indices)
                    .collect::<HashSet<_>>();
                let mut mismatched_ring_sectors =
                    HashMap::with_capacity(common_ring_indices.len());
                for ring_index in common_ring_indices {
                    let our_sector_indices = &our_ring_sector_hashes
                        [ring_index]
                        .keys()
                        .collect::<HashSet<_>>();
                    let other_sector_indices = &other_ring_sector_hashes
                        [ring_index]
                        .keys()
                        .collect::<HashSet<_>>();

                    let mut mismatched_sector_indices = our_sector_indices
                        .symmetric_difference(other_sector_indices)
                        .map(|index| **index)
                        .collect::<Vec<_>>();

                    let common_sector_indices = our_sector_indices
                        .intersection(other_sector_indices)
                        .collect::<HashSet<_>>();

                    for sector_index in common_sector_indices {
                        if our_ring_sector_hashes[ring_index][sector_index]
                            != other_ring_sector_hashes[ring_index]
                                [sector_index]
                        {
                            mismatched_sector_indices.push(**sector_index);
                        }
                    }

                    mismatched_ring_sectors
                        .insert(**ring_index, mismatched_sector_indices);
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

/// The differences between two snapshots.
#[derive(Debug)]
#[cfg_attr(test, derive(Eq, PartialEq))]
pub enum SnapshotDiff {
    /// The snapshots are identical.
    Identical,
    /// The snapshots cannot be compared.
    ///
    /// This can happen if the historical time boundary doesn't match or if the snapshot types
    /// don't match.
    CannotCompare,
    // Historical mismatch
    /// The disc hashes do not match.
    DiscMismatch,
    /// These disc sectors are missing or do not match.
    DiscSectorMismatches(Vec<u32>),
    /// These disc sector slices are missing or do not match, and further these slices are missing
    /// or do not match.
    DiscSectorSliceMismatches(HashMap<u32, Vec<u64>>),
    // Recent mismatch
    /// These rings do not match.
    RingMismatches(Vec<u32>),
    /// These rings do not match, and further these sectors within those rings do not match.
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

    #[test]
    fn minimal_ring_mismatch() {
        let timestamp = Timestamp::now();
        let snapshot_1 = DhtSnapshot::Minimal {
            disc_boundary: timestamp,
            disc_top_hash: bytes::Bytes::from(vec![1; 32]),
            ring_top_hashes: vec![
                bytes::Bytes::from(vec![1]),
                bytes::Bytes::from(vec![2]),
            ],
        };

        let snapshot_2 = DhtSnapshot::Minimal {
            disc_boundary: timestamp,
            disc_top_hash: bytes::Bytes::from(vec![1; 32]),
            ring_top_hashes: vec![
                bytes::Bytes::from(vec![1]),
                bytes::Bytes::from(vec![3]),
            ],
        };

        assert_eq!(
            snapshot_1.compare(&snapshot_2),
            SnapshotDiff::RingMismatches(vec![1])
        );
    }

    #[test]
    fn minimal_disc_and_ring_mismatch() {
        let timestamp = Timestamp::now();
        let snapshot_1 = DhtSnapshot::Minimal {
            disc_boundary: timestamp,
            disc_top_hash: bytes::Bytes::from(vec![1; 32]),
            ring_top_hashes: vec![
                bytes::Bytes::from(vec![1]),
                bytes::Bytes::from(vec![7]),
            ],
        };

        let snapshot_2 = DhtSnapshot::Minimal {
            disc_boundary: timestamp,
            disc_top_hash: bytes::Bytes::from(vec![2; 32]),
            ring_top_hashes: vec![
                bytes::Bytes::from(vec![1]),
                bytes::Bytes::from(vec![3]),
            ],
        };

        // Always chooses the disc mismatch over the ring mismatch, to prioritise historical data.
        assert_eq!(snapshot_1.compare(&snapshot_2), SnapshotDiff::DiscMismatch);
    }

    #[test]
    fn minimal_disc_wrong_number_of_rings() {
        let timestamp = Timestamp::now();
        let snapshot_1 = DhtSnapshot::Minimal {
            disc_boundary: timestamp,
            disc_top_hash: bytes::Bytes::from(vec![1; 32]),
            ring_top_hashes: vec![
                bytes::Bytes::from(vec![1]),
                bytes::Bytes::from(vec![4]),
            ],
        };

        let snapshot_2 = DhtSnapshot::Minimal {
            disc_boundary: timestamp,
            disc_top_hash: bytes::Bytes::from(vec![1; 32]),
            ring_top_hashes: vec![
                bytes::Bytes::from(vec![1]),
                bytes::Bytes::from(vec![3]),
                bytes::Bytes::from(vec![5]),
            ],
        };

        assert_eq!(
            snapshot_1.compare(&snapshot_2),
            SnapshotDiff::CannotCompare
        );
    }

    #[test]
    fn disc_sector_boundary_mismatch() {
        let timestamp = Timestamp::now();
        let snapshot_1 = DhtSnapshot::DiscSectors {
            disc_boundary: timestamp,
            disc_sector_top_hashes: HashMap::new(),
        };

        let snapshot_2 = DhtSnapshot::DiscSectors {
            disc_boundary: timestamp + Duration::from_secs(1),
            disc_sector_top_hashes: HashMap::new(),
        };

        assert_eq!(
            snapshot_1.compare(&snapshot_2),
            SnapshotDiff::CannotCompare
        );
    }

    #[test]
    fn disc_sector_mismatch_finds_missing_sectors() {
        let timestamp = Timestamp::now();
        let snapshot_1 = DhtSnapshot::DiscSectors {
            disc_boundary: timestamp,
            disc_sector_top_hashes: vec![
                (0, bytes::Bytes::new()),
                (1, bytes::Bytes::new()),
            ]
            .into_iter()
            .collect(),
        };

        let snapshot_2 = DhtSnapshot::DiscSectors {
            disc_boundary: timestamp,
            disc_sector_top_hashes: vec![
                (0, bytes::Bytes::new()),
                (2, bytes::Bytes::new()),
            ]
            .into_iter()
            .collect(),
        };

        assert_eq!(
            snapshot_1.compare(&snapshot_2),
            SnapshotDiff::DiscSectorMismatches(vec![1, 2])
        );
    }

    #[test]
    fn disc_sector_mismatch_finds_mismatched_sectors() {
        let timestamp = Timestamp::now();
        let snapshot_1 = DhtSnapshot::DiscSectors {
            disc_boundary: timestamp,
            disc_sector_top_hashes: vec![
                (0, bytes::Bytes::new()),
                (1, bytes::Bytes::from_static(&[1])),
            ]
            .into_iter()
            .collect(),
        };

        let snapshot_2 = DhtSnapshot::DiscSectors {
            disc_boundary: timestamp,
            disc_sector_top_hashes: vec![
                (0, bytes::Bytes::new()),
                (1, bytes::Bytes::from_static(&[2])),
            ]
            .into_iter()
            .collect(),
        };

        assert_eq!(
            snapshot_1.compare(&snapshot_2),
            SnapshotDiff::DiscSectorMismatches(vec![1])
        );
    }

    #[test]
    fn disc_sector_details_boundary_mismatch() {
        let timestamp = Timestamp::now();
        let snapshot_1 = DhtSnapshot::DiscSectorDetails {
            disc_boundary: timestamp,
            disc_sector_hashes: HashMap::new(),
        };

        let snapshot_2 = DhtSnapshot::DiscSectorDetails {
            disc_boundary: timestamp + Duration::from_secs(1),
            disc_sector_hashes: HashMap::new(),
        };

        assert_eq!(
            snapshot_1.compare(&snapshot_2),
            SnapshotDiff::CannotCompare
        );
    }

    #[test]
    fn disc_sector_details_mismatch_preserves_missing_sectors() {
        let timestamp = Timestamp::now();
        let slices_1 = vec![(0, bytes::Bytes::from_static(&[1]))]
            .into_iter()
            .collect();
        let snapshot_1 = DhtSnapshot::DiscSectorDetails {
            disc_boundary: timestamp,
            disc_sector_hashes: vec![(0, HashMap::new()), (1, slices_1)]
                .into_iter()
                .collect(),
        };

        let slices_2 = vec![(0, bytes::Bytes::from_static(&[7]))]
            .into_iter()
            .collect();
        let snapshot_2 = DhtSnapshot::DiscSectorDetails {
            disc_boundary: timestamp,
            disc_sector_hashes: vec![(0, HashMap::new()), (2, slices_2)]
                .into_iter()
                .collect(),
        };

        assert_eq!(
            snapshot_1.compare(&snapshot_2),
            SnapshotDiff::DiscSectorSliceMismatches(
                vec![(1, vec![0]), (2, vec![0])].into_iter().collect()
            )
        );
    }

    #[test]
    fn disc_sector_details_mismatch_finds_missing_slices() {
        let timestamp = Timestamp::now();
        let slices_1 = vec![(10, bytes::Bytes::from_static(&[1]))]
            .into_iter()
            .collect();
        let snapshot_1 = DhtSnapshot::DiscSectorDetails {
            disc_boundary: timestamp,
            disc_sector_hashes: vec![(1, slices_1)].into_iter().collect(),
        };

        let slices_2 = vec![(20, bytes::Bytes::from_static(&[7]))]
            .into_iter()
            .collect();
        let snapshot_2 = DhtSnapshot::DiscSectorDetails {
            disc_boundary: timestamp,
            disc_sector_hashes: vec![(1, slices_2)].into_iter().collect(),
        };

        assert_eq!(
            snapshot_1.compare(&snapshot_2),
            SnapshotDiff::DiscSectorSliceMismatches(
                vec![(1, vec![10, 20])].into_iter().collect()
            )
        );
    }

    #[test]
    fn disc_sector_details_mismatch_finds_slice_mismatches() {
        let timestamp = Timestamp::now();
        let slices_1 = vec![(10, bytes::Bytes::from_static(&[1]))]
            .into_iter()
            .collect();
        let snapshot_1 = DhtSnapshot::DiscSectorDetails {
            disc_boundary: timestamp,
            disc_sector_hashes: vec![(1, slices_1)].into_iter().collect(),
        };

        let slices_2 = vec![(10, bytes::Bytes::from_static(&[7]))]
            .into_iter()
            .collect();
        let snapshot_2 = DhtSnapshot::DiscSectorDetails {
            disc_boundary: timestamp,
            disc_sector_hashes: vec![(1, slices_2)].into_iter().collect(),
        };

        assert_eq!(
            snapshot_1.compare(&snapshot_2),
            SnapshotDiff::DiscSectorSliceMismatches(
                vec![(1, vec![10])].into_iter().collect()
            )
        );
    }

    #[test]
    fn ring_sector_details_boundary_mismatch() {
        let timestamp = Timestamp::now();
        let snapshot_1 = DhtSnapshot::RingSectorDetails {
            disc_boundary: timestamp,
            ring_sector_hashes: HashMap::new(),
        };

        let snapshot_2 = DhtSnapshot::RingSectorDetails {
            disc_boundary: timestamp + Duration::from_secs(1),
            ring_sector_hashes: HashMap::new(),
        };

        assert_eq!(
            snapshot_1.compare(&snapshot_2),
            SnapshotDiff::CannotCompare
        );
    }

    #[test]
    fn ring_sector_details_mismatch_cannot_compare_different_number_of_rings() {
        let timestamp = Timestamp::now();
        let snapshot_1 = DhtSnapshot::RingSectorDetails {
            disc_boundary: timestamp,
            ring_sector_hashes: vec![(0, HashMap::new()), (1, HashMap::new())]
                .into_iter()
                .collect(),
        };

        let snapshot_2 = DhtSnapshot::RingSectorDetails {
            disc_boundary: timestamp,
            ring_sector_hashes: vec![(0, HashMap::new())].into_iter().collect(),
        };

        assert_eq!(
            snapshot_1.compare(&snapshot_2),
            SnapshotDiff::CannotCompare
        );
    }

    #[test]
    fn ring_sector_details_mismatch_cannot_compare_different_rings() {
        let timestamp = Timestamp::now();
        let snapshot_1 = DhtSnapshot::RingSectorDetails {
            disc_boundary: timestamp,
            ring_sector_hashes: vec![(0, HashMap::new())].into_iter().collect(),
        };

        let snapshot_2 = DhtSnapshot::RingSectorDetails {
            disc_boundary: timestamp,
            ring_sector_hashes: vec![(1, HashMap::new())].into_iter().collect(),
        };

        assert_eq!(
            snapshot_1.compare(&snapshot_2),
            SnapshotDiff::CannotCompare
        );
    }

    #[test]
    fn ring_sector_details_mismatch_detects_sector_mismatches() {
        let timestamp = Timestamp::now();
        let snapshot_1 = DhtSnapshot::RingSectorDetails {
            disc_boundary: timestamp,
            ring_sector_hashes: vec![(
                0,
                vec![(0, bytes::Bytes::from_static(&[1]))]
                    .into_iter()
                    .collect(),
            )]
            .into_iter()
            .collect(),
        };

        let snapshot_2 = DhtSnapshot::RingSectorDetails {
            disc_boundary: timestamp,
            ring_sector_hashes: vec![(
                0,
                vec![(0, bytes::Bytes::from_static(&[5]))]
                    .into_iter()
                    .collect(),
            )]
            .into_iter()
            .collect(),
        };

        assert_eq!(
            snapshot_1.compare(&snapshot_2),
            SnapshotDiff::RingSectorMismatches(
                vec![(0, vec![0])].into_iter().collect()
            )
        );
    }
}
