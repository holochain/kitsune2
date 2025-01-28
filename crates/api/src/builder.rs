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

    /// The [bootstrap::BootstrapFactory] to be used for creating
    /// [bootstrap::Bootstrap] instances for initial WAN discovery.
    pub bootstrap: bootstrap::DynBootstrapFactory,

    /// The [fetch::FetchFactory] to be used for creating
    /// [fetch::Fetch] instances.
    pub fetch: fetch::DynFetchFactory,

    /// The [transport::TransportFactory] to be used for creating
    /// [transport::Transport] instances.
    pub transport: transport::DynTransportFactory,

    /// The [op_store::OpStoreFactory] to be used for creating
    /// [op_store::OpStore] instances.
    pub op_store: op_store::DynOpStoreFactory,

    /// The [peer_meta_store::PeerMetaStoreFactory] to be used for creating
    /// [peer_meta_store::PeerMetaStore] instances.
    pub peer_meta_store: peer_meta_store::DynPeerMetaStoreFactory,

    /// The [gossip::GossipFactory] to be used for creating
    /// [gossip::Gossip] instances.
    pub gossip: gossip::DynGossipFactory,
    /// The [local_agent_store::LocalAgentStoreFactory] to be used for creating
    /// [local_agent_store::LocalAgentStore] instances.
    pub local_agent_store: Arc<dyn LocalAgentStoreFactory>,
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

            config.mark_defaults_set();
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

        self.config.mark_validated();

        Ok(())
    }

    /// Generate the actual kitsune instance, validating configuration
    /// if that has not already explicitly been done.
    pub async fn build(
        self,
        handler: kitsune::DynKitsuneHandler,
    ) -> K2Result<kitsune::DynKitsune> {
        if !self.config.mark_validated() {
            self.validate_config()?;
        }

        self.config.mark_runtime();
        let builder = Arc::new(self);

        builder.kitsune.create(builder.clone(), handler).await
    }
}
