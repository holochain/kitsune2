use kitsune2_api::*;
use kitsune2_core::{
    factories::{self, MemOpStoreFactory},
    Ed25519Verifier,
};
use kitsune2_gossip::K2GossipFactory;
use kitsune2_transport_tx5::Tx5TransportFactory;

/// Construct a default production builder for Kitsune2.
///
/// - `verifier` - The default verifier is [Ed25519Verifier].
/// - `kitsune` - The default top-level kitsune module is [factories::CoreKitsuneFactory].
/// - `space` - The default space module is [factories::CoreSpaceFactory].
/// - `peer_store` - The default peer store is [factories::MemPeerStoreFactory].
/// - `bootstrap` - The default bootstrap is [factories::CoreBootstrapFactory].
/// - `fetch` - The default fetch module is [factories::CoreFetchFactory].
/// - `transport` - The default transport is [Tx5TransportFactory].
/// - `op_store` - The default op store is [MemOpStoreFactory].
///                Note: you will likely want to implement your own op store.
/// - `peer_meta_store` - The default peer meta store is [factories::MemPeerMetaStoreFactory].
///                       Note: you will likely want to implement your own peer meta store.
/// - `gossip` - The default gossip module is [K2GossipFactory].
/// - `local_agent_store` - The default local agent store is [factories::CoreLocalAgentStoreFactory].
/// - `publish` - The default publish module is [factories::CorePublishFactory].
pub fn default_builder() -> Builder {
    Builder {
        config: Config::default(),
        verifier: std::sync::Arc::new(Ed25519Verifier),
        kitsune: factories::CoreKitsuneFactory::create(),
        space: factories::CoreSpaceFactory::create(),
        peer_store: factories::MemPeerStoreFactory::create(),
        bootstrap: factories::CoreBootstrapFactory::create(),
        fetch: factories::CoreFetchFactory::create(),
        transport: Tx5TransportFactory::create(),
        op_store: MemOpStoreFactory::create(),
        peer_meta_store: factories::MemPeerMetaStoreFactory::create(),
        gossip: K2GossipFactory::create(),
        local_agent_store: factories::CoreLocalAgentStoreFactory::create(),
        publish: factories::CorePublishFactory::create(),
    }
}
