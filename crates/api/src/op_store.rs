//! Kitsune2 op store types.

use crate::{K2Result, OpId, Timestamp};
use futures::future::BoxFuture;
use std::cmp::Ordering;
use std::sync::Arc;

/// An op with metadata.
///
/// This is the basic unit of data in the kitsune2 system.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct MetaOp {
    /// The id of the op.
    pub op_id: OpId,

    /// The creation timestamp of the op.
    ///
    /// This must be the same for everyone who sees this op.
    ///
    /// The host must reject the op if the timestamp does not agree with any timestamps inside the
    /// op data.
    pub timestamp: Timestamp,

    /// The actual op data.
    pub op_data: Vec<u8>,

    /// Crdt-style add-only opaque implementor-use flags.
    pub op_flags: std::collections::HashSet<String>,
}

impl Ord for MetaOp {
    fn cmp(&self, other: &Self) -> Ordering {
        (&self.timestamp, &self.op_id).cmp(&(&other.timestamp, &other.op_id))
    }
}

impl PartialOrd for MetaOp {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// The response to a request for a bounded list of op hashes.
///
/// See [`OpStore::retrieve_op_hashes`].
pub struct BoundedOpHashResponse {
    /// The list of op ids.
    pub op_ids: Vec<OpId>,

    /// The timestamp of the last op in the list.
    pub last_timestamp: Option<Timestamp>,
}

impl BoundedOpHashResponse {
    /// Create an empty response.
    pub fn empty() -> Self {
        Self {
            op_ids: Vec::with_capacity(0),
            last_timestamp: None,
        }
    }
}

/// The API that a kitsune2 host must implement to provide data persistence for kitsune2.
pub trait OpStore: 'static + Send + Sync + std::fmt::Debug {
    /// Process incoming ops.
    fn ingest_op_list(
        &self,
        op_list: Vec<MetaOp>,
    ) -> BoxFuture<'_, K2Result<()>>;

    /// Retrieve a batch of op ids from the host.
    ///
    /// The implementor is required to:
    /// - Query a sorted list of ops
    /// - Returns ops with a timestamp >= the provided timestamp
    /// - Return as many ops as can fit within the provided limit_bytes, but no more.
    ///
    /// The return value is a tuple of the op ids and the timestamp of the last op in the batch.
    /// A timestamp must be provided if the list is not empty.
    fn retrieve_op_hashes(
        &self,
        timestamp: Timestamp,
        limit_bytes: u32,
    ) -> BoxFuture<'_, K2Result<BoundedOpHashResponse>>;

    /// Retrieve a batch of ops from the host by time range.
    fn retrieve_op_hashes_in_time_slice(
        &self,
        start: Timestamp,
        end: Timestamp,
    ) -> BoxFuture<'_, K2Result<Vec<OpId>>>;

    /// Store the hash of a time slice.
    fn store_slice_hash(
        &self,
        slice_id: u64,
        slice_hash: Vec<u8>,
    ) -> BoxFuture<'_, K2Result<()>>;

    ///
    fn slice_hash_count(
        &self,
    ) -> BoxFuture<'_, K2Result<u64>>;

    /// Retrieve the hash of a time slice.
    fn retrieve_slice_hash(
        &self,
        slice_id: u64,
    ) -> BoxFuture<'_, K2Result<Option<Vec<u8>>>>;
}

/// Trait-object version of kitsune2 op store.
pub type DynOpStore = Arc<dyn OpStore>;
