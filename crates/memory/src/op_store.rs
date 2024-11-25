use futures::future::BoxFuture;
use futures::FutureExt;
use kitsune2_api::{K2Result, MetaOp, OpId, OpStore, Timestamp};
use std::collections::{BTreeSet, HashMap};
use tokio::sync::RwLock;

#[derive(Debug, Default)]
pub struct Kitsune2MemoryOpStore(pub RwLock<Kitsune2MemoryOpStoreInner>);

impl std::ops::Deref for Kitsune2MemoryOpStore {
    type Target = RwLock<Kitsune2MemoryOpStoreInner>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Debug, Default)]
pub struct Kitsune2MemoryOpStoreInner {
    op_list: BTreeSet<MetaOp>,
    time_slice_hashes: HashMap<u64, Vec<u8>>,
}

impl OpStore for Kitsune2MemoryOpStore {
    fn process_incoming_ops(
        &self,
        op_list: Vec<MetaOp>,
    ) -> BoxFuture<'_, K2Result<()>> {
        async move {
            self.write()
                .await
                .op_list
                .append(&mut op_list.into_iter().collect());
            Ok(())
        }
        .boxed()
    }

    fn retrieve_op_hashes_in_time_slice(
        &self,
        start: Timestamp,
        end: Timestamp,
    ) -> BoxFuture<'_, K2Result<Vec<OpId>>> {
        async move {
            let self_lock = self.read().await;
            Ok(self_lock
                .op_list
                .iter()
                .filter(|op| op.timestamp >= start && op.timestamp <= end)
                .map(|op| op.op_id.clone())
                .collect())
        }
        .boxed()
    }

    fn store_slice_hash(
        &self,
        slice_id: u64,
        slice_hash: Vec<u8>,
    ) -> BoxFuture<'_, K2Result<()>> {
        async move {
            self.write()
                .await
                .time_slice_hashes
                .insert(slice_id, slice_hash);
            Ok(())
        }
        .boxed()
    }

    fn slice_hash_count(&self) -> BoxFuture<'_, K2Result<u64>> {
        async move { Ok(self.read().await.time_slice_hashes.len() as u64) }
            .boxed()
    }

    fn retrieve_slice_hash(
        &self,
        slice_id: u64,
    ) -> BoxFuture<'_, K2Result<Option<Vec<u8>>>> {
        async move {
            Ok(self.read().await.time_slice_hashes.get(&slice_id).cloned())
        }
        .boxed()
    }
}
