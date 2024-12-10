//! The core bootstrap implementation provided by Kitsune2.

use kitsune2_api::{bootstrap::*, config::*, *};
use std::sync::Arc;

const MOD_NAME: &str = "CoreBootstrap";

/// Configuration parameters for [CoreBootstrapFactory].
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CoreBootstrapConfig {
    /// The url of the kitsune2 bootstrap server. E.g. `https://boot.kitsu.ne`.
    ///
    /// This defaults to a "test:" scheme with a thread id. This
    /// gives us separate buckets to partition rust tests that just
    /// happen to be running in the same process. If you are starting
    /// kitsune nodes across multiple threads that you want to communicate
    /// with each other for testing, you'll need to specify an explicit
    /// `test:<my-unique-string-here>` to this config.
    pub server_url: String,
}

impl Default for CoreBootstrapConfig {
    fn default() -> Self {
        Self {
            server_url: format!("test:{:?}", std::thread::current().id()),
        }
    }
}

impl ModConfig for CoreBootstrapConfig {}

/// The core bootstrap implementation provided by Kitsune2.
#[derive(Debug)]
pub struct CoreBootstrapFactory {}

impl CoreBootstrapFactory {
    /// Construct a new CoreBootstrapFactory.
    pub fn create() -> DynBootstrapFactory {
        let out: DynBootstrapFactory = Arc::new(CoreBootstrapFactory {});
        out
    }
}

impl BootstrapFactory for CoreBootstrapFactory {
    fn default_config(&self, config: &mut Config) -> K2Result<()> {
        config.add_default_module_config::<CoreBootstrapConfig>(
            MOD_NAME.into(),
        )?;
        Ok(())
    }

    fn create(
        &self,
        builder: Arc<builder::Builder>,
        peer_store: peer_store::DynPeerStore,
        space: SpaceId,
    ) -> BoxFut<'static, K2Result<DynBootstrap>> {
        Box::pin(async move {
            let config = builder
                .config
                .get_module_config::<CoreBootstrapConfig>(MOD_NAME)?;
            let out: DynBootstrap =
                Arc::new(CoreBootstrap::new(config, peer_store, space));
            Ok(out)
        })
    }
}

#[derive(Debug)]
struct CoreBootstrap {}

impl CoreBootstrap {
    pub fn new(
        _config: CoreBootstrapConfig,
        _peer_store: peer_store::DynPeerStore,
        _space: SpaceId,
    ) -> Self {
        Self {}
    }
}

impl Bootstrap for CoreBootstrap {
    fn put(&self, _info: Arc<agent::AgentInfoSigned>) {
        todo!()
    }
}
