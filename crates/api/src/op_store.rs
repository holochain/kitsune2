//! Kitsune2 op store types.

use crate::{
    builder, config, BoxFut, DhtArc, K2Result, OpId, SpaceId, Timestamp,
};
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

    /// The actual op data.
    pub op_data: Vec<u8>,
}

include!("../proto/gen/kitsune2.op_store.rs");

impl From<MetaOp> for Op {
    fn from(value: MetaOp) -> Self {
        Self {
            data: value.op_data.into(),
        }
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
    pub timestamp: Timestamp,
}

impl Ord for StoredOp {
    fn cmp(&self, other: &Self) -> Ordering {
        (&self.timestamp, &self.op_id).cmp(&(&other.timestamp, &other.op_id))
    }
}

impl PartialOrd for StoredOp {
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
        arc: DhtArc,
        start: Timestamp,
        end: Timestamp,
    ) -> BoxFuture<'_, K2Result<Vec<OpId>>>;

    /// Retrieve a list of ops by their op ids.
    ///
    /// This should be used to get op data for ops.
    fn retrieve_ops(
        &self,
        op_ids: Vec<OpId>,
    ) -> BoxFuture<'_, K2Result<Vec<MetaOp>>>;

    /// Store the combined hash of a time slice.
    fn store_slice_hash(
        &self,
        arc: DhtArc,
        slice_id: u64,
        slice_hash: bytes::Bytes,
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
        slice_id: u64,
    ) -> BoxFuture<'_, K2Result<Option<bytes::Bytes>>>;

    /// Retrieve all slice hashes for a given arc.
    fn retrieve_slice_hashes(
        &self,
        arc: DhtArc,
    ) -> BoxFuture<'_, K2Result<Vec<(u64, bytes::Bytes)>>>;
}

/// Trait-object version of kitsune2 op store.
pub type DynOpStore = Arc<dyn OpStore>;

/// A factory for constructing [OpStore] instances.
pub trait OpStoreFactory: 'static + Send + Sync + std::fmt::Debug {
    /// Help the builder construct a default config from the chosen
    /// module factories.
    fn default_config(&self, config: &mut config::Config) -> K2Result<()>;

    /// Construct an op store instance.
    fn create(
        &self,
        builder: Arc<builder::Builder>,
        space: SpaceId,
    ) -> BoxFut<'static, K2Result<DynOpStore>>;
}

/// Trait-object [crate::bootstrap::BootstrapFactory].
pub type DynOpStoreFactory = Arc<dyn OpStoreFactory>;

#[cfg(test)]
mod test {
    use crate::MetaOp;

    use super::*;
    use prost::Message;

    #[test]
    fn happy_meta_op_encode_decode() {
        let meta_op = MetaOp {
            op_id: OpId::from(bytes::Bytes::from_static(b"some_op_id")),
            op_data: vec![1; 128],
        };
        let op = Op::from(meta_op);
        let op_enc = op.encode_to_vec();
        let op_dec = Op::decode(op_enc.as_slice()).unwrap();

        assert_eq!(op_dec, op);
    }
}
