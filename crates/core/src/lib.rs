#![deny(missing_docs)]
//! Reference implementations of the [Kitsune2 API](kitsune2_api).

use kitsune2_api::*;
use std::sync::{Arc, Mutex};

/// A default [Verifier] based on ed25519.
#[derive(Debug)]
pub struct Ed25519Verifier;

impl Verifier for Ed25519Verifier {
    fn verify(
        &self,
        agent_info: &AgentInfo,
        message: &[u8],
        signature: &[u8],
    ) -> bool {
        use ed25519_dalek::Verifier;

        let agent: [u8; 32] = match (***agent_info.agent).try_into() {
            Ok(agent) => agent,
            Err(_) => return false,
        };

        let agent = match ed25519_dalek::VerifyingKey::from_bytes(&agent) {
            Ok(agent) => agent,
            Err(_) => return false,
        };

        let signature: [u8; 64] = match signature.try_into() {
            Ok(signature) => signature,
            Err(_) => return false,
        };

        let signature = ed25519_dalek::Signature::from_bytes(&signature);

        agent.verify(message, &signature).is_ok()
    }
}

struct Ed25519LocalAgentInner {
    cb: Option<Arc<dyn Fn() + 'static + Send + Sync>>,
    cur: DhtArc,
    tgt: DhtArc,
}

/// A default in-memory [LocalAgent] based on ed25519.
pub struct Ed25519LocalAgent {
    pk: ed25519_dalek::SigningKey,
    id: AgentId,
    inner: Mutex<Ed25519LocalAgentInner>,
}

impl std::fmt::Debug for Ed25519LocalAgent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let storage_arc = self.inner.lock().unwrap().cur;
        f.debug_struct("Ed25519LocalAgent")
            .field("agent", &self.id)
            .field("storage_arc", &storage_arc)
            .finish()
    }
}

impl Default for Ed25519LocalAgent {
    fn default() -> Self {
        Self::new(ed25519_dalek::SigningKey::generate(&mut rand::thread_rng()))
    }
}

impl Ed25519LocalAgent {
    fn new(pk: ed25519_dalek::SigningKey) -> Self {
        let id = bytes::Bytes::copy_from_slice(pk.verifying_key().as_bytes());
        Self {
            pk,
            id: id.into(),
            inner: Mutex::new(Ed25519LocalAgentInner {
                cb: None,
                cur: DhtArc::Empty,
                tgt: DhtArc::Empty,
            }),
        }
    }

    /// Construct an instance from seed bytes.
    pub fn from_seed(seed: &[u8; 32]) -> Self {
        Self::new(ed25519_dalek::SigningKey::from_bytes(seed))
    }

    /// Sign a message with this local agent.
    pub fn sign(&self, message: &[u8]) -> bytes::Bytes {
        use ed25519_dalek::Signer;
        bytes::Bytes::copy_from_slice(&self.pk.sign(message).to_bytes())
    }
}

impl Signer for Ed25519LocalAgent {
    fn sign<'a, 'b: 'a, 'c: 'a>(
        &'a self,
        _agent_info: &'b AgentInfo,
        message: &'c [u8],
    ) -> BoxFut<'a, K2Result<bytes::Bytes>> {
        Box::pin(async move { Ok(self.sign(message)) })
    }
}

impl LocalAgent for Ed25519LocalAgent {
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

/// Construct a default builder for use in tests.
///
/// - `verifier` - The default verifier is [Ed25519Verifier].
/// - `kitsune` - The default top-level kitsune module is
///               [factories::CoreKitsuneFactory].
/// - `space` - The default space module is [factories::CoreSpaceFactory].
/// - `peer_store` - The default peer store is [factories::MemPeerStoreFactory].
/// - `bootstrap` - The default bootstrap is [factories::MemBootstrapFactory].
/// - `fetch` - The default fetch module is [factories::CoreFetchFactory].
/// - `report` - The default report module is [factories::CoreReportFactory].
/// - `transport` - The default transport is [factories::MemTransportFactory].
/// - `op_store` - The default op store is [factories::MemOpStoreFactory].
/// - `peer_meta_store` - The default peer meta store is [factories::MemPeerMetaStoreFactory].
/// - `gossip` - The default gossip module is [factories::CoreGossipStubFactory].
/// - `local_agent_store` - The default local agent store is [factories::CoreLocalAgentStoreFactory].
/// - `publish` - The default publish module is [factories::CorePublishFactory].
/// - `blocks` - The default blocks module is [factories::MemBlocksFactory].
pub fn default_test_builder() -> Builder {
    Builder {
        config: Config::default(),
        verifier: Arc::new(Ed25519Verifier),
        auth_material: None,
        kitsune: factories::CoreKitsuneFactory::create(),
        space: factories::CoreSpaceFactory::create(),
        peer_store: factories::MemPeerStoreFactory::create(),
        bootstrap: factories::MemBootstrapFactory::create(),
        fetch: factories::CoreFetchFactory::create(),
        report: factories::CoreReportFactory::create(),
        transport: factories::MemTransportFactory::create(),
        op_store: factories::MemOpStoreFactory::create(),
        peer_meta_store: factories::MemPeerMetaStoreFactory::create(),
        gossip: factories::CoreGossipStubFactory::create(),
        local_agent_store: factories::CoreLocalAgentStoreFactory::create(),
        publish: factories::CorePublishFactory::create(),
        blocks: factories::MemBlocksFactory::create(),
    }
}

pub mod factories;

mod common;
pub use common::*;

#[cfg(any(doc, docsrs))]
pub mod doc;

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn ed25519_sanity() {
        use kitsune2_api::*;
        use kitsune2_test_utils::agent::*;

        let i1 = AgentBuilder::default().build(Ed25519LocalAgent::default());
        let enc = i1.encode().unwrap();
        let i2 =
            AgentInfoSigned::decode(&Ed25519Verifier, enc.as_bytes()).unwrap();

        assert_eq!(i1, i2);
    }
}
