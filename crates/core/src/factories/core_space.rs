//! The core space implementation provided by Kitsune2.

use kitsune2_api::{config::*, space::*, *};
use std::sync::{Arc, Mutex, Weak};

mod protocol;

const MOD_NAME: &str = "CoreSpace";

/// Configuration parameters for [CoreSpaceFactory].
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CoreSpaceConfig {}

impl ModConfig for CoreSpaceConfig {}

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
        config.add_default_module_config::<CoreSpaceConfig>(MOD_NAME.into())?;
        Ok(())
    }

    fn create(
        &self,
        builder: Arc<builder::Builder>,
        handler: DynSpaceHandler,
        space: SpaceId,
        tx: transport::DynTransport,
    ) -> BoxFut<'static, K2Result<DynSpace>> {
        Box::pin(async move {
            let config = builder
                .config
                .get_module_config::<CoreSpaceConfig>(MOD_NAME)?;
            let peer_store = builder.peer_store.create(builder.clone()).await?;
            let inner = Arc::new(Mutex::new(InnerData {
                local_agent_map: std::collections::HashMap::new(),
                current_url: None,
            }));
            let out: DynSpace = Arc::new_cyclic(move |this| {
                tx.register_space_handler(
                    space.clone(),
                    Arc::new(TxHandlerTranslator(handler, this.clone())),
                );
                CoreSpace::new(config, space, tx, peer_store, inner)
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

impl transport::TxBaseHandler for TxHandlerTranslator {
    fn new_listening_address(&self, this_url: Url) {
        if let Some(this) = self.1.upgrade() {
            this.new_url(this_url);
        }
    }
}

impl transport::TxSpaceHandler for TxHandlerTranslator {
    fn recv_space_notify(
        &self,
        _peer: Url,
        space: SpaceId,
        data: bytes::Bytes,
    ) -> K2Result<()> {
        let dec = protocol::K2SpaceProto::decode(&data)?;
        self.0.recv_notify(
            dec.to_agent.into(),
            dec.from_agent.into(),
            space,
            dec.data,
        )
    }
}

struct InnerData {
    local_agent_map: std::collections::HashMap<AgentId, agent::DynLocalAgent>,
    current_url: Option<Url>,
}

struct CoreSpace {
    space: SpaceId,
    tx: transport::DynTransport,
    peer_store: peer_store::DynPeerStore,
    inner: Arc<Mutex<InnerData>>,
}

impl std::fmt::Debug for CoreSpace {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CoreSpace")
            .field("space", &self.space)
            .finish()
    }
}

impl CoreSpace {
    pub fn new(
        _config: CoreSpaceConfig,
        space: SpaceId,
        tx: transport::DynTransport,
        peer_store: peer_store::DynPeerStore,
        inner: Arc<Mutex<InnerData>>,
    ) -> Self {
        Self {
            space,
            tx,
            peer_store,
            inner,
        }
    }

    pub fn new_url(&self, this_url: Url) {
        let mut lock = self.inner.lock().unwrap();
        lock.current_url = Some(this_url);
        for local_agent in lock.local_agent_map.values() {
            local_agent.invoke_cb();
        }
    }
}

impl Space for CoreSpace {
    fn peer_store(&self) -> &peer_store::DynPeerStore {
        &self.peer_store
    }

    fn local_agent_join(
        &self,
        local_agent: agent::DynLocalAgent,
    ) -> BoxFut<'_, K2Result<()>> {
        Box::pin(async move {
            // set some starting values
            local_agent.set_tgt_storage_arc_hint(DhtArc::Empty);
            local_agent.set_cur_storage_arc(DhtArc::Empty);

            // update our local map
            self.inner
                .lock()
                .unwrap()
                .local_agent_map
                .insert(local_agent.agent().clone(), local_agent.clone());

            // TODO - inform gossip module of new join
            // TODO - inform sharding module of new join

            let inner = self.inner.clone();
            let space = self.space.clone();
            let local_agent2 = local_agent.clone();
            let peer_store = self.peer_store.clone();
            local_agent.register_cb(Arc::new(move || {
                let inner = inner.clone();
                let space = space.clone();
                let local_agent2 = local_agent2.clone();
                let peer_store = peer_store.clone();
                tokio::task::spawn(async move {
                    // TODO - call an update function on the gossip module.
                    // TODO - call an update function on the sharding module.

                    let url = inner.lock().unwrap().current_url.clone();

                    if let Some(url) = url {
                        // sign a new agent info
                        let created_at = Timestamp::now();
                        let expires_at = created_at
                            + std::time::Duration::from_secs(60 * 20);
                        let info = agent::AgentInfo {
                            agent: local_agent2.agent().clone(),
                            space,
                            created_at,
                            expires_at,
                            is_tombstone: false,
                            url: Some(url),
                            storage_arc: local_agent2.get_cur_storage_arc(),
                        };

                        let info = match agent::AgentInfoSigned::sign(
                            &local_agent2,
                            info,
                        )
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
                        if let Err(err) = peer_store.insert(vec![info]).await {
                            tracing::warn!(
                                ?err,
                                "failed to add agent info to peer store"
                            );
                        }

                        // TODO - send the new agent info to bootstrap.
                    }
                });
            }));

            // trigger the update
            local_agent.invoke_cb();

            Ok(())
        })
    }

    fn local_agent_leave(&self, local_agent: id::AgentId) -> BoxFut<'_, ()> {
        Box::pin(async move {
            // TODO - inform gossip module of leave
            // TODO - inform sharding module of leave

            let local_agent = self
                .inner
                .lock()
                .unwrap()
                .local_agent_map
                .remove(&local_agent);

            if let Some(local_agent) = local_agent {
                // register a dummy update cb
                local_agent.register_cb(Arc::new(|| ()));

                // sign a new tombstone
                let created_at = Timestamp::now();
                let expires_at =
                    created_at + std::time::Duration::from_secs(60 * 20);
                let info = agent::AgentInfo {
                    agent: local_agent.agent().clone(),
                    space: self.space.clone(),
                    created_at,
                    expires_at,
                    is_tombstone: true,
                    url: None,
                    storage_arc: DhtArc::Empty,
                };

                let info = match agent::AgentInfoSigned::sign(
                    &local_agent,
                    info,
                )
                .await
                {
                    Err(err) => {
                        tracing::warn!(?err, "failed to sign agent info");
                        return;
                    }
                    Ok(info) => info,
                };

                if let Err(err) = self.peer_store.insert(vec![info]).await {
                    tracing::warn!(
                        ?err,
                        "failed to add agent info to peer store"
                    );
                }

                // TODO - send the tombstone info to bootstrap.
            }
        })
    }

    fn send_notify(
        &self,
        to_agent: AgentId,
        from_agent: AgentId,
        data: bytes::Bytes,
    ) -> BoxFut<'_, K2Result<()>> {
        Box::pin(async move {
            let info = match self.peer_store.get(to_agent.clone()).await? {
                Some(info) => info,
                None => {
                    // TODO - once discovery is implemented try to
                    //        look up the peer from the network.
                    return Err(K2Error::other(format!(
                        "to_agent {to_agent} not found"
                    )));
                }
            };
            let url = match &info.url {
                Some(url) => url.clone(),
                None => {
                    return Err(K2Error::other(format!(
                        "to_agent {to_agent} is offline"
                    )));
                }
            };

            let enc = protocol::K2SpaceProto {
                to_agent: to_agent.into(),
                from_agent: from_agent.into(),
                data,
            }
            .encode()?;

            self.tx
                .send_space_notify(url, self.space.clone(), enc)
                .await
        })
    }
}

#[cfg(test)]
mod test;
