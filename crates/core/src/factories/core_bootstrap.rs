//! The core bootstrap implementation provided by Kitsune2.

use base64::Engine;
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
        pub server_url: Option<String>,

        /// Base64-encoded auth material for the bootstrap service.
        /// When set via a per-space config override, this takes
        /// precedence over `builder.auth_material_bootstrap`.
        pub auth_material_base64: Option<String>,

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
                server_url: None,
                auth_material_base64: None,
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

    /// Validate the bootstrap configuration for the given context.
    ///
    /// If `is_space` is true, the `bootstrap.server_url` must be set.
    fn validate_config_for_context(
        config: &Config,
        is_space: bool,
    ) -> K2Result<()> {
        const ERR: &str = "invalid bootstrap server_url";

        let config: CoreBootstrapModConfig = config.get_module_config()?;

        if is_space && config.core_bootstrap.server_url.is_none() {
            return Err(K2Error::other(
                "bootstrap server_url must be set before creating a space",
            ));
        }

        if let Some(server_url) = &config.core_bootstrap.server_url {
            let url = url::Url::parse(server_url)
                .map_err(|e| K2Error::other_src(ERR, e))?;

            if url.cannot_be_a_base() {
                return Err(K2Error::other(ERR));
            }

            if !matches!(url.scheme(), "http" | "https") {
                return Err(K2Error::other(ERR));
            }
        }

        Ok(())
    }
}

impl BootstrapFactory for CoreBootstrapFactory {
    fn default_config(&self, config: &mut Config) -> K2Result<()> {
        config.set_module_config(&CoreBootstrapModConfig::default())
    }

    fn validate_config(&self, config: &Config) -> K2Result<()> {
        Self::validate_config_for_context(config, false)
    }

    fn create(
        &self,
        builder: Arc<Builder>,
        peer_store: DynPeerStore,
        space_id: SpaceId,
    ) -> BoxFut<'static, K2Result<DynBootstrap>> {
        Box::pin(async move {
            Self::validate_config_for_context(&builder.config, true)?;
            let config: CoreBootstrapModConfig =
                builder.config.get_module_config()?;
            let out: DynBootstrap = Arc::new(CoreBootstrap::new(
                builder,
                config.core_bootstrap,
                peer_store,
                space_id,
            )?);
            Ok(out)
        })
    }
}

#[derive(Clone, Copy, Debug)]
struct BootstrapTransportUrlState {
    available: bool,
    epoch: u64,
}

#[derive(Debug)]
struct PushItem {
    epoch: u64,
    info: Arc<AgentInfoSigned>,
}

type PushSend = tokio::sync::mpsc::Sender<PushItem>;
type PushRecv = tokio::sync::mpsc::Receiver<PushItem>;

#[derive(Debug)]
struct CoreBootstrap {
    space: SpaceId,
    push_send: PushSend,
    transport_url_available_tx:
        tokio::sync::watch::Sender<BootstrapTransportUrlState>,
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
        space: SpaceId,
    ) -> K2Result<Self> {
        let auth_material = if let Some(b64) = &config.auth_material_base64 {
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(b64)
                .map_err(|e| {
                K2Error::other_src("invalid base64 in auth_material_base64", e)
            })?;
            Arc::new(Some(kitsune2_bootstrap_client::AuthMaterial::new(bytes)))
        } else {
            Arc::new(builder.auth_material_bootstrap.clone().map(|bytes| {
                kitsune2_bootstrap_client::AuthMaterial::new(bytes)
            }))
        };

        let (push_send, push_recv) = tokio::sync::mpsc::channel(1024);
        let (transport_url_available_tx, transport_url_available_rx) =
            tokio::sync::watch::channel(BootstrapTransportUrlState {
                available: true,
                epoch: 0,
            });

        let push_task = tokio::task::spawn(push_task(
            config.clone(),
            transport_url_available_rx.clone(),
            push_recv,
            auth_material.clone(),
        ));

        let poll_task = tokio::task::spawn(poll_task(
            builder,
            config,
            space.clone(),
            peer_store,
            auth_material,
            transport_url_available_rx,
        ));

        Ok(Self {
            space,
            push_send,
            transport_url_available_tx,
            push_task,
            poll_task,
        })
    }
}

impl Bootstrap for CoreBootstrap {
    fn put(&self, info: Arc<AgentInfoSigned>) {
        // ignore puts outside our space.
        if info.space != self.space {
            tracing::error!(
                ?info,
                "Logic Error: Attempting to put an agent outside of this space"
            );
            return;
        }

        let state = *self.transport_url_available_tx.borrow();
        // Live records are regenerated on recovery; tombstones are not.
        if !state.available && !info.is_tombstone {
            return;
        }
        if let Err(err) = self.push_send.try_send(PushItem {
            epoch: state.epoch,
            info,
        }) {
            tracing::warn!(?err, "Bootstrap overloaded, dropping put");
        }
    }

    fn set_transport_url_available(&self, is_transport_url_available: bool) {
        let current = *self.transport_url_available_tx.borrow();
        let epoch = if current.available && !is_transport_url_available {
            current.epoch.wrapping_add(1)
        } else {
            current.epoch
        };
        self.transport_url_available_tx.send_replace(
            BootstrapTransportUrlState {
                available: is_transport_url_available,
                epoch,
            },
        );
    }
}

async fn wait_for_bootstrap_available(
    transport_url_available: &mut tokio::sync::watch::Receiver<
        BootstrapTransportUrlState,
    >,
) -> bool {
    transport_url_available
        .wait_for(|state| state.available)
        .await
        .is_ok()
}

async fn push_task(
    config: CoreBootstrapConfig,
    mut transport_url_available: tokio::sync::watch::Receiver<
        BootstrapTransportUrlState,
    >,
    mut push_recv: PushRecv,
    auth_material: Arc<Option<kitsune2_bootstrap_client::AuthMaterial>>,
) {
    // Already checked to be a valid URL by the config validation.
    let server_url = url::Url::parse(
        config
            .server_url
            .as_ref()
            .expect("bootstrap url not checked"),
    )
    .expect("invalid server url");

    while let Some(item) = push_recv.recv().await {
        let mut wait: Option<std::time::Duration> = None;
        loop {
            if !wait_for_bootstrap_available(&mut transport_url_available).await
            {
                return;
            }
            if item.epoch != transport_url_available.borrow().epoch
                && !item.info.is_tombstone
            {
                break;
            }

            let result = tokio::task::spawn_blocking({
                let auth_material = auth_material.clone();
                let server_url = server_url.clone();
                let info = item.info.clone();
                move || {
                    kitsune2_bootstrap_client::blocking_put_auth(
                        server_url,
                        &info,
                        auth_material.as_ref().as_ref(),
                    )
                }
            })
            .await;

            if matches!(result, Ok(Ok(_))) {
                break;
            }
            tracing::debug!(
                ?result,
                "Failed to push agent info to bootstrap server"
            );
            if item.info.expires_at <= Timestamp::now() {
                break;
            }

            wait = Some(match wait {
                None => config.backoff_min(),
                Some(previous) => (previous * 2).min(config.backoff_max()),
            });
            tokio::select! {
                _ = tokio::time::sleep(wait.expect("set above")) => {}
                changed = transport_url_available.changed() => {
                    if changed.is_err() {
                        return;
                    }
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
    mut transport_url_available: tokio::sync::watch::Receiver<
        BootstrapTransportUrlState,
    >,
) {
    // Already checked to be a valid URL by the config validation.
    let server_url = url::Url::parse(
        config
            .server_url
            .as_ref()
            .expect("bootstrap url not checked"),
    )
    .expect("invalid server url");
    let mut wait = config.backoff_min();

    loop {
        if !wait_for_bootstrap_available(&mut transport_url_available).await {
            return;
        }
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
        tokio::select! {
            _ = tokio::time::sleep(wait) => {}
            changed = transport_url_available.changed() => {
                if changed.is_err() {
                    return;
                }
            }
        }
    }
}

#[cfg(test)]
mod test;
