//! Kitsune2 op store types.

use crate::{
    BoxFut, DhtArc, K2Result, OpId, SpaceId, Timestamp, builder, config,
};
use bytes::Bytes;
use futures::future::BoxFuture;
#[cfg(feature = "mockall")]
use mockall::automock;
use std::cmp::Ordering;
use std::sync::Arc;

pub(crate) mod proto {
    include!("../proto/gen/kitsune2.op_store.rs");
}

pub use proto::Op;

/// An op with metadata.
///
/// This is the basic unit of data in the kitsune2 system.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct MetaOp {
    /// The id of the op.
    pub op_id: OpId,

    /// The actual op data.
    pub op_data: Bytes,
}

impl From<Bytes> for Op {
    fn from(value: Bytes) -> Self {
        Self { data: value }
    }
}

/// An op that has been stored by the Kitsune host.
///
/// This is the basic unit of data that the host is expected to store. Whether that storage is
/// persistent may depend on the use-case. While Kitsune is holding references by hash, the host
/// is expected to store the actual data.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct StoredOp {
    /// The id of the op.
    pub op_id: OpId,

    /// The creation timestamp of the op.
    ///
    /// This must be the same for everyone who sees this op.
    ///
    /// Note that this means any op implementation must include a consistent timestamp in the op
    /// data so that it can be provided back to Kitsune.
    pub created_at: Timestamp,
}

impl Ord for StoredOp {
    fn cmp(&self, other: &Self) -> Ordering {
        (&self.created_at, &self.op_id).cmp(&(&other.created_at, &other.op_id))
    }
}

impl PartialOrd for StoredOp {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// The API that a kitsune2 host must implement to provide data persistence for kitsune2.
#[cfg_attr(any(test, feature = "mockall"), automock)]
pub trait OpStore: 'static + Send + Sync + std::fmt::Debug {
    /// Process incoming ops.
    ///
    /// Pass the incoming ops to the host for processing. The host is expected to store the ops
    /// if it is able to process them.
    fn process_incoming_ops(
        &self,
        op_list: Vec<Bytes>,
    ) -> BoxFuture<'_, K2Result<Vec<OpId>>>;

    /// Retrieve a batch of ops from the host by time range.
    ///
    /// This must be the timestamp of the op, not the time that we saw the op or chose to store it.
    /// The returned ops must be ordered by timestamp, ascending.
    ///
    /// # Returns
    ///
    /// - As many op ids as can be returned within the `limit_bytes` limit, within the arc and time
    ///   bounds.
    /// - The total size of the op data that is pointed to by the returned op ids.
    fn retrieve_op_hashes_in_time_slice(
        &self,
        arc: DhtArc,
        start: Timestamp,
        end: Timestamp,
    ) -> BoxFuture<'_, K2Result<(Vec<OpId>, u32)>>;

    /// Retrieve a list of ops by their op ids.
    ///
    /// This should be used to get op data for ops.
    fn retrieve_ops(
        &self,
        op_ids: Vec<OpId>,
    ) -> BoxFuture<'_, K2Result<Vec<MetaOp>>>;

    /// Retrieve a size-bounded list of op ids that have been stored within the given `arc` since
    /// `start`.
    ///
    /// The `start` timestamp is used to retrieve ops by their `stored_at` timestamp rather than
    /// their creation timestamp. This means that the `start` value can be used to page an op
    /// store.
    ///
    /// The `limit_bytes` applies to the size of the op data, not the size of the op ids. This can
    /// be thought of as a "page size" for the op data. Where the size is the size of the data
    /// rather than the number of ops.
    ///
    /// If the limit is applied, then the timestamp of the last op id is returned.
    /// Otherwise, the timestamp for when this operation started is returned.
    /// Either way, the returned timestamp should be used as the `start` value for the next call
    /// to this op store.
    ///
    /// # Returns
    ///
    /// - As many op ids as can be returned within the `limit_bytes` limit.
    /// - The total size of the op data that is pointed to by the returned op ids.
    /// - A new timestamp to be used for the next query.
    fn retrieve_op_ids_bounded(
        &self,
        arc: DhtArc,
        start: Timestamp,
        limit_bytes: u32,
    ) -> BoxFuture<'_, K2Result<(Vec<OpId>, u32, Timestamp)>>;

    /// Store the combined hash of a time slice.
    fn store_slice_hash(
        &self,
        arc: DhtArc,
        slice_index: u64,
        slice_hash: Bytes,
    ) -> BoxFuture<'_, K2Result<()>>;

    /// Count the number of stored time slices.
    fn slice_hash_count(&self, arc: DhtArc) -> BoxFuture<'_, K2Result<u64>>;

    /// Retrieve the combined hash of a time slice.
    ///
    /// If the slice is not found, return None.
    /// If the time slice is present, then it must be identical to what was stored by Kitsune2
    /// using [Self::store_slice_hash].
    fn retrieve_slice_hash(
        &self,
        arc: DhtArc,
        slice_index: u64,
    ) -> BoxFuture<'_, K2Result<Option<Bytes>>>;

    /// Retrieve all slice hashes for a given arc.
    fn retrieve_slice_hashes(
        &self,
        arc: DhtArc,
    ) -> BoxFuture<'_, K2Result<Vec<(u64, Bytes)>>>;
}

/// Trait-object [OpStore].
pub type DynOpStore = Arc<dyn OpStore>;

/// A factory for constructing [OpStore] instances.
pub trait OpStoreFactory: 'static + Send + Sync + std::fmt::Debug {
    /// Help the builder construct a default config from the chosen
    /// module factories.
    fn default_config(&self, config: &mut config::Config) -> K2Result<()>;

    /// Validate configuration.
    fn validate_config(&self, config: &config::Config) -> K2Result<()>;

    /// Construct an op store instance.
    fn create(
        &self,
        builder: Arc<builder::Builder>,
        space: SpaceId,
    ) -> BoxFut<'static, K2Result<DynOpStore>>;
}

/// Trait-object [OpStoreFactory].
pub type DynOpStoreFactory = Arc<dyn OpStoreFactory>;

#[cfg(test)]
mod test {
    use super::*;
    use prost::Message;

    #[test]
    fn happy_meta_op_encode_decode() {
        let op = Op::from(Bytes::from(vec![1; 128]));
        let op_enc = op.encode_to_vec();
        let op_dec = Op::decode(op_enc.as_slice()).unwrap();

        assert_eq!(op_dec, op);
    }
}
