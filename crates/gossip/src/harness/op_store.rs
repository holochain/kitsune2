use bytes::Bytes;
use kitsune2_api::{
    BoxFut, Builder, Config, DhtArc, DynOpStore, K2Error, K2Result, MetaOp,
    OpId, OpStore, OpStoreFactory, SpaceId, Timestamp,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use tokio::sync::RwLock;

/// The inner state for the [K2GossipMemoryOpStore].
///
/// This can be treated as the single op store for a Kitsune2 instance.
pub type GossipOpStore = Arc<RwLock<Kitsune2MemoryOpStoreInner>>;

/// The mem op store implementation provided by Kitsune2.
#[derive(Debug)]
pub struct K2GossipMemOpStoreFactory {
    pub(crate) store: GossipOpStore,
}

impl OpStoreFactory for K2GossipMemOpStoreFactory {
    fn default_config(&self, _config: &mut Config) -> K2Result<()> {
        Ok(())
    }

    fn validate_config(&self, _config: &Config) -> K2Result<()> {
        Ok(())
    }

    fn create(
        &self,
        _builder: Arc<Builder>,
        _space: SpaceId,
    ) -> BoxFut<'static, K2Result<DynOpStore>> {
        let inner = self.store.clone();
        Box::pin(async move {
            let out: DynOpStore = Arc::new(K2GossipMemoryOpStore { inner });
            Ok(out)
        })
    }
}

/// This is the storage record for an op with computed fields.
///
/// Test data should create [kitsune2_core::factories::MemoryOp]s and not be aware of this type.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryOpRecord {
    /// The id (hash) of the op
    pub op_id: OpId,
    /// The creation timestamp of this op
    pub created_at: Timestamp,
    /// The timestamp at which this op was stored by us
    pub stored_at: Timestamp,
    /// The data for the op
    pub op_data: Vec<u8>,
    /// Whether this op has been processed yet.
    ///
    /// This is used to track whether the op has been passed to the DHT model.
    pub processed: bool,
}

impl From<Bytes> for MemoryOpRecord {
    fn from(value: Bytes) -> Self {
        let inner: kitsune2_core::factories::MemoryOp = value.into();
        Self {
            op_id: inner.compute_op_id(),
            created_at: inner.created_at,
            stored_at: Timestamp::now(),
            op_data: inner.op_data,
            processed: false,
        }
    }
}

#[derive(Debug)]
struct K2GossipMemoryOpStore {
    inner: Arc<RwLock<Kitsune2MemoryOpStoreInner>>,
}

impl std::ops::Deref for K2GossipMemoryOpStore {
    type Target = RwLock<Kitsune2MemoryOpStoreInner>;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

/// The inner state of a [K2GossipMemoryOpStore].
#[derive(Debug, Default)]
pub struct Kitsune2MemoryOpStoreInner {
    /// The stored op data.
    pub op_list: HashMap<OpId, MemoryOpRecord>,
    /// The time slice hashes.
    pub time_slice_hashes: TimeSliceHashStore,
}

impl OpStore for K2GossipMemoryOpStore {
    fn process_incoming_ops(
        &self,
        op_list: Vec<Bytes>,
    ) -> BoxFut<'_, K2Result<Vec<OpId>>> {
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
    ) -> BoxFut<'_, K2Result<(Vec<OpId>, u32)>> {
        Box::pin(async move {
            let self_lock = self.read().await;

            let mut used_bytes = 0;
            let mut candidate_ops = self_lock
                .op_list
                .iter()
                .filter(|(_, op)| {
                    let loc = op.op_id.loc();
                    op.created_at >= start
                        && op.created_at < end
                        && arc.contains(loc)
                })
                .collect::<Vec<_>>();
            candidate_ops.sort_by_key(|a| a.1.created_at);

            Ok((
                candidate_ops
                    .iter()
                    .map(|(op_id, record)| {
                        used_bytes += record.op_data.len() as u32;
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
    ) -> BoxFut<'_, K2Result<Vec<MetaOp>>> {
        Box::pin(async move {
            let self_lock = self.read().await;
            Ok(op_ids
                .iter()
                .filter_map(|op_id| {
                    self_lock.op_list.get(op_id).map(|op| MetaOp {
                        op_id: op.op_id.clone(),
                        op_data: kitsune2_core::factories::MemoryOp {
                            created_at: op.created_at,
                            op_data: op.op_data.clone(),
                        }
                        .into(),
                    })
                })
                .collect())
        })
    }

    fn filter_out_existing_ops(
        &self,
        op_ids: Vec<OpId>,
    ) -> BoxFut<'_, K2Result<Vec<OpId>>> {
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
    ) -> BoxFut<'_, K2Result<(Vec<OpId>, u32, Timestamp)>> {
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
                .take_while(|op| {
                    let data_len = op.op_data.len() as u32;
                    if total_bytes + data_len <= limit_bytes {
                        total_bytes += data_len;
                        true
                    } else {
                        last_op_timestamp = Some(op.stored_at);
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
    ) -> BoxFut<'_, K2Result<Option<Timestamp>>> {
        Box::pin(async move {
            Ok(self
                .read()
                .await
                .op_list
                .iter()
                .filter_map(|(_, op)| {
                    if arc.contains(op.op_id.loc()) {
                        Some(op.created_at)
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
    ) -> BoxFut<'_, K2Result<()>> {
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
    fn slice_hash_count(&self, arc: DhtArc) -> BoxFut<'_, K2Result<u64>> {
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
    ) -> BoxFut<'_, K2Result<Option<bytes::Bytes>>> {
        Box::pin(async move {
            Ok(self.read().await.time_slice_hashes.get(&arc, slice_index))
        })
    }

    /// Retrieve the hashes of all time slices.
    fn retrieve_slice_hashes(
        &self,
        arc: DhtArc,
    ) -> BoxFut<'_, K2Result<Vec<(u64, bytes::Bytes)>>> {
        Box::pin(async move {
            let self_lock = self.read().await;
            Ok(self_lock.time_slice_hashes.get_all(&arc))
        })
    }
}

#[derive(Debug, Default)]
#[cfg_attr(test, derive(Clone))]
pub struct TimeSliceHashStore {
    inner: HashMap<DhtArc, BTreeMap<u64, Bytes>>,
}

impl TimeSliceHashStore {
    /// Insert a hash at the given slice id.
    pub fn insert(
        &mut self,
        arc: DhtArc,
        slice_id: u64,
        hash: Bytes,
    ) -> K2Result<()> {
        if hash.is_empty() {
            return Err(K2Error::other("Cannot insert empty combined hash"));
        }

        self.inner.entry(arc).or_default().insert(slice_id, hash);

        Ok(())
    }

    pub fn get(&self, arc: &DhtArc, slice_id: u64) -> Option<Bytes> {
        self.inner
            .get(arc)
            .and_then(|by_arc| by_arc.get(&slice_id))
            .cloned()
    }

    pub fn get_all(&self, arc: &DhtArc) -> Vec<(u64, Bytes)> {
        self.inner
            .get(arc)
            .map(|by_arc| {
                by_arc
                    .iter()
                    .map(|(id, hash)| (*id, hash.clone()))
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn highest_stored_id(&self, arc: &DhtArc) -> Option<u64> {
        self.inner
            .get(arc)
            .and_then(|by_arc| by_arc.iter().last().map(|(id, _)| *id))
    }
}
