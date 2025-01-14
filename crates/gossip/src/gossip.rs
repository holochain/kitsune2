use crate::common::{local_agent_state, send_gossip_message, GossipResponse};
use crate::protocol::k2_gossip_message::GossipMessage;
use crate::protocol::{
    deserialize_gossip_message, ArcSetMessage, K2GossipAcceptMessage,
    K2GossipDiffMessage, K2GossipInitiateMessage, K2GossipMessage,
};
use crate::MOD_NAME;
use kitsune2_api::agent::{AgentInfoSigned, DynVerifier};
use kitsune2_api::peer_store::DynPeerStore;
use kitsune2_api::space::{DynSpace, Space};
use kitsune2_api::transport::{DynTransport, TxBaseHandler, TxModuleHandler};
use kitsune2_api::{
    AgentId, DynGossip, DynGossipFactory, DynOpStore, Gossip, GossipFactory,
    K2Error, K2Result, SpaceId, Url,
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
        _config: &mut kitsune2_api::config::Config,
    ) -> K2Result<()> {
        // TODO config
        Ok(())
    }

    fn create(
        &self,
        _builder: Arc<kitsune2_api::builder::Builder>,
        space_id: SpaceId,
        space: DynSpace,
        peer_store: DynPeerStore,
        op_store: DynOpStore,
        transport: DynTransport,
        agent_verifier: DynVerifier,
    ) -> kitsune2_api::BoxFut<'static, K2Result<DynGossip>> {
        let gossip: DynGossip = Arc::new(K2Gossip::create(
            space_id,
            space,
            peer_store,
            op_store,
            transport,
            agent_verifier,
        ));
        Box::pin(async move { Ok(gossip) })
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
    space_id: SpaceId,
    // This is a weak reference because we need to call the space, but we do not create and own it.
    // Only a problem in this case because we register the gossip module with the transport and
    // create a cycle.
    space: Weak<dyn Space>,
    peer_store: DynPeerStore,
    _op_store: DynOpStore,
    agent_verifier: DynVerifier,
    response_tx: tokio::sync::mpsc::Sender<GossipResponse>,
    _response_task: Arc<DropAbortHandle>,
}

impl K2Gossip {
    /// Construct a new [K2Gossip] instance.
    pub fn create(
        space_id: SpaceId,
        space: DynSpace,
        peer_store: DynPeerStore,
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
            space_id: space_id.clone(),
            space: Arc::downgrade(&space),
            peer_store,
            _op_store: op_store,
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
        space: DynSpace,
        peer_store: DynPeerStore,
    ) -> K2Result<()> {
        let Some(target_url) = peer_store
            .get(target.clone())
            .await?
            .and_then(|t| t.url.clone())
        else {
            tracing::info!("initiate_gossip: target not found: {:?}", target);
            return Ok(());
        };

        let (our_agents, our_arc_set) = local_agent_state(space).await?;

        let initiate = K2GossipInitiateMessage {
            participating_agents: our_agents
                .into_iter()
                .map(|a| a.0 .0)
                .collect(),
            arc_set: Some(ArcSetMessage {
                arc_sectors: our_arc_set.into_raw().collect(),
            }),
            new_since: 0,             // TODO get from peer meta
            max_new_bytes: 32 * 1024, // TODO get from config
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
            if let Err(e) = this.respond_to_msg(from_url, msg).await {
                tracing::error!("could not respond to gossip message: {:?}", e);
            }
        });

        Ok(())
    }

    async fn respond_to_msg(
        &self,
        from_url: Url,
        msg: GossipMessage,
    ) -> K2Result<()> {
        let res = match msg {
            GossipMessage::Initiate(initiate) => {
                // TODO check how recently this peer initiated with us.
                //      Should be rate-limited to the gossip interval.

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
                tracing::info!("Other arc set: {:?}", other_arc_set);

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

                let missing_agents = initiate
                    .participating_agents
                    .into_iter()
                    .filter(|a| {
                        !send_agents.contains(&AgentId::from((*a).clone()))
                    })
                    .collect();

                Ok(Some(K2GossipMessage {
                    gossip_message: Some(GossipMessage::Accept(
                        K2GossipAcceptMessage {
                            participating_agents: send_agents
                                .into_iter()
                                .map(|a| a.0 .0)
                                .collect::<Vec<_>>(),
                            arc_set: Some(ArcSetMessage {
                                arc_sectors: our_arc_set.into_raw().collect(),
                            }),
                            missing_agents,
                            new_since: 0,
                            max_new_bytes: 32 * 1024,
                            new_ops: vec![],
                            updated_new_since: 0,
                        },
                    )),
                }))
            }
            GossipMessage::Accept(accept) => {
                // TODO check that we have a session active for this peer because we must have
                //      sent an initiate message, otherwise this is unsolicited.

                println!("Accept: {:?}", accept);

                let mut send_agent_infos = vec![];
                for missing_agent in accept.missing_agents {
                    if let Ok(Some(agent_info)) =
                        self.peer_store.get(AgentId::from(missing_agent)).await
                    {
                        send_agent_infos.push(agent_info);
                    }
                }

                Ok(Some(K2GossipMessage {
                    gossip_message: Some(GossipMessage::Diff(
                        K2GossipDiffMessage {
                            missing_agents: vec![],
                            provided_agents: send_agent_infos
                                .into_iter()
                                .map(|a| a.encode().map(|b| b.into()))
                                .collect::<K2Result<Vec<_>>>()?,
                            new_ops: vec![],
                            updated_new_since: 0,
                        },
                    )),
                }))
            }
            GossipMessage::Diff(diff) => {
                // TODO session check

                tracing::info!("Diff: {:?}", diff);

                // TODO check that the incoming agents are the one we requested
                let mut agents = Vec::with_capacity(diff.provided_agents.len());
                for agent in diff.provided_agents {
                    let agent_info = AgentInfoSigned::decode(
                        &self.agent_verifier,
                        agent.as_ref(),
                    )?;
                    agents.push(agent_info);
                }
                tracing::info!("Storing agents: {:?}", agents);
                self.peer_store.insert(agents).await?;

                Ok(None)
            }
        }?;

        if let Some(msg) = res {
            send_gossip_message(&self.response_tx, from_url, msg)?;
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
        println!("recv_module_msg: {:?}", data);

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
    use kitsune2_api::DhtArc;
    use kitsune2_core::{default_test_builder, Ed25519LocalAgent};
    use kitsune2_test_utils::enable_tracing;

    #[derive(Debug, Clone)]
    struct TestGossip {
        gossip: K2Gossip,
        peer_store: DynPeerStore,
        space: DynSpace,
    }

    impl TestGossip {
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

        pub async fn new_instance(&self) -> TestGossip {
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

            let gossip = K2Gossip::create(
                self.space_id.clone(),
                space.clone(),
                space.peer_store().clone(),
                self.builder
                    .op_store
                    .create(self.builder.clone(), self.space_id.clone())
                    .await
                    .unwrap(),
                transport.clone(),
                self.builder.verifier.clone(),
            );

            TestGossip {
                gossip,
                peer_store: space.peer_store().clone(),
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
    async fn push_agents_after_initiate() {
        enable_tracing();

        let space = test_space();
        let factory = TestGossipFactory::create(space.clone()).await;
        let harness_1 = factory.new_instance().await;
        let agent_1 = harness_1.join_local_agent().await;
        harness_1
            .space
            .update_tgt_storage_arc_hint(agent_1.agent().clone(), DhtArc::FULL)
            .await;
        let agent_info_1 = harness_1
            .peer_store
            .get(agent_1.agent().clone())
            .await
            .unwrap()
            .unwrap();

        let harness_2 = factory.new_instance().await;
        let agent_2 = harness_2.join_local_agent().await;
        harness_2
            .space
            .update_tgt_storage_arc_hint(agent_2.agent().clone(), DhtArc::FULL)
            .await;
        harness_2
            .peer_store
            .insert(vec![agent_info_1])
            .await
            .unwrap();

        // Join another agent to the space
        // This one will participate in gossip but the other harness won't know about it.
        let secret_agent = harness_2.join_local_agent().await;

        assert!(harness_1
            .peer_store
            .get(secret_agent.agent().clone())
            .await
            .unwrap()
            .is_none());

        harness_2
            .gossip
            .initiate_gossip(
                agent_1.agent().clone(),
                harness_2.space.clone(),
                harness_2.peer_store.clone(),
            )
            .await
            .unwrap();

        tokio::time::timeout(std::time::Duration::from_millis(100), {
            let harness_1 = harness_1.clone();
            async move {
                loop {
                    let has_agent = harness_1
                        .peer_store
                        .get(secret_agent.agent().clone())
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
