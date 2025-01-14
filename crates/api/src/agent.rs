//! Types dealing with agent metadata.
//!
//! [AgentInfo] and the wrapping [AgentInfoSigned] define a pattern for
//! cryptographically verifiable declarations of network reachability.
//!
//! To facilitate ease of debugging (See our unit tests in this module!),
//! The canonical encoding for this info is JSON.
//!
//! #### Json Schemas
//!
//! ```json
//! {
//!   "title": "AgentInfoSigned",
//!   "type": "object",
//!   "properties": {
//!     "agentInfo": { "type": "string", "required": true, "description": "json AgentInfo" },
//!     "signature": { "type": "string", "required": true, "description": "base64" }
//!   }
//! }
//! ```
//!
//! ```json
//! {
//!   "title": "AgentInfo",
//!   "type": "object",
//!   "properties": {
//!     "agent": { "type": "string", "required": true, "description": "base64" },
//!     "space": { "type": "string", "required": true, "description": "base64" },
//!     "createdAt": {
//!         "type": "string",
//!         "required": true,
//!         "description": "i64 micros since unix epoch"
//!     },
//!     "expiresAt": {
//!         "type": "string",
//!         "required": true,
//!         "description": "i64 micros since unix epoch",
//!     },
//!     "isTombstone": { "type": "boolean", "required": true },
//!     "url": { "type": "string", "description": "optional" },
//!     "storageArc": {
//!       "type": "array",
//!       "description": "optional",
//!       "items": [
//!         {
//!             "type": "number",
//!             "required": true,
//!             "description": "u32 arc start loc"
//!         },
//!         {
//!             "type": "number",
//!             "required": true,
//!             "description": "u32 arc end loc"
//!         }
//!       ]
//!     }
//!   }
//! }
//! ```
//!
//! #### Cryptography
//!
//! This module and its data structures are designed to be agnostic to
//! cryptography. It exposes the [Signer] and [Verifier] traits to allow
//! implementors to choose the algorithm to be used.
//!
//! The underlying data structures, however, cannot be quite so agnostic.
//!
//! By convention, absent other indications, the [AgentInfo::agent] property
//! will be an ed25519 public key, and the [AgentInfoSigned::get_signature] will
//! be an ed25519 signature.
//!
//! Future versions of this library may look for an optional "alg" property
//! on the [AgentInfo] type before falling back to this usage of ed25519.
//! These other algorithms may treat the [AgentInfo::agent] property as a
//! hash of the public key instead of the public key itself, and find the
//! public key instead on an "algPubKey" property. (Some post-quantum
//! algorithms have ridiculously long key material.)

use crate::*;
use std::sync::Arc;

/// Defines a type capable of cryptographic signatures.
pub trait Signer {
    /// Sign the encoded data, returning the resulting detached signature bytes.
    fn sign<'a, 'b: 'a, 'c: 'a>(
        &'a self,
        agent_info: &'b AgentInfo,
        message: &'c [u8],
    ) -> BoxFut<'a, K2Result<bytes::Bytes>>;
}

/// Defines a type capable of cryptographic verification.
pub trait Verifier: std::fmt::Debug {
    /// Verify the provided detached signature over the provided message.
    /// Returns `true` if the signature is valid.
    fn verify(
        &self,
        agent_info: &AgentInfo,
        message: &[u8],
        signature: &[u8],
    ) -> bool;
}

/// Trait-object [Verifier].
pub type DynVerifier = Arc<dyn Verifier + 'static + Send + Sync>;

impl Verifier for DynVerifier {
    fn verify(
        &self,
        agent_info: &AgentInfo,
        message: &[u8],
        signature: &[u8],
    ) -> bool {
        (**self).verify(agent_info, message, signature)
    }
}

/// A "Local" agent is an agent that is connected to the local Kitsune2 node,
/// and is able to sign messages and agent infos.
pub trait LocalAgent: Signer + 'static + Send + Sync + std::fmt::Debug {
    /// The [AgentId] of this local agent.
    fn agent(&self) -> &AgentId;

    /// Register a callback to be invoked when [Self::invoke_cb] is called.
    /// Implementations need only track a single cb. If this is called again,
    /// use only the new one.
    fn register_cb(&self, cb: Arc<dyn Fn() + 'static + Send + Sync>);

    /// Invoke the registered cb if one has been set.
    /// This can be treated as a no-op rather than an error if [Self::register_cb] has not yet been called.
    fn invoke_cb(&self);

    /// Access the current storage arc for this local agent.
    ///
    /// This will be used by the space module to construct [AgentInfoSigned].
    fn get_cur_storage_arc(&self) -> DhtArc;

    /// Set the current storage arc for this local agent.
    /// This will be initially set to zero on space join.
    /// The gossip module will update this as data is collected.
    fn set_cur_storage_arc(&self, arc: DhtArc);

    /// This is a chance for the implementor to influence how large
    /// a storage arc should be for this agent. The gossip module will
    /// attempt to collect enough data for claiming storage authority
    /// over this range.
    fn get_tgt_storage_arc(&self) -> DhtArc;

    /// The sharding module will attempt to determine an ideal target
    /// arc for this agent. An implementation is free to use or discard
    /// this information when returning the arc in [Self::get_tgt_storage_arc].
    /// This will initially be set to zero on join, but the sharding module
    /// may later update this to FULL or a true target value.
    fn set_tgt_storage_arc_hint(&self, arc: DhtArc);
}

/// Trait-object [LocalAgent].
pub type DynLocalAgent = Arc<dyn LocalAgent>;

impl LocalAgent for DynLocalAgent {
    fn agent(&self) -> &AgentId {
        (**self).agent()
    }

    fn register_cb(&self, cb: Arc<dyn Fn() + 'static + Send + Sync>) {
        (**self).register_cb(cb);
    }

    fn invoke_cb(&self) {
        (**self).invoke_cb();
    }

    fn get_cur_storage_arc(&self) -> DhtArc {
        (**self).get_cur_storage_arc()
    }

    fn set_cur_storage_arc(&self, arc: DhtArc) {
        (**self).set_cur_storage_arc(arc);
    }

    fn get_tgt_storage_arc(&self) -> DhtArc {
        (**self).get_tgt_storage_arc()
    }

    fn set_tgt_storage_arc_hint(&self, arc: DhtArc) {
        (**self).set_tgt_storage_arc_hint(arc);
    }
}

impl Signer for DynLocalAgent {
    fn sign<'a, 'b: 'a, 'c: 'a>(
        &'a self,
        agent_info: &'b AgentInfo,
        message: &'c [u8],
    ) -> BoxFut<'a, K2Result<bytes::Bytes>> {
        (**self).sign(agent_info, message)
    }
}

mod serde_string_timestamp {
    pub fn serialize<S>(
        t: &crate::Timestamp,
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&t.as_micros().to_string())
    }

    pub fn deserialize<'de, D>(
        deserializer: D,
    ) -> Result<crate::Timestamp, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s: &'de str = serde::Deserialize::deserialize(deserializer)?;
        let i: i64 = s.parse().map_err(serde::de::Error::custom)?;
        Ok(crate::Timestamp::from_micros(i))
    }
}

/// AgentInfo stores metadata related to agents.
#[derive(
    Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq, Hash,
)]
#[serde(rename_all = "camelCase")]
pub struct AgentInfo {
    /// The agent id.
    pub agent: AgentId,

    /// The space id.
    pub space: SpaceId,

    /// When this metadata was created.
    #[serde(with = "serde_string_timestamp")]
    pub created_at: Timestamp,

    /// When this metadata will expire.
    #[serde(with = "serde_string_timestamp")]
    pub expires_at: Timestamp,

    /// If `true`, this metadata is a tombstone, indicating
    /// the agent has gone offline, and is no longer reachable.
    pub is_tombstone: bool,

    /// If set, this indicates the primary url at which this agent may
    /// be reached. This should largely only be UNSET if this is a tombstone.
    pub url: Option<Url>,

    /// The arc over which this agent claims authority.
    #[serde(default = "DhtArc::default")]
    pub storage_arc: DhtArc,
}

/// Signed agent information.
#[derive(PartialEq, Eq, Hash)]
pub struct AgentInfoSigned {
    /// The decoded information associated with this agent.
    agent_info: AgentInfo,

    /// The encoded information that was signed.
    encoded: String,

    /// The signature.
    signature: bytes::Bytes,
}

impl std::fmt::Debug for AgentInfoSigned {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("AgentInfoSigned(")?;
        f.write_str(&self.encoded)?;
        f.write_str(")")
    }
}

impl AgentInfoSigned {
    /// Generate a signed agent info by signing an agent info.
    pub async fn sign<S: Signer>(
        signer: &S,
        agent_info: AgentInfo,
    ) -> K2Result<std::sync::Arc<Self>> {
        let encoded = serde_json::to_string(&agent_info)
            .map_err(|e| K2Error::other_src("encoding agent_info", e))?;
        let signature = signer
            .sign(&agent_info, encoded.as_bytes())
            .await
            .map_err(|e| K2Error::other_src("signing agent_info", e))?;
        Ok(std::sync::Arc::new(Self {
            agent_info,
            encoded,
            signature,
        }))
    }

    /// Decode a canonical json encoding of a signed agent info.
    pub fn decode<V: Verifier>(
        verifier: &V,
        encoded: &[u8],
    ) -> K2Result<std::sync::Arc<Self>> {
        #[derive(serde::Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Ref {
            agent_info: String,
            #[serde(with = "crate::serde_bytes_base64")]
            signature: bytes::Bytes,
        }
        let v: Ref = serde_json::from_slice(encoded)
            .map_err(|e| K2Error::other_src("decoding agent_info", e))?;
        Self::from_parts(verifier, v.agent_info, v.signature)
    }

    /// Decode a canonical json encoding of a list of signed agent infos.
    pub fn decode_list<V: Verifier>(
        verifier: &V,
        encoded: &[u8],
    ) -> K2Result<Vec<K2Result<std::sync::Arc<Self>>>> {
        #[derive(serde::Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Ref {
            agent_info: String,
            #[serde(with = "crate::serde_bytes_base64")]
            signature: bytes::Bytes,
        }
        let v: Vec<Ref> = serde_json::from_slice(encoded)
            .map_err(|e| K2Error::other_src("decoding agent_info", e))?;
        Ok(v.into_iter()
            .map(|v| Self::from_parts(verifier, v.agent_info, v.signature))
            .collect())
    }

    /// Create an instance from its parts.
    ///
    /// Useful when receiving an agent info over the wire.
    pub fn from_parts<V: Verifier>(
        verifier: &V,
        agent_info: String,
        signature: bytes::Bytes,
    ) -> K2Result<std::sync::Arc<Self>> {
        let info: AgentInfo = serde_json::from_str(&agent_info)
            .map_err(|e| K2Error::other_src("decoding inner agent_info", e))?;
        if !verifier.verify(&info, agent_info.as_bytes(), &signature) {
            return Err(K2Error::other("InvalidSignature"));
        }
        Ok(std::sync::Arc::new(Self {
            agent_info: info,
            encoded: agent_info,
            signature,
        }))
    }

    /// Get the canonical json encoding of this signed agent info.
    pub fn encode(&self) -> K2Result<String> {
        #[derive(serde::Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Ref<'a> {
            agent_info: &'a String,
            #[serde(with = "crate::serde_bytes_base64")]
            signature: &'a bytes::Bytes,
        }
        serde_json::to_string(&Ref {
            agent_info: &self.encoded,
            signature: &self.signature,
        })
        .map_err(|e| K2Error::other_src("encoding agent_info", e))
    }

    /// Access the inner [AgentInfo] data. Note, you can instead just deref.
    pub fn get_agent_info(&self) -> &AgentInfo {
        self
    }

    /// Access the canonical encoded inner agent info.
    pub fn get_encoded(&self) -> &str {
        &self.encoded
    }

    /// Access the signature over the encoded inner agent info.
    pub fn get_signature(&self) -> &bytes::Bytes {
        &self.signature
    }
}

impl std::ops::Deref for AgentInfoSigned {
    type Target = AgentInfo;

    fn deref(&self) -> &Self::Target {
        &self.agent_info
    }
}

#[cfg(test)]
mod test {
    use super::*;

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

    #[tokio::test(flavor = "multi_thread")]
    async fn happy_encode_decode() {
        let agent: AgentId = bytes::Bytes::from_static(b"test-agent").into();
        let space: SpaceId = bytes::Bytes::from_static(b"test-space").into();
        let now = Timestamp::from_micros(1731690797907204);
        let later = Timestamp::from_micros(now.as_micros() + 72_000_000_000);
        let url = Some(Url::from_str("ws://test.com:80/test-url").unwrap());
        let storage_arc = DhtArc::Arc(42, u32::MAX / 13);

        let enc = AgentInfoSigned::sign(
            &TestCrypto,
            AgentInfo {
                agent: agent.clone(),
                space: space.clone(),
                created_at: now,
                expires_at: later,
                is_tombstone: false,
                url: url.clone(),
                storage_arc,
            },
        )
        .await
        .unwrap()
        .encode()
        .unwrap();

        assert_eq!(
            r#"{"agentInfo":"{\"agent\":\"dGVzdC1hZ2VudA\",\"space\":\"dGVzdC1zcGFjZQ\",\"createdAt\":\"1731690797907204\",\"expiresAt\":\"1731762797907204\",\"isTombstone\":false,\"url\":\"ws://test.com:80/test-url\",\"storageArc\":[42,330382099]}","signature":"ZmFrZS1zaWduYXR1cmU"}"#,
            enc
        );

        let dec = AgentInfoSigned::decode(&TestCrypto, enc.as_bytes()).unwrap();
        assert_eq!(agent, dec.agent);
        assert_eq!(space, dec.space);
        assert_eq!(now, dec.created_at);
        assert_eq!(later, dec.expires_at);
        assert!(!dec.is_tombstone);
        assert_eq!(url, dec.url);
        assert_eq!(storage_arc, dec.storage_arc);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn ignores_future_extension_fields() {
        AgentInfoSigned::decode(&TestCrypto, br#"{"agentInfo":"{\"agent\":\"dGVzdC1hZ2VudA\",\"space\":\"dGVzdC1zcGFjZQ\",\"createdAt\":\"1731690797907204\",\"expiresAt\":\"1731762797907204\",\"isTombstone\":false,\"url\":\"ws://test.com:80/test-url\",\"storageArc\":[42,330382099],\"fakeField\":\"bla\"}","signature":"ZmFrZS1zaWduYXR1cmU","fakeField2":"bla2"}"#).unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn fills_in_default_fields() {
        let dec = AgentInfoSigned::decode(&TestCrypto, br#"{"agentInfo":"{\"agent\":\"dGVzdC1hZ2VudA\",\"space\":\"dGVzdC1zcGFjZQ\",\"createdAt\":\"1731690797907204\",\"expiresAt\":\"1731762797907204\",\"isTombstone\":false}","signature":"ZmFrZS1zaWduYXR1cmU"}"#).unwrap();
        assert!(dec.url.is_none());
        assert_eq!(DhtArc::Empty, dec.storage_arc);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn dies_with_invalid_signature() {
        let dec = AgentInfoSigned::decode(&TestCrypto, br#"{"agentInfo":"{\"agent\":\"dGVzdC1hZ2VudA\",\"space\":\"dGVzdC1zcGFjZQ\",\"createdAt\":\"1731690797907204\",\"expiresAt\":\"1731762797907204\",\"isTombstone\":false}","signature":""}"#).unwrap_err();
        assert!(dec.to_string().contains("InvalidSignature"));
    }
}
