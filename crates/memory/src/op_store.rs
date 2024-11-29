use crate::op_store::time_slice_hash_store::TimeSliceHashStore;
use crate::time_slice_hash_store::SliceHash;
use futures::future::BoxFuture;
use futures::FutureExt;
use kitsune2_api::{K2Result, MetaOp, OpId, OpStore, Timestamp};
use std::collections::BTreeSet;
use tokio::sync::RwLock;

pub mod time_slice_hash_store;

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
    time_slice_hashes: TimeSliceHashStore,
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
                .filter(|op| op.timestamp >= start && op.timestamp < end)
                .map(|op| op.op_id.clone())
                .collect())
        }
        .boxed()
    }

    /// Store the combined hash of a time slice.
    ///
    /// The `slice_id` is the index of the time slice. This is a 0-based index. So for a given
    /// time period being used to slice time, the first `slice_hash` at `slice_id` 0 would
    /// represent the combined hash of all known ops in the time slice `[0, period)`. Then `slice_id`
    /// 1 would represent the combined hash of all known ops in the time slice `[period, 2*period)`.
    fn store_slice_hash(
        &self,
        slice_id: u64,
        slice_hash: bytes::Bytes,
    ) -> BoxFuture<'_, K2Result<()>> {
        async move {
            self.write()
                .await
                .time_slice_hashes
                .insert(slice_id, slice_hash)
        }
        .boxed()
    }

    /// Retrieve the count of time slice hashes stored.
    ///
    /// Note that this is not the number of hashes that have been provided at unique `slice_id`s.
    /// Start from time slice id 0 and count up to the highest stored id. This value should be the
    /// count based on the highest stored id.
    /// This value is easier to compare between peers because it tracks an absolute number of time
    /// slices that have had a slice hash computed and stored. The number of stored hashes would
    /// only be comparable if the earliest point that data has been seen for is known between two
    /// peers.
    fn slice_hash_count(&self) -> BoxFuture<'_, K2Result<u64>> {
        // +1 to convert from a 0-based index to a count
        async move { Ok(self.read().await.time_slice_hashes.highest_stored_id.map(|id| id + 1).unwrap_or_default()) }
            .boxed()
    }

    /// Retrieve the hash of a time slice.
    ///
    /// This must be the same value provided by the caller to `store_slice_hash` for the same `slice_id`.
    /// If `store_slice_hash` has been called multiple times for the same `slice_id`, the most recent value is returned.
    /// If the caller has never provided a value for this `slice_id`, return `None`.
    fn retrieve_slice_hash(
        &self,
        slice_id: u64,
    ) -> BoxFuture<'_, K2Result<Option<bytes::Bytes>>> {
        async move {
            Ok(self.read().await.time_slice_hashes.get(slice_id).and_then(
                |x| match x {
                    SliceHash::Single { hash, .. } => Some(hash.clone()),
                    SliceHash::Block { .. } => None,
                },
            ))
        }
        .boxed()
    }
}
