//! Protocol definitions for the gossip module.

use bytes::{Bytes, BytesMut};
use kitsune2_api::agent::AgentInfoSigned;
use kitsune2_api::id::encode_ids;
use kitsune2_api::{AgentId, K2Error, K2Result, OpId, Timestamp};
use kitsune2_dht::snapshot::DhtSnapshot;
use prost::{bytes, Message};
use std::collections::HashMap;
use std::sync::Arc;

include!("../proto/gen/kitsune2.gossip.rs");

#[derive(Debug)]
pub enum GossipMessage {
    Initiate(K2GossipInitiateMessage),
    Accept(K2GossipAcceptMessage),
    NoDiff(K2GossipNoDiffMessage),
    DiscSectorsDiff(K2GossipDiscSectorsDiffMessage),
    RingSectorDetailsDiff(K2GossipRingSectorDetailsDiffMessage),
    Agents(K2GossipAgentsMessage),
}

/// Deserialize a gossip message
pub fn deserialize_gossip_message(value: Bytes) -> K2Result<GossipMessage> {
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
        k2_gossip_message::GossipMessageType::DiscSectorsDiff => {
            let inner = K2GossipDiscSectorsDiffMessage::decode(outer.data)
                .map_err(K2Error::other)?;
            Ok(GossipMessage::DiscSectorsDiff(inner))
        }
        k2_gossip_message::GossipMessageType::RingSectorDetailsDiff => {
            let inner =
                K2GossipRingSectorDetailsDiffMessage::decode(outer.data)
                    .map_err(K2Error::other)?;
            Ok(GossipMessage::RingSectorDetailsDiff(inner))
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
pub fn serialize_gossip_message(value: GossipMessage) -> K2Result<Bytes> {
    let mut out = BytesMut::new();

    let (msg_type, data) = serialize_inner_gossip_message(value)?;

    K2GossipMessage {
        msg_type: msg_type as i32,
        data,
    }
    .encode(&mut out)
    .map_err(|e| {
        K2Error::other(format!("Failed to serialize gossip message: {:?}", e))
    })?;

    Ok(out.freeze())
}

fn serialize_inner_gossip_message(
    value: GossipMessage,
) -> K2Result<(k2_gossip_message::GossipMessageType, Bytes)> {
    let mut out = BytesMut::new();

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
        GossipMessage::DiscSectorsDiff(inner) => {
            inner.encode(&mut out).map_err(|e| {
                K2Error::other(format!(
                    "Failed to serialize gossip message: {:?}",
                    e
                ))
            })?;

            Ok((
                k2_gossip_message::GossipMessageType::DiscSectorsDiff,
                out.freeze(),
            ))
        }
        GossipMessage::RingSectorDetailsDiff(inner) => {
            inner.encode(&mut out).map_err(|e| {
                K2Error::other(format!(
                    "Failed to serialize gossip message: {:?}",
                    e
                ))
            })?;

            Ok((
                k2_gossip_message::GossipMessageType::RingSectorDetailsDiff,
                out.freeze(),
            ))
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
) -> Vec<Bytes> {
    encode_ids(agent_ids)
}

/// Encode agent infos as [AgentInfoMessage]s
pub(crate) fn encode_agent_infos(
    agent_infos: impl IntoIterator<Item = Arc<AgentInfoSigned>>,
) -> K2Result<Vec<Bytes>> {
    agent_infos
        .into_iter()
        .map(|a| a.encode().map(|a| Bytes::from(a.as_bytes().to_vec())))
        .collect::<K2Result<Vec<_>>>()
}

/// Encode op ids as bytes
pub(crate) fn encode_op_ids(
    op_ids: impl IntoIterator<Item = OpId>,
) -> Vec<Bytes> {
    encode_ids(op_ids)
}

impl TryFrom<DhtSnapshot> for SnapshotMinimalMessage {
    type Error = K2Error;

    fn try_from(value: DhtSnapshot) -> K2Result<Self> {
        match value {
            DhtSnapshot::Minimal {
                disc_boundary,
                disc_top_hash,
                ring_top_hashes
            } => {
                Ok(SnapshotMinimalMessage {
                    disc_boundary: disc_boundary.as_micros(),
                    disc_top_hash: disc_top_hash.into(),
                    ring_top_hashes: ring_top_hashes.into_iter().map(|h| h.into()).collect(),
                })
            }
            _ => {
                Err(K2Error::other("Only DhtSnapshot::Minimal can be converted to a SnapshotMinimalMessage".to_string()))
            }
        }
    }
}

impl From<SnapshotMinimalMessage> for DhtSnapshot {
    fn from(value: SnapshotMinimalMessage) -> Self {
        DhtSnapshot::Minimal {
            disc_boundary: Timestamp::from_micros(value.disc_boundary),
            disc_top_hash: value.disc_top_hash,
            ring_top_hashes: value.ring_top_hashes,
        }
    }
}

impl TryFrom<DhtSnapshot> for SnapshotDiscSectorsMessage {
    type Error = K2Error;

    fn try_from(value: DhtSnapshot) -> K2Result<Self> {
        match value {
            DhtSnapshot::DiscSectors {
                disc_boundary,
                disc_sector_top_hashes,
            } => {
                Ok(SnapshotDiscSectorsMessage {
                    disc_boundary: disc_boundary.as_micros(),
                    disc_sectors: disc_sector_top_hashes.keys().cloned().collect(),
                    disc_sector_hashes: disc_sector_top_hashes.values().cloned().collect(),
                })
            }
            _ => {
                Err(K2Error::other("Only DhtSnapshot::DiscSectors can be converted to a SnapshotDiscSectorsMessage".to_string()))
            }
        }
    }
}

impl TryFrom<SnapshotDiscSectorsMessage> for DhtSnapshot {
    type Error = K2Error;

    fn try_from(
        value: SnapshotDiscSectorsMessage,
    ) -> Result<Self, Self::Error> {
        if value.disc_sectors.len() != value.disc_sector_hashes.len() {
            return Err(K2Error::other(
                "Mismatched disc sector and hash lengths".to_string(),
            ));
        }

        Ok(DhtSnapshot::DiscSectors {
            disc_boundary: Timestamp::from_micros(value.disc_boundary),
            disc_sector_top_hashes: value
                .disc_sectors
                .into_iter()
                .zip(value.disc_sector_hashes)
                .collect(),
        })
    }
}

impl TryFrom<DhtSnapshot> for SnapshotRingSectorDetailsMessage {
    type Error = K2Error;

    fn try_from(value: DhtSnapshot) -> K2Result<Self> {
        match value {
            DhtSnapshot::RingSectorDetails {
                disc_boundary,
                ring_sector_hashes,
            } => {
                Ok(SnapshotRingSectorDetailsMessage {
                    disc_boundary: disc_boundary.as_micros(),
                    ring_indices: ring_sector_hashes.keys().cloned().collect(),
                    ring_sector_hashes: ring_sector_hashes.values().map(|m| {
                        RingSectorHashes {
                            sector_indices: m.keys().cloned().collect(),
                            hashes: m.values().cloned().collect(),
                        }
                    }).collect(),
                })
            }
            _ => {
                Err(K2Error::other("Only DhtSnapshot::RingSectorDetails can be converted to a SnapshotRingSectorDetailsMessage".to_string()))
            }
        }
    }
}

impl TryFrom<SnapshotRingSectorDetailsMessage> for DhtSnapshot {
    type Error = K2Error;

    fn try_from(value: SnapshotRingSectorDetailsMessage) -> K2Result<Self> {
        if value.ring_indices.len() != value.ring_sector_hashes.len() {
            return Err(K2Error::other(
                "Mismatched ring sector and hash lengths".to_string(),
            ));
        }

        Ok(DhtSnapshot::RingSectorDetails {
            disc_boundary: Timestamp::from_micros(value.disc_boundary),
            ring_sector_hashes: value
                .ring_indices
                .into_iter()
                .zip(value.ring_sector_hashes.into_iter().map(|r| {
                    if r.sector_indices.len() != r.hashes.len() {
                        return Err(K2Error::other(
                            "Mismatched sector and hash lengths".to_string(),
                        ));
                    }

                    Ok(r.sector_indices.into_iter().zip(r.hashes).collect())
                }))
                .map(|(a, b)| match b {
                    Ok(b) => Ok((a, b)),
                    Err(e) => Err(e),
                })
                .collect::<K2Result<HashMap<_, _>>>()?,
        })
    }
}
