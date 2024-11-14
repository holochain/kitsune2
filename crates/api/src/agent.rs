//! Types dealing with agent metadata.

use crate::*;

/// Additional metadata associated with an agent.
/// This struct represents the extensibility of agent info,
/// everything in here must be optional or provide a sane default.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub struct AgentInfoMetadata {
    /// If set, this indicates the primary url at which this agent may
    /// be reached. This should largely only be UNSET if this is a tombstone.
    pub url1: Option<String>,

    /// If unset, this agent is claiming a zero storage arc,
    /// that is, they are saying they store nothing.
    /// If set, this indicates the inclusive bounds which this agent
    /// claims they are an authority for storage. Note, if the first
    /// bound is after the second bound, that means the claim wraps
    /// around the end of u32::MAX to the other side.
    pub storage_arc1: Option<(u32, u32)>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", tag = "type")]
struct AgentInfoStringTimestamps {
    agent: AgentId,
    space: SpaceId,
    created_at: String,
    expires_at: String,
    is_tombstone: bool,
    metadata: AgentInfoMetadata,
}

impl std::convert::TryFrom<AgentInfoStringTimestamps> for AgentInfo {
    type Error = std::io::Error;
    fn try_from(a: AgentInfoStringTimestamps) -> Result<Self, Self::Error> {
        let created_at: i64 = a.created_at.parse().map_err(std::io::Error::other)?;
        let expires_at: i64 = a.expires_at.parse().map_err(std::io::Error::other)?;
        Ok(Self {
            agent: a.agent,
            space: a.space,
            created_at: Timestamp::from_micros(created_at),
            expires_at: Timestamp::from_micros(expires_at),
            is_tombstone: a.is_tombstone,
            metadata: a.metadata,
        })
    }
}

impl From<AgentInfo> for AgentInfoStringTimestamps {
    fn from(a: AgentInfo) -> Self {
        Self {
            agent: a.agent,
            space: a.space,
            created_at: a.created_at.as_micros().to_string(),
            expires_at: a.expires_at.as_micros().to_string(),
            is_tombstone: a.is_tombstone,
            metadata: a.metadata,
        }
    }
}

/// Agent information.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", tag = "type", try_from = "AgentInfoStringTimestamps", into = "AgentInfoStringTimestamps")]
pub struct AgentInfo {
    /// The agent id.
    pub agent: AgentId,

    /// The space id.
    pub space: SpaceId,

    /// When this metadata was created.
    pub created_at: Timestamp,

    /// When this metadata will expire.
    pub expires_at: Timestamp,

    /// If `true`, this metadata is a tombstone, indicating
    /// the agent has gone offline, and is no longer reachable.
    pub is_tombstone: bool,

    /// Additional metadata associated with this agent.
    pub metadata: AgentInfoMetadata,
}

/// Signed agent information.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub struct AgentInfoSigned {
    /// The decoded information associated with this agent.
    pub info: AgentInfo,

    /// The encoded information that was signed.
    pub encoded: String,

    /// The signature.
    pub signature: bytes::Bytes,

    /// Make sure this struct cannot be constructed manually.
    /// It should either come from signing an [AgentInfo] or from serde.
    #[serde(skip)]
    _private: (),
}

impl std::ops::Deref for AgentInfoSigned {
    type Target = AgentInfo;

    fn deref(&self) -> &Self::Target {
        &self.info
    }
}
