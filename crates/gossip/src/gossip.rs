use crate::common::{local_agent_state, send_gossip_message, GossipResponse};
use crate::peer_meta_store::K2PeerMetaStore;
use crate::protocol::k2_gossip_message::GossipMessage;
use crate::protocol::{
    deserialize_gossip_message, encode_agent_ids, encode_agent_infos,
    encode_op_ids, AgentInfoMessage, ArcSetMessage, K2GossipAcceptMessage,
    K2GossipAgentsMessage, K2GossipInitiateMessage, K2GossipMessage,
    K2GossipNoDiffMessage,
};
use crate::{K2GossipConfig, K2GossipModConfig, MOD_NAME};
use kitsune2_api::agent::{AgentInfoSigned, DynVerifier};
use kitsune2_api::peer_store::DynPeerStore;
use kitsune2_api::space::{DynSpace, Space};
use kitsune2_api::transport::{DynTransport, TxBaseHandler, TxModuleHandler};
use kitsune2_api::{
    AgentId, DynGossip, DynGossipFactory, DynOpStore, DynPeerMetaStore, Gossip,
    GossipFactory, K2Error, K2Result, SpaceId, Timestamp, Url, UNIX_TIMESTAMP,
};
use kitsune2_dht::ArcSet;
use std::sync::{Arc, Weak};

/// A factory for creating K2Gossip instances.
#[derive(Debug)]
pub struct K2GossipFactory;

impl K2GossipFactory {
    /// Construct a new [K2GossipFactory].
    pub fn create() -> DynGossipFactory {
        Arc::new(K2GossipFactory)
    }
}

impl GossipFactory for K2GossipFactory {
    fn default_config(
        &self,
        config: &mut kitsune2_api::config::Config,
    ) -> K2Result<()> {
        config.set_module_config(&K2GossipModConfig::default())
    }

    fn create(
        &self,
        builder: Arc<kitsune2_api::builder::Builder>,
        space_id: SpaceId,
        space: DynSpace,
        peer_store: DynPeerStore,
        peer_meta_store: DynPeerMetaStore,
        op_store: DynOpStore,
        transport: DynTransport,
        agent_verifier: DynVerifier,
    ) -> kitsune2_api::BoxFut<'static, K2Result<DynGossip>> {
        Box::pin(async move {
            let config: K2GossipConfig = builder.config.get_module_config()?;

            let gossip: DynGossip = Arc::new(K2Gossip::create(
                config,
                space_id,
                space,
                peer_store,
                peer_meta_store,
                op_store,
                transport,
                agent_verifier,
            ));

            Ok(gossip)
        })
    }
}

#[derive(Debug)]
struct DropAbortHandle {
    name: String,
    handle: tokio::task::AbortHandle,
}

impl Drop for DropAbortHandle {
    fn drop(&mut self) {
        tracing::info!("Aborting: {}", self.name);
        self.handle.abort();
    }
}

/// The gossip implementation.
///
/// This type acts as both an implementation of the [Gossip] trait and a [TxModuleHandler].
#[derive(Debug, Clone)]
struct K2Gossip {
    config: Arc<K2GossipConfig>,
    space_id: SpaceId,
    // This is a weak reference because we need to call the space, but we do not create and own it.
    // Only a problem in this case because we register the gossip module with the transport and
    // create a cycle.
    space: Weak<dyn Space>,
    peer_store: DynPeerStore,
    peer_meta_store: Arc<K2PeerMetaStore>,
    op_store: DynOpStore,
    agent_verifier: DynVerifier,
    response_tx: tokio::sync::mpsc::Sender<GossipResponse>,
    _response_task: Arc<DropAbortHandle>,
}

impl K2Gossip {
    /// Construct a new [K2Gossip] instance.
    #[allow(clippy::too_many_arguments)]
    pub fn create(
        config: K2GossipConfig,
        space_id: SpaceId,
        space: DynSpace,
        peer_store: DynPeerStore,
        peer_meta_store: DynPeerMetaStore,
        op_store: DynOpStore,
        transport: DynTransport,
        agent_verifier: DynVerifier,
    ) -> K2Gossip {
        let (response_tx, mut rx) =
            tokio::sync::mpsc::channel::<GossipResponse>(1024);
        let response_task = tokio::task::spawn({
            let space_id = space_id.clone();
            let transport = transport.clone();
            async move {
                while let Some(msg) = rx.recv().await {
                    if let Err(e) = transport
                        .send_module(
                            msg.1,
                            space_id.clone(),
                            MOD_NAME.to_string(),
                            msg.0,
                        )
                        .await
                    {
                        tracing::error!("could not send response: {:?}", e);
                    };
                }
            }
        })
        .abort_handle();

        let gossip = K2Gossip {
            config: Arc::new(config),
            space_id: space_id.clone(),
            space: Arc::downgrade(&space),
            peer_store,
            peer_meta_store: Arc::new(K2PeerMetaStore::new(
                peer_meta_store,
                space_id.clone(),
            )),
            op_store,
            agent_verifier,
            response_tx,
            _response_task: Arc::new(DropAbortHandle {
                name: "Gossip response task".to_string(),
                handle: response_task,
            }),
        };

        transport.register_module_handler(
            space_id,
            MOD_NAME.to_string(),
            Arc::new(gossip.clone()),
        );

        gossip
    }
}

impl K2Gossip {
    // TODO dead code until the initiate task is created
    #[allow(dead_code)]
    pub(crate) async fn initiate_gossip(
        &self,
        target: AgentId,
    ) -> K2Result<()> {
        let Some(target_url) = self
            .peer_store
            .get(target.clone())
            .await?
            .and_then(|t| t.url.clone())
        else {
            tracing::info!("initiate_gossip: target not found: {:?}", target);
            return Ok(());
        };

        let space = self
            .space
            .upgrade()
            .ok_or_else(|| K2Error::other("space was dropped"))?;
        let (our_agents, our_arc_set) = local_agent_state(space).await?;

        let new_since = self
            .peer_meta_store
            .new_ops_bookmark(target.clone())
            .await?
            .unwrap_or(UNIX_TIMESTAMP);

        let initiate = K2GossipInitiateMessage {
            participating_agents: our_agents
                .into_iter()
                .map(|a| a.0 .0)
                .collect(),
            arc_set: Some(ArcSetMessage {
                arc_sectors: our_arc_set.into_raw().collect(),
            }),
            new_since: new_since.as_micros(),
            max_new_bytes: self.config.max_gossip_op_bytes,
        };

        tracing::trace!("initiate_gossip with {:?}: {:?}", target, initiate);

        send_gossip_message(&self.response_tx, target_url, initiate)?;

        Ok(())
    }

    /// Handle a gossip message.
    ///
    /// Produces a response message if the input is acceptable and a response is required.
    /// Otherwise, returns None.
    fn handle_gossip_message(
        &self,
        from: AgentId,
        from_url: Url,
        msg: K2GossipMessage,
    ) -> K2Result<()> {
        tracing::debug!(
            "handle_gossip_message from: {:?}, msg: {:?}",
            from,
            msg
        );

        let Some(msg) = msg.gossip_message else {
            return Err(K2Error::other("no gossip message"));
        };

        let this = self.clone();
        tokio::task::spawn(async move {
            if let Err(e) = this.respond_to_msg(from, from_url, msg).await {
                tracing::error!("could not respond to gossip message: {:?}", e);
            }
        });

        Ok(())
    }

    async fn respond_to_msg(
        &self,
        from: AgentId,
        from_url: Url,
        msg: GossipMessage,
    ) -> K2Result<()> {
        let res = match msg {
            GossipMessage::Initiate(initiate) => {
                // Rate limit incoming gossip messages by peer
                if let Some(timestamp) = self
                    .peer_meta_store
                    .last_gossip_timestamp(from.clone())
                    .await?
                {
                    let elapsed =
                        (Timestamp::now() - timestamp).map_err(|_| {
                            K2Error::other("could not calculate elapsed time")
                        })?;

                    if elapsed < self.config.interval() {
                        tracing::info!("peer {:?} attempted to initiate too soon {:?} < {:?}", from, elapsed, self.config.interval());
                        return Err(K2Error::other("initiate too soon"));
                    }
                }

                println!("Accepting with op store {:?}", self.op_store);

                // Note the gap between the read and write here. It's possible that both peers
                // could initiate at the same time. This is slightly wasteful but shouldn't be a
                // problem.
                self.peer_meta_store
                    .set_last_gossip_timestamp(from.clone(), Timestamp::now())
                    .await?;

                let other_arc_set = match initiate.arc_set {
                    Some(arc_set) => {
                        ArcSet::from_raw(arc_set.arc_sectors.into_iter())
                    }
                    None => {
                        return Err(K2Error::other(
                            "no arc set in initiate message",
                        ));
                    }
                };

                let space = self
                    .space
                    .upgrade()
                    .ok_or_else(|| K2Error::other("space was dropped"))?;
                let (send_agents, our_arc_set) =
                    local_agent_state(space).await?;
                let common_arc_set = our_arc_set.intersection(&other_arc_set);
                if common_arc_set.covered_sector_count() == 0 {
                    tracing::info!("no common arc set, continue to sync agents but not ops");
                }

                let missing_agents = self
                    .filter_known_agents(&initiate.participating_agents)
                    .await?;

                let new_since = self
                    .peer_meta_store
                    .new_ops_bookmark(from.clone())
                    .await?
                    .unwrap_or(UNIX_TIMESTAMP);

                let (new_ops, new_bookmark) = self
                    .op_store
                    .retrieve_op_ids_bounded(
                        Timestamp::from_micros(initiate.new_since),
                        initiate.max_new_bytes as usize,
                    )
                    .await?;

                Ok(Some(K2GossipMessage {
                    gossip_message: Some(GossipMessage::Accept(
                        K2GossipAcceptMessage {
                            participating_agents: encode_agent_ids(send_agents),
                            arc_set: Some(ArcSetMessage {
                                arc_sectors: our_arc_set.into_raw().collect(),
                            }),
                            missing_agents,
                            new_since: new_since.as_micros(),
                            max_new_bytes: self.config.max_gossip_op_bytes,
                            new_ops: encode_op_ids(new_ops),
                            updated_new_since: new_bookmark.as_micros(),
                        },
                    )),
                }))
            }
            GossipMessage::Accept(accept) => {
                // TODO check that we have a session active for this peer because we must have
                //      sent an initiate message, otherwise this is unsolicited.

                // Only once the other peer has accepted should we record that we've tried to
                // gossip with them. Otherwise, we risk blocking each other if we both record
                // a last gossip timestamp and try to initiate at the same time.
                self.peer_meta_store
                    .set_last_gossip_timestamp(from.clone(), Timestamp::now())
                    .await?;

                let missing_agents = self
                    .filter_known_agents(&accept.participating_agents)
                    .await?;

                let send_agent_infos =
                    self.load_agent_infos(accept.missing_agents).await;

                self.update_new_ops_bookmark(
                    from.clone(),
                    Timestamp::from_micros(accept.updated_new_since),
                )
                .await?;

                // TODO Send to fetch queue
                println!("Discovered op ids: {:?}", accept.new_ops);
                self.peer_meta_store
                    .set_new_ops_bookmark(
                        from.clone(),
                        Timestamp::from_micros(accept.updated_new_since),
                    )
                    .await?;

                let (new_ops, new_bookmark) = self
                    .op_store
                    .retrieve_op_ids_bounded(
                        Timestamp::from_micros(accept.new_since),
                        accept.max_new_bytes as usize,
                    )
                    .await?;

                Ok(Some(K2GossipMessage {
                    gossip_message: Some(GossipMessage::NoDiff(
                        K2GossipNoDiffMessage {
                            missing_agents,
                            provided_agents: encode_agent_infos(
                                send_agent_infos,
                            ),
                            new_ops: encode_op_ids(new_ops),
                            updated_new_since: new_bookmark.as_micros(),
                            cannot_compare: false,
                        },
                    )),
                }))
            }
            GossipMessage::NoDiff(no_diff) => {
                // TODO session check

                self.receive_agent_infos(no_diff.provided_agents).await?;

                self.update_new_ops_bookmark(
                    from.clone(),
                    Timestamp::from_micros(no_diff.updated_new_since),
                )
                .await?;

                // TODO Send to fetch queue
                println!("Discovered op ids: {:?}", no_diff.new_ops);
                self.peer_meta_store
                    .set_new_ops_bookmark(
                        from.clone(),
                        Timestamp::from_micros(no_diff.updated_new_since),
                    )
                    .await?;

                if no_diff.missing_agents.is_empty() {
                    Ok(None)
                } else {
                    let send_agent_infos =
                        self.load_agent_infos(no_diff.missing_agents).await;

                    Ok(Some(K2GossipMessage {
                        gossip_message: Some(GossipMessage::Agents(
                            K2GossipAgentsMessage {
                                provided_agents: encode_agent_infos(
                                    send_agent_infos,
                                ),
                            },
                        )),
                    }))
                }
            }
            GossipMessage::Agents(agents) => {
                // TODO session check

                self.receive_agent_infos(agents.provided_agents).await?;

                Ok(None)
            }
        }?;

        if let Some(msg) = res {
            send_gossip_message(&self.response_tx, from_url, msg)?;
        }

        Ok(())
    }

    /// Filter out agents that are already known and return a list of unknown agents.
    ///
    /// This is useful when receiving a list of agents from a peer, and we want to filter out
    /// the ones we already know about. The resulting list should be sent back as a request
    /// to get infos for the unknown agents.
    async fn filter_known_agents<T: Into<AgentId> + Clone>(
        &self,
        agents: &[T],
    ) -> K2Result<Vec<T>> {
        let mut out = Vec::new();
        for agent in agents {
            let agent_id = agent.clone().into();
            if self.peer_store.get(agent_id).await?.is_none() {
                out.push(agent.clone());
            }
        }

        Ok(out)
    }

    /// Load agent infos from the peer store.
    ///
    /// Loads any of the requested agents that are available in the peer store.
    async fn load_agent_infos<T: Into<AgentId> + Clone>(
        &self,
        requested: Vec<T>,
    ) -> Vec<Arc<AgentInfoSigned>> {
        if requested.is_empty() {
            return vec![];
        }

        let mut agent_infos = vec![];
        for missing_agent in requested {
            if let Ok(Some(agent_info)) =
                self.peer_store.get(missing_agent.clone().into()).await
            {
                agent_infos.push(agent_info);
            }
        }

        agent_infos
    }

    /// Receive agent info messages from the network.
    ///
    /// Each info is checked against the verifier and then stored in the peer store.
    async fn receive_agent_infos(
        &self,
        provided_agents: Vec<AgentInfoMessage>,
    ) -> K2Result<()> {
        if provided_agents.is_empty() {
            return Ok(());
        }

        // TODO check that the incoming agents are the one we requested
        let mut agents = Vec::with_capacity(provided_agents.len());
        for agent in provided_agents {
            let agent_info = AgentInfoSigned::from_parts(
                &self.agent_verifier,
                String::from_utf8(agent.encoded.to_vec())
                    .map_err(|e| K2Error::other_src("Invalid agent info", e))?,
                agent.signature,
            )?;
            agents.push(agent_info);
        }
        tracing::info!("Storing agents: {:?}", agents);
        self.peer_store.insert(agents).await?;

        Ok(())
    }

    async fn update_new_ops_bookmark(
        &self,
        from: AgentId,
        updated_bookmark: Timestamp,
    ) -> K2Result<()> {
        let previous_bookmark =
            self.peer_meta_store.new_ops_bookmark(from.clone()).await?;

        if previous_bookmark
            .map(|previous_bookmark| previous_bookmark <= updated_bookmark)
            .unwrap_or(true)
        {
            self.peer_meta_store
                .set_new_ops_bookmark(from.clone(), updated_bookmark)
                .await?;
        } else {
            // This could happen due to a clock issue. If it happens frequently, or by a
            // large margin, it could be a sign of malicious activity.
            tracing::warn!(
                "new bookmark is older than previous bookmark from peer: {:?}",
                from
            );
        }

        Ok(())
    }
}

impl Gossip for K2Gossip {}

impl TxBaseHandler for K2Gossip {}
impl TxModuleHandler for K2Gossip {
    fn recv_module_msg(
        &self,
        peer: Url,
        space: SpaceId,
        module: String,
        data: bytes::Bytes,
    ) -> K2Result<()> {
        tracing::trace!("Incoming module message: {:?}", data);

        if self.space_id != space {
            return Err(K2Error::other("wrong space"));
        }

        if module != MOD_NAME {
            return Err(K2Error::other(format!(
                "wrong module name: {}",
                module
            )));
        }

        let peer_id = peer
            .peer_id()
            .ok_or_else(|| K2Error::other("no peer id"))?
            .as_bytes()
            .to_vec();
        let agent_id = AgentId::from(bytes::Bytes::from(peer_id));

        let msg = deserialize_gossip_message(data)?;
        match self.handle_gossip_message(agent_id, peer, msg) {
            Ok(()) => Ok(()),
            Err(e) => {
                tracing::error!("could not handle gossip message: {:?}", e);
                Err(e)
            }
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use kitsune2_api::agent::{DynLocalAgent, LocalAgent};
    use kitsune2_api::builder::Builder;
    use kitsune2_api::space::SpaceHandler;
    use kitsune2_api::transport::{TxHandler, TxSpaceHandler};
    use kitsune2_core::factories::MemoryOp;
    use kitsune2_core::{default_test_builder, Ed25519LocalAgent};
    use kitsune2_test_utils::enable_tracing;

    #[derive(Debug, Clone)]
    struct GossipTestHarness {
        gossip: K2Gossip,
        peer_store: DynPeerStore,
        op_store: DynOpStore,
        space: DynSpace,
    }

    impl GossipTestHarness {
        async fn join_local_agent(&self) -> DynLocalAgent {
            let agent_1 = Arc::new(Ed25519LocalAgent::default());
            self.space.local_agent_join(agent_1.clone()).await.unwrap();

            // Wait for the agent info to be published
            // This means tests can rely on the agent being available in the peer store
            tokio::time::timeout(std::time::Duration::from_secs(5), {
                let agent_id = agent_1.agent().clone();
                let peer_store = self.peer_store.clone();
                async move {
                    while !peer_store
                        .get_all()
                        .await
                        .unwrap()
                        .iter()
                        .any(|a| a.agent.clone() == agent_id)
                    {
                        tokio::time::sleep(std::time::Duration::from_millis(5))
                            .await;
                    }
                }
            })
            .await
            .unwrap();

            agent_1
        }

        async fn wait_for_agent_in_peer_store(&self, agent: AgentId) {
            tokio::time::timeout(std::time::Duration::from_millis(100), {
                let this = self.clone();
                async move {
                    loop {
                        let has_agent = this
                            .peer_store
                            .get(agent.clone())
                            .await
                            .unwrap()
                            .is_some();

                        if has_agent {
                            break;
                        }

                        tokio::time::sleep(std::time::Duration::from_millis(5))
                            .await;
                    }
                }
            })
            .await
            .unwrap();
        }
    }

    struct TestGossipFactory {
        space_id: SpaceId,
        builder: Arc<Builder>,
    }

    impl TestGossipFactory {
        pub async fn create(space: SpaceId) -> TestGossipFactory {
            let mut builder = default_test_builder();
            // Replace the core builder with a real gossip factory
            builder.gossip = K2GossipFactory::create();
            let builder = Arc::new(builder.with_default_config().unwrap());

            TestGossipFactory {
                space_id: space,
                builder,
            }
        }

        pub async fn new_instance(&self) -> GossipTestHarness {
            #[derive(Debug)]
            struct NoopHandler;
            impl TxBaseHandler for NoopHandler {}
            impl TxHandler for NoopHandler {}
            impl TxSpaceHandler for NoopHandler {}
            impl SpaceHandler for NoopHandler {}

            let transport = self
                .builder
                .transport
                .create(self.builder.clone(), Arc::new(NoopHandler))
                .await
                .unwrap();

            let space = self
                .builder
                .space
                .create(
                    self.builder.clone(),
                    Arc::new(NoopHandler),
                    self.space_id.clone(),
                    transport.clone(),
                )
                .await
                .unwrap();

            let op_store = self
                .builder
                .op_store
                .create(self.builder.clone(), self.space_id.clone())
                .await
                .unwrap();

            let gossip = K2Gossip::create(
                K2GossipConfig::default(),
                self.space_id.clone(),
                space.clone(),
                space.peer_store().clone(),
                self.builder
                    .peer_meta_store
                    .create(self.builder.clone())
                    .await
                    .unwrap(),
                op_store.clone(),
                transport.clone(),
                self.builder.verifier.clone(),
            );

            GossipTestHarness {
                gossip,
                peer_store: space.peer_store().clone(),
                op_store,
                space,
            }
        }
    }

    fn test_space() -> SpaceId {
        SpaceId::from(bytes::Bytes::from_static(b"test-space"))
    }

    #[tokio::test]
    async fn create_gossip_instance() {
        let factory = TestGossipFactory::create(test_space()).await;
        factory.new_instance().await;
    }

    #[tokio::test]
    async fn two_way_agent_sync() {
        enable_tracing();

        let space = test_space();
        let factory = TestGossipFactory::create(space.clone()).await;
        let harness_1 = factory.new_instance().await;
        let agent_1 = harness_1.join_local_agent().await;
        let agent_info_1 = harness_1
            .peer_store
            .get(agent_1.agent().clone())
            .await
            .unwrap()
            .unwrap();

        let harness_2 = factory.new_instance().await;
        harness_2.join_local_agent().await;
        harness_2
            .peer_store
            .insert(vec![agent_info_1])
            .await
            .unwrap();

        // Join extra agents for each peer. These will take a few seconds to be
        // found by bootstrap. Try to sync them with gossip.
        let secret_agent_1 = harness_1.join_local_agent().await;
        let secret_agent_2 = harness_2.join_local_agent().await;

        assert!(harness_1
            .peer_store
            .get(secret_agent_2.agent().clone())
            .await
            .unwrap()
            .is_none());
        assert!(harness_2
            .peer_store
            .get(secret_agent_1.agent().clone())
            .await
            .unwrap()
            .is_none());

        harness_2
            .gossip
            .initiate_gossip(agent_1.agent().clone())
            .await
            .unwrap();

        harness_1
            .wait_for_agent_in_peer_store(secret_agent_2.agent().clone())
            .await;
        harness_2
            .wait_for_agent_in_peer_store(secret_agent_1.agent().clone())
            .await;
    }

    #[tokio::test]
    async fn two_way_op_sync() {
        enable_tracing();

        let space = test_space();
        let factory = TestGossipFactory::create(space.clone()).await;
        let harness_1 = factory.new_instance().await;
        let agent_1 = harness_1.join_local_agent().await;
        harness_1
            .op_store
            .process_incoming_ops(vec![MemoryOp::new(
                Timestamp::now(),
                vec![2; 128],
            )
            .into()])
            .await
            .unwrap();

        println!("Op store after insert 1: {:?}", harness_1.op_store);

        let harness_2 = factory.new_instance().await;
        harness_2.join_local_agent().await;
        harness_2
            .op_store
            .process_incoming_ops(vec![MemoryOp::new(
                Timestamp::now(),
                vec![2; 128],
            )
            .into()])
            .await
            .unwrap();

        harness_2
            .gossip
            .initiate_gossip(agent_1.agent().clone())
            .await
            .unwrap();

        tokio::time::sleep(std::time::Duration::from_secs(5)).await;

        // TODO fetch ops and assert that the op stores get updated.
    }
}
