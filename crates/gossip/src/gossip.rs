use crate::initiate::spawn_initiate_task;
use crate::peer_meta_store::K2PeerMetaStore;
use crate::protocol::{
    deserialize_gossip_message, encode_agent_ids, serialize_gossip_message,
    ArcSetMessage, GossipMessage, K2GossipInitiateMessage,
};
use crate::state::GossipRoundState;
use crate::timeout::spawn_timeout_task;
use crate::update::spawn_dht_update_task;
use crate::{K2GossipConfig, K2GossipModConfig, MOD_NAME};
use bytes::Bytes;
use kitsune2_api::agent::DynVerifier;
use kitsune2_api::fetch::DynFetch;
use kitsune2_api::peer_store::DynPeerStore;
use kitsune2_api::transport::{DynTransport, TxBaseHandler, TxModuleHandler};
use kitsune2_api::{
    BoxFut, DynGossip, DynGossipFactory, DynLocalAgentStore, DynOpStore,
    DynPeerMetaStore, Gossip, GossipFactory, K2Error, K2Result, SpaceId,
    StoredOp, Timestamp, Url, UNIX_TIMESTAMP,
};
use kitsune2_dht::{Dht, DhtApi};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc::Sender;
use tokio::sync::{Mutex, RwLock};
use tokio::time::Instant;

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

    fn validate_config(
        &self,
        _config: &kitsune2_api::config::Config,
    ) -> K2Result<()> {
        Ok(())
    }

    fn create(
        &self,
        builder: Arc<kitsune2_api::builder::Builder>,
        space_id: SpaceId,
        peer_store: DynPeerStore,
        local_agent_store: DynLocalAgentStore,
        peer_meta_store: DynPeerMetaStore,
        op_store: DynOpStore,
        transport: DynTransport,
        fetch: DynFetch,
    ) -> kitsune2_api::BoxFut<'static, K2Result<DynGossip>> {
        Box::pin(async move {
            let config =
                builder.config.get_module_config::<K2GossipModConfig>()?;

            let gossip: DynGossip = Arc::new(
                K2Gossip::create(
                    config.k2_gossip,
                    space_id,
                    peer_store,
                    local_agent_store,
                    peer_meta_store,
                    op_store,
                    transport,
                    fetch,
                    builder.verifier.clone(),
                )
                .await?,
            );

            Ok(gossip)
        })
    }
}

#[derive(Debug)]
pub(crate) struct DropAbortHandle {
    pub(crate) name: String,
    pub(crate) handle: tokio::task::AbortHandle,
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
pub(crate) struct K2Gossip {
    pub(crate) config: Arc<K2GossipConfig>,
    /// The state of the current initiated gossip round.
    ///
    /// We only initiate one round at a time, so this is a single value.
    pub(crate) initiated_round_state: Arc<Mutex<Option<GossipRoundState>>>,
    /// The state of currently accepted gossip rounds.
    ///
    /// This is a map of agent ids to their round state. We can accept gossip from multiple agents
    /// at once, mostly to avoid coordinating initiation.
    pub(crate) accepted_round_states:
        Arc<RwLock<HashMap<Url, Arc<Mutex<GossipRoundState>>>>>,
    /// The DHT model.
    ///
    /// This is used to calculate the diff between local DHT state and the remote state of peers
    /// during gossip rounds.
    pub(crate) dht: Arc<RwLock<dyn DhtApi>>,
    pub(crate) space_id: SpaceId,
    // This is a weak reference because we need to call the space, but we do not create and own it.
    // Only a problem in this case because we register the gossip module with the transport and
    // create a cycle.
    pub(crate) peer_store: DynPeerStore,
    pub(crate) local_agent_store: DynLocalAgentStore,
    pub(crate) peer_meta_store: Arc<K2PeerMetaStore>,
    pub(crate) op_store: DynOpStore,
    pub(crate) fetch: DynFetch,
    pub(crate) agent_verifier: DynVerifier,
    pub(crate) response_tx: Sender<GossipResponse>,
    pub(crate) _response_task: Arc<DropAbortHandle>,
    pub(crate) _initiate_task: Arc<Option<DropAbortHandle>>,
    pub(crate) _timeout_task: Arc<Option<DropAbortHandle>>,
    pub(crate) _dht_update_task: Arc<Option<DropAbortHandle>>,
}

impl K2Gossip {
    /// Construct a new [K2Gossip] instance.
    #[allow(clippy::too_many_arguments)]
    pub async fn create(
        config: K2GossipConfig,
        space_id: SpaceId,
        peer_store: DynPeerStore,
        local_agent_store: DynLocalAgentStore,
        peer_meta_store: DynPeerMetaStore,
        op_store: DynOpStore,
        transport: DynTransport,
        fetch: DynFetch,
        agent_verifier: DynVerifier,
    ) -> K2Result<K2Gossip> {
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

        // Initialise the DHT model from the op store.
        //
        // This might take a while if the op store is large!
        let start = Instant::now();
        let dht =
            Dht::try_from_store(Timestamp::now(), op_store.clone()).await?;
        tracing::info!("DHT model initialised in {:?}", start.elapsed());

        let mut gossip = K2Gossip {
            config: Arc::new(config),
            initiated_round_state: Default::default(),
            accepted_round_states: Default::default(),
            dht: Arc::new(RwLock::new(dht)),
            space_id: space_id.clone(),
            peer_store,
            local_agent_store,
            peer_meta_store: Arc::new(K2PeerMetaStore::new(
                peer_meta_store,
                space_id.clone(),
            )),
            op_store,
            fetch,
            agent_verifier,
            response_tx,
            _response_task: Arc::new(DropAbortHandle {
                name: "Gossip response task".to_string(),
                handle: response_task,
            }),
            _initiate_task: Default::default(),
            _timeout_task: Default::default(),
            _dht_update_task: Default::default(),
        };

        transport.register_module_handler(
            space_id,
            MOD_NAME.to_string(),
            Arc::new(gossip.clone()),
        );

        let initiate_task =
            spawn_initiate_task(gossip.config.clone(), gossip.clone());
        gossip._initiate_task = Arc::new(Some(DropAbortHandle {
            name: "Gossip initiate task".to_string(),
            handle: initiate_task,
        }));
        let timeout_task =
            spawn_timeout_task(gossip.config.clone(), gossip.clone());
        gossip._timeout_task = Arc::new(Some(DropAbortHandle {
            name: "Gossip timeout task".to_string(),
            handle: timeout_task,
        }));
        let dht_update_task = spawn_dht_update_task(gossip.dht.clone());
        gossip._dht_update_task = Arc::new(Some(DropAbortHandle {
            name: "Gossip DHT update task".to_string(),
            handle: dht_update_task,
        }));

        Ok(gossip)
    }
}

impl K2Gossip {
    pub(crate) async fn initiate_gossip(
        &self,
        target_peer_url: Url,
    ) -> K2Result<bool> {
        let mut initiated_lock = self.initiated_round_state.lock().await;
        if initiated_lock.is_some() {
            tracing::debug!("initiate_gossip: already initiated");
            return Ok(false);
        }

        tracing::debug!(
            "Attempting to initiate gossip with {}",
            target_peer_url
        );

        // Before we send the initiate message, check whether the target has already
        // initiated with us. If they have, we can just skip initiating.
        if self
            .accepted_round_states
            .read()
            .await
            .contains_key(&target_peer_url)
        {
            tracing::info!("initiate_gossip: already accepted");
            return Ok(false);
        }

        let (our_agents, our_arc_set) = self.local_agent_state().await?;

        let new_since = self
            .peer_meta_store
            .new_ops_bookmark(target_peer_url.clone())
            .await?
            .unwrap_or(UNIX_TIMESTAMP);

        let round_state = GossipRoundState::new(
            target_peer_url.clone(),
            our_agents.clone(),
            our_arc_set.clone(),
        );
        let initiate = K2GossipInitiateMessage {
            session_id: round_state.session_id.clone(),
            participating_agents: encode_agent_ids(our_agents),
            arc_set: Some(ArcSetMessage {
                value: our_arc_set.encode(),
            }),
            new_since: new_since.as_micros(),
            max_new_bytes: self.config.max_gossip_op_bytes,
        };

        // Before we send the initiate message, check whether the target has already
        // initiated with us. If they have, we can just skip initiating.
        if self
            .accepted_round_states
            .read()
            .await
            .contains_key(&target_peer_url)
        {
            tracing::info!("initiate_gossip: already accepted");
            return Ok(false);
        }

        tracing::trace!(
            ?initiate,
            "Initiate gossip with {:?}",
            target_peer_url,
        );

        send_gossip_message(
            &self.response_tx,
            target_peer_url,
            GossipMessage::Initiate(initiate),
        )?;
        *initiated_lock = Some(round_state);

        Ok(true)
    }

    /// Handle an incoming gossip message.
    ///
    /// Produces a response message if the input is acceptable and a response is required.
    /// Otherwise, returns None.
    fn handle_gossip_message(
        &self,
        from_peer: Url,
        msg: GossipMessage,
    ) -> K2Result<()> {
        tracing::debug!(?msg, "handle_gossip_message from: {:?}", from_peer,);

        let this = self.clone();
        tokio::task::spawn(async move {
            if let Err(e) = this.respond_to_msg(from_peer, msg).await {
                tracing::error!("could not respond to gossip message: {:?}", e);
            }
        });

        Ok(())
    }
}

impl Gossip for K2Gossip {
    fn inform_ops_stored(
        &self,
        ops: Vec<StoredOp>,
    ) -> BoxFut<'_, K2Result<()>> {
        Box::pin(
            async move { self.dht.write().await.inform_ops_stored(ops).await },
        )
    }
}

impl TxBaseHandler for K2Gossip {}
impl TxModuleHandler for K2Gossip {
    fn recv_module_msg(
        &self,
        peer: Url,
        space: SpaceId,
        module: String,
        data: Bytes,
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

        let msg = deserialize_gossip_message(data)?;
        match self.handle_gossip_message(peer, msg) {
            Ok(()) => Ok(()),
            Err(e) => {
                tracing::error!("could not handle gossip message: {:?}", e);
                Err(e)
            }
        }
    }
}

pub(crate) struct GossipResponse(pub(crate) Bytes, pub(crate) Url);

pub(crate) fn send_gossip_message(
    tx: &Sender<GossipResponse>,
    target_url: Url,
    msg: GossipMessage,
) -> K2Result<()> {
    tracing::debug!("Sending gossip response to {:?}: {:?}", target_url, msg);
    tx.try_send(GossipResponse(serialize_gossip_message(msg)?, target_url))
        .map_err(|e| K2Error::other_src("could not send response", e))
}

#[cfg(test)]
mod test {
    use super::*;
    use kitsune2_api::agent::{AgentInfoSigned, LocalAgent};
    use kitsune2_api::builder::Builder;
    use kitsune2_api::space::{DynSpace, SpaceHandler};
    use kitsune2_api::transport::{TxHandler, TxSpaceHandler};
    use kitsune2_api::{AgentId, DhtArc, OpId};
    use kitsune2_core::factories::MemoryOp;
    use kitsune2_core::{default_test_builder, Ed25519LocalAgent};
    use kitsune2_dht::UNIT_TIME;
    use kitsune2_test_utils::enable_tracing;
    use kitsune2_test_utils::noop_bootstrap::NoopBootstrapFactory;
    use kitsune2_test_utils::space::TEST_SPACE_ID;
    use std::time::Duration;

    #[derive(Debug, Clone)]
    struct GossipTestHarness {
        _gossip: DynGossip,
        peer_store: DynPeerStore,
        op_store: DynOpStore,
        space: DynSpace,
        peer_meta_store: Arc<K2PeerMetaStore>,
    }

    impl GossipTestHarness {
        async fn join_local_agent(
            &self,
            target_arc: DhtArc,
        ) -> Arc<AgentInfoSigned> {
            let local_agent = Arc::new(Ed25519LocalAgent::default());
            local_agent.set_tgt_storage_arc_hint(target_arc);

            self.space
                .local_agent_join(local_agent.clone())
                .await
                .unwrap();

            // Wait for the agent info to be published
            // This means tests can rely on the agent being available in the peer store
            tokio::time::timeout(Duration::from_secs(5), {
                let agent_id = local_agent.agent().clone();
                let peer_store = self.peer_store.clone();
                async move {
                    while !peer_store
                        .get_all()
                        .await
                        .unwrap()
                        .iter()
                        .any(|a| a.agent.clone() == agent_id)
                    {
                        tokio::time::sleep(Duration::from_millis(5)).await;
                    }
                }
            })
            .await
            .unwrap();

            self.peer_store
                .get(local_agent.agent().clone())
                .await
                .unwrap()
                .unwrap()
        }

        async fn wait_for_agent_in_peer_store(&self, agent: AgentId) {
            tokio::time::timeout(Duration::from_millis(100), {
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

                        tokio::time::sleep(Duration::from_millis(5)).await;
                    }
                }
            })
            .await
            .unwrap();
        }

        async fn wait_for_ops(&self, op_ids: Vec<OpId>) -> Vec<MemoryOp> {
            tokio::time::timeout(Duration::from_millis(500), {
                let this = self.clone();
                async move {
                    loop {
                        tokio::time::sleep(Duration::from_millis(10)).await;

                        let ops = this
                            .op_store
                            .retrieve_ops(op_ids.clone())
                            .await
                            .unwrap();

                        if ops.len() != op_ids.len() {
                            tracing::info!(
                                "Have {}/{} requested ops",
                                ops.len(),
                                op_ids.len()
                            );
                            continue;
                        }

                        return ops
                            .into_iter()
                            .map(|op| {
                                let out: MemoryOp = op.op_data.into();

                                out
                            })
                            .collect::<Vec<_>>();
                    }
                }
            })
            .await
            .unwrap()
        }
    }

    struct TestGossipFactory {
        space_id: SpaceId,
        builder: Arc<Builder>,
    }

    impl TestGossipFactory {
        pub async fn create(
            space: SpaceId,
            bootstrap: bool,
        ) -> TestGossipFactory {
            let mut builder =
                default_test_builder().with_default_config().unwrap();
            // Replace the core builder with a real gossip factory
            builder.gossip = K2GossipFactory::create();

            if !bootstrap {
                builder.bootstrap = Arc::new(NoopBootstrapFactory);
            }

            builder
                .config
                .set_module_config(&K2GossipModConfig {
                    k2_gossip: K2GossipConfig {
                        initiate_interval_ms: 10,
                        min_initiate_interval_ms: 10,
                        ..Default::default()
                    },
                })
                .unwrap();
            let builder = Arc::new(builder);

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

            let peer_meta_store = K2PeerMetaStore::new(
                space.peer_meta_store().clone(),
                self.space_id.clone(),
            );

            GossipTestHarness {
                _gossip: space.gossip().clone(),
                peer_store: space.peer_store().clone(),
                op_store: space.op_store().clone(),
                space,
                peer_meta_store: Arc::new(peer_meta_store),
            }
        }
    }

    #[tokio::test]
    async fn two_way_agent_sync() {
        enable_tracing();

        let space = TEST_SPACE_ID;
        let factory = TestGossipFactory::create(space.clone(), false).await;
        let harness_1 = factory.new_instance().await;
        let agent_info_1 = harness_1.join_local_agent(DhtArc::FULL).await;

        let harness_2 = factory.new_instance().await;
        harness_2.join_local_agent(DhtArc::FULL).await;

        // Join extra agents for each peer. These will take a few seconds to be
        // found by bootstrap. Try to sync them with gossip.
        let secret_agent_1 = harness_1.join_local_agent(DhtArc::FULL).await;
        assert!(harness_2
            .peer_store
            .get(secret_agent_1.agent.clone())
            .await
            .unwrap()
            .is_none());
        let secret_agent_2 = harness_2.join_local_agent(DhtArc::FULL).await;
        assert!(harness_1
            .peer_store
            .get(secret_agent_2.agent.clone())
            .await
            .unwrap()
            .is_none());

        // Simulate peer discovery so that gossip can sync agents
        harness_2
            .peer_store
            .insert(vec![agent_info_1.clone()])
            .await
            .unwrap();

        harness_1
            .wait_for_agent_in_peer_store(secret_agent_2.agent.clone())
            .await;
        harness_2
            .wait_for_agent_in_peer_store(secret_agent_1.agent.clone())
            .await;
    }

    #[tokio::test]
    async fn two_way_op_sync() {
        enable_tracing();

        let space = TEST_SPACE_ID;
        let factory = TestGossipFactory::create(space.clone(), true).await;
        let harness_1 = factory.new_instance().await;
        harness_1.join_local_agent(DhtArc::FULL).await;
        let op_1 = MemoryOp::new(Timestamp::now(), vec![1; 128]);
        let op_id_1 = op_1.compute_op_id();
        harness_1
            .op_store
            .process_incoming_ops(vec![op_1.clone().into()])
            .await
            .unwrap();

        let harness_2 = factory.new_instance().await;
        harness_2.join_local_agent(DhtArc::FULL).await;
        let op_2 = MemoryOp::new(Timestamp::now(), vec![2; 128]);
        let op_id_2 = op_2.compute_op_id();
        harness_2
            .op_store
            .process_incoming_ops(vec![op_2.clone().into()])
            .await
            .unwrap();

        let received_ops = harness_1.wait_for_ops(vec![op_id_2]).await;
        assert_eq!(1, received_ops.len());
        assert_eq!(op_2, received_ops[0]);

        let received_ops = harness_2.wait_for_ops(vec![op_id_1]).await;
        assert_eq!(1, received_ops.len());
        assert_eq!(op_1, received_ops[0]);
    }

    #[tokio::test]
    async fn disc_sync() {
        enable_tracing();

        let space = TEST_SPACE_ID;
        let factory = TestGossipFactory::create(space.clone(), false).await;

        let harness_1 = factory.new_instance().await;
        let agent_info_1 = harness_1.join_local_agent(DhtArc::FULL).await;
        let op_1 = MemoryOp::new(Timestamp::from_micros(100), vec![1; 128]);
        let op_id_1 = op_1.compute_op_id();
        harness_1
            .op_store
            .process_incoming_ops(vec![op_1.clone().into()])
            .await
            .unwrap();
        harness_1
            .space
            .inform_ops_stored(vec![op_1.clone().into()])
            .await
            .unwrap();

        let harness_2 = factory.new_instance().await;
        let agent_info_2 = harness_2.join_local_agent(DhtArc::FULL).await;
        let op_2 = MemoryOp::new(Timestamp::from_micros(500), vec![2; 128]);
        let op_id_2 = op_2.compute_op_id();
        harness_2
            .op_store
            .process_incoming_ops(vec![op_2.clone().into()])
            .await
            .unwrap();
        harness_2
            .space
            .inform_ops_stored(vec![op_2.clone().into()])
            .await
            .unwrap();

        // Inform the harnesses that we've already synced up to now. This will force the
        // ops we just created to be treated as historical disc ops.
        harness_1
            .peer_meta_store
            .set_new_ops_bookmark(
                agent_info_2.url.clone().unwrap(),
                Timestamp::now(),
            )
            .await
            .unwrap();
        harness_2
            .peer_meta_store
            .set_new_ops_bookmark(
                agent_info_1.url.clone().unwrap(),
                Timestamp::now(),
            )
            .await
            .unwrap();

        // Simulate peer discovery so that gossip can perform a disc sync
        harness_1
            .peer_store
            .insert(vec![agent_info_2.clone()])
            .await
            .unwrap();

        let received_ops = harness_1.wait_for_ops(vec![op_id_2]).await;
        assert_eq!(1, received_ops.len());
        assert_eq!(op_2, received_ops[0]);

        let received_ops = harness_2.wait_for_ops(vec![op_id_1]).await;
        assert_eq!(1, received_ops.len());
        assert_eq!(op_1, received_ops[0]);
    }

    #[tokio::test]
    async fn ring_sync() {
        enable_tracing();

        let space = TEST_SPACE_ID;
        let factory = TestGossipFactory::create(space.clone(), false).await;
        let harness_1 = factory.new_instance().await;
        let agent_info_1 = harness_1.join_local_agent(DhtArc::FULL).await;
        let op_1 = MemoryOp::new(
            (Timestamp::now() - 2 * UNIT_TIME).unwrap(),
            vec![1; 128],
        );
        let op_id_1 = op_1.compute_op_id();
        harness_1
            .op_store
            .process_incoming_ops(vec![op_1.clone().into()])
            .await
            .unwrap();
        harness_1
            .space
            .inform_ops_stored(vec![op_1.clone().into()])
            .await
            .unwrap();

        let harness_2 = factory.new_instance().await;
        let agent_info_2 = harness_2.join_local_agent(DhtArc::FULL).await;
        let op_2 = MemoryOp::new(
            (Timestamp::now() - 2 * UNIT_TIME).unwrap(),
            vec![2; 128],
        );
        let op_id_2 = op_2.compute_op_id();
        harness_2
            .op_store
            .process_incoming_ops(vec![op_2.clone().into()])
            .await
            .unwrap();
        harness_2
            .space
            .inform_ops_stored(vec![op_2.clone().into()])
            .await
            .unwrap();

        // Inform the harnesses that we've already synced up to now. This will force the
        // ops we just created to be treated as historical ring ops.
        harness_1
            .peer_meta_store
            .set_new_ops_bookmark(
                agent_info_2.url.clone().unwrap(),
                Timestamp::now(),
            )
            .await
            .unwrap();
        harness_2
            .peer_meta_store
            .set_new_ops_bookmark(
                agent_info_1.url.clone().unwrap(),
                Timestamp::now(),
            )
            .await
            .unwrap();

        // Simulate peer discovery so that gossip can perform a ring sync
        harness_1
            .peer_store
            .insert(vec![agent_info_2.clone()])
            .await
            .unwrap();

        let received_ops = harness_1.wait_for_ops(vec![op_id_2]).await;
        assert_eq!(1, received_ops.len());
        assert_eq!(op_2, received_ops[0]);

        let received_ops = harness_2.wait_for_ops(vec![op_id_1]).await;
        assert_eq!(1, received_ops.len());
        assert_eq!(op_1, received_ops[0]);
    }
}
