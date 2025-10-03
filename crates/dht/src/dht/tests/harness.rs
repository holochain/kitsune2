use crate::arc_set::ArcSet;
use crate::dht::snapshot::DhtSnapshot;
use crate::test::test_store;
use crate::{Dht, DhtApi, DhtSnapshotNextAction};
use kitsune2_api::{
    AgentId, DhtArc, DynOpStore, K2Result, OpId, StoredOp, Timestamp,
};
use kitsune2_core::factories::MemoryOp;
use rand::RngCore;
use std::collections::HashMap;

/// Intended to represent a single agent in a network, which knows how to sync with
/// some other agent.
pub(crate) struct DhtSyncHarness {
    pub(crate) store: DynOpStore,
    pub(crate) dht: Dht,
    pub(crate) arc: DhtArc,
    pub(crate) agent_id: AgentId,
    pub(crate) discovered_ops: HashMap<AgentId, Vec<OpId>>,
}

pub(crate) enum SyncWithOutcome {
    InSync,
    CannotCompare,
    SyncedDisc,
    SyncedRings,
    DiscoveredInSync,
}

impl DhtSyncHarness {
    pub(crate) async fn new(current_time: Timestamp, arc: DhtArc) -> Self {
        let store = test_store().await;
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

    pub(crate) async fn inject_ops(
        &mut self,
        op_list: Vec<MemoryOp>,
    ) -> K2Result<()> {
        self.store
            .process_incoming_ops(
                op_list.iter().map(|op| op.clone().into()).collect(),
            )
            .await?;

        self.dht
            .inform_ops_stored(
                op_list.into_iter().map(|op| op.into()).collect(),
            )
            .await?;

        Ok(())
    }

    pub(crate) async fn apply_op_sync(
        &mut self,
        other: &mut Self,
    ) -> K2Result<()> {
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

    pub(crate) async fn is_in_sync_with(&self, other: &Self) -> K2Result<bool> {
        let arc_set_1 = ArcSet::new(vec![self.arc])?;
        let arc_set_2 = ArcSet::new(vec![other.arc])?;
        let arc_set = arc_set_1.intersection(&arc_set_2);
        let initial_snapshot =
            self.dht.snapshot_minimal(arc_set.clone()).await?;
        match other
            .dht
            .handle_snapshot(initial_snapshot, None, arc_set.clone(), 1_000)
            .await?
            .0
        {
            DhtSnapshotNextAction::Identical => Ok(true),
            _ => Ok(false),
        }
    }

    pub(crate) async fn sync_with(
        &mut self,
        other: &mut Self,
    ) -> K2Result<SyncWithOutcome> {
        let arc_set_1 = ArcSet::new(vec![self.arc])?;
        let arc_set_2 = ArcSet::new(vec![other.arc])?;
        let arc_set = arc_set_1.intersection(&arc_set_2);

        // Create the initial snapshot locally
        let initial_snapshot =
            self.dht.snapshot_minimal(arc_set.clone()).await?;

        // Send it to the other agent and have them diff against it
        let outcome = other
            .dht
            .handle_snapshot(initial_snapshot, None, arc_set.clone(), 1_000)
            .await?
            .0;

        match outcome {
            DhtSnapshotNextAction::Identical => {
                // Nothing to do, the agents are in sync
                Ok(SyncWithOutcome::InSync)
            }
            DhtSnapshotNextAction::CannotCompare => {
                // Permit this for testing purposes, it would be treated as an error in
                // real usage
                Ok(SyncWithOutcome::CannotCompare)
            }
            DhtSnapshotNextAction::NewSnapshot(new_snapshot) => {
                match new_snapshot {
                    DhtSnapshot::Minimal { .. } => {
                        panic!(
                            "A minimal snapshot cannot be produced from a minimal snapshot"
                        );
                    }
                    DhtSnapshot::DiscSectors { .. } => {
                        // This means there's a historical mismatch, so we need to continue
                        // down this path another step. Do that in another function!
                        self.sync_disc_with(other, &arc_set, new_snapshot).await
                    }
                    DhtSnapshot::DiscSectorDetails { .. } => {
                        panic!(
                            "A sector details snapshot cannot be produced from a minimal snapshot"
                        );
                    }
                    DhtSnapshot::RingSectorDetails { .. } => {
                        // This means there's a recent mismatch in the partial time slices.
                        // Similar to above, continue in another function.
                        self.sync_rings_with(other, &arc_set, new_snapshot)
                            .await
                    }
                }
            }
            DhtSnapshotNextAction::NewSnapshotAndHashList(_, _)
            | DhtSnapshotNextAction::HashList(_) => {
                panic!(
                    "A hash list cannot be produced from a minimal snapshot"
                );
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
            .handle_snapshot(snapshot, None, arc_set.clone(), 1_000)
            .await?
            .0;

        let our_details_snapshot = match outcome {
            DhtSnapshotNextAction::NewSnapshot(new_snapshot) => new_snapshot,
            DhtSnapshotNextAction::Identical => {
                // This can't happen in tests but in a real implementation it's possible that
                // missing ops might show up while we're syncing so this isn't an error, just
                // a shortcut and we can stop syncing
                return Ok(SyncWithOutcome::DiscoveredInSync);
            }
            DhtSnapshotNextAction::NewSnapshotAndHashList(_, _)
            | DhtSnapshotNextAction::HashList(_)
            | DhtSnapshotNextAction::CannotCompare => {
                // A real implementation would treat these as errors because they are logic
                // errors
                panic!("Unexpected outcome: {outcome:?}");
            }
        };

        // At this point, the snapshot must be a disc details snapshot
        match our_details_snapshot {
            DhtSnapshot::DiscSectorDetails { .. } => {}
            _ => panic!("Expected a DiscSectorDetails snapshot"),
        }

        // Now we need to ask the other agent to diff against this details snapshot
        let outcome = other
            .dht
            .handle_snapshot(
                our_details_snapshot.clone(),
                None,
                arc_set.clone(),
                1_000,
            )
            .await?
            .0;

        let (snapshot, hash_list_from_other) = match outcome {
            DhtSnapshotNextAction::NewSnapshotAndHashList(
                new_snapshot,
                hash_list,
            ) => (new_snapshot, hash_list),
            DhtSnapshotNextAction::Identical => {
                // Nothing to do, the agents are in sync
                return Ok(SyncWithOutcome::InSync);
            }
            DhtSnapshotNextAction::NewSnapshot(_)
            | DhtSnapshotNextAction::HashList(_)
            | DhtSnapshotNextAction::CannotCompare => {
                // A real implementation would treat these as errors because they are logic
                // errors
                panic!("Unexpected outcome: {outcome:?}");
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
            .handle_snapshot(
                snapshot,
                Some(our_details_snapshot),
                arc_set.clone(),
                1000,
            )
            .await?
            .0;

        let hash_list_from_self = match outcome {
            DhtSnapshotNextAction::HashList(hash_list) => hash_list,
            DhtSnapshotNextAction::Identical => {
                // Nothing to do, the agents are in sync
                return Ok(SyncWithOutcome::InSync);
            }
            DhtSnapshotNextAction::NewSnapshot(_)
            | DhtSnapshotNextAction::NewSnapshotAndHashList(_, _)
            | DhtSnapshotNextAction::CannotCompare => {
                // A real implementation would treat these as errors because they are logic
                // errors
                panic!("Unexpected outcome: {outcome:?}");
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
        other_details_snapshot: DhtSnapshot,
    ) -> K2Result<SyncWithOutcome> {
        match other_details_snapshot {
            DhtSnapshot::RingSectorDetails { .. } => {}
            _ => panic!("Expected a RingSectorDetails snapshot"),
        }

        // We expect the sync to have been initiated by self, so the ring sector details should
        // have been sent to us
        let outcome = self
            .dht
            .handle_snapshot(
                other_details_snapshot.clone(),
                None,
                arc_set.clone(),
                1_000,
            )
            .await?
            .0;

        let (snapshot, hash_list_from_self) = match outcome {
            DhtSnapshotNextAction::Identical => {
                // Nothing to do, the agents are in sync
                return Ok(SyncWithOutcome::InSync);
            }
            DhtSnapshotNextAction::NewSnapshotAndHashList(
                new_snapshot,
                hash_list,
            ) => (new_snapshot, hash_list),
            DhtSnapshotNextAction::CannotCompare
            | DhtSnapshotNextAction::HashList(_)
            | DhtSnapshotNextAction::NewSnapshot(_) => {
                panic!("Unexpected outcome: {outcome:?}");
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
            .handle_snapshot(
                snapshot,
                Some(other_details_snapshot),
                arc_set.clone(),
                1_000,
            )
            .await?
            .0;

        let hash_list_from_other = match outcome {
            DhtSnapshotNextAction::Identical => {
                // Nothing to do, the agents are in sync
                return Ok(SyncWithOutcome::InSync);
            }
            DhtSnapshotNextAction::HashList(hash_list) => hash_list,
            DhtSnapshotNextAction::CannotCompare
            | DhtSnapshotNextAction::NewSnapshotAndHashList(_, _)
            | DhtSnapshotNextAction::NewSnapshot(_) => {
                panic!("Unexpected outcome: {outcome:?}");
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

async fn transfer_ops(
    source: DynOpStore,
    target: DynOpStore,
    target_dht: &mut Dht,
    requested_ops: Vec<OpId>,
) -> K2Result<()> {
    let selected = source
        .retrieve_ops(requested_ops)
        .await?
        .into_iter()
        .map(|op| op.op_data)
        .collect::<Vec<_>>();
    target.process_incoming_ops(selected.clone()).await?;

    let stored_ops = selected
        .into_iter()
        .map(|op| MemoryOp::from(op).into())
        .collect::<Vec<StoredOp>>();
    target_dht.inform_ops_stored(stored_ops).await?;

    Ok(())
}
