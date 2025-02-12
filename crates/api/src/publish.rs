//! Kitsune2 publish types.

use crate::{builder, config};
use crate::{
    transport::DynTransport, BoxFut, DynFetch, K2Result, OpId, SpaceId, Url,
};
use bytes::{Bytes, BytesMut};
use prost::Message;
use std::sync::Arc;

pub(crate) mod proto {
    include!("../proto/gen/kitsune2.publish.rs");
}

pub use proto::{k2_publish_message::*, K2PublishMessage, PublishOps};

impl From<Vec<OpId>> for PublishOps {
    fn from(value: Vec<OpId>) -> Self {
        Self {
            op_ids: value.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<PublishOps> for Vec<OpId> {
    fn from(value: PublishOps) -> Self {
        value.op_ids.into_iter().map(Into::into).collect()
    }
}

/// Serialize list of op ids to request.
pub fn serialize_publish_ops(value: Vec<OpId>) -> Bytes {
    let mut out = BytesMut::new();
    PublishOps::from(value)
        .encode(&mut out)
        .expect("failed to encode publish ops request");
    out.freeze()
}

/// Serialize list of op ids to fetch request message.
pub fn serialize_publish_ops_message(value: Vec<OpId>) -> Bytes {
    let mut out = BytesMut::new();
    let data = serialize_publish_ops(value);
    let publish_message = K2PublishMessage {
        publish_message_type: PublishMessageType::Ops.into(),
        data,
    };
    publish_message
        .encode(&mut out)
        .expect("failed to encode publish ops message");
    out.freeze()
}

/// Trait for implementing a publish module to publish ops to other peers.
pub trait Publish: 'static + Send + Sync + std::fmt::Debug {
    /// Add op ids to be published to a peer.
    fn publish_ops(
        &self,
        op_ids: Vec<OpId>,
        target: Url,
    ) -> BoxFut<'_, K2Result<()>>;
}

/// Trait object [Publish].
pub type DynPublish = Arc<dyn Publish>;

/// A factory for creating Publish instances.
pub trait PublishFactory: 'static + Send + Sync + std::fmt::Debug {
    /// Help the builder construct a default config from the chosen
    /// module factories.
    fn default_config(&self, config: &mut config::Config) -> K2Result<()>;

    /// Validate configuration.
    fn validate_config(&self, config: &config::Config) -> K2Result<()>;

    /// Construct a Publish instance.
    fn create(
        &self,
        builder: Arc<builder::Builder>,
        space_id: SpaceId,
        fetch: DynFetch,
        transport: DynTransport,
    ) -> BoxFut<'static, K2Result<DynPublish>>;
}

/// Trait object [PublishFactory].
pub type DynPublishFactory = Arc<dyn PublishFactory>;

#[cfg(test)]
mod test {
    use super::*;
    use crate::id::Id;
    use prost::Message;

    #[test]
    fn happy_publish_ops_encode_decode() {
        let op_id_1 = OpId::from(Bytes::from_static(b"some_op_id"));
        let op_id_2 = OpId::from(Bytes::from_static(b"another_op_id"));
        let op_id_vec = vec![op_id_1, op_id_2];
        let op_ids = PublishOps::from(op_id_vec.clone());

        let op_ids_enc = op_ids.encode_to_vec();
        let op_ids_dec = PublishOps::decode(op_ids_enc.as_slice()).unwrap();
        let op_ids_dec_vec = Vec::from(op_ids_dec.clone());

        assert_eq!(op_ids, op_ids_dec);
        assert_eq!(op_id_vec, op_ids_dec_vec);
    }

    #[test]
    fn happy_publish_ops_message_encode_decode() {
        let op_id = OpId(Id(bytes::Bytes::from_static(b"id_1")));
        let op_ids = vec![op_id];
        let publish_ops = serialize_publish_ops_message(op_ids.clone());

        let publish_ops_message_dec =
            K2PublishMessage::decode(publish_ops).unwrap();
        assert_eq!(
            publish_ops_message_dec.publish_message_type,
            i32::from(PublishMessageType::Ops)
        );
        let request_dec =
            PublishOps::decode(publish_ops_message_dec.data).unwrap();
        let op_ids_dec = request_dec
            .op_ids
            .into_iter()
            .map(Into::<OpId>::into)
            .collect::<Vec<_>>();
        assert_eq!(op_ids, op_ids_dec);
    }
}
