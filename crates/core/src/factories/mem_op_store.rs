//! The mem op store implementation provided by Kitsune2.

use crate::factories::mem_op_store::time_slice_hash_store::TimeSliceHashStore;
use futures::future::BoxFuture;
use kitsune2_api::builder::Builder;
use kitsune2_api::config::Config;
use kitsune2_api::{
    BoxFut, DhtArc, DynOpStore, DynOpStoreFactory, K2Error, K2Result, MetaOp,
    Op, OpId, OpStore, OpStoreFactory, SpaceId, StoredOp, Timestamp,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

mod time_slice_hash_store;

#[cfg(test)]
mod test;

/// The mem op store implementation provided by Kitsune2.
#[derive(Debug)]
pub struct MemOpStoreFactory {}

impl MemOpStoreFactory {
    /// Construct a new MemOpStoreFactory.
    pub fn create() -> DynOpStoreFactory {
        let out: DynOpStoreFactory = Arc::new(MemOpStoreFactory {});
        out
    }
}

impl OpStoreFactory for MemOpStoreFactory {
    fn default_config(&self, _config: &mut Config) -> K2Result<()> {
        Ok(())
    }

    fn create(
        &self,
        _builder: Arc<Builder>,
        space: SpaceId,
    ) -> BoxFut<'static, K2Result<DynOpStore>> {
        Box::pin(async move {
            let out: DynOpStore = Arc::new(Kitsune2MemoryOpStore::new(space));
            Ok(out)
        })
    }
}

/// This is a stub implementation of an op that will be serialized
/// via serde_json (with inefficient encoding of the payload) to be
/// used for testing purposes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MemoryOp {
    /// The creation timestamp of this op
    pub created_at: Timestamp,
    /// The data for the op
    pub op_data: Vec<u8>,
}

impl MemoryOp {
    /// Create a new [MemoryOp].
    pub fn new(timestamp: Timestamp, payload: Vec<u8>) -> Self {
        Self {
            created_at: timestamp,
            op_data: payload,
        }
    }

    /// Compute the op id for this op.
    ///
    /// Note that this produces predictable op ids for testing purposes.
    /// It is simply the first 32 bytes of the op data.
    pub fn compute_op_id(&self) -> OpId {
        let mut value =
            self.op_data.as_slice()[..32.min(self.op_data.len())].to_vec();
        value.resize(32, 0);
        OpId::from(bytes::Bytes::from(value))
    }
}

impl From<bytes::Bytes> for MemoryOp {
    fn from(value: bytes::Bytes) -> Self {
        serde_json::from_slice(&value)
            .expect("failed to deserialize MemoryOp from bytes")
    }
}

impl From<MemoryOp> for bytes::Bytes {
    fn from(value: MemoryOp) -> Self {
        serde_json::to_vec(&value)
            .expect("failed to serialize MemoryOp to bytes")
            .into()
    }
}

/// This is the storage record for an op with computed fields.
///
/// Test data should create [MemoryOp]s and not be aware of this type.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct MemoryOpRecord {
    /// The id (hash) of the op
    pub op_id: OpId,
    /// The creation timestamp of this op
    pub created_at: Timestamp,
    /// The timestamp at which this op was stored by us
    pub stored_at: Timestamp,
    /// The data for the op
    pub op_data: Vec<u8>,
}

impl From<bytes::Bytes> for MemoryOpRecord {
    fn from(value: bytes::Bytes) -> Self {
        let inner: MemoryOp = value.into();
        Self {
            op_id: inner.compute_op_id(),
            created_at: inner.created_at,
            stored_at: Timestamp::now(),
            op_data: inner.op_data,
        }
    }
}

impl From<MemoryOp> for StoredOp {
    fn from(value: MemoryOp) -> Self {
        StoredOp {
            op_id: value.compute_op_id(),
            created_at: value.created_at,
        }
    }
}

impl From<Op> for MemoryOp {
    fn from(value: Op) -> Self {
        value.data.into()
    }
}

#[derive(Debug)]
struct Kitsune2MemoryOpStore {
    _space: SpaceId,
    inner: RwLock<Kitsune2MemoryOpStoreInner>,
}

impl Kitsune2MemoryOpStore {
    pub fn new(space: SpaceId) -> Self {
        Self {
            _space: space,
            inner: Default::default(),
        }
    }
}

impl std::ops::Deref for Kitsune2MemoryOpStore {
    type Target = RwLock<Kitsune2MemoryOpStoreInner>;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

#[derive(Debug, Default)]
struct Kitsune2MemoryOpStoreInner {
    op_list: HashMap<OpId, MemoryOpRecord>,
    time_slice_hashes: TimeSliceHashStore,
}

impl OpStore for Kitsune2MemoryOpStore {
    fn process_incoming_ops(
        &self,
        op_list: Vec<bytes::Bytes>,
    ) -> BoxFuture<'_, K2Result<Vec<OpId>>> {
        Box::pin(async move {
            let ops_to_add = op_list
                .iter()
                .map(|op| -> serde_json::Result<(OpId, MemoryOpRecord)> {
                    let op = MemoryOpRecord::from(op.clone());
                    Ok((op.op_id.clone(), op))
                })
                .collect::<Result<Vec<_>, _>>().map_err(|e| {
                K2Error::other_src("Failed to deserialize op data, are you using `Kitsune2MemoryOp`s?", e)
            })?;

            let op_ids = ops_to_add
                .iter()
                .map(|(op_id, _)| op_id.clone())
                .collect::<Vec<_>>();
            self.write().await.op_list.extend(ops_to_add);
            Ok(op_ids)
        })
    }

    fn retrieve_op_hashes_in_time_slice(
        &self,
        arc: DhtArc,
        start: Timestamp,
        end: Timestamp,
    ) -> BoxFuture<'_, K2Result<Vec<OpId>>> {
        Box::pin(async move {
            let self_lock = self.read().await;
            Ok(self_lock
                .op_list
                .iter()
                .filter(|(_, op)| {
                    let loc = op.op_id.loc();
                    op.created_at >= start
                        && op.created_at < end
                        && arc.contains(loc)
                })
                .map(|(op_id, _)| op_id.clone())
                .collect())
        })
    }

    fn retrieve_ops(
        &self,
        op_ids: Vec<OpId>,
    ) -> BoxFuture<'_, K2Result<Vec<MetaOp>>> {
        Box::pin(async move {
            let self_lock = self.read().await;
            Ok(op_ids
                .iter()
                .filter_map(|op_id| {
                    self_lock.op_list.get(op_id).map(|op| MetaOp {
                        op_id: op.op_id.clone(),
                        op_data: MemoryOp {
                            created_at: op.created_at,
                            op_data: op.op_data.clone(),
                        }
                        .into(),
                    })
                })
                .collect())
        })
    }

    /// Store the combined hash of a time slice.
    ///
    /// The `slice_id` is the index of the time slice. This is a 0-based index. So for a given
    /// time period being used to slice time, the first `slice_hash` at `slice_id` 0 would
    /// represent the combined hash of all known ops in the time slice `[0, period)`. Then `slice_id`
    /// 1 would represent the combined hash of all known ops in the time slice `[period, 2*period)`.
    fn store_slice_hash(
        &self,
        arc: DhtArc,
        slice_id: u64,
        slice_hash: bytes::Bytes,
    ) -> BoxFuture<'_, K2Result<()>> {
        Box::pin(async move {
            self.write()
                .await
                .time_slice_hashes
                .insert(arc, slice_id, slice_hash)
        })
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
    fn slice_hash_count(&self, arc: DhtArc) -> BoxFuture<'_, K2Result<u64>> {
        // +1 to convert from a 0-based index to a count
        Box::pin(async move {
            Ok(self
                .read()
                .await
                .time_slice_hashes
                .highest_stored_id(&arc)
                .map(|id| id + 1)
                .unwrap_or_default())
        })
    }

    /// Retrieve the hash of a time slice.
    ///
    /// This must be the same value provided by the caller to `store_slice_hash` for the same `slice_id`.
    /// If `store_slice_hash` has been called multiple times for the same `slice_id`, the most recent value is returned.
    /// If the caller has never provided a value for this `slice_id`, return `None`.
    fn retrieve_slice_hash(
        &self,
        arc: DhtArc,
        slice_id: u64,
    ) -> BoxFuture<'_, K2Result<Option<bytes::Bytes>>> {
        Box::pin(async move {
            Ok(self.read().await.time_slice_hashes.get(&arc, slice_id))
        })
    }

    /// Retrieve the hashes of all time slices.
    fn retrieve_slice_hashes(
        &self,
        arc: DhtArc,
    ) -> BoxFuture<'_, K2Result<Vec<(u64, bytes::Bytes)>>> {
        Box::pin(async move {
            let self_lock = self.read().await;
            Ok(self_lock.time_slice_hashes.get_all(&arc))
        })
    }
}
