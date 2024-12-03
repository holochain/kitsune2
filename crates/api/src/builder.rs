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

    /// The [space::SpaceFactory] to be used for creating
    /// [space::Space] instances.
    pub space: space::DynSpaceFactory,

    /// The [peer_store::PeerStoreFactory] to be used for creating
    /// [peer_store::PeerStore] instances.
    pub peer_store: peer_store::DynPeerStoreFactory,
}

impl Builder {
    /// Construct a default config given the configured module factories.
    /// Note, this should be called before freezing the Builder instance
    /// in an Arc<>.
    pub fn set_default_config(&mut self) -> K2Result<()> {
        let Self {
            config,
            verifier: _,
            space,
            peer_store,
        } = self;

        space.default_config(config)?;
        peer_store.default_config(config)?;

        Ok(())
    }

    /// This will generate an actual kitsune instance.
    // TODO - the result type of this build function is temporarilly
    //        an Arc of the builder itself. Once we have the Kitsune
    //        factory, this will produce an actual Kitsune instance.
    pub fn build(self) -> Arc<Self> {
        Arc::new(self)
    }
}
