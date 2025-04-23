//! The core space implementation provided by Kitsune2.

use kitsune2_api::*;
use std::sync::{Arc, RwLock, Weak};

/// CoreSpace configuration types.
mod config {
    /// Configuration parameters for [CoreSpaceFactory](super::CoreSpaceFactory).
    #[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
    #[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
    #[serde(rename_all = "camelCase")]
    pub struct CoreSpaceConfig {
        /// The interval in millis at which we check for about to expire
        /// local agent infos.
        ///
        /// Default: 60s.
        #[cfg_attr(feature = "schema", schemars(default))]
        pub re_sign_freq_ms: u32,

        /// The time in millis before an agent info expires, after which we will
        /// re-sign them.
        ///
        /// Default: 5m.
        #[cfg_attr(feature = "schema", schemars(default))]
        pub re_sign_expire_time_ms: u32,
    }

    impl Default for CoreSpaceConfig {
        fn default() -> Self {
            Self {
                re_sign_freq_ms: 1000 * 60,
                re_sign_expire_time_ms: 1000 * 60 * 5,
            }
        }
    }

    impl CoreSpaceConfig {
        /// Get re_sign_freq as a [std::time::Duration].
        pub fn re_sign_freq(&self) -> std::time::Duration {
            std::time::Duration::from_millis(self.re_sign_freq_ms as u64)
        }

        /// Get re_sign_expire_time_ms as a [std::time::Duration].
        pub fn re_sign_expire_time_ms(&self) -> std::time::Duration {
            std::time::Duration::from_millis(self.re_sign_expire_time_ms as u64)
        }
    }

    /// Module-level configuration for CoreSpace.
    #[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
    #[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
    #[serde(rename_all = "camelCase")]
    pub struct CoreSpaceModConfig {
        /// CoreSpace configuration.
        pub core_space: CoreSpaceConfig,
    }
}

use crate::get_all_remote_agents;
pub use config::*;

/// The core space implementation provided by Kitsune2.
/// You probably will have no reason to use something other than this.
/// This abstraction is mainly here for testing purposes.
#[derive(Debug)]
pub struct CoreSpaceFactory {}

impl CoreSpaceFactory {
    /// Construct a new CoreSpaceFactory.
    pub fn create() -> DynSpaceFactory {
        let out: DynSpaceFactory = Arc::new(CoreSpaceFactory {});
        out
    }
}

impl SpaceFactory for CoreSpaceFactory {
    fn default_config(&self, config: &mut Config) -> K2Result<()> {
        config.set_module_config(&CoreSpaceModConfig::default())
    }

    fn validate_config(&self, _config: &Config) -> K2Result<()> {
        Ok(())
    }

    fn create(
        &self,
        builder: Arc<Builder>,
        handler: DynSpaceHandler,
        space: SpaceId,
        tx: DynTransport,
    ) -> BoxFut<'static, K2Result<DynSpace>> {
        Box::pin(async move {
            let config: CoreSpaceModConfig =
                builder.config.get_module_config()?;
            let peer_store = builder.peer_store.create(builder.clone()).await?;
            let bootstrap = builder
                .bootstrap
                .create(builder.clone(), peer_store.clone(), space.clone())
                .await?;
            let local_agent_store =
                builder.local_agent_store.create(builder.clone()).await?;
            let inner = Arc::new(RwLock::new(InnerData { current_url: None }));
            let op_store = builder
                .op_store
                .create(builder.clone(), space.clone())
                .await?;
            let fetch = builder
                .fetch
                .create(
                    builder.clone(),
                    space.clone(),
                    op_store.clone(),
                    tx.clone(),
                )
                .await?;
            let publish = builder
                .publish
                .create(
                    builder.clone(),
                    space.clone(),
                    fetch.clone(),
                    peer_store.clone(),
                    tx.clone(),
                )
                .await?;
            let peer_meta_store = builder
                .peer_meta_store
                .create(builder.clone(), space.clone())
                .await?;
            let gossip = builder
                .gossip
                .create(
                    builder.clone(),
                    space.clone(),
                    peer_store.clone(),
                    local_agent_store.clone(),
                    peer_meta_store.clone(),
                    op_store.clone(),
                    tx.clone(),
                    fetch.clone(),
                )
                .await?;

            let out: DynSpace = Arc::new_cyclic(move |this| {
                let current_url = tx.register_space_handler(
                    space.clone(),
                    Arc::new(TxHandlerTranslator(handler, this.clone())),
                );
                inner.write().unwrap().current_url = current_url;
                CoreSpace::new(
                    config.core_space,
                    space,
                    tx,
                    peer_store,
                    bootstrap,
                    local_agent_store,
                    peer_meta_store,
                    inner,
                    op_store,
                    fetch,
                    publish,
                    gossip,
                )
            });
            Ok(out)
        })
    }
}

struct TxHandlerTranslator(DynSpaceHandler, Weak<CoreSpace>);

impl std::fmt::Debug for TxHandlerTranslator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TxHandlerTranslator").finish()
    }
}

impl TxBaseHandler for TxHandlerTranslator {
    fn new_listening_address(&self, this_url: Url) -> BoxFut<'static, ()> {
        let space = self.1.upgrade();
        Box::pin(async move {
            if let Some(this) = space {
                this.new_url(this_url).await;
            }
        })
    }
}

impl TxSpaceHandler for TxHandlerTranslator {
    fn recv_space_notify(
        &self,
        peer: Url,
        space: SpaceId,
        data: bytes::Bytes,
    ) -> K2Result<()> {
        self.0.recv_notify(peer, space, data)
    }
}

struct InnerData {
    current_url: Option<Url>,
}

struct CoreSpace {
    space: SpaceId,
    tx: DynTransport,
    peer_store: DynPeerStore,
    bootstrap: DynBootstrap,
    local_agent_store: DynLocalAgentStore,
    peer_meta_store: DynPeerMetaStore,
    op_store: DynOpStore,
    fetch: DynFetch,
    publish: DynPublish,
    gossip: DynGossip,
    inner: Arc<RwLock<InnerData>>,
    task_check_agent_infos: tokio::task::JoinHandle<()>,
}

impl Drop for CoreSpace {
    fn drop(&mut self) {
        tracing::trace!("Dropping core space");

        self.task_check_agent_infos.abort();
    }
}

impl std::fmt::Debug for CoreSpace {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CoreSpace")
            .field("space", &self.space)
            .finish()
    }
}

impl CoreSpace {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        config: CoreSpaceConfig,
        space: SpaceId,
        tx: DynTransport,
        peer_store: DynPeerStore,
        bootstrap: DynBootstrap,
        local_agent_store: DynLocalAgentStore,
        peer_meta_store: DynPeerMetaStore,
        inner: Arc<RwLock<InnerData>>,
        op_store: DynOpStore,
        fetch: DynFetch,
        publish: DynPublish,
        gossip: DynGossip,
    ) -> Self {
        let task_check_agent_infos = tokio::task::spawn(check_agent_infos(
            config,
            peer_store.clone(),
            local_agent_store.clone(),
        ));
        Self {
            space,
            tx,
            peer_store,
            bootstrap,
            local_agent_store,
            peer_meta_store,
            inner,
            op_store,
            task_check_agent_infos,
            fetch,
            publish,
            gossip,
        }
    }

    pub async fn new_url(&self, this_url: Url) {
        {
            let mut lock = self.inner.write().unwrap();
            lock.current_url = Some(this_url);
        }

        if let Ok(local_agents) = self.local_agent_store.get_all().await {
            for local_agent in local_agents {
                local_agent.invoke_cb();
            }
        }
    }
}

impl Space for CoreSpace {
    fn peer_store(&self) -> &DynPeerStore {
        &self.peer_store
    }

    fn local_agent_store(&self) -> &DynLocalAgentStore {
        &self.local_agent_store
    }

    fn op_store(&self) -> &DynOpStore {
        &self.op_store
    }

    fn fetch(&self) -> &DynFetch {
        &self.fetch
    }

    fn publish(&self) -> &DynPublish {
        &self.publish
    }

    fn gossip(&self) -> &DynGossip {
        &self.gossip
    }

    fn peer_meta_store(&self) -> &DynPeerMetaStore {
        &self.peer_meta_store
    }

    fn current_url(&self) -> Option<Url> {
        self.inner.read().unwrap().current_url.clone()
    }

    fn local_agent_join(
        &self,
        local_agent: DynLocalAgent,
    ) -> BoxFut<'_, K2Result<()>> {
        Box::pin(async move {
            // set some starting values
            local_agent.set_cur_storage_arc(DhtArc::Empty);

            // update our local map
            self.local_agent_store.add(local_agent.clone()).await?;

            let inner = self.inner.clone();
            let space = self.space.clone();
            let local_agent2 = local_agent.clone();
            let peer_store = self.peer_store.clone();
            let local_agent_store = self.local_agent_store.clone();
            let publish = self.publish.clone();
            let bootstrap = self.bootstrap.clone();
            local_agent.register_cb(Arc::new(move || {
                let inner = inner.clone();
                let space = space.clone();
                let local_agent2 = local_agent2.clone();
                let peer_store = peer_store.clone();
                let local_agent_store = local_agent_store.clone();
                let publish = publish.clone();
                let bootstrap = bootstrap.clone();
                tokio::task::spawn(async move {
                    let url = inner.read().unwrap().current_url.clone();

                    if let Some(url) = url {
                        // sign a new agent info
                        let created_at = Timestamp::now();
                        let expires_at = created_at
                            + std::time::Duration::from_secs(60 * 20);
                        let info = AgentInfo {
                            agent: local_agent2.agent().clone(),
                            space,
                            created_at,
                            expires_at,
                            is_tombstone: false,
                            url: Some(url),
                            storage_arc: local_agent2.get_cur_storage_arc(),
                        };

                        let info =
                            match AgentInfoSigned::sign(&local_agent2, info)
                                .await
                            {
                                Err(err) => {
                                    tracing::warn!(
                                        ?err,
                                        "failed to sign agent info",
                                    );
                                    return;
                                }
                                Ok(info) => info,
                            };

                        // add it to the peer_store.
                        if let Err(err) =
                            peer_store.insert(vec![info.clone()]).await
                        {
                            tracing::warn!(
                                ?err,
                                "failed to add agent info to peer store"
                            );
                        }

                        // add it to bootstrapping.
                        bootstrap.put(info.clone());

                        // and send it to our peers.
                        if let Err(err) = broadcast_agent_info(peer_store, local_agent_store, publish, info).await {
                            tracing::warn!(?err, "Failed to broadcast agent info")
                        }
                    } else {
                        tracing::info!("Not updating agent info because we don't have a current url");
                    }
                });
            }));

            // trigger the update
            local_agent.invoke_cb();

            Ok(())
        })
    }

    fn local_agent_leave(&self, local_agent: AgentId) -> BoxFut<'_, ()> {
        Box::pin(async move {
            // TODO - inform sharding module of leave

            let local_agent = self.local_agent_store.remove(local_agent).await;

            if let Some(local_agent) = local_agent {
                // register a dummy update cb
                local_agent.register_cb(Arc::new(|| ()));

                // sign a new tombstone
                let created_at = Timestamp::now();
                let expires_at =
                    created_at + std::time::Duration::from_secs(60 * 20);
                let info = AgentInfo {
                    agent: local_agent.agent().clone(),
                    space: self.space.clone(),
                    created_at,
                    expires_at,
                    is_tombstone: true,
                    url: None,
                    storage_arc: DhtArc::Empty,
                };

                let info = match AgentInfoSigned::sign(&local_agent, info).await
                {
                    Err(err) => {
                        tracing::warn!(?err, "failed to sign agent info");
                        return;
                    }
                    Ok(info) => info,
                };

                if let Err(err) =
                    self.peer_store.insert(vec![info.clone()]).await
                {
                    tracing::warn!(
                        ?err,
                        "failed to tombstone agent info in peer store"
                    );
                }

                // also send the tombstone to the bootstrap server
                self.bootstrap.put(info);
            }
        })
    }

    fn send_notify(
        &self,
        to_peer: Url,
        data: bytes::Bytes,
    ) -> BoxFut<'_, K2Result<()>> {
        self.tx.send_space_notify(to_peer, self.space.clone(), data)
    }

    fn inform_ops_stored(
        &self,
        ops: Vec<StoredOp>,
    ) -> BoxFut<'_, K2Result<()>> {
        self.gossip.inform_ops_stored(ops)
    }
}

async fn check_agent_infos(
    config: CoreSpaceConfig,
    peer_store: DynPeerStore,
    local_agent_store: DynLocalAgentStore,
) {
    loop {
        // only check at this rate
        tokio::time::sleep(config.re_sign_freq()).await;

        // only re-sign if they expire within this time
        let cutoff = Timestamp::now() + config.re_sign_expire_time_ms();

        // get all the local agents
        let Ok(agents) = local_agent_store.get_all().await else {
            tracing::error!(
                "error fetching local agents in re-signing before expiry logic"
            );
            continue;
        };

        for agent in agents {
            // is this agent going to expire?
            let should_re_sign =
                match peer_store.get(agent.agent().clone()).await {
                    Ok(Some(info)) => info.expires_at <= cutoff,
                    Ok(None) => true,
                    Err(err) => {
                        tracing::debug!(
                        ?err,
                        "error fetching agent in re-signing before expiry logic"
                    );
                        true
                    }
                };

            if should_re_sign {
                // if so, re-sign it
                agent.invoke_cb();
            }
        }
    }
}

async fn broadcast_agent_info(
    peer_store: DynPeerStore,
    local_agent_store: DynLocalAgentStore,
    publish: DynPublish,
    agent_info_signed: Arc<AgentInfoSigned>,
) -> K2Result<()> {
    let all_remote_agents =
        get_all_remote_agents(peer_store, local_agent_store).await?;

    let results = futures::future::join_all(
        all_remote_agents.into_iter().filter_map(|a| {
            if let Some(url) = a.url.clone() {
                let publish = publish.clone();
                let agent_info_signed = agent_info_signed.clone();
                Some(Box::pin(async move {
                    publish.publish_agent(agent_info_signed, url).await
                }))
            } else {
                None
            }
        }),
    )
    .await;

    let ok = results.iter().filter(|r| r.is_ok()).count();
    tracing::info!("Broadcast new agent info to {} peers", ok);

    Ok(())
}

#[cfg(test)]
mod test;
