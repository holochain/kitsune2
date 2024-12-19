//! Top-level DHT model.

use crate::arc_set::ArcSet;
use crate::PartitionedHashes;
use kitsune2_api::{DynOpStore, K2Result, OpId, StoredOp, Timestamp};
use snapshot::{DhtSnapshot, SnapshotDiff};

mod snapshot;
#[cfg(test)]
mod tests;

pub struct Dht {
    partition: PartitionedHashes,
}

#[derive(Debug)]
pub enum DhtSnapshotNextAction {
    Identical,
    CannotCompare,
    NewSnapshot(DhtSnapshot),
    NewSnapshotAndHashList(DhtSnapshot, Vec<OpId>),
    HashList(Vec<OpId>),
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
            self.partition.disc_top_hash(arc_set, store).await?;

        Ok(DhtSnapshot::Minimal {
            disc_top_hash,
            disc_boundary,
            ring_top_hashes: self.partition.ring_top_hashes(arc_set),
        })
    }

    pub async fn handle_snapshot(
        &self,
        their_snapshot: &DhtSnapshot,
        our_previous_snapshot: Option<DhtSnapshot>,
        arc_set: &ArcSet,
        store: DynOpStore,
    ) -> K2Result<DhtSnapshotNextAction> {
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
                self.snapshot_minimal(arc_set, store.clone()).await?
            }
            DhtSnapshot::DiscSectors { .. } => {
                self.snapshot_disc_sectors(arc_set, store.clone()).await?
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
                                store.clone(),
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
                        store.clone(),
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
                    self.snapshot_disc_sectors(arc_set, store).await?,
                ))
            }
            SnapshotDiff::DiscSectorMismatches(mismatched_sectors) => {
                Ok(DhtSnapshotNextAction::NewSnapshot(
                    self.snapshot_disc_sector_details(
                        mismatched_sectors,
                        arc_set,
                        store,
                    )
                    .await?,
                ))
            }
            SnapshotDiff::DiscSectorSliceMismatches(mismatched_slice_ids) => {
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
