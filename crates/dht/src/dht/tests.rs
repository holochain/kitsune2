mod harness;

use super::*;
use crate::dht::tests::harness::SyncWithOutcome;
use harness::DhtSyncHarness;
use kitsune2_api::{DhtArc, OpId, OpStore, UNIX_TIMESTAMP};
use kitsune2_memory::{Kitsune2MemoryOp, Kitsune2MemoryOpStore};
use std::sync::Arc;
use std::time::Duration;

const SECTOR_SIZE: u32 = 1u32 << 23;

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

    let snapshot = dht.snapshot_minimal(&arc_set).await.unwrap();
    match snapshot {
        DhtSnapshot::Minimal {
            disc_boundary,
            disc_top_hash,
            ring_top_hashes,
        } => {
            assert_eq!(dht.partition.full_slice_end_timestamp(), disc_boundary);
            assert_eq!(bytes::Bytes::from(vec![7; 32]), disc_top_hash);
            assert!(!ring_top_hashes.is_empty());
        }
        s => panic!("Unexpected snapshot type: {:?}", s),
    }
}

#[tokio::test]
async fn cannot_take_minimal_snapshot_with_empty_arc_set() {
    let current_time = Timestamp::now();
    let dht1 = DhtSyncHarness::new(current_time, DhtArc::Empty).await;

    let err = dht1
        .dht
        .snapshot_minimal(&ArcSet::new(SECTOR_SIZE, vec![dht1.arc]).unwrap())
        .await
        .unwrap_err();
    assert_eq!("No arcs to snapshot (src: None)", err.to_string());
}

#[tokio::test]
async fn cannot_handle_snapshot_with_empty_arc_set() {
    let current_time = Timestamp::now();
    let dht1 = DhtSyncHarness::new(current_time, DhtArc::Empty).await;

    // Declare a full arc to get a snapshot
    let snapshot = dht1
        .dht
        .snapshot_minimal(
            &ArcSet::new(SECTOR_SIZE, vec![DhtArc::FULL]).unwrap(),
        )
        .await
        .unwrap();

    // Now try to compare that snapshot to ourselves with an empty arc set
    let err = dht1
        .dht
        .handle_snapshot(
            &snapshot,
            None,
            &ArcSet::new(SECTOR_SIZE, vec![DhtArc::Empty]).unwrap(),
        )
        .await
        .unwrap_err();

    assert_eq!("No arcs to snapshot (src: None)", err.to_string());
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
    let op_id = OpId::from(bytes::Bytes::from(vec![41; 32]));
    dht1.inject_ops(vec![Kitsune2MemoryOp::new(
        op_id.clone(),
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
    assert_eq!(vec![op_id], dht2.discovered_ops[&dht1.agent_id]);

    assert!(!dht1.is_in_sync_with(&dht2).await.unwrap());

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
            OpId::from(bytes::Bytes::from(vec![7; 4])),
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
        OpId::from(bytes::Bytes::from(vec![7; 4])),
        dht1.dht.partition.full_slice_end_timestamp(),
        vec![],
    )])
    .await
    .unwrap();

    dht2.inject_ops(vec![Kitsune2MemoryOp::new(
        OpId::from(bytes::Bytes::from(vec![13; 4])),
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
    let mut dht1 =
        DhtSyncHarness::new(current_time, DhtArc::Arc(0, 3 * SECTOR_SIZE - 1))
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
    let mut dht1 =
        DhtSyncHarness::new(current_time, DhtArc::Arc(0, 3 * SECTOR_SIZE - 1))
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
