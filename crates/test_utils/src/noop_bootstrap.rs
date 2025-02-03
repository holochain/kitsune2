//! A no-op bootstrap implementation.
//!
//! This is useful for testing or for cases where you don't want bootstrap to discover peers.

use kitsune2_api::agent::AgentInfoSigned;
use kitsune2_api::bootstrap::{Bootstrap, BootstrapFactory, DynBootstrap};
use kitsune2_api::builder::Builder;
use kitsune2_api::config::Config;
use kitsune2_api::peer_store::DynPeerStore;
use kitsune2_api::{BoxFut, K2Result, SpaceId};
use std::sync::Arc;

/// A factory for constructing [NoopBootstrap] instances.
#[derive(Debug)]
pub struct NoopBootstrapFactory;

impl BootstrapFactory for NoopBootstrapFactory {
    fn default_config(&self, _config: &mut Config) -> K2Result<()> {
        Ok(())
    }

    fn validate_config(&self, _config: &Config) -> K2Result<()> {
        Ok(())
    }

    fn create(
        &self,
        _builder: Arc<Builder>,
        _peer_store: DynPeerStore,
        _space: SpaceId,
    ) -> BoxFut<'static, K2Result<DynBootstrap>> {
        Box::pin(async move {
            let bootstrap: DynBootstrap = Arc::new(NoopBootstrap {});
            Ok(bootstrap)
        })
    }
}

/// A bootstrap implementation that does nothing.
#[derive(Debug)]
pub struct NoopBootstrap;

impl Bootstrap for NoopBootstrap {
    fn put(&self, _info: Arc<AgentInfoSigned>) {
        // no-op
    }
}
