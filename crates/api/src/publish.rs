//! Kitsune2 publish types.

use crate::{
    AgentInfoSigned, DynPeerMetaStore, DynPeerStore, K2Error, builder, config,
};
use crate::{
    BoxFut, DynFetch, K2Result, OpId, SpaceId, Url, transport::DynTransport,
};
use bytes::{Bytes, BytesMut};
use prost::Message;
use std::sync::Arc;

pub(crate) mod proto {
    include!("../proto/gen/kitsune2.publish.rs");
}

pub use proto::{
    K2PublishMessage, PublishAgent, PublishOpEntry, PublishOps,
    k2_publish_message::*,
};

/// A single op to be published, with optional host-supplied metadata.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct PublishOp {
    /// The ID of the op.
    pub op_id: OpId,

    /// Optional metadata to carry alongside the op through the fetch queue to the op store.
    pub metadata: Option<Bytes>,
}

impl From<Vec<PublishOp>> for PublishOps {
    fn from(value: Vec<PublishOp>) -> Self {
        Self {
            entries: value
                .into_iter()
                .map(|op| PublishOpEntry {
                    op_id: op.op_id.into(),
                    metadata: op.metadata,
                })
                .collect(),
        }
    }
}

impl From<PublishOps> for Vec<PublishOp> {
    fn from(value: PublishOps) -> Self {
        value
            .entries
            .into_iter()
            .map(|entry| PublishOp {
                op_id: OpId::from(entry.op_id),
                metadata: entry.metadata,
            })
            .collect()
    }
}

/// Serialize a list of publish ops.
fn serialize_publish_ops(ops: Vec<PublishOp>) -> Bytes {
    let mut out = BytesMut::new();
    PublishOps::from(ops)
        .encode(&mut out)
        .expect("failed to encode publish ops request");
    out.freeze()
}

/// Serialize a list of publish ops into a publish ops wire message.
pub fn serialize_publish_ops_message(ops: Vec<PublishOp>) -> Bytes {
    let mut out = BytesMut::new();
    let data = serialize_publish_ops(ops);
    let publish_message = K2PublishMessage {
        publish_message_type: PublishMessageType::Ops.into(),
        data,
    };
    publish_message
        .encode(&mut out)
        .expect("failed to encode publish ops message");
    out.freeze()
}

impl TryFrom<&Arc<AgentInfoSigned>> for PublishAgent {
    type Error = K2Error;

    fn try_from(value: &Arc<AgentInfoSigned>) -> K2Result<Self> {
        let agent_info_encoded = value.encode()?;
        Ok(Self {
            agent_info: agent_info_encoded,
        })
    }
}

/// Serialize AgentInfoSigned
pub fn serialize_publish_agent(
    value: &Arc<AgentInfoSigned>,
) -> K2Result<Bytes> {
    let mut out = BytesMut::new();
    PublishAgent::try_from(value)?
        .encode(&mut out)
        .expect("failed to encode publish agent request");
    Ok(out.freeze())
}

/// Serialize agent publish message.
pub fn serialize_publish_agent_message(
    value: &Arc<AgentInfoSigned>,
) -> K2Result<Bytes> {
    let mut out = BytesMut::new();
    let data = serialize_publish_agent(value)?;
    let publish_message = K2PublishMessage {
        publish_message_type: PublishMessageType::Agent.into(),
        data,
    };
    publish_message
        .encode(&mut out)
        .expect("failed to encode publish agent message");
    Ok(out.freeze())
}

/// Trait for implementing a publish module to publish ops to other peers.
pub trait Publish: 'static + Send + Sync + std::fmt::Debug {
    /// Add ops to be published to a peer.
    ///
    /// Returns an error if any op's metadata exceeds the configured limit.
    fn publish_ops(
        &self,
        ops: Vec<PublishOp>,
        target: Url,
    ) -> BoxFut<'_, K2Result<()>>;

    /// Add agent info to be published to a peer.
    fn publish_agent(
        &self,
        agent_info: Arc<AgentInfoSigned>,
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
        peer_store: DynPeerStore,
        peer_meta_store: DynPeerMetaStore,
        transport: DynTransport,
    ) -> BoxFut<'static, K2Result<DynPublish>>;
}

/// Trait object [PublishFactory].
pub type DynPublishFactory = Arc<dyn PublishFactory>;

#[cfg(test)]
mod test {
    use super::*;
    use prost::Message;

    #[test]
    fn happy_publish_ops_encode_decode() {
        let op_1 = PublishOp {
            op_id: OpId::from(Bytes::from_static(b"some_op_id")),
            metadata: Some(Bytes::from_static(b"meta1")),
        };
        let op_2 = PublishOp {
            op_id: OpId::from(Bytes::from_static(b"another_op_id")),
            metadata: None,
        };
        let ops = vec![op_1, op_2];
        let publish_ops = PublishOps::from(ops.clone());

        let enc = publish_ops.encode_to_vec();
        let dec = PublishOps::decode(enc.as_slice()).unwrap();
        let dec_ops = Vec::<PublishOp>::from(dec);

        assert_eq!(ops, dec_ops);
    }

    #[test]
    fn happy_publish_ops_message_encode_decode() {
        let op = PublishOp {
            op_id: OpId::from(Bytes::from_static(b"id_1")),
            metadata: Some(Bytes::from_static(b"host_meta")),
        };
        let ops = vec![op.clone()];
        let message = serialize_publish_ops_message(ops);

        let decoded = K2PublishMessage::decode(message).unwrap();
        assert_eq!(
            decoded.publish_message_type,
            i32::from(PublishMessageType::Ops)
        );
        let pub_ops = PublishOps::decode(decoded.data).unwrap();
        let decoded_ops = Vec::<PublishOp>::from(pub_ops);
        assert_eq!(vec![op], decoded_ops);
    }
}
