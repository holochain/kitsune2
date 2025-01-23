//! Top-level DHT model.
//!
//! This module is largely implemented in terms of the [HashPartition] and
//! [PartitionedTime](crate::time::TimePartition) types. It combines these types into a single
//! model that can be used to track the state of a distributed hash table (DHT).
//!
//! On top of the inner types, this type adds the ability to compare two DHT models and determine
//! a set of op hashes that may need to be fetched from one model to the other to bring them into
//! sync. The comparison process is symmetric, meaning that both parties will end up with the same
//! list of op hashes to fetch regardless of who initiated the comparison. Comparison is initiated
//! using the [Dht::snapshot_minimal] method which produces a minimal snapshot of the DHT model.
//!
//! The set of op hashes to fetch is unlikely to be the exact ops that are missing but rather a
//! tradeoff between the number of steps required to determine the set of possible missing ops and
//! the number of op hashes that have to be sent. In the case of recent ops, this is a two-step
//! process to compare rings by exchanging [DhtSnapshot::Minimal] and then
//! [DhtSnapshot::RingSectorDetails]. In the case of historical ops, this is a three-step process
//! to compare discs by exchanging [DhtSnapshot::Minimal], [DhtSnapshot::DiscSectors] and then
//! [DhtSnapshot::DiscSectorDetails].

use crate::arc_set::ArcSet;
use crate::HashPartition;
use kitsune2_api::{DynOpStore, K2Error, K2Result, OpId, StoredOp, Timestamp};
use snapshot::{DhtSnapshot, SnapshotDiff};
use std::fmt::Formatter;

pub mod snapshot;
#[cfg(test)]
mod tests;

/// The top-level DHT model.
///
/// Represents a distributed hash table (DHT) model that can be compared with other instances of
/// itself to determine if they are in sync and which regions to sync if they are not.
pub struct Dht {
    partition: HashPartition,
    store: DynOpStore,
}

impl std::fmt::Debug for Dht {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Dht")
            .field("partition", &self.partition)
            .finish()
    }
}

/// The next action to take after comparing two DHT snapshots.
#[derive(Debug)]
pub enum DhtSnapshotNextAction {
    /// No further action, these DHTs are in sync.
    Identical,
    /// The two DHT snapshots cannot be compared.
    ///
    /// This can happen if the time slices of the two DHTs are not aligned or one side is following
    /// a different comparison flow to what we're expecting.
    CannotCompare,
    /// The two DHT snapshots are different, and we need to drill down to the next level of detail.
    NewSnapshot(DhtSnapshot),
    /// The two DHT snapshots are different, and we have drilled down to the most detailed level.
    ///
    /// The yielded op hashes should be checked by the other party and any missing ops should be
    /// fetched from us.
    NewSnapshotAndHashList(DhtSnapshot, Vec<OpId>),
    /// The two DHT snapshots are different, and we have drilled down to the most detailed level.
    ///
    /// This is the final step in the comparison process. The yielded op hashes should be fetched
    /// from the other party. No further snapshots are required for this comparison.
    HashList(Vec<OpId>),
}

impl Dht {
    /// Create a new DHT instance from an op store.
    ///
    /// Creates the inner [HashPartition] using the store. The sizing for the sectors and time
    /// slices are currently hard-coded. This will create 512 sectors and each full time slice will
    /// be approximately 5.3 days.
    pub async fn try_from_store(
        current_time: Timestamp,
        store: DynOpStore,
    ) -> K2Result<Dht> {
        Ok(Dht {
            partition: HashPartition::try_from_store(
                9,
                current_time,
                store.clone(),
            )
            .await?,
            store,
        })
    }

    /// Get the next time at which the DHT model should be updated.
    ///
    /// When this time is reached, [Dht::update] should be called.
    pub fn next_update_at(&self) -> Timestamp {
        self.partition.next_update_at()
    }

    /// Update the DHT model.
    ///
    /// This delegates to [HashPartition::update] to update the inner hash partition.
    pub async fn update(&mut self, current_time: Timestamp) -> K2Result<()> {
        self.partition
            .update(self.store.clone(), current_time)
            .await
    }

    /// Inform the DHT model that some ops have been stored.
    ///
    /// This will figure out where the incoming ops belong in the DHT model based on their hash
    /// and timestamp.
    ///
    /// See also [HashPartition::inform_ops_stored] for more details.
    pub async fn inform_ops_stored(
        &mut self,
        stored_ops: Vec<StoredOp>,
    ) -> K2Result<()> {
        self.partition
            .inform_ops_stored(self.store.clone(), stored_ops)
            .await
    }

    /// Get a minimal snapshot of the DHT model.
    ///
    /// This is the entry point for comparing state with another DHT model. A minimal snapshot may
    /// be enough to check that two DHTs are in sync. The receiver should call [Dht::handle_snapshot]
    /// which will determine if the two DHTs are in sync or if a more detailed snapshot is required.
    ///
    /// # Errors
    ///
    /// Returns an error if there are no arcs to snapshot. If there is no overlap between the arc
    /// sets of two DHT models then there is no point in comparing them because it will always
    /// yield an empty diff. The [ArcSet::covered_sector_count] should be checked before calling
    /// this method.
    pub async fn snapshot_minimal(
        &self,
        arc_set: &ArcSet,
    ) -> K2Result<DhtSnapshot> {
        if arc_set.covered_sector_count() == 0 {
            return Err(K2Error::other("No arcs to snapshot"));
        }

        let (disc_top_hash, disc_boundary) = self
            .partition
            .disc_top_hash(arc_set, self.store.clone())
            .await?;

        Ok(DhtSnapshot::Minimal {
            disc_top_hash,
            disc_boundary,
            ring_top_hashes: self.partition.ring_top_hashes(arc_set),
        })
    }

    /// Handle a snapshot from another DHT model.
    ///
    /// This is a two-step process. First the type of the incoming snapshot is checked and a
    /// snapshot of the same type is computed. Secondly, the two snapshots are compared to determine
    /// what action should be taken next.
    ///
    /// The state flow is as follows:
    /// - If the two snapshots are identical, the function will return [DhtSnapshotNextAction::Identical].
    /// - If the two snapshots cannot be compared, the function will return [DhtSnapshotNextAction::CannotCompare].
    ///   This can happen if the time slices of the two DHTs are not aligned or one side is
    ///   following a different flow to what we're expecting.
    /// - If the snapshots are different, the function will return [DhtSnapshotNextAction::NewSnapshot]
    ///   with a more detailed snapshot of the DHT model.
    /// - When the most detailed snapshot type is reached, the function will return [DhtSnapshotNextAction::NewSnapshotAndHashList]
    /// - The new snapshot from [DhtSnapshotNextAction::NewSnapshotAndHashList] should be sent to
    ///   the other party so that they can compare it with their own snapshot and determine which op
    ///   hashes they need to fetch.
    /// - On the final comparison step, the function will return [DhtSnapshotNextAction::HashList]
    ///   with a list of op hashes. This list should be sent to the other party so that they can
    ///   fetch any missing ops.
    ///
    /// Notice that the final step would require re-computing the most detailed snapshot type. This
    /// is expensive. To avoid having to recompute a snapshot we've already computed, the caller
    /// MUST capture the snapshot from [DhtSnapshotNextAction::NewSnapshot] when it contains either
    /// [DhtSnapshot::DiscSectorDetails] or [DhtSnapshot::RingSectorDetails]. This snapshot can be
    /// provided back to this function in the `our_previous_snapshot` parameter. In all other cases,
    /// the caller should provide `None` for `our_previous_snapshot`.
    ///
    /// Note also that there are two possible routes through the comparison process. The first is
    /// when the historical disc mismatches, the second is when the recent rings mismatch. The
    /// historical disc mismatch is prioritised, so if a mismatch is detected there then the sync
    /// process will resolve that. Otherwise, the recent rings mismatch will be resolved. That means
    /// that it may take up to two rounds of sync to resolve all mismatches. Of course, both the
    /// disc and the rings must be considered a moving target so it cannot be assumed that 2 rounds
    /// are actually enough to resolve all mismatches.
    ///
    /// The `arc_set` parameter is used to determine which arcs are relevant to the DHT model. This
    /// should be the [ArcSet::intersection] of the arc sets of the two DHT models to be compared.
    ///
    /// # Errors
    ///
    /// Returns an error if there are no arcs to snapshot. If there is no overlap between the arc
    /// sets of two DHT models then there is no point in comparing them because it will always
    /// yield an empty diff. The [ArcSet::covered_sector_count] should be checked before calling
    /// this method.
    pub async fn handle_snapshot(
        &self,
        their_snapshot: &DhtSnapshot,
        our_previous_snapshot: Option<DhtSnapshot>,
        arc_set: &ArcSet,
    ) -> K2Result<DhtSnapshotNextAction> {
        if arc_set.covered_sector_count() == 0 {
            return Err(K2Error::other("No arcs to snapshot"));
        }

        let is_final = matches!(
            our_previous_snapshot,
            Some(
                DhtSnapshot::DiscSectorDetails { .. }
                    | DhtSnapshot::RingSectorDetails { .. }
            )
        );

        // Check what snapshot we've been sent and compute a matching snapshot.
        // In the case where we've already produced a most details snapshot type, we can use the
        // already computed snapshot.
        let our_snapshot = match &their_snapshot {
            DhtSnapshot::Minimal { .. } => {
                self.snapshot_minimal(arc_set).await?
            }
            DhtSnapshot::DiscSectors { .. } => {
                self.snapshot_disc_sectors(arc_set).await?
            }
            DhtSnapshot::DiscSectorDetails {
                disc_sector_hashes, ..
            } => match our_previous_snapshot {
                Some(snapshot @ DhtSnapshot::DiscSectorDetails { .. }) => {
                    #[cfg(test)]
                    {
                        let would_have_used = self
                            .snapshot_disc_sector_details(
                                disc_sector_hashes.keys().cloned().collect(),
                                arc_set,
                                self.store.clone(),
                            )
                            .await?;

                        assert_eq!(would_have_used, snapshot);
                    }

                    // There is no value in recomputing if we already have a matching snapshot.
                    // The disc sector details only requires a list of mismatched sectors which
                    // we already had when we computed the previous detailed snapshot.
                    // What we were missing previously was the detailed snapshot from the other
                    // party, which we now have and can use to produce a hash list.
                    snapshot
                }
                _ => {
                    self.snapshot_disc_sector_details(
                        disc_sector_hashes.keys().cloned().collect(),
                        arc_set,
                        self.store.clone(),
                    )
                    .await?
                }
            },
            DhtSnapshot::RingSectorDetails {
                ring_sector_hashes, ..
            } => {
                match our_previous_snapshot {
                    Some(snapshot @ DhtSnapshot::RingSectorDetails { .. }) => {
                        #[cfg(test)]
                        {
                            let would_have_used = self
                                .snapshot_ring_sector_details(
                                    ring_sector_hashes
                                        .keys()
                                        .cloned()
                                        .collect(),
                                    arc_set,
                                )?;

                            assert_eq!(would_have_used, snapshot);
                        }

                        // No need to recompute, see the comment above for DiscSectorDetails
                        snapshot
                    }
                    _ => self.snapshot_ring_sector_details(
                        ring_sector_hashes.keys().cloned().collect(),
                        arc_set,
                    )?,
                }
            }
        };

        // Now compare the snapshots to determine what to do next.
        // We will either send a more detailed snapshot back or a list of possible mismatched op
        // hashes. In the case that we produce a most detailed snapshot type, we can send the list
        // of op hashes at the same time.
        match our_snapshot.compare(their_snapshot) {
            SnapshotDiff::Identical => Ok(DhtSnapshotNextAction::Identical),
            SnapshotDiff::CannotCompare => {
                Ok(DhtSnapshotNextAction::CannotCompare)
            }
            SnapshotDiff::DiscMismatch => {
                Ok(DhtSnapshotNextAction::NewSnapshot(
                    self.snapshot_disc_sectors(arc_set).await?,
                ))
            }
            SnapshotDiff::DiscSectorMismatches(mismatched_sectors) => {
                Ok(DhtSnapshotNextAction::NewSnapshot(
                    self.snapshot_disc_sector_details(
                        mismatched_sectors,
                        arc_set,
                        self.store.clone(),
                    )
                    .await?,
                ))
            }
            SnapshotDiff::DiscSectorSliceMismatches(
                mismatched_slice_indices,
            ) => {
                let mut out = Vec::new();
                for (sector_index, missing_slices) in mismatched_slice_indices {
                    let Ok(arc) =
                        self.partition.dht_arc_for_sector_index(sector_index)
                    else {
                        tracing::error!(
                            "Sector index {} out of bounds, ignoring",
                            sector_index
                        );
                        continue;
                    };

                    for missing_slice in missing_slices {
                        let Ok((start, end)) = self
                            .partition
                            .time_bounds_for_full_slice_index(missing_slice)
                        else {
                            tracing::error!(
                                "Missing slice {} out of bounds, ignoring",
                                missing_slice
                            );
                            continue;
                        };

                        out.extend(
                            self.store
                                .retrieve_op_hashes_in_time_slice(
                                    arc, start, end,
                                )
                                .await?,
                        );
                    }
                }

                Ok(if is_final {
                    DhtSnapshotNextAction::HashList(out)
                } else {
                    DhtSnapshotNextAction::NewSnapshotAndHashList(
                        our_snapshot,
                        out,
                    )
                })
            }
            SnapshotDiff::RingMismatches(mismatched_rings) => {
                Ok(DhtSnapshotNextAction::NewSnapshot(
                    self.snapshot_ring_sector_details(
                        mismatched_rings,
                        arc_set,
                    )?,
                ))
            }
            SnapshotDiff::RingSectorMismatches(mismatched_sectors) => {
                let mut out = Vec::new();

                for (ring_index, missing_sectors) in mismatched_sectors {
                    for sector_index in missing_sectors {
                        let Ok(arc) = self
                            .partition
                            .dht_arc_for_sector_index(sector_index)
                        else {
                            tracing::error!(
                                "Sector index {} out of bounds, ignoring",
                                sector_index
                            );
                            continue;
                        };

                        let Ok((start, end)) = self
                            .partition
                            .time_bounds_for_partial_slice_index(ring_index)
                        else {
                            tracing::error!(
                                "Partial slice index {} out of bounds, ignoring",
                                ring_index
                            );
                            continue;
                        };

                        out.extend(
                            self.store
                                .retrieve_op_hashes_in_time_slice(
                                    arc, start, end,
                                )
                                .await?,
                        );
                    }
                }

                Ok(if is_final {
                    DhtSnapshotNextAction::HashList(out)
                } else {
                    DhtSnapshotNextAction::NewSnapshotAndHashList(
                        our_snapshot,
                        out,
                    )
                })
            }
        }
    }

    async fn snapshot_disc_sectors(
        &self,
        arc_set: &ArcSet,
    ) -> K2Result<DhtSnapshot> {
        let (disc_sector_top_hashes, disc_boundary) = self
            .partition
            .disc_sector_hashes(arc_set, self.store.clone())
            .await?;

        Ok(DhtSnapshot::DiscSectors {
            disc_sector_top_hashes,
            disc_boundary,
        })
    }

    async fn snapshot_disc_sector_details(
        &self,
        mismatched_sector_indices: Vec<u32>,
        arc_set: &ArcSet,
        store: DynOpStore,
    ) -> K2Result<DhtSnapshot> {
        let (disc_sector_hashes, disc_boundary) = self
            .partition
            .disc_sector_sector_details(
                arc_set,
                mismatched_sector_indices,
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
        let (ring_sector_hashes, disc_boundary) =
            self.partition.ring_details(arc_set, mismatched_rings)?;

        Ok(DhtSnapshot::RingSectorDetails {
            ring_sector_hashes,
            disc_boundary,
        })
    }
}
