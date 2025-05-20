//! The mem bootstrap implementation provided by Kitsune2.

use kitsune2_api::*;
use std::sync::{Arc, Mutex};

/// MemBootstrap configuration types.
mod config {
    /// Configuration parameters for [MemBootstrapFactory](super::MemBootstrapFactory).
    #[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
    #[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
    #[serde(rename_all = "camelCase")]
    pub struct MemBootstrapConfig {
        /// Since rust test runs multiple tests in the same process,
        /// we cannot just have a single global bootstrap test store.
        /// This defaults to the current thread id when this config instance
        /// is constructed. This should be sufficient for most needs.
        /// However, if you are creating kitsune nodes in tests from
        /// different tasks, you may need to pick an explicit id for this value.
        pub test_id: String,

        /// How often in ms to update the peer store with bootstrap infos.
        ///
        /// Default: 5s.
        #[cfg_attr(feature = "schema", schemars(default))]
        pub poll_freq_ms: u32,
    }

    impl Default for MemBootstrapConfig {
        fn default() -> Self {
            Self {
                test_id: format!("{:?}", std::thread::current().id()),
                poll_freq_ms: 5000,
            }
        }
    }

    /// Module-level configuration for MemBootstrap.
    #[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
    #[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
    #[serde(rename_all = "camelCase")]
    pub struct MemBootstrapModConfig {
        /// MemBootstrap configuration.
        pub mem_bootstrap: MemBootstrapConfig,
    }
}

pub use config::*;

/// The mem bootstrap implementation provided by Kitsune2.
#[derive(Debug)]
pub struct MemBootstrapFactory {}

impl MemBootstrapFactory {
    /// Construct a new MemBootstrapFactory.
    pub fn create() -> DynBootstrapFactory {
        let out: DynBootstrapFactory = Arc::new(MemBootstrapFactory {});
        out
    }

    /// Testing hook to trigger an immediate bootstrap pull of all
    /// polling tasks that are currently registered.
    pub fn trigger_immediate_poll() {
        NOTIFY.notify_waiters();
    }
}

impl BootstrapFactory for MemBootstrapFactory {
    fn default_config(&self, config: &mut Config) -> K2Result<()> {
        config.set_module_config(&MemBootstrapModConfig::default())
    }

    fn validate_config(&self, _config: &Config) -> K2Result<()> {
        Ok(())
    }

    fn create(
        &self,
        builder: Arc<Builder>,
        peer_store: DynPeerStore,
        space: SpaceId,
    ) -> BoxFut<'static, K2Result<DynBootstrap>> {
        Box::pin(async move {
            let config: MemBootstrapModConfig =
                builder.config.get_module_config()?;
            let out: DynBootstrap = Arc::new(MemBootstrap::new(
                space,
                config.mem_bootstrap,
                peer_store,
            ));
            Ok(out)
        })
    }
}

#[derive(Debug)]
struct MemBootstrap {
    space: SpaceId,
    test_id: TestId,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for MemBootstrap {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl MemBootstrap {
    pub fn new(
        space: SpaceId,
        config: MemBootstrapConfig,
        peer_store: DynPeerStore,
    ) -> Self {
        let test_id: TestId = config.test_id.into_boxed_str().into();
        let test_id2 = test_id.clone();
        let poll_freq =
            std::time::Duration::from_millis(config.poll_freq_ms as u64);
        let space2 = space.clone();
        let task = tokio::task::spawn(async move {
            loop {
                let info_list = stat_process(test_id2.clone(), &space2, None);
                peer_store.insert(info_list).await.unwrap();
                tokio::select! {
                    _ = tokio::time::sleep(poll_freq) => (),
                    _ = NOTIFY.notified() => (),
                }
            }
        });
        Self {
            space,
            test_id,
            task,
        }
    }
}

impl Bootstrap for MemBootstrap {
    fn put(&self, info: Arc<AgentInfoSigned>) {
        let _ = stat_process(self.test_id.clone(), &self.space, Some(info));
    }
}

static NOTIFY: tokio::sync::Notify = tokio::sync::Notify::const_new();

type TestId = Arc<str>;
type Store = Vec<Arc<AgentInfoSigned>>;
type SpaceMap = std::collections::HashMap<SpaceId, Store>;
type TestMap = std::collections::HashMap<TestId, SpaceMap>;
static STAT: std::sync::OnceLock<Mutex<TestMap>> = std::sync::OnceLock::new();
fn stat_process(
    test_id: TestId,
    space: &SpaceId,
    info: Option<Arc<AgentInfoSigned>>,
) -> Vec<Arc<AgentInfoSigned>> {
    let mut lock = STAT.get_or_init(Default::default).lock().unwrap();
    let map = lock.entry(test_id).or_default();
    let store = map.entry(space.clone()).or_default();
    let now = Timestamp::now();
    store.retain(|a| {
        if let Some(info) = info.as_ref() {
            if a.agent == info.agent {
                return false;
            }
        }
        if a.expires_at <= now {
            return false;
        }
        true
    });
    if let Some(info) = info {
        while store.len() > 31 {
            store.remove(16);
        }
        store.push(info);
    }
    store.clone()
}

#[cfg(test)]
mod test;
