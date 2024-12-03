#![deny(missing_docs)]
//! Kitsune2 p2p / dht communication framework.

use kitsune2_api::{builder::Builder, config::Config, *};

/// A default [kitsune2_api::agent::Verifier] based on ed25519_dalek.
#[derive(Debug)]
pub struct Ed25519Verifier;

impl agent::Verifier for Ed25519Verifier {
    fn verify(
        &self,
        agent_info: &agent::AgentInfo,
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

/// Construct a production-ready default builder.
///
/// - `verifier` - The default verifier is [Ed25519Verifier].
/// - `space` - The default space module is [factories::CoreSpaceFactory].
/// - `peer_store` - The default peer store is [factories::MemPeerStoreFactory].
pub fn default_builder() -> Builder {
    Builder {
        config: Config::default(),
        verifier: std::sync::Arc::new(Ed25519Verifier),
        space: factories::CoreSpaceFactory::create(),
        peer_store: factories::MemPeerStoreFactory::create(),
    }
}

pub mod factories;
