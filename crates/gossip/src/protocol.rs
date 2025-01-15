//! Protocol definitions for the gossip module.

use kitsune2_api::agent::AgentInfoSigned;
use kitsune2_api::{AgentId, K2Error, K2Result, OpId};
use prost::{bytes, Message};
use std::sync::Arc;

include!("../proto/gen/kitsune2.gossip.rs");

#[derive(Debug)]
pub enum GossipMessage {
    Initiate(K2GossipInitiateMessage),
    Accept(K2GossipAcceptMessage),
    NoDiff(K2GossipNoDiffMessage),
    Agents(K2GossipAgentsMessage),
}

/// Deserialize a gossip message
pub fn deserialize_gossip_message(
    value: bytes::Bytes,
) -> K2Result<GossipMessage> {
    let outer = K2GossipMessage::decode(value).map_err(K2Error::other)?;

    match outer.msg_type() {
        k2_gossip_message::GossipMessageType::Initiate => {
            let inner = K2GossipInitiateMessage::decode(outer.data)
                .map_err(K2Error::other)?;
            Ok(GossipMessage::Initiate(inner))
        }
        k2_gossip_message::GossipMessageType::Accept => {
            let inner = K2GossipAcceptMessage::decode(outer.data)
                .map_err(K2Error::other)?;
            Ok(GossipMessage::Accept(inner))
        }
        k2_gossip_message::GossipMessageType::NoDiff => {
            let inner = K2GossipNoDiffMessage::decode(outer.data)
                .map_err(K2Error::other)?;
            Ok(GossipMessage::NoDiff(inner))
        }
        k2_gossip_message::GossipMessageType::Agents => {
            let inner = K2GossipAgentsMessage::decode(outer.data)
                .map_err(K2Error::other)?;
            Ok(GossipMessage::Agents(inner))
        }
        _ => Err(K2Error::other("Unknown gossip message type".to_string())),
    }
}

/// Serialize a gossip message
pub fn serialize_gossip_message(
    value: GossipMessage,
) -> K2Result<bytes::Bytes> {
    let mut out = bytes::BytesMut::new();

    let (msg_type, inner) = serialize_inner_gossip_message(value)?;

    K2GossipMessage {
        msg_type: msg_type as i32,
        data: inner,
    }
    .encode(&mut out)
    .map_err(|e| {
        K2Error::other(format!("Failed to serialize gossip message: {:?}", e))
    })?;

    Ok(out.freeze())
}

fn serialize_inner_gossip_message(
    value: GossipMessage,
) -> K2Result<(k2_gossip_message::GossipMessageType, bytes::Bytes)> {
    let mut out = bytes::BytesMut::new();

    match value {
        GossipMessage::Initiate(inner) => {
            inner.encode(&mut out).map_err(|e| {
                K2Error::other(format!(
                    "Failed to serialize gossip message: {:?}",
                    e
                ))
            })?;

            Ok((k2_gossip_message::GossipMessageType::Initiate, out.freeze()))
        }
        GossipMessage::Accept(inner) => {
            inner.encode(&mut out).map_err(|e| {
                K2Error::other(format!(
                    "Failed to serialize gossip message: {:?}",
                    e
                ))
            })?;

            Ok((k2_gossip_message::GossipMessageType::Accept, out.freeze()))
        }
        GossipMessage::NoDiff(inner) => {
            inner.encode(&mut out).map_err(|e| {
                K2Error::other(format!(
                    "Failed to serialize gossip message: {:?}",
                    e
                ))
            })?;

            Ok((k2_gossip_message::GossipMessageType::NoDiff, out.freeze()))
        }
        GossipMessage::Agents(inner) => {
            inner.encode(&mut out).map_err(|e| {
                K2Error::other(format!(
                    "Failed to serialize gossip message: {:?}",
                    e
                ))
            })?;

            Ok((k2_gossip_message::GossipMessageType::Agents, out.freeze()))
        }
    }
}

/// Encode agent ids as bytes
pub(crate) fn encode_agent_ids(
    agent_ids: impl IntoIterator<Item = AgentId>,
) -> Vec<bytes::Bytes> {
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
) -> Vec<bytes::Bytes> {
    op_ids.into_iter().map(|o| o.0 .0).collect::<Vec<_>>()
}
