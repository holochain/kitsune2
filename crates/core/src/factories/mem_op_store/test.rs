use crate::factories::mem_op_store::Kitsune2MemoryOpStore;
use crate::factories::MemoryOp;
use kitsune2_api::{DynOpStore, SpaceId, Timestamp};
use std::sync::Arc;

#[tokio::test]
async fn process_and_retrieve_op() {
    let op_store: DynOpStore = Arc::new(Kitsune2MemoryOpStore::new(
        SpaceId::from(bytes::Bytes::from_static(b"test")),
    ));

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
