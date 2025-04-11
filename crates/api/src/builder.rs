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
    pub config: Config,

    /// The [Verifier] to use for this Kitsune2 instance.
    pub verifier: DynVerifier,

    /// The [KitsuneFactory] to be used for creating
    /// [Kitsune] module instances.
    pub kitsune: DynKitsuneFactory,

    /// The [SpaceFactory] to be used for creating
    /// [Space] instances.
    pub space: DynSpaceFactory,

    /// The [PeerStoreFactory] to be used for creating
    /// [PeerStore] instances.
    pub peer_store: DynPeerStoreFactory,

    /// The [BootstrapFactory] to be used for creating
    /// [Bootstrap] instances for initial WAN discovery.
    pub bootstrap: DynBootstrapFactory,

    /// The [FetchFactory] to be used for creating
    /// [Fetch] instances.
    pub fetch: DynFetchFactory,

    /// The [TransportFactory] to be used for creating
    /// [Transport] instances.
    pub transport: DynTransportFactory,

    /// The [OpStoreFactory] to be used for creating
    /// [OpStore] instances.
    pub op_store: DynOpStoreFactory,

    /// The [PeerMetaStoreFactory] to be used for creating
    /// [PeerMetaStore] instances.
    pub peer_meta_store: DynPeerMetaStoreFactory,

    /// The [GossipFactory] to be used for creating
    /// [Gossip] instances.
    pub gossip: DynGossipFactory,

    /// The [LocalAgentStoreFactory] to be used for creating
    /// [LocalAgentStore] instances.
    pub local_agent_store: Arc<dyn LocalAgentStoreFactory>,

    /// The [PublishFactory] to be used for creating [Publish]
    /// instances
    pub publish: DynPublishFactory,
}

impl Builder {
    /// Construct a default config given the configured module factories.
    ///
    /// Note, this should be called before [Self::build] or otherwise
    /// freezing the Builder instance in an Arc<>.
    pub fn with_default_config(mut self) -> K2Result<Self> {
        {
            let Self {
                config,
                verifier: _,
                kitsune,
                space,
                peer_store,
                bootstrap,
                fetch,
                transport,
                op_store,
                peer_meta_store,
                gossip,
                local_agent_store,
                publish,
            } = &mut self;

            kitsune.default_config(config)?;
            space.default_config(config)?;
            peer_store.default_config(config)?;
            bootstrap.default_config(config)?;
            fetch.default_config(config)?;
            transport.default_config(config)?;
            op_store.default_config(config)?;
            peer_meta_store.default_config(config)?;
            gossip.default_config(config)?;
            local_agent_store.default_config(config)?;
            publish.default_config(config)?;
        }

        Ok(self)
    }

    /// Validate the current configuration.
    pub fn validate_config(&self) -> K2Result<()> {
        self.kitsune.validate_config(&self.config)?;
        self.space.validate_config(&self.config)?;
        self.peer_store.validate_config(&self.config)?;
        self.bootstrap.validate_config(&self.config)?;
        self.fetch.validate_config(&self.config)?;
        self.transport.validate_config(&self.config)?;
        self.op_store.validate_config(&self.config)?;
        self.peer_meta_store.validate_config(&self.config)?;
        self.gossip.validate_config(&self.config)?;
        self.local_agent_store.validate_config(&self.config)?;
        self.publish.validate_config(&self.config)?;

        self.config.mark_validated();

        Ok(())
    }

    /// Generate the actual kitsune instance, validating configuration
    /// if that has not already explicitly been done.
    pub async fn build(self) -> K2Result<DynKitsune> {
        if !self.config.mark_validated() {
            self.validate_config()?;
        }

        self.config.mark_runtime();
        let builder = Arc::new(self);

        builder.kitsune.create(builder.clone()).await
    }
}
