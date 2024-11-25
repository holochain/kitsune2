/// An entry with known content.
#[derive(Debug)]
pub struct ParsedEntry {
    /// agent
    pub agent: ed25519_dalek::VerifyingKey,

    /// space
    pub space: bytes::Bytes,

    /// created_at
    pub created_at: i64,

    /// expires_at
    pub expires_at: i64,

    /// is_tombstone
    pub is_tombstone: bool,

    /// encoded
    pub encoded: String,

    /// signature
    pub signature: ed25519_dalek::Signature,
}

impl ParsedEntry {
    /// Parse entry from a slice.
    pub fn try_from_slice(slice: &[u8]) -> std::io::Result<Self> {
        use base64::prelude::*;

        #[derive(serde::Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Outer {
            agent_info: String,
            signature: String,
        }

        #[derive(serde::Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Inner {
            agent: String,
            space: String,
            created_at: String,
            expires_at: String,
            is_tombstone: bool,
        }

        let out: Outer = serde_json::from_slice(slice)?;
        let inn: Inner = serde_json::from_str(&out.agent_info)?;

        let agent: [u8; 32] = BASE64_URL_SAFE_NO_PAD
            .decode(inn.agent)
            .map_err(std::io::Error::other)?
            .try_into()
            .map_err(|_| std::io::Error::other("InvalidAgentPubKey"))?;

        let signature: [u8; 64] = BASE64_URL_SAFE_NO_PAD
            .decode(out.signature)
            .map_err(std::io::Error::other)?
            .try_into()
            .map_err(|_| std::io::Error::other("InvalidSignature"))?;

        Ok(ParsedEntry {
            agent: ed25519_dalek::VerifyingKey::from_bytes(&agent)
                .map_err(std::io::Error::other)?,
            space: BASE64_URL_SAFE_NO_PAD
                .decode(inn.space)
                .map_err(std::io::Error::other)?
                .into(),
            created_at: inn
                .created_at
                .parse()
                .map_err(std::io::Error::other)?,
            expires_at: inn
                .expires_at
                .parse()
                .map_err(std::io::Error::other)?,
            is_tombstone: inn.is_tombstone,
            encoded: out.agent_info,
            signature: ed25519_dalek::Signature::from_bytes(&signature),
        })
    }
}
