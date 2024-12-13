//! Kitsune2 fetch types.

use std::sync::Arc;

use prost::Message;

use crate::{
    builder, config, id::Id, peer_store::DynPeerStore, transport::DynTransport,
    AgentId, BoxFut, K2Error, K2Result, OpId, SpaceId,
};

include!("../proto/gen/kitsune2.fetch.rs");

impl From<Vec<OpId>> for OpIds {
    fn from(value: Vec<OpId>) -> Self {
        Self {
            data: value.into_iter().map(|op_id| op_id.0 .0).collect(),
        }
    }
}

impl From<OpIds> for Vec<OpId> {
    fn from(value: OpIds) -> Self {
        value.data.into_iter().map(|id| OpId(Id(id))).collect()
    }
}

/// Serialize list of op ids for sending over the wire.
pub fn serialize_op_ids(value: Vec<OpId>) -> bytes::Bytes {
    let bytes = OpIds::from(value).encode_to_vec();
    bytes::Bytes::copy_from_slice(&bytes)
}

/// Deserialize list of op ids.
pub fn deserialize_op_ids(value: bytes::Bytes) -> K2Result<Vec<OpId>> {
    let op_ids = OpIds::decode(value).map_err(K2Error::other)?;
    let vec = Vec::from(op_ids);
    Ok(vec)
}

/// Trait for implementing a fetch module to fetch ops from other agents.
pub trait Fetch: 'static + Send + Sync + std::fmt::Debug {
    /// Add op ids to be fetched.
    fn add_ops(
        &self,
        op_list: Vec<OpId>,
        source: AgentId,
    ) -> BoxFut<'_, K2Result<()>>;
}

/// Trait object [Fetch].
pub type DynFetch = Arc<dyn Fetch>;

/// A factory for creating Fetch instances.
pub trait FetchFactory: 'static + Send + Sync + std::fmt::Debug {
    /// Help the builder construct a default config from the chosen
    /// module factories.
    fn default_config(&self, config: &mut config::Config) -> K2Result<()>;

    /// Construct a Fetch instance.
    fn create(
        &self,
        builder: Arc<builder::Builder>,
        space_id: SpaceId,
        peer_store: DynPeerStore,
        transport: DynTransport,
    ) -> BoxFut<'static, K2Result<DynFetch>>;
}

/// Trait object [FetchFactory].
pub type DynFetchFactory = Arc<dyn FetchFactory>;

#[cfg(test)]
mod test {
    use crate::id::Id;

    use super::*;
    use prost::Message;

    #[test]
    fn happy_encode_decode() {
        let op_id_1 = OpId(Id(bytes::Bytes::from_static(b"some_op_id")));
        let op_id_2 = OpId(Id(bytes::Bytes::from_static(b"another_op_id")));
        let op_id_vec = vec![op_id_1, op_id_2];
        let op_ids = OpIds::from(op_id_vec.clone());

        let op_ids_enc = op_ids.encode_to_vec();
        let op_ids_dec = OpIds::decode(op_ids_enc.as_slice()).unwrap();
        let op_ids_dec_vec = Vec::from(op_ids_dec.clone());

        assert_eq!(op_ids, op_ids_dec);
        assert_eq!(op_id_vec, op_ids_dec_vec);
    }

    #[test]
    fn bytes_from_op_ids() {
        let op_id_1 = OpId(Id(bytes::Bytes::from_static(b"some_op_id")));
        let op_id_2 = OpId(Id(bytes::Bytes::from_static(b"another_op_id")));
        let op_id_vec = vec![op_id_1, op_id_2];
        let bytes = serialize_op_ids(op_id_vec.clone());
        let op_id_vec_deserialized = deserialize_op_ids(bytes.clone()).unwrap();
        assert_eq!(op_id_vec_deserialized, op_id_vec);
    }
}
