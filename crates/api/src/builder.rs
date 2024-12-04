//! Builder-related types.

use crate::*;
use std::sync::Arc;

/// The general Kitsune2 builder.
/// This contains both configuration and factory instances,
/// allowing construction of runtime module instances.
#[derive(Debug)]
pub struct Builder {
    /// The module configuration to be used when building modules.
    /// This can be loaded from disk or modified before freezing the builder.
    pub config: crate::config::Config,

    /// The [agent::Verifier] to use for this Kitsune2 instance.
    pub verifier: agent::DynVerifier,

    /// The [kitsune::KitsuneFactory] to be used for creating
    /// [kitsune::Kitsune] module instances.
    pub kitsune: kitsune::DynKitsuneFactory,

    /// The [space::SpaceFactory] to be used for creating
    /// [space::Space] instances.
    pub space: space::DynSpaceFactory,

    /// The [peer_store::PeerStoreFactory] to be used for creating
    /// [peer_store::PeerStore] instances.
    pub peer_store: peer_store::DynPeerStoreFactory,

    /// The [fetch::FetchQueueFactory] to be used for creating
    /// [fetch::FetchQueue] instances.
    pub fetch_queue: fetch::DynFetchFactory,
}

impl Builder {
    /// Construct a default config given the configured module factories.
    /// Note, this should be called before freezing the Builder instance
    /// in an Arc<>.
    pub fn set_default_config(&mut self) -> K2Result<()> {
        let Self {
            config,
            verifier: _,
            kitsune,
            space,
            peer_store,
            fetch_queue,
        } = self;

        kitsune.default_config(config)?;
        space.default_config(config)?;
        peer_store.default_config(config)?;
        fetch_queue.default_config(config)?;

        Ok(())
    }

    /// This will generate an actual kitsune instance.
    pub async fn build(
        self,
        handler: kitsune::DynKitsuneHandler,
    ) -> K2Result<kitsune::DynKitsune> {
        let builder = Arc::new(self);
        builder.kitsune.create(builder.clone(), handler).await
    }
}
