//! The mem op store implementation provided by Kitsune2.

use crate::factories::mem_op_store::time_slice_hash_store::TimeSliceHashStore;
use bytes::Bytes;
use futures::future::BoxFuture;
use kitsune2_api::builder::Builder;
use kitsune2_api::config::Config;
use kitsune2_api::{
    BoxFut, DhtArc, DynOpStore, DynOpStoreFactory, K2Error, K2Result, MetaOp,
    OpId, OpStore, OpStoreFactory, SpaceId, StoredOp, Timestamp,
    UNIX_TIMESTAMP,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

mod time_slice_hash_store;

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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Kitsune2MemoryOp {
    /// The id (hash) of the op
    pub op_id: OpId,
    /// The creation timestamp of this op
    pub timestamp: Timestamp,
    /// The timestamp at which this op was stored by us
    pub stored_at: Timestamp,
    /// The payload of the op
    pub payload: Vec<u8>,
}

impl Kitsune2MemoryOp {
    /// Create a new [Kitsune2MemoryOp]
    pub fn new(op_id: OpId, timestamp: Timestamp, payload: Vec<u8>) -> Self {
        Self {
            op_id,
            timestamp,
            stored_at: UNIX_TIMESTAMP,
            payload,
        }
    }
}

impl From<Kitsune2MemoryOp> for StoredOp {
    fn from(value: Kitsune2MemoryOp) -> Self {
        StoredOp {
            op_id: value.op_id,
            timestamp: value.timestamp,
        }
    }
}

impl From<Bytes> for Kitsune2MemoryOp {
    fn from(value: Bytes) -> Self {
        serde_json::from_slice(&value)
            .expect("failed to deserialize Kitsune2MemoryOp from Op")
    }
}

impl From<Kitsune2MemoryOp> for Bytes {
    fn from(value: Kitsune2MemoryOp) -> Self {
        serde_json::to_vec(&value)
            .expect("failed to serialize Op to Kitsune2MemoryOp")
            .into()
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
    op_list: HashMap<OpId, Kitsune2MemoryOp>,
    time_slice_hashes: TimeSliceHashStore,
}

impl OpStore for Kitsune2MemoryOpStore {
    fn process_incoming_ops(
        &self,
        op_list: Vec<Bytes>,
    ) -> BoxFuture<'_, K2Result<()>> {
        Box::pin(async move {
            let ops_to_add = op_list
                .iter()
                .map(|op| -> serde_json::Result<(OpId, Kitsune2MemoryOp)> {
                    let mut op = Kitsune2MemoryOp::from(op.clone());
                    op.stored_at = Timestamp::now();
                    Ok((op.op_id.clone(), op))
                })
                .collect::<Result<Vec<_>, _>>().map_err(|e| {
                K2Error::other_src("Failed to deserialize op data, are you using `Kitsune2MemoryOp`s?", e)
            })?;

            self.write().await.op_list.extend(ops_to_add);
            Ok(())
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
                    op.timestamp >= start
                        && op.timestamp < end
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
                        op_data: serde_json::to_vec(op)
                            .expect("Failed to serialize op"),
                    })
                })
                .collect())
        })
    }

    fn retrieve_op_ids_bounded(
        &self,
        start: Timestamp,
        limit_bytes: usize,
    ) -> BoxFuture<'_, K2Result<(Vec<OpId>, Timestamp)>> {
        Box::pin(async move {
            let new_start = Timestamp::now();

            let self_lock = self.read().await;

            // Capture all ops that are after the start time
            let mut candidate_ops = self_lock
                .op_list
                .values()
                .filter(|op| op.stored_at >= start)
                .collect::<Vec<_>>();

            tracing::info!(
                "Have {} candidate ops of {} possible",
                candidate_ops.len(),
                self_lock.op_list.len()
            );

            // Sort the ops by the time they were stored
            candidate_ops.sort_by(|a, b| a.stored_at.cmp(&b.stored_at));

            // Now take as many ops as we can up to the limit
            let mut total_bytes = 0;
            let mut last_op_timestamp = None;
            let op_ids = candidate_ops
                .into_iter()
                .take_while(|op| {
                    total_bytes += op.payload.len();

                    if total_bytes <= limit_bytes {
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
                if let Some(ts) = last_op_timestamp {
                    ts
                } else {
                    new_start
                },
            ))
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
