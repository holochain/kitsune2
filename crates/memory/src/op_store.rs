use crate::error::Kitsune2MemoryError;
use futures::future::BoxFuture;
use futures::FutureExt;
use kitsune2_api::{BoundedOpHashResponse, MetaOp, OpId, OpStore, Timestamp};
use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Default)]
pub struct Kitsune2MemoryOpStore(pub Arc<RwLock<Kitsune2MemoryOpStoreInner>>);

impl std::ops::Deref for Kitsune2MemoryOpStore {
    type Target = Arc<RwLock<Kitsune2MemoryOpStoreInner>>;

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
    type Error = Kitsune2MemoryError;

    fn ingest_op_list(
        &self,
        op_list: Vec<MetaOp>,
    ) -> BoxFuture<'_, Result<(), Self::Error>> {
        async move {
            self.write()
                .await
                .op_list
                .append(&mut op_list.into_iter().collect());
            Ok(())
        }
        .boxed()
    }

    fn retrieve_op_hashes(
        &self,
        timestamp: Timestamp,
        limit_bytes: u32,
    ) -> BoxFuture<'_, Result<BoundedOpHashResponse, Self::Error>>
    {
        async move {
            let mut bytes = 0;
            let self_lock = self.read().await;
            let mut iter = self_lock.op_list.iter();

            let first_op = iter.find(|op| op.timestamp >= timestamp);
            let Some(op) = first_op else {
                return Ok(BoundedOpHashResponse::empty());
            };

            let mut out = Vec::new();
            bytes += op.op_data.len() as u32;
            out.push(op.op_id.clone());
            let mut last_timestamp = Some(op.timestamp);

            for op in iter {
                let op_bytes = op.op_data.len() as u32;
                if bytes + op_bytes > limit_bytes {
                    break;
                }
                bytes += op_bytes;
                out.push(op.op_id.clone());
                last_timestamp = Some(op.timestamp);
            }
            Ok(BoundedOpHashResponse {
                op_ids: out,
                last_timestamp,
            })
        }
        .boxed()
    }

    fn retrieve_op_hashes_in_time_slice(
        &self,
        start: Timestamp,
        end: Timestamp,
    ) -> BoxFuture<'_, Result<Vec<OpId>, Self::Error>> {
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
    ) -> BoxFuture<'_, Result<(), Self::Error>> {
        async move {
            self.write()
                .await
                .time_slice_hashes
                .insert(slice_id, slice_hash);
            Ok(())
        }
        .boxed()
    }

    fn retrieve_slice_hash(
        &self,
        slice_id: u64,
    ) -> BoxFuture<'_, Result<Option<Vec<u8>>, Self::Error>> {
        async move {
            Ok(self.read().await.time_slice_hashes.get(&slice_id).cloned())
        }
        .boxed()
    }
}
