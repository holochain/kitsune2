//! Types dealing with agent metadata.

use crate::*;

/// Defines a type capable of cryptographic signatures.
pub trait Signer {
    /// Sign the encoded data, returning the resulting detached signature bytes.
    fn sign(
        &self,
        agent_info: &AgentInfo,
        message: &[u8],
    ) -> BoxFut<'_, std::io::Result<bytes::Bytes>>;
}

/// Defines a type capable of cryptographic verification.
pub trait Verifier {
    /// Verify the provided detached signature over the provided message.
    /// Returns `true` if the signature is valid.
    fn verify(
        &self,
        agent_info: &AgentInfo,
        message: &[u8],
        signature: &[u8],
    ) -> bool;
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
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
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
    pub url: Option<String>,

    /// If unset, this agent is claiming a zero storage arc,
    /// that is, they are saying they store nothing.
    /// If set, this indicates the inclusive bounds which this agent
    /// claims they are an authority for storage. Note, if the first
    /// bound is larger than the second bound, that means the claim wraps
    /// around the end of u32::MAX to the other side.
    pub storage_arc: Option<(u32, u32)>,
}

/// Signed agent information.
#[derive(Debug)]
// no, this is not non-exhaustive, we just want to prevent manual creation
#[allow(clippy::manual_non_exhaustive)]
pub struct AgentInfoSigned {
    /// The decoded information associated with this agent.
    pub agent_info: AgentInfo,

    /// The encoded information that was signed.
    pub encoded: String,

    /// The signature.
    pub signature: bytes::Bytes,

    /// Make sure this struct cannot be constructed manually.
    /// It should either come from signing an [AgentInfo] or from serde.
    _private: (),
}

impl AgentInfoSigned {
    /// Generate a signed agent info by signing an agent info.
    pub async fn sign<S: Signer>(
        signer: &S,
        agent_info: AgentInfo,
    ) -> std::io::Result<std::sync::Arc<Self>> {
        let encoded = serde_json::to_string(&agent_info)?;
        let signature = signer.sign(&agent_info, encoded.as_bytes()).await?;
        Ok(std::sync::Arc::new(Self {
            agent_info,
            encoded,
            signature,
            _private: (),
        }))
    }

    /// Decode a canonical json encoding of a signed agent info.
    pub fn decode<V: Verifier>(
        verifier: &V,
        encoded: &[u8],
    ) -> std::io::Result<Self> {
        #[derive(serde::Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Ref {
            agent_info: String,
            #[serde(with = "crate::serde_bytes_base64")]
            signature: bytes::Bytes,
        }
        let v: Ref = serde_json::from_slice(encoded)?;
        let agent_info: AgentInfo = serde_json::from_str(&v.agent_info)?;
        if !verifier.verify(&agent_info, v.agent_info.as_bytes(), &v.signature)
        {
            return Err(std::io::Error::other("InvalidSignature"));
        }
        Ok(AgentInfoSigned {
            agent_info,
            encoded: v.agent_info,
            signature: v.signature,
            _private: (),
        })
    }

    /// Get the canonical json encoding of this signed agent info.
    pub fn encode(&self) -> std::io::Result<String> {
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
        .map_err(std::convert::Into::into)
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

    struct TestCrypto;

    impl Signer for TestCrypto {
        fn sign(
            &self,
            _agent_info: &AgentInfo,
            _encoded: &[u8],
        ) -> BoxFut<'_, std::io::Result<bytes::Bytes>> {
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
        let later = Timestamp::from_micros(1731762797907204);
        let url = Some("test-url".into());
        let storage_arc = Some((42, u32::MAX / 13));

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
            r#"{"agentInfo":"{\"agent\":\"dGVzdC1hZ2VudA\",\"space\":\"dGVzdC1zcGFjZQ\",\"createdAt\":\"1731690797907204\",\"expiresAt\":\"1731762797907204\",\"isTombstone\":false,\"url\":\"test-url\",\"storageArc\":[42,330382099]}","signature":"ZmFrZS1zaWduYXR1cmU"}"#,
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
}
