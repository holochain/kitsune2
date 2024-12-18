use std::sync::Arc;

use kitsune2_api::{
    agent::{AgentInfo, AgentInfoSigned, Verifier},
    id::Id,
    AgentId, DhtArc, SpaceId, Timestamp, Url,
};

const SPACE_1: SpaceId = SpaceId(Id(bytes::Bytes::from_static(b"space1")));

/// Agent builder for testing.
#[derive(Debug, Default)]
pub struct AgentBuilder {
    /// Optional agent id.
    pub agent: Option<AgentId>,
    /// Optional space id.
    pub space: Option<SpaceId>,
    /// Optional created at timestamp.
    pub created_at: Option<Timestamp>,
    /// Optional expires at timestamp.
    pub expires_at: Option<Timestamp>,
    /// Optional tombstone flag.
    pub is_tombstone: Option<bool>,
    /// Optional peer url.
    pub url: Option<Option<Url>>,
    /// Optional storage arc.
    pub storage_arc: Option<DhtArc>,
}

impl AgentBuilder {
    /// Build an agent from given values or defaults.
    pub fn build(self) -> Arc<AgentInfoSigned> {
        static NEXT_AGENT: std::sync::atomic::AtomicU64 =
            std::sync::atomic::AtomicU64::new(1);
        let agent = self.agent.unwrap_or_else(|| {
            let a =
                NEXT_AGENT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let a = a.to_le_bytes();
            AgentId(Id(bytes::Bytes::copy_from_slice(&a)))
        });
        let space = self.space.unwrap_or_else(|| SPACE_1.clone());
        let created_at = self.created_at.unwrap_or_else(Timestamp::now);
        let expires_at = self.expires_at.unwrap_or_else(|| {
            created_at + std::time::Duration::from_secs(60 * 20)
        });
        let is_tombstone = self.is_tombstone.unwrap_or(false);
        let url = self.url.unwrap_or(None);
        let storage_arc = self.storage_arc.unwrap_or(DhtArc::FULL);
        let agent_info = serde_json::to_string(&AgentInfo {
            agent,
            space,
            created_at,
            expires_at,
            is_tombstone,
            url,
            storage_arc,
        })
        .unwrap();
        #[derive(serde::Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Enc {
            agent_info: String,
            signature: String,
        }
        let encoded = serde_json::to_string(&Enc {
            agent_info,
            signature: "".into(),
        })
        .unwrap();
        #[derive(Debug)]
        struct V;
        impl Verifier for V {
            fn verify(&self, _i: &AgentInfo, _m: &[u8], _s: &[u8]) -> bool {
                true
            }
        }
        // going through this trouble to use decode because it's sync
        AgentInfoSigned::decode(&V, encoded.as_bytes()).unwrap()
    }
}
