use crate::factories::mem_op_store::Kitsune2MemoryOpStore;
use crate::factories::MemoryOp;
use kitsune2_api::{DhtArc, DynOpStore, SpaceId, Timestamp};
use kitsune2_test_utils::space::TEST_SPACE_ID;
use std::sync::Arc;

fn op_at_location(loc: u32) -> MemoryOp {
    let mut out = Vec::with_capacity(32);
    out.extend_from_slice(&loc.to_le_bytes());
    out.resize(32, 0);

    MemoryOp::new(Timestamp::now(), out)
}

#[tokio::test]
async fn process_and_retrieve_op() {
    let op_store: DynOpStore =
        Arc::new(Kitsune2MemoryOpStore::new(TEST_SPACE_ID));

    let op = MemoryOp::new(Timestamp::now(), vec![1, 2, 3]);
    let op_id = op.compute_op_id();
    op_store
        .process_incoming_ops(vec![op.clone().into()])
        .await
        .unwrap();

    let retrieved = op_store.retrieve_ops(vec![op_id.clone()]).await.unwrap();
    assert_eq!(retrieved.len(), 1);
    assert_eq!(retrieved[0].op_id, op_id);

    let out: MemoryOp = serde_json::from_slice(&retrieved[0].op_data).unwrap();
    assert_eq!(op, out);
}

#[tokio::test]
async fn retrieve_op_ids_bounded_empty_arc() {
    let op_store: DynOpStore =
        Arc::new(Kitsune2MemoryOpStore::new(TEST_SPACE_ID));

    op_store
        .process_incoming_ops(vec![op_at_location(1).clone().into()])
        .await
        .unwrap();

    let ops_in_empty_arc = op_store
        .retrieve_op_ids_bounded(DhtArc::Empty, Timestamp::from_micros(0), 100)
        .await
        .unwrap();
    assert!(ops_in_empty_arc.0.is_empty());
    assert_eq!(0, ops_in_empty_arc.1);
    // Notice that the bookmark moves even though we didn't retrieve any ops. This means that
    // changing the arc invalidates the bookmark.
    assert!(ops_in_empty_arc.2.as_micros() > 0);
}

#[tokio::test]
async fn retrieve_op_ids_bounded_limit_applied() {
    let op_store: DynOpStore = Arc::new(Kitsune2MemoryOpStore::new(
        SpaceId::from(bytes::Bytes::from_static(b"test")),
    ));

    op_store
        .process_incoming_ops(vec![
            op_at_location(1).clone().into(),
            op_at_location(2).clone().into(),
            op_at_location(3).clone().into(),
        ])
        .await
        .unwrap();

    let before_query = Timestamp::now();
    let ops_in_arc = op_store
        .retrieve_op_ids_bounded(
            DhtArc::Arc(0, 32),
            Timestamp::from_micros(0),
            68,
        )
        .await
        .unwrap();
    assert_eq!(2, ops_in_arc.0.len());
    assert_eq!(64, ops_in_arc.1);
    // Should give us back the stored timestamp since we hit the limit.
    // Otherwise, we'd get a timestamp from the query time, which would be after `before_query`.
    assert!(ops_in_arc.2.as_micros() < before_query.as_micros());
}

#[tokio::test]
async fn retrieve_op_ids_bounded_across_multiple_arcs() {
    let op_store: DynOpStore = Arc::new(Kitsune2MemoryOpStore::new(
        SpaceId::from(bytes::Bytes::from_static(b"test")),
    ));

    op_store
        .process_incoming_ops(vec![
            op_at_location(1).clone().into(),
            op_at_location(2).clone().into(),
            op_at_location(3).clone().into(),
            op_at_location(50).clone().into(),
            op_at_location(51).clone().into(),
            op_at_location(52).clone().into(),
        ])
        .await
        .unwrap();

    let bookmark = Timestamp::from_micros(0);
    let mut limit_bytes = 164;

    let ops_in_arc_1 = op_store
        .retrieve_op_ids_bounded(DhtArc::Arc(0, 32), bookmark, limit_bytes)
        .await
        .unwrap();

    assert_eq!(3, ops_in_arc_1.0.len());
    assert_eq!(96, ops_in_arc_1.1);
    let new_bookmark_1 = ops_in_arc_1.2;

    limit_bytes -= ops_in_arc_1.1;
    let op_in_arc_2 = op_store
        .retrieve_op_ids_bounded(DhtArc::Arc(32, 64), bookmark, limit_bytes)
        .await
        .unwrap();

    assert_eq!(2, op_in_arc_2.0.len());
    assert_eq!(64, op_in_arc_2.1);
    let new_bookmark_2 = op_in_arc_2.2;

    // Select the minimum bookmark. This will still advance but we want to avoid missing ops
    // as much as possible.
    let new_bookmark =
        &[new_bookmark_1, new_bookmark_2].into_iter().min().unwrap();

    assert!(*new_bookmark > bookmark);

    let ops_in_arc_1 = op_store
        .retrieve_op_ids_bounded(DhtArc::Arc(0, 32), *new_bookmark, 1_000)
        .await
        .unwrap();

    // Nothing more to fetch in this arc.
    assert!(ops_in_arc_1.0.is_empty());

    let ops_in_arc_2 = op_store
        .retrieve_op_ids_bounded(DhtArc::Arc(32, 64), *new_bookmark, 1_000)
        .await
        .unwrap();

    // Gets the remaining op in the second arc.
    assert_eq!(1, ops_in_arc_2.0.len());
}
