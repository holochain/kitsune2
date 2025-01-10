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
    let mut out = bytes::BytesMut::new();

    value.encode(&mut out).map_err(|e| {
        K2Error::other(format!("Failed to serialize gossip message: {:?}", e))
    })?;

    Ok(out.freeze())
}
