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

/// The API that a kitsune2 host must implement to provide data persistence for kitsune2.
pub trait OpStore: 'static + Send + Sync + std::fmt::Debug {
    /// Process incoming ops.
    ///
    /// Pass the incoming ops to the host for processing. The host is expected to store the ops
    /// if it is able to process them.
    fn process_incoming_ops(
        &self,
        op_list: Vec<MetaOp>,
    ) -> BoxFuture<'_, K2Result<()>>;

    /// Retrieve a batch of ops from the host by time range.
    ///
    /// This must be the timestamp of the op, not the time that we saw the op or chose to store it.
    /// The returned ops must be ordered by timestamp, ascending.
    fn retrieve_op_hashes_in_time_slice(
        &self,
        start: Timestamp,
        end: Timestamp,
    ) -> BoxFuture<'_, K2Result<Vec<OpId>>>;

    /// Store the combined hash of a time slice.
    fn store_slice_hash(
        &self,
        slice_id: u64,
        slice_hash: Vec<u8>,
    ) -> BoxFuture<'_, K2Result<()>>;

    /// Count the number of stored time slices.
    fn slice_hash_count(&self) -> BoxFuture<'_, K2Result<u64>>;

    /// Retrieve the combined hash of a time slice.
    ///
    /// If the slice is not found, return None.
    /// If the time slice is present, then it must be identical to what was stored by Kitsune2
    /// using [Self::store_slice_hash].
    fn retrieve_slice_hash(
        &self,
        slice_id: u64,
    ) -> BoxFuture<'_, K2Result<Option<Vec<u8>>>>;
}

/// Trait-object version of kitsune2 op store.
pub type DynOpStore = Arc<dyn OpStore>;
