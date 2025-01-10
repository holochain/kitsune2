//! Protocol definitions for the gossip module.

use kitsune2_api::{K2Error, K2Result};
use prost::{bytes, Message};

include!("../proto/gen/kitsune2.gossip.rs");

/// Deserialize a gossip message
pub fn deserialize_gossip_message(
    value: bytes::Bytes,
) -> K2Result<K2GossipProto> {
    K2GossipProto::decode(value).map_err(K2Error::other)
}

/// Serialize a gossip message
pub fn serialize_gossip_message(
    value: K2GossipProto,
) -> K2Result<bytes::Bytes> {
    let mut buf = Vec::new();
    value.encode(&mut buf).map_err(K2Error::other)?;
    Ok(bytes::Bytes::from(buf))
}
