//! The core bootstrap implementation provided by Kitsune2.

use kitsune2_api::*;
use std::sync::Arc;

/// CoreBootstrap configuration types.
pub mod config {
    /// Configuration parameters for [CoreBootstrapFactory](super::CoreBootstrapFactory).
    #[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
    #[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
    #[serde(rename_all = "camelCase")]
    pub struct CoreBootstrapConfig {
        /// The url of the kitsune2 bootstrap server. E.g. `https://boot.kitsu.ne`.
        pub server_url: String,

        /// Minimum backoff in ms to use for both push and poll retry loops.
        ///
        /// Default: 5 seconds.
        #[cfg_attr(feature = "schema", schemars(default))]
        pub backoff_min_ms: u32,

        /// Maximum backoff in ms to use for both push and poll retry loops.
        ///
        /// Default: 5 minutes.
        #[cfg_attr(feature = "schema", schemars(default))]
        pub backoff_max_ms: u32,
    }

    impl Default for CoreBootstrapConfig {
        fn default() -> Self {
            Self {
                server_url: "<https://your.bootstrap.url>".into(),
                backoff_min_ms: 1000 * 5,
                backoff_max_ms: 1000 * 60 * 5,
            }
        }
    }

    impl CoreBootstrapConfig {
        /// Get the minimum backoff duration.
        pub fn backoff_min(&self) -> std::time::Duration {
            std::time::Duration::from_millis(self.backoff_min_ms as u64)
        }

        /// Get the maximum backoff duration.
        pub fn backoff_max(&self) -> std::time::Duration {
            std::time::Duration::from_millis(self.backoff_max_ms as u64)
        }
    }

    /// Module-level configuration for CoreBootstrap.
    #[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
    #[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
    #[serde(rename_all = "camelCase")]
    pub struct CoreBootstrapModConfig {
        /// CoreBootstrap configuration.
        pub core_bootstrap: CoreBootstrapConfig,
    }
}

pub use config::*;

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
        config.set_module_config(&CoreBootstrapModConfig::default())
    }

    fn validate_config(&self, config: &Config) -> K2Result<()> {
        const ERR: &str = "invalid bootstrap server_url";

        let config: CoreBootstrapModConfig = config.get_module_config()?;

        let url = url::Url::parse(&config.core_bootstrap.server_url)
            .map_err(|e| K2Error::other_src(ERR, e))?;

        if url.cannot_be_a_base() {
            return Err(K2Error::other(ERR));
        }

        match url.scheme() {
            "http" | "https" => Ok(()),
            _ => Err(K2Error::other(ERR)),
        }
    }

    fn create(
        &self,
        builder: Arc<Builder>,
        peer_store: DynPeerStore,
        space_id: SpaceId,
    ) -> BoxFut<'static, K2Result<DynBootstrap>> {
        Box::pin(async move {
            let config: CoreBootstrapModConfig =
                builder.config.get_module_config()?;
            let out: DynBootstrap = Arc::new(CoreBootstrap::new(
                builder,
                config.core_bootstrap,
                peer_store,
                space_id,
            ));
            Ok(out)
        })
    }
}

type PushSend = tokio::sync::mpsc::Sender<Arc<AgentInfoSigned>>;
type PushRecv = tokio::sync::mpsc::Receiver<Arc<AgentInfoSigned>>;

#[derive(Debug)]
struct CoreBootstrap {
    space_id: SpaceId,
    push_send: PushSend,
    push_task: tokio::task::JoinHandle<()>,
    poll_task: tokio::task::JoinHandle<()>,
}

impl Drop for CoreBootstrap {
    fn drop(&mut self) {
        self.push_task.abort();
        self.poll_task.abort();
    }
}

impl CoreBootstrap {
    pub fn new(
        builder: Arc<Builder>,
        config: CoreBootstrapConfig,
        peer_store: DynPeerStore,
        space_id: SpaceId,
    ) -> Self {
        let auth_material =
            Arc::new(builder.auth_material.as_ref().map(|auth_material| {
                kitsune2_bootstrap_client::AuthMaterial::new(
                    auth_material.clone(),
                )
            }));

        let (push_send, push_recv) = tokio::sync::mpsc::channel(1024);

        let push_task = tokio::task::spawn(push_task(
            config.clone(),
            push_send.clone(),
            push_recv,
            auth_material.clone(),
        ));

        let poll_task = tokio::task::spawn(poll_task(
            builder,
            config,
            space_id.clone(),
            peer_store,
            auth_material,
        ));

        Self {
            space_id,
            push_send,
            push_task,
            poll_task,
        }
    }
}

impl Bootstrap for CoreBootstrap {
    fn put(&self, info: Arc<AgentInfoSigned>) {
        // ignore puts outside our space.
        if info.space_id != self.space_id {
            tracing::error!(
                ?info,
                "Logic Error: Attempting to put an agent outside of this space"
            );
            return;
        }

        // if we can't push onto our large buffer channel... we've got problems
        if let Err(err) = self.push_send.try_send(info) {
            tracing::warn!(?err, "Bootstrap overloaded, dropping put");
        }
    }
}

async fn push_task(
    config: CoreBootstrapConfig,
    push_send: PushSend,
    mut push_recv: PushRecv,
    auth_material: Arc<Option<kitsune2_bootstrap_client::AuthMaterial>>,
) {
    // Already checked to be a valid URL by the config validation.
    let server_url =
        url::Url::parse(&config.server_url).expect("invalid server url");

    let mut wait = None;

    while let Some(info) = push_recv.recv().await {
        match tokio::task::spawn_blocking({
            let auth_material = auth_material.clone();
            let server_url = server_url.clone();
            let info = info.clone();
            move || {
                kitsune2_bootstrap_client::blocking_put_auth(
                    server_url,
                    &info,
                    auth_material.as_ref().as_ref(),
                )
            }
        })
        .await
        {
            Ok(Ok(_)) => {
                // the put was successful, we don't need to wait
                // before sending the next info if it is ready
                wait = None;
            }
            err => {
                tracing::debug!(
                    ?err,
                    "Failed to push agent info to bootstrap server"
                );

                let now = Timestamp::now();

                // the put failed, send it back to try again if not expired
                if info.expires_at > now {
                    let _ = push_send.try_send(info);
                }

                // we need to configure a backoff so we don't hammer the server
                match wait {
                    None => wait = Some(config.backoff_min()),
                    Some(p) => {
                        let mut p = p * 2;
                        if p > config.backoff_max() {
                            p = config.backoff_max();
                        }
                        wait = Some(p);
                    }
                }

                // wait for the backoff time
                if let Some(wait) = &wait {
                    tokio::time::sleep(*wait).await;
                }
            }
        }
    }
}

async fn poll_task(
    builder: Arc<Builder>,
    config: CoreBootstrapConfig,
    space_id: SpaceId,
    peer_store: DynPeerStore,
    auth_material: Arc<Option<kitsune2_bootstrap_client::AuthMaterial>>,
) {
    // Already checked to be a valid URL by the config validation.
    let server_url =
        url::Url::parse(&config.server_url).expect("invalid server url");

    let mut wait = config.backoff_min();

    loop {
        match tokio::task::spawn_blocking({
            let auth_material = auth_material.clone();
            let server_url = server_url.clone();
            let space_id = space_id.clone();
            let verifier = builder.verifier.clone();
            move || {
                kitsune2_bootstrap_client::blocking_get_auth(
                    server_url,
                    space_id.clone(),
                    verifier,
                    auth_material.as_ref().as_ref(),
                )
            }
        })
        .await
        .map_err(|_| K2Error::other("task join error"))
        {
            Err(err) | Ok(Err(err)) => {
                tracing::debug!(?err, "failure contacting bootstrap server");
            }
            Ok(Ok(list)) => {
                let _ = peer_store.insert(list).await;
            }
        }

        wait *= 2;
        if wait > config.backoff_max() {
            wait = config.backoff_max();
        }

        tokio::time::sleep(wait).await;
    }
}

#[cfg(test)]
mod test;
