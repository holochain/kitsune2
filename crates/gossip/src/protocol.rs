//! Protocol definitions for the gossip module.

use crate::protocol::k2_gossip_accept_message::SnapshotMinimalMessage;
use crate::protocol::k2_gossip_disc_sectors_diff_message::SnapshotDiscSectorsMessage;
use crate::protocol::k2_gossip_ring_sector_details_diff_message::{
    RingSectorHashes, SnapshotRingSectorDetailsMessage,
};
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
    DiscSectorDetailsDiff(K2GossipDiscSectorDetailsDiffMessage),
    DiscSectorDetailsResponseDiff(K2GossipDiscSectorDetailsDiffResponseMessage),
    RingSectorDetailsDiff(K2GossipRingSectorDetailsDiffMessage),
    Hashes(K2GossipHashesMessage),
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
        k2_gossip_message::GossipMessageType::DiscSectorDetailsDiff => {
            let inner =
                K2GossipDiscSectorDetailsDiffMessage::decode(outer.data)
                    .map_err(K2Error::other)?;
            Ok(GossipMessage::DiscSectorDetailsDiff(inner))
        }
        k2_gossip_message::GossipMessageType::DiscSectorDetailsResponseDiff => {
            let inner = K2GossipDiscSectorDetailsDiffResponseMessage::decode(
                outer.data,
            )
            .map_err(K2Error::other)?;
            Ok(GossipMessage::DiscSectorDetailsResponseDiff(inner))
        }
        k2_gossip_message::GossipMessageType::RingSectorDetailsDiff => {
            let inner =
                K2GossipRingSectorDetailsDiffMessage::decode(outer.data)
                    .map_err(K2Error::other)?;
            Ok(GossipMessage::RingSectorDetailsDiff(inner))
        }
        k2_gossip_message::GossipMessageType::Hashes => {
            let inner = K2GossipHashesMessage::decode(outer.data)
                .map_err(K2Error::other)?;
            Ok(GossipMessage::Hashes(inner))
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
    let out = BytesMut::new();

    fn encode<T: prost::Message>(msg: T, mut out: BytesMut) -> K2Result<Bytes> {
        msg.encode(&mut out).map_err(|e| {
            K2Error::other(format!(
                "Failed to serialize gossip message: {:?}",
                e
            ))
        })?;

        Ok(out.freeze())
    }

    match value {
        GossipMessage::Initiate(inner) => Ok((
            k2_gossip_message::GossipMessageType::Initiate,
            encode(inner, out)?,
        )),
        GossipMessage::Accept(inner) => Ok((
            k2_gossip_message::GossipMessageType::Accept,
            encode(inner, out)?,
        )),
        GossipMessage::NoDiff(inner) => Ok((
            k2_gossip_message::GossipMessageType::NoDiff,
            encode(inner, out)?,
        )),
        GossipMessage::DiscSectorsDiff(inner) => Ok((
            k2_gossip_message::GossipMessageType::DiscSectorsDiff,
            encode(inner, out)?,
        )),
        GossipMessage::DiscSectorDetailsDiff(inner) => Ok((
            k2_gossip_message::GossipMessageType::DiscSectorDetailsDiff,
            encode(inner, out)?,
        )),
        GossipMessage::DiscSectorDetailsResponseDiff(inner) => Ok((
            k2_gossip_message::GossipMessageType::DiscSectorDetailsResponseDiff,
            encode(inner, out)?,
        )),
        GossipMessage::RingSectorDetailsDiff(inner) => Ok((
            k2_gossip_message::GossipMessageType::RingSectorDetailsDiff,
            encode(inner, out)?,
        )),
        GossipMessage::Hashes(inner) => Ok((
            k2_gossip_message::GossipMessageType::Hashes,
            encode(inner, out)?,
        )),
        GossipMessage::Agents(inner) => Ok((
            k2_gossip_message::GossipMessageType::Agents,
            encode(inner, out)?,
        )),
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
                    disc_top_hash,
                    ring_top_hashes: ring_top_hashes.into_iter().collect(),
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

impl TryFrom<DhtSnapshot> for SnapshotDiscSectorDetailsMessage {
    type Error = K2Error;

    fn try_from(value: DhtSnapshot) -> K2Result<Self> {
        match value {
            DhtSnapshot::DiscSectorDetails {
                disc_boundary,
                disc_sector_hashes,
            } => {
                Ok(SnapshotDiscSectorDetailsMessage {
                    disc_boundary: disc_boundary.as_micros(),
                    sector_indices: disc_sector_hashes.keys().cloned().collect(),
                    disc_slice_hashes: disc_sector_hashes.values().map(|m| {
                        DiscSliceHashes {
                            slice_indices: m.keys().cloned().collect(),
                            hashes: m.values().cloned().collect(),
                        }
                    }).collect()
                })
            }
            _ => {
                Err(K2Error::other("Only DhtSnapshot::DiscSectorDetails can be converted to a SnapshotDiscSectorDetailsMessage".to_string()))
            }
        }
    }
}

impl TryFrom<SnapshotDiscSectorDetailsMessage> for DhtSnapshot {
    type Error = K2Error;

    fn try_from(value: SnapshotDiscSectorDetailsMessage) -> K2Result<Self> {
        if value.sector_indices.len() != value.disc_slice_hashes.len() {
            return Err(K2Error::other(
                "Mismatched sector and hash lengths".to_string(),
            ));
        }

        Ok(DhtSnapshot::DiscSectorDetails {
            disc_boundary: Timestamp::from_micros(value.disc_boundary),
            disc_sector_hashes: value
                .sector_indices
                .into_iter()
                .zip(value.disc_slice_hashes.into_iter().map(|r| {
                    if r.slice_indices.len() != r.hashes.len() {
                        return Err(K2Error::other(
                            "Mismatched slice and hash lengths".to_string(),
                        ));
                    }

                    Ok(r.slice_indices.into_iter().zip(r.hashes).collect())
                }))
                .map(|(a, b)| match b {
                    Ok(b) => Ok((a, b)),
                    Err(e) => Err(e),
                })
                .collect::<K2Result<HashMap<_, _>>>()?,
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
