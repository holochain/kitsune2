//! The mem op store implementation provided by Kitsune2.

use crate::factories::mem_op_store::time_slice_hash_store::TimeSliceHashStore;
use bytes::Bytes;
use futures::future::BoxFuture;
use kitsune2_api::*;
use kitsune2_api::{
    BoxFut, DhtArc, DynOpStore, DynOpStoreFactory, K2Error, K2Result, MetaOp,
    Op, OpId, OpStore, OpStoreFactory, SpaceId, StoredOp, Timestamp,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::convert::Infallible;
use std::marker::PhantomData;
use std::sync::Arc;
use tokio::sync::RwLock;

mod time_slice_hash_store;

#[cfg(test)]
mod test;

/// The mem op store implementation provided by Kitsune2.
#[derive(Debug)]
pub struct MemOpStoreFactory<MemOp> {
    _mem_op: PhantomData<MemOp>,
}

impl<MemOp: MemoryOp> Default for MemOpStoreFactory<MemOp> {
    fn default() -> Self {
        Self {
            _mem_op: PhantomData::<MemOp>::default(),
        }
    }
}

impl<MemOp: MemoryOp> MemOpStoreFactory<MemOp> {
    /// Construct a new MemOpStoreFactory.
    pub fn create() -> DynOpStoreFactory {
        let out: DynOpStoreFactory =
            Arc::new(MemOpStoreFactory::<MemOp>::default());
        out
    }
}

impl<MemOp: MemoryOp> OpStoreFactory for MemOpStoreFactory<MemOp> {
    fn default_config(&self, _config: &mut Config) -> K2Result<()> {
        Ok(())
    }

    fn validate_config(&self, _config: &Config) -> K2Result<()> {
        Ok(())
    }

    fn create(
        &self,
        _builder: Arc<Builder>,
        space_id: SpaceId,
    ) -> BoxFut<'static, K2Result<DynOpStore>> {
        Box::pin(async move {
            let out: DynOpStore =
                Arc::new(Kitsune2MemoryOpStore::<MemOp>::new(space_id));
            Ok(out)
        })
    }
}

/// This is a stub implementation of an op that will be serialized
/// via serde_json (with inefficient encoding of the payload) to be
/// used for testing purposes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TestMemoryOp {
    /// The creation timestamp of this op
    pub created_at: Timestamp,
    /// The data for the op
    pub op_data: Vec<u8>,
}

impl MemoryOp for TestMemoryOp {
    type Error = Infallible;

    fn from_bytes(bytes: Bytes) -> Result<Self, Infallible> {
        Ok(serde_json::from_slice(&bytes)
            .expect("failed to deserialize MemoryOp from bytes"))
    }

    fn to_bytes(&self) -> Result<Bytes, Infallible> {
        Ok(serde_json::to_vec(&self)
            .expect("failed to serialize MemoryOp to bytes")
            .into())
    }

    /// Compute the op id for this op.
    ///
    /// Note that this produces predictable op ids for testing purposes.
    /// It is simply the first 32 bytes of the op data.
    fn compute_op_id(&self) -> OpId {
        let mut value =
            self.op_data.as_slice()[..32.min(self.op_data.len())].to_vec();
        value.resize(32, 0);
        OpId::from(bytes::Bytes::from(value))
    }

    fn created_at(&self) -> Timestamp {
        self.created_at
    }
}

impl TestMemoryOp {
    /// Deserialize a [TestMemoryOp] from bytes.
    pub fn from_bytes(bytes: Bytes) -> Self {
        MemoryOp::from_bytes(bytes).unwrap()
    }

    /// Serialize a [TestMemoryOp] to bytes.
    pub fn to_bytes(&self) -> Bytes {
        MemoryOp::to_bytes(self).unwrap()
    }

    /// Compute the op id for this op.
    ///
    /// Note that this produces predictable op ids for testing purposes.
    /// It is simply the first 32 bytes of the op data.
    pub fn compute_op_id(&self) -> OpId {
        MemoryOp::compute_op_id(self)
    }

    /// Get the creation timestamp of the op.
    pub fn created_at(&self) -> Timestamp {
        self.created_at
    }
}

impl TestMemoryOp {
    /// Create a new [MemoryOp].
    pub fn new(timestamp: Timestamp, payload: Vec<u8>) -> Self {
        Self {
            created_at: timestamp,
            op_data: payload,
        }
    }
}

impl From<Bytes> for TestMemoryOp {
    fn from(value: Bytes) -> Self {
        serde_json::from_slice(&value)
            .expect("failed to deserialize MemoryOp from bytes")
    }
}

impl From<TestMemoryOp> for Bytes {
    fn from(value: TestMemoryOp) -> Self {
        serde_json::to_vec(&value)
            .expect("failed to serialize MemoryOp to bytes")
            .into()
    }
}

/// This is the storage record for an op with computed fields.
///
/// Test data should create [MemoryOp]s and not be aware of this type.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryOpRecord<MemOp> {
    /// The cached id (hash) of the op
    pub op_id: OpId,
    /// The timestamp at which this op was stored by us
    pub stored_at: Timestamp,
    /// The op data
    pub op: MemOp,
}

impl<MemOp: MemoryOp> MemoryOpRecord<MemOp> {
    /// Get the creation timestamp of the op.
    pub fn created_at(&self) -> Timestamp {
        self.op.created_at()
    }
}

impl<MemOp: MemoryOp> MemoryOpRecord<MemOp> {
    /// Create a new [MemoryOpRecord] from bytes.
    pub fn new(value: Bytes) -> Result<Self, MemOp::Error> {
        let inner = MemOp::from_bytes(value)?;
        Ok(Self {
            op_id: inner.compute_op_id(),
            stored_at: Timestamp::now(),
            op: inner,
        })
    }
}

impl From<TestMemoryOp> for StoredOp {
    fn from(value: TestMemoryOp) -> Self {
        StoredOp {
            op_id: value.compute_op_id(),
            created_at: value.created_at,
        }
    }
}

impl From<Op> for TestMemoryOp {
    fn from(value: Op) -> Self {
        value.data.into()
    }
}

/// A trait for op implementations that are stored in the mem op store.
///
/// This trait is used to ensure that all op implementations are compatible with the mem op store.
/// It is also used to ensure that the op implementations are serialized and deserialized correctly.
pub trait MemoryOp:
    Clone + PartialEq + Send + Sync + Into<StoredOp> + std::fmt::Debug + 'static
{
    /// The error for serializing and deserializing the op.
    type Error: std::fmt::Display + std::fmt::Debug + 'static + Send + Sync;

    /// Deserialize an op from bytes.
    fn from_bytes(bytes: Bytes) -> Result<Self, Self::Error>;
    /// Serialize an op to bytes.
    fn to_bytes(&self) -> Result<Bytes, Self::Error>;
    /// Compute the op id for this op.
    fn compute_op_id(&self) -> OpId;
    /// Get the creation timestamp of the op.
    fn created_at(&self) -> Timestamp;
}

/// The in-memory op store implementation for Kitsune2.
///
/// Intended for testing only, because it provides no persistence of op data.
#[derive(Debug)]
struct Kitsune2MemoryOpStore<MemOp> {
    _space_id: SpaceId,
    inner: RwLock<Kitsune2MemoryOpStoreInner<MemOp>>,
}

impl<MemOp: MemoryOp> Kitsune2MemoryOpStore<MemOp> {
    /// Create a new [Kitsune2MemoryOpStore].
    pub fn new(space_id: SpaceId) -> Self {
        Self {
            _space_id: space_id,
            inner: Default::default(),
        }
    }
}

impl<MemOp: MemoryOp> std::ops::Deref for Kitsune2MemoryOpStore<MemOp> {
    type Target = RwLock<Kitsune2MemoryOpStoreInner<MemOp>>;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

/// The inner state of a [Kitsune2MemoryOpStore].
#[derive(Debug)]
struct Kitsune2MemoryOpStoreInner<MemOp> {
    /// The stored op data.
    pub op_list: HashMap<OpId, MemoryOpRecord<MemOp>>,
    /// The time slice hashes.
    pub time_slice_hashes: TimeSliceHashStore,
}

impl<MemOp: MemoryOp> Default for Kitsune2MemoryOpStoreInner<MemOp> {
    fn default() -> Self {
        Self {
            op_list: Default::default(),
            time_slice_hashes: Default::default(),
        }
    }
}

impl<MemOp: MemoryOp> OpStore for Kitsune2MemoryOpStore<MemOp> {
    fn process_incoming_ops(
        &self,
        op_list: Vec<Bytes>,
    ) -> BoxFuture<'_, K2Result<Vec<OpId>>> {
        Box::pin(async move {
            let ops_to_add = op_list
                .iter()
                .map(|op| -> Result<(OpId, MemoryOpRecord<MemOp>), MemOp::Error> {
                    let op = MemOp::from_bytes(op.clone())?;
                    let op_id = op.compute_op_id();
                    let record = MemoryOpRecord {
                        op_id: op_id.clone(),
                        stored_at: Timestamp::now(),
                        op,
                    };
                    Ok((op_id, record))
                })
                .collect::<Result<Vec<_>, _>>().map_err(|e| {
                K2Error::other("Failed to deserialize op data, are you using `Kitsune2MemoryOp`s?")
            })?;

            let mut op_ids = Vec::with_capacity(ops_to_add.len());
            let mut lock = self.write().await;
            for (op_id, record) in ops_to_add {
                lock.op_list.entry(op_id.clone()).or_insert(record);
                op_ids.push(op_id);
            }

            Ok(op_ids)
        })
    }

    fn retrieve_op_hashes_in_time_slice(
        &self,
        arc: DhtArc,
        start: Timestamp,
        end: Timestamp,
    ) -> BoxFuture<'_, K2Result<(Vec<OpId>, u32)>> {
        Box::pin(async move {
            let self_lock = self.read().await;

            let mut used_bytes = 0;
            let mut candidate_ops = self_lock
                .op_list
                .iter()
                .filter(|(_, record)| {
                    let loc = record.op.compute_op_id().loc();
                    record.created_at() >= start
                        && record.created_at() < end
                        && arc.contains(loc)
                })
                .collect::<Vec<_>>();
            candidate_ops.sort_by_key(|a| a.1.created_at());

            Ok((
                candidate_ops
                    .iter()
                    .map(|(op_id, record)| {
                        used_bytes += record
                            .op
                            .to_bytes()
                            .map(|b| b.len() as u32)
                            .unwrap_or(0);
                        (*op_id).clone()
                    })
                    .collect(),
                used_bytes,
            ))
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
                    self_lock.op_list.get(op_id).map(|record| MetaOp {
                        op_id: op_id.clone(),
                        op_data: record.op.to_bytes().unwrap().into(),
                    })
                })
                .collect())
        })
    }

    fn filter_out_existing_ops(
        &self,
        op_ids: Vec<OpId>,
    ) -> BoxFuture<'_, K2Result<Vec<OpId>>> {
        Box::pin(async move {
            let self_lock = self.read().await;
            Ok(op_ids
                .into_iter()
                .filter(|op_id| !self_lock.op_list.contains_key(op_id))
                .collect())
        })
    }

    fn retrieve_op_ids_bounded(
        &self,
        arc: DhtArc,
        start: Timestamp,
        limit_bytes: u32,
    ) -> BoxFuture<'_, K2Result<(Vec<OpId>, u32, Timestamp)>> {
        Box::pin(async move {
            let new_start = Timestamp::now();

            let self_lock = self.read().await;

            // Capture all ops that are within the arc and after the start time
            let mut candidate_ops = self_lock
                .op_list
                .values()
                .filter(|op| {
                    arc.contains(op.op_id.loc()) && op.stored_at >= start
                })
                .collect::<Vec<_>>();

            // Sort the ops by the time they were stored
            candidate_ops.sort_by(|a, b| a.stored_at.cmp(&b.stored_at));

            // Now take as many ops as we can up to the limit
            let mut total_bytes = 0;
            let mut last_op_timestamp = None;
            let op_ids = candidate_ops
                .into_iter()
                .take_while(|record| {
                    let data_len = record
                        .op
                        .to_bytes()
                        .map(|b| b.len() as u32)
                        .unwrap_or(0);
                    if total_bytes + data_len <= limit_bytes {
                        total_bytes += data_len;
                        true
                    } else {
                        last_op_timestamp = Some(record.stored_at);
                        false
                    }
                })
                .map(|op| op.op_id.clone())
                .collect();

            Ok((
                op_ids,
                total_bytes,
                if let Some(ts) = last_op_timestamp {
                    ts
                } else {
                    new_start
                },
            ))
        })
    }

    fn earliest_timestamp_in_arc(
        &self,
        arc: DhtArc,
    ) -> BoxFuture<'_, K2Result<Option<Timestamp>>> {
        Box::pin(async move {
            Ok(self
                .read()
                .await
                .op_list
                .iter()
                .filter_map(|(_, op)| {
                    if arc.contains(op.op_id.loc()) {
                        Some(op.created_at())
                    } else {
                        None
                    }
                })
                .min())
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
        slice_index: u64,
        slice_hash: bytes::Bytes,
    ) -> BoxFuture<'_, K2Result<()>> {
        Box::pin(async move {
            self.write().await.time_slice_hashes.insert(
                arc,
                slice_index,
                slice_hash,
            )
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
        slice_index: u64,
    ) -> BoxFuture<'_, K2Result<Option<bytes::Bytes>>> {
        Box::pin(async move {
            Ok(self.read().await.time_slice_hashes.get(&arc, slice_index))
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
