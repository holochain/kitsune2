//! Test Utility helpers around agents and agent info.

use kitsune2_api::*;
use std::sync::{Arc, Mutex};

/// A default test space id.
pub const TEST_SPACE: SpaceId =
    SpaceId(id::Id(bytes::Bytes::from_static(b"test-space")));

/// The test signature bytes.
pub const TEST_SIG: &[u8] = b"test-signature";

/// Test Verifier considers only signatures with the bytes
/// `b"test-signature"` to be valid.
#[derive(Debug)]
pub struct TestVerifier;

impl agent::Verifier for TestVerifier {
    fn verify(
        &self,
        _agent_info: &agent::AgentInfo,
        _message: &[u8],
        signature: &[u8],
    ) -> bool {
        signature == TEST_SIG
    }
}

struct TestLocalAgentInner {
    cb: Option<Arc<dyn Fn() + 'static + Send + Sync>>,
    cur: DhtArc,
    tgt: DhtArc,
}

/// A test local agent is generated from incrementing id keys,
/// and always signs with the signature `b"test-signature"`.
pub struct TestLocalAgent {
    id: AgentId,
    inner: Mutex<TestLocalAgentInner>,
}

impl std::fmt::Debug for TestLocalAgent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let storage_arc = self.inner.lock().unwrap().cur;
        f.debug_struct("TestLocalAgent")
            .field("agent", &self.id)
            .field("storage_arc", &storage_arc)
            .finish()
    }
}

impl Default for TestLocalAgent {
    fn default() -> Self {
        use std::sync::atomic::*;
        static NXT: AtomicU64 = AtomicU64::new(1);
        let nxt = NXT.fetch_add(1, Ordering::Relaxed);
        let mut id = bytes::BytesMut::new();
        id.extend_from_slice(b"test-");
        id.extend_from_slice(&nxt.to_le_bytes());
        let id = AgentId::from(id.freeze());
        Self {
            id,
            inner: Mutex::new(TestLocalAgentInner {
                cb: None,
                cur: DhtArc::Empty,
                tgt: DhtArc::Empty,
            }),
        }
    }
}

impl agent::Signer for TestLocalAgent {
    fn sign<'a, 'b: 'a, 'c: 'a>(
        &'a self,
        _agent_info: &'b agent::AgentInfo,
        _message: &'c [u8],
    ) -> BoxFut<'a, K2Result<bytes::Bytes>> {
        Box::pin(async move { Ok(bytes::Bytes::from_static(TEST_SIG)) })
    }
}

impl agent::LocalAgent for TestLocalAgent {
    fn agent(&self) -> &AgentId {
        &self.id
    }

    fn register_cb(&self, cb: Arc<dyn Fn() + 'static + Send + Sync>) {
        self.inner.lock().unwrap().cb = Some(cb);
    }

    fn invoke_cb(&self) {
        let cb = self.inner.lock().unwrap().cb.clone();
        if let Some(cb) = cb {
            cb();
        }
    }

    fn get_cur_storage_arc(&self) -> DhtArc {
        self.inner.lock().unwrap().cur
    }

    fn set_cur_storage_arc(&self, arc: DhtArc) {
        self.inner.lock().unwrap().cur = arc;
    }

    fn get_tgt_storage_arc(&self) -> DhtArc {
        self.inner.lock().unwrap().tgt
    }

    fn set_tgt_storage_arc_hint(&self, arc: DhtArc) {
        self.inner.lock().unwrap().tgt = arc;
    }
}

/// Agent builder for testing.
#[derive(Debug, Default)]
pub struct AgentBuilder {
    /// Optional agent id. If this is provided it will overwrite the agent
    /// provided by the signer.
    pub agent: Option<AgentId>,
    /// Optional space id. If not provided, this will be `b"test-space"`.
    pub space: Option<SpaceId>,
    /// Optional created at timestamp. If not provided, will be now.
    pub created_at: Option<Timestamp>,
    /// Optional expires at timestamp. If not provided, will be now + 20min.
    pub expires_at: Option<Timestamp>,
    /// Optional tombstone flag. If not provided, will be false.
    pub is_tombstone: Option<bool>,
    /// Optional peer url. If not provided, will be None.
    pub url: Option<Option<Url>>,
    /// Optional storage arc. If not provided, will be DhtArc::FULL.
    pub storage_arc: Option<DhtArc>,
}

impl AgentBuilder {
    /// Build an agent from given values or defaults.
    pub fn build<A: agent::LocalAgent>(
        self,
        a: A,
    ) -> Arc<agent::AgentInfoSigned> {
        let agent = self.agent.unwrap_or_else(|| a.agent().clone());
        let space = self.space.unwrap_or_else(|| TEST_SPACE.clone());
        let created_at = self.created_at.unwrap_or_else(Timestamp::now);
        let expires_at = self.expires_at.unwrap_or_else(|| {
            created_at + std::time::Duration::from_secs(60 * 20)
        });
        let is_tombstone = self.is_tombstone.unwrap_or(false);
        let url = self.url.unwrap_or(None);
        let storage_arc = self.storage_arc.unwrap_or(DhtArc::FULL);
        let agent_info = agent::AgentInfo {
            agent,
            space,
            created_at,
            expires_at,
            is_tombstone,
            url,
            storage_arc,
        };
        futures::executor::block_on(agent::AgentInfoSigned::sign(
            &a, agent_info,
        ))
        .unwrap()
    }
}
