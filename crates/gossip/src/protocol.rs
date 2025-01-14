//! Protocol definitions for the gossip module.

use bytes::Bytes;
use kitsune2_api::agent::AgentInfoSigned;
use kitsune2_api::{AgentId, K2Error, K2Result, OpId};
use prost::{bytes, Message};
use std::sync::Arc;

include!("../proto/gen/kitsune2.gossip.rs");

/// Deserialize a gossip message
pub fn deserialize_gossip_message(
    value: bytes::Bytes,
) -> K2Result<K2GossipMessage> {
    K2GossipMessage::decode(value).map_err(K2Error::other)
}

/// Serialize a gossip message
pub fn serialize_gossip_message(
    value: K2GossipMessage,
) -> K2Result<bytes::Bytes> {
    let mut out = bytes::BytesMut::new();

    value.encode(&mut out).map_err(|e| {
        K2Error::other(format!("Failed to serialize gossip message: {:?}", e))
    })?;

    Ok(out.freeze())
}

/// Encode agent ids as bytes
pub(crate) fn encode_agent_ids(
    agent_ids: impl IntoIterator<Item = AgentId>,
) -> Vec<Bytes> {
    agent_ids.into_iter().map(|a| a.0 .0).collect::<Vec<_>>()
}

/// Encode agent infos as [AgentInfoMessage]s
pub(crate) fn encode_agent_infos(
    agent_infos: impl IntoIterator<Item = Arc<AgentInfoSigned>>,
) -> Vec<AgentInfoMessage> {
    agent_infos
        .into_iter()
        .map(|a| AgentInfoMessage {
            encoded: bytes::Bytes::from(a.get_encoded().as_bytes().to_vec()),
            signature: a.get_signature().clone(),
        })
        .collect::<Vec<_>>()
}

/// Encode op ids as bytes
pub(crate) fn encode_op_ids(
    op_ids: impl IntoIterator<Item = OpId>,
) -> Vec<Bytes> {
    op_ids.into_iter().map(|o| o.0 .0).collect::<Vec<_>>()
}
