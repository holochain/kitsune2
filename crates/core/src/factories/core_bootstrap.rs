//! The core bootstrap implementation provided by Kitsune2.

use kitsune2_api::{bootstrap::*, config::*, *};
use std::sync::Arc;

#[cfg(feature = "test_utils")]
use kitsune2_test_utils::test_bootstrap_srv::TestBootstrapSrv;

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
            let out: DynBootstrap = Arc::new(CoreBootstrap::new(
                builder, config, peer_store, space,
            ));
            Ok(out)
        })
    }
}

type PushSend = tokio::sync::mpsc::Sender<Arc<agent::AgentInfoSigned>>;
type PushRecv = tokio::sync::mpsc::Receiver<Arc<agent::AgentInfoSigned>>;

#[derive(Debug)]
struct CoreBootstrap {
    space: SpaceId,
    push_send: PushSend,
    push_task: tokio::task::JoinHandle<()>,
    poll_task: tokio::task::JoinHandle<()>,
    #[cfg(feature = "test_utils")]
    _test_server: Option<Arc<TestBootstrapSrv>>,
}

impl Drop for CoreBootstrap {
    fn drop(&mut self) {
        self.push_task.abort();
        self.poll_task.abort();
    }
}

impl CoreBootstrap {
    pub fn new(
        builder: Arc<builder::Builder>,
        config: CoreBootstrapConfig,
        peer_store: peer_store::DynPeerStore,
        space: SpaceId,
    ) -> Self {
        #[cfg(not(feature = "test_utils"))]
        let server_url: Arc<str> = config.server_url.into_boxed_str().into();

        #[cfg(feature = "test_utils")]
        let (server_url, _test_server) = {
            let mut server_url: Arc<str> =
                config.server_url.into_boxed_str().into();
            let test_server = if server_url.starts_with("test:") {
                let test_server = TestBootstrapSrv::new(server_url);
                server_url = test_server.server_address().into();
                Some(test_server)
            } else {
                None
            };

            (server_url, test_server)
        };

        let (push_send, push_recv) = tokio::sync::mpsc::channel(1024);

        let push_task = tokio::task::spawn(push_task(
            server_url.clone(),
            push_send.clone(),
            push_recv,
        ));

        let poll_task = tokio::task::spawn(poll_task(
            builder,
            server_url,
            space.clone(),
            peer_store,
        ));

        Self {
            space,
            push_send,
            push_task,
            poll_task,
            #[cfg(feature = "test_utils")]
            _test_server,
        }
    }
}

impl Bootstrap for CoreBootstrap {
    fn put(&self, info: Arc<agent::AgentInfoSigned>) {
        // ignore puts outside our space.
        if info.space != self.space {
            return;
        }

        // if we can't push onto our large buffer channel... we've got problems
        let _ = self.push_send.try_send(info);
    }
}

async fn push_task(
    server_url: Arc<str>,
    push_send: PushSend,
    mut push_recv: PushRecv,
) {
    const MIN: std::time::Duration = std::time::Duration::from_secs(5);
    const MAX: std::time::Duration = std::time::Duration::from_secs(60 * 5);
    let mut wait = None;

    while let Some(info) = push_recv.recv().await {
        let url =
            format!("{server_url}/bootstrap/{}/{}", &info.space, &info.agent,);
        let enc = match info.encode() {
            Err(_) => continue,
            Ok(enc) => enc,
        };
        match tokio::task::spawn_blocking(move || {
            ureq::put(&url).send_string(&enc)
        })
        .await
        {
            Ok(Ok(_)) => {
                wait = None;
            }
            _ => {
                // send it back to try again
                let _ = push_send.try_send(info);
                match wait {
                    None => wait = Some(MIN),
                    Some(p) => {
                        let mut p = p * 2;
                        if p > MAX {
                            p = MAX;
                        }
                        wait = Some(p);
                    }
                }
                if let Some(wait) = &wait {
                    tokio::time::sleep(*wait).await;
                }
            }
        }
    }
}

async fn poll_task(
    builder: Arc<builder::Builder>,
    server_url: Arc<str>,
    space: SpaceId,
    peer_store: peer_store::DynPeerStore,
) {
    const MAX: std::time::Duration = std::time::Duration::from_secs(60 * 5);
    let mut wait = std::time::Duration::from_secs(5);

    loop {
        let url = format!("{server_url}/bootstrap/{space}");
        if let Ok(Ok(data)) = tokio::task::spawn_blocking(move || {
            ureq::get(&url)
                .call()
                .map_err(std::io::Error::other)?
                .into_string()
        })
        .await
        {
            if let Ok(list) = agent::AgentInfoSigned::decode_list(
                &builder.verifier,
                data.as_bytes(),
            ) {
                // count decoding a success, and set the wait to max
                wait = MAX;

                let list = list
                    .into_iter()
                    .filter_map(|l| match l {
                        Ok(l) => Some(l),
                        Err(_) => None,
                    })
                    .collect::<Vec<_>>();

                let _ = peer_store.insert(list).await;
            }
        }

        wait *= 2;
        if wait > MAX {
            wait = MAX;
        }

        tokio::time::sleep(wait).await;
    }
}
