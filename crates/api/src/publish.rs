//! Kitsune2 fetch types.

use crate::{agent, builder, AgentInfoMessage, K2Error};
use crate::{
    transport::DynTransport, AgentInfoSigned, BoxFut, DynFetch, DynPeerStore,
    K2Result, OpId, SpaceId, Url,
};
use bytes::{Bytes, BytesMut};
use prost::Message;
use std::sync::Arc;

pub(crate) mod proto {
    include!("../proto/gen/kitsune2.publish.rs");
}

pub use proto::{
    k2_publish_message::*, K2PublishMessage, PublishAgent, PublishOps,
};

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

impl TryFrom<&AgentInfoSigned> for AgentInfoMessage {
    type Error = K2Error;

    fn try_from(value: &AgentInfoSigned) -> K2Result<Self> {
        let agent_info_encoded = value.encode()?;
        Ok(Self {
            data: agent_info_encoded,
        })
    }
}

impl TryFrom<&AgentInfoSigned> for PublishAgent {
    type Error = K2Error;

    fn try_from(value: &AgentInfoSigned) -> K2Result<Self> {
        let agent_info_message = value.try_into()?;
        Ok(Self {
            agent_info: Some(agent_info_message),
        })
    }
}

impl From<PublishAgent> for AgentInfoSigned {
    fn from(value: PublishAgent) -> Self {
        value.into()
    }
}

/// Serialize list of op ids to request.
pub fn serialize_publish_agent(value: &AgentInfoSigned) -> K2Result<Bytes> {
    let mut out = BytesMut::new();
    PublishAgent::try_from(value)?
        .encode(&mut out)
        .expect("failed to encode publish agent request");
    Ok(out.freeze())
}

/// Serialize list of op ids to fetch request message.
pub fn serialize_publish_agent_message(
    value: &AgentInfoSigned,
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

/// Trait for implementing a fetch module to fetch ops from other agents.
pub trait Publish: 'static + Send + Sync + std::fmt::Debug {
    /// Add op ids to be published to a peer.
    fn publish_ops(
        &self,
        op_ids: Vec<OpId>,
        target: Url,
    ) -> BoxFut<'_, K2Result<()>>;

    /// Add agent info to be published to a peer.
    fn publish_agent(
        &self,
        agent_info: AgentInfoSigned,
        target: Url,
    ) -> BoxFut<'_, K2Result<()>>;
}

/// Trait object [Fetch].
pub type DynPublish = Arc<dyn Publish>;

/// A factory for creating Fetch instances.
pub trait PublishFactory: 'static + Send + Sync + std::fmt::Debug {
    /// Construct a Fetch instance.
    fn create(
        &self,
        builder: Arc<builder::Builder>,
        space_id: SpaceId,
        fetch: DynFetch,
        peer_store: DynPeerStore,
        transport: DynTransport,
    ) -> BoxFut<'static, K2Result<DynPublish>>;
}

/// Trait object [FetchFactory].
pub type DynPublishFactory = Arc<dyn PublishFactory>;

#[cfg(test)]
mod test {
    use super::*;
    use crate::{
        id::Id, AgentId, AgentInfo, DhtArc, Signer, Timestamp, Verifier,
    };
    use prost::Message;

    const SIG: &[u8] = b"fake-signature";

    #[derive(Debug)]
    struct TestCrypto;

    impl Signer for TestCrypto {
        fn sign<'a, 'b: 'a, 'c: 'a>(
            &'a self,
            _agent_info: &'b AgentInfo,
            _encoded: &'c [u8],
        ) -> BoxFut<'a, K2Result<bytes::Bytes>> {
            Box::pin(async move { Ok(bytes::Bytes::from_static(SIG)) })
        }
    }

    impl Verifier for TestCrypto {
        fn verify(
            &self,
            _agent_info: &AgentInfo,
            _message: &[u8],
            signature: &[u8],
        ) -> bool {
            signature == SIG
        }
    }

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

    // #[tokio::test(flavor = "multi_thread")]
    // async fn happy_publish_agent_encode_decode() {
    //     let agent: AgentId = bytes::Bytes::from_static(b"test-agent").into();
    //     let space: SpaceId = bytes::Bytes::from_static(b"test-space").into();
    //     let now = Timestamp::from_micros(1731690797907204);
    //     let later = Timestamp::from_micros(now.as_micros() + 72_000_000_000);
    //     let url = Some(Url::from_str("ws://test.com:80/test-url").unwrap());
    //     let storage_arc = DhtArc::Arc(42, u32::MAX / 13);

    //     let agent_info = AgentInfoSigned::sign(
    //         &TestCrypto,
    //         AgentInfo {
    //             agent: agent.clone(),
    //             space: space.clone(),
    //             created_at: now,
    //             expires_at: later,
    //             is_tombstone: false,
    //             url: url.clone(),
    //             storage_arc,
    //         },
    //     )
    //     .await
    //     .unwrap();

    //     let publish_agent = PublishAgent::from(agent_info.get_mut().clone());
    // }
}
