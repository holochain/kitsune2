use crate::op_store::time_slice_hash_store::TimeSliceHashStore;
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
    /// Note that this is not the total number of hashes of a time slice at a unique `slice_id`.
    /// This value is the count, based on the highest stored id, starting from time slice id 0 and counting up to the highest stored id. In other words it is the id of the most recent time slice plus 1.
    ///
    /// This value is easier to compare between peers because it ignores sync progress. A simple
    /// count cannot tell the difference between a peer that has synced the first 4 time slices,
    /// and a peer who has synced the first 3 time slices and created one recent one. However,
    /// using the highest stored id shows the difference to be 4 and say 300 respectively.
    /// Equally, the literal count is more useful if the DHT contains a large amount of data and
    /// a peer might allocate a recent full slice before completing its initial sync. That situation
    /// could be created by a configuration that chooses small time-slices. However, in the general
    /// case, the highest stored id is more useful.
    fn slice_hash_count(&self) -> BoxFuture<'_, K2Result<u64>> {
        // +1 to convert from a 0-based index to a count
        async move {
            Ok(self
                .read()
                .await
                .time_slice_hashes
                .highest_stored_id()
                .map(|id| id + 1)
                .unwrap_or_default())
        }
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
        async move { Ok(self.read().await.time_slice_hashes.get(slice_id)) }
            .boxed()
    }
}
