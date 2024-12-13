//! Kitsune2 fetch types.

use std::sync::Arc;

use crate::{
    builder, config, peer_store::DynPeerStore, transport::DynTransport,
    AgentId, BoxFut, K2Result, OpId, SpaceId,
};

include!("../proto/gen/kitsune2.fetch.rs");

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

    #[test]
    fn happy_encode_decode() {
        use prost::Message;
        let op_id_1 = OpId(Id(bytes::Bytes::from_static(b"some_op_id")));
        let op_id_2 = OpId(Id(bytes::Bytes::from_static(b"another_op_id")));
        let op_ids = vec![op_id_1, op_id_2];
        let op_id_bytes = op_ids
            .into_iter()
            .map(|op_id| op_id.0 .0)
            .collect::<Vec<_>>();

        let ops = Ops {
            ids: op_id_bytes.clone(),
        };

        let op_ids_enc = ops.encode_to_vec();
        let op_ids_dec = Ops::decode(op_ids_enc.as_slice()).unwrap();

        assert_eq!(op_id_bytes, op_ids_dec.ids);
    }
}
