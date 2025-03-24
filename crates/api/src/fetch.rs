//! Kitsune2 fetch types.

use crate::{
    builder, config, transport::DynTransport, BoxFut, DynOpStore, K2Result,
    OpId, SpaceId, Url,
};
use crate::{op_store, Timestamp};
use bytes::{Bytes, BytesMut};
use prost::Message;
use std::collections::HashMap;
use std::sync::Arc;

pub(crate) mod proto {
    include!("../proto/gen/kitsune2.fetch.rs");
}

pub use proto::{
    k2_fetch_message::*, FetchRequest, FetchResponse, K2FetchMessage,
};

impl From<Vec<OpId>> for FetchRequest {
    fn from(value: Vec<OpId>) -> Self {
        Self {
            op_ids: value.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<FetchRequest> for Vec<OpId> {
    fn from(value: FetchRequest) -> Self {
        value.op_ids.into_iter().map(Into::into).collect()
    }
}

impl From<Vec<Bytes>> for FetchResponse {
    fn from(value: Vec<Bytes>) -> Self {
        Self {
            ops: value.into_iter().map(Into::into).collect(),
        }
    }
}

/// Serialize list of op ids to request.
pub fn serialize_request(value: Vec<OpId>) -> Bytes {
    let mut out = BytesMut::new();
    FetchRequest::from(value)
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
    FetchResponse::from(value)
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
        op_ids: Vec<OpId>,
        source: Url,
    ) -> BoxFut<'_, K2Result<()>>;

    /// Get a state summary from the fetch module.
    fn get_state_summary(&self) -> BoxFut<'_, K2Result<FetchStateSummary>>;
}

/// Trait object [Fetch].
pub type DynFetch = Arc<dyn Fetch>;

/// A factory for creating Fetch instances.
pub trait FetchFactory: 'static + Send + Sync + std::fmt::Debug {
    /// Help the builder construct a default config from the chosen
    /// module factories.
    fn default_config(&self, config: &mut config::Config) -> K2Result<()>;

    /// Validate configuration.
    fn validate_config(&self, config: &config::Config) -> K2Result<()>;

    /// Construct a Fetch instance.
    fn create(
        &self,
        builder: Arc<builder::Builder>,
        space_id: SpaceId,
        op_store: DynOpStore,
        transport: DynTransport,
    ) -> BoxFut<'static, K2Result<DynFetch>>;
}

/// Trait object [FetchFactory].
pub type DynFetchFactory = Arc<dyn FetchFactory>;

/// Summary of the fetch state.
#[derive(Debug)]
pub struct FetchStateSummary {
    /// The op ids that are currently being fetched.
    ///
    /// Each op id is associated with one or more peer URL from which the op data could be
    /// requested.
    pub pending_requests: HashMap<OpId, Vec<Url>>,

    /// The peer URL for nodes that are currently on backoff because of failed fetch requests, and the timestamp when that backoff will expire.
    ///
    /// If peers are in here then they are not being used as potential sources in
    /// [`FetchStateSummary::pending_requests`].
    pub peers_on_backoff: HashMap<Url, Timestamp>,
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::id::Id;
    use prost::Message;

    #[test]
    fn happy_request_encode_decode() {
        let op_id_1 = OpId::from(Bytes::from_static(b"some_op_id"));
        let op_id_2 = OpId::from(Bytes::from_static(b"another_op_id"));
        let op_id_vec = vec![op_id_1, op_id_2];
        let op_ids = FetchRequest::from(op_id_vec.clone());

        let op_ids_enc = op_ids.encode_to_vec();
        let op_ids_dec = FetchRequest::decode(op_ids_enc.as_slice()).unwrap();
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

        let response = FetchResponse::decode(ops_enc).unwrap();
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
        let request_dec = FetchRequest::decode(fetch_message_dec.data).unwrap();
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
        let response_dec =
            FetchResponse::decode(fetch_message_dec.data).unwrap();
        let actual_ops_data = response_dec
            .ops
            .into_iter()
            .map(|op| op.data)
            .collect::<Vec<_>>();
        assert_eq!(expected_ops_data, actual_ops_data);
    }
}
