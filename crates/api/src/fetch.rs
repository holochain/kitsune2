//! Kitsune2 fetch types.

use crate::{
    builder, config, peer_store::DynPeerStore, transport::DynTransport,
    AgentId, BoxFut, DynOpStore, K2Result, OpId, SpaceId,
};
use bytes::{Bytes, BytesMut};
use k2_fetch_message::FetchMessageType;
use prost::Message;
use std::sync::Arc;

include!("../proto/gen/kitsune2.fetch.rs");

impl From<Vec<OpId>> for Request {
    fn from(value: Vec<OpId>) -> Self {
        Self {
            op_ids: value.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<Request> for Vec<OpId> {
    fn from(value: Request) -> Self {
        value.op_ids.into_iter().map(Into::into).collect()
    }
}

impl From<Vec<Bytes>> for Response {
    fn from(value: Vec<Bytes>) -> Self {
        Self {
            ops: value.into_iter().map(Into::into).collect(),
        }
    }
}

/// Serialize list of op ids to request.
pub fn serialize_request(value: Vec<OpId>) -> Bytes {
    let mut out = BytesMut::new();
    Request::from(value)
        .encode(&mut out)
        .expect("failed to encode request");
    out.freeze()
}

/// Serialize list of op ids to fetch request message.
pub fn serialize_request_message(value: Vec<OpId>) -> Bytes {
    let mut out = BytesMut::new();
    let data = serialize_request(value);
    let fetch_message = K2FetchMessage {
        fetch_message_type: FetchMessageType::Request.into(),
        data,
    };
    fetch_message
        .encode(&mut out)
        .expect("failed to encode fetch request message");
    out.freeze()
}

/// Serialize list of ops to response.
pub fn serialize_response(value: Vec<Bytes>) -> Bytes {
    let mut out = BytesMut::new();
    Response::from(value)
        .encode(&mut out)
        .expect("failed to encode response");
    out.freeze()
}

/// Serialize list of ops to fetch response message.
pub fn serialize_response_message(value: Vec<Bytes>) -> Bytes {
    let mut out = BytesMut::new();
    let data = serialize_response(value);
    let fetch_message = K2FetchMessage {
        fetch_message_type: FetchMessageType::Response.into(),
        data,
    };
    fetch_message
        .encode(&mut out)
        .expect("failed to encode fetch response message");
    out.freeze()
}

/// Trait for implementing a fetch module to fetch ops from other agents.
pub trait Fetch: 'static + Send + Sync + std::fmt::Debug {
    /// Add op ids to be fetched from a peer.
    fn request_ops(
        &self,
        ops: Vec<OpId>,
        from: AgentId,
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
        op_store: DynOpStore,
        transport: DynTransport,
    ) -> BoxFut<'static, K2Result<DynFetch>>;
}

/// Trait object [FetchFactory].
pub type DynFetchFactory = Arc<dyn FetchFactory>;

#[cfg(test)]
mod test {
    use super::*;
    use crate::id::Id;
    use prost::Message;

    #[test]
    fn happy_request_encode_decode() {
        let op_id_1 = OpId::from(bytes::Bytes::from_static(b"some_op_id"));
        let op_id_2 = OpId::from(bytes::Bytes::from_static(b"another_op_id"));
        let op_id_vec = vec![op_id_1, op_id_2];
        let op_ids = Request::from(op_id_vec.clone());

        let op_ids_enc = op_ids.encode_to_vec();
        let op_ids_dec = Request::decode(op_ids_enc.as_slice()).unwrap();
        let op_ids_dec_vec = Vec::from(op_ids_dec.clone());

        assert_eq!(op_ids, op_ids_dec);
        assert_eq!(op_id_vec, op_ids_dec_vec);
    }

    #[test]
    fn happy_response_encode_decode() {
        // Not real op payloads, any bytes will do to check the round trip encoding/decoding
        // of the response type
        let op_1 = bytes::Bytes::from(vec![0]);
        let op_2 = bytes::Bytes::from(vec![1]);
        let expected_ops_data = vec![op_1, op_2];
        let ops_enc = serialize_response(expected_ops_data.clone());

        let response = Response::decode(ops_enc).unwrap();
        let actual_ops_data = response
            .ops
            .into_iter()
            .map(|op| op.data)
            .collect::<Vec<_>>();
        assert_eq!(expected_ops_data, actual_ops_data);
    }

    #[test]
    fn happy_fetch_request_encode_decode() {
        let op_id = OpId(Id(bytes::Bytes::from_static(b"id_1")));
        let op_ids = vec![op_id];
        let fetch_request = serialize_request_message(op_ids.clone());

        let fetch_message_dec = K2FetchMessage::decode(fetch_request).unwrap();
        assert_eq!(
            fetch_message_dec.fetch_message_type,
            i32::from(FetchMessageType::Request)
        );
        let request_dec = Request::decode(fetch_message_dec.data).unwrap();
        let op_ids_dec = request_dec
            .op_ids
            .into_iter()
            .map(Into::<OpId>::into)
            .collect::<Vec<_>>();
        assert_eq!(op_ids, op_ids_dec);
    }

    #[test]
    fn happy_fetch_response_encode_decode() {
        let op = bytes::Bytes::from(vec![0]);
        let expected_ops_data = vec![op];
        let fetch_response =
            serialize_response_message(expected_ops_data.clone());

        let fetch_message_dec = K2FetchMessage::decode(fetch_response).unwrap();
        assert_eq!(
            fetch_message_dec.fetch_message_type,
            i32::from(FetchMessageType::Response)
        );
        let response_dec = Response::decode(fetch_message_dec.data).unwrap();
        let actual_ops_data = response_dec
            .ops
            .into_iter()
            .map(|op| op.data)
            .collect::<Vec<_>>();
        assert_eq!(expected_ops_data, actual_ops_data);
    }
}
