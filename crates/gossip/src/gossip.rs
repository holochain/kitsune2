use crate::error::K2GossipError;
use crate::initiate::spawn_initiate_task;
use crate::peer_meta_store::K2PeerMetaStore;
use crate::protocol::{
    deserialize_gossip_message, encode_agent_ids, serialize_gossip_message,
    ArcSetMessage, GossipMessage, K2GossipInitiateMessage,
};
use crate::state::{GossipRoundState, RoundStage};
use crate::storage_arc::update_storage_arcs;
use crate::timeout::spawn_timeout_task;
use crate::update::spawn_dht_update_task;
use crate::{K2GossipConfig, K2GossipModConfig, MOD_NAME};
use bytes::Bytes;
use kitsune2_api::*;
use kitsune2_dht::{Dht, DhtApi};
use std::collections::HashMap;
use std::sync::{Arc, Weak};
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
    fn default_config(&self, config: &mut Config) -> K2Result<()> {
        config.set_module_config(&K2GossipModConfig::default())
    }

    fn validate_config(&self, _config: &Config) -> K2Result<()> {
        Ok(())
    }

    fn create(
        &self,
        builder: Arc<Builder>,
        space_id: SpaceId,
        peer_store: DynPeerStore,
        local_agent_store: DynLocalAgentStore,
        peer_meta_store: DynPeerMetaStore,
        op_store: DynOpStore,
        transport: DynTransport,
        fetch: DynFetch,
    ) -> BoxFut<'static, K2Result<DynGossip>> {
        Box::pin(async move {
            let config =
                builder.config.get_module_config::<K2GossipModConfig>()?;

            let gossip: DynGossip = K2Gossip::create(
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
            .await?;

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
    pub(crate) transport: Weak<dyn Transport>,
    pub(crate) _initiate_task: Arc<std::sync::Mutex<Option<DropAbortHandle>>>,
    pub(crate) _timeout_task: Arc<std::sync::Mutex<Option<DropAbortHandle>>>,
    pub(crate) _dht_update_task: Arc<std::sync::Mutex<Option<DropAbortHandle>>>,
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
    ) -> K2Result<Arc<K2Gossip>> {
        // Initialise the DHT model from the op store.
        //
        // This might take a while if the op store is large!
        let start = Instant::now();
        let dht =
            Dht::try_from_store(Timestamp::now(), op_store.clone()).await?;
        tracing::info!("DHT model initialised in {:?}", start.elapsed());

        let gossip = K2Gossip {
            config: Arc::new(config),
            initiated_round_state: Default::default(),
            accepted_round_states: Default::default(),
            dht: Arc::new(RwLock::new(dht)),
            space_id: space_id.clone(),
            peer_store,
            local_agent_store,
            peer_meta_store: Arc::new(K2PeerMetaStore::new(peer_meta_store)),
            op_store,
            fetch,
            agent_verifier,
            transport: Arc::downgrade(&transport),
            _initiate_task: Default::default(),
            _timeout_task: Default::default(),
            _dht_update_task: Default::default(),
        };

        let gossip = Arc::new(gossip);

        transport.register_module_handler(
            space_id,
            MOD_NAME.to_string(),
            gossip.clone(),
        );

        let (force_initiate, initiate_task) =
            spawn_initiate_task(gossip.config.clone(), Arc::downgrade(&gossip));
        *gossip._initiate_task.lock().unwrap() = Some(DropAbortHandle {
            name: "Gossip initiate task".to_string(),
            handle: initiate_task,
        });
        let timeout_task = spawn_timeout_task(
            gossip.config.clone(),
            force_initiate,
            Arc::downgrade(&gossip),
        );
        *gossip._timeout_task.lock().unwrap() = Some(DropAbortHandle {
            name: "Gossip timeout task".to_string(),
            handle: timeout_task,
        });
        let dht_update_task = spawn_dht_update_task(gossip.dht.clone());
        *gossip._dht_update_task.lock().unwrap() = Some(DropAbortHandle {
            name: "Gossip DHT update task".to_string(),
            handle: dht_update_task,
        });

        Ok(gossip)
    }
}

impl K2Gossip {
    /// Attempt to initiate gossip.
    ///
    /// If we have an initiated state, or an accepted state from this peer, this will fail and
    /// return `false`. Otherwise, we will send a [`K2GossipInitiateMessage`] to this peer URL and
    /// return `true`.
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

        let (our_agents, our_arc_set) = self.local_agent_state().await?;

        let new_since =
            self.get_request_new_since(target_peer_url.clone()).await?;

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
            max_op_data_bytes: self.config.max_gossip_op_bytes,
            tie_breaker: match &round_state.stage {
                RoundStage::Initiated(i) => i.tie_breaker,
                _ => unreachable!(),
            },
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

        // Record that we're initiating gossip with this peer.
        // Even if this fails, we should still record that this peer has been selected, and they
        // shouldn't be a valid target for the next initiation if this fails.
        self.peer_meta_store
            .set_last_gossip_timestamp(
                target_peer_url.clone(),
                Timestamp::now(),
            )
            .await?;

        self.send_gossip_message(
            GossipMessage::Initiate(initiate),
            target_peer_url,
        )
        .await?;
        *initiated_lock = Some(round_state);

        Ok(true)
    }

    pub(crate) async fn update_storage_arcs(
        &self,
        next_action: &kitsune2_dht::DhtSnapshotNextAction,
        their_snapshot: &kitsune2_dht::DhtSnapshot,
        common_arc_set: kitsune2_dht::ArcSet,
    ) -> K2Result<()> {
        if !matches!(
            next_action,
            kitsune2_dht::DhtSnapshotNextAction::CannotCompare
        ) {
            // As long as the comparison was successful, use this diff to update storage arcs.
            if let Err(e) = update_storage_arcs(
                their_snapshot,
                self.local_agent_store.get_all().await?,
                common_arc_set.clone(),
            ) {
                tracing::error!("Error updating storage arcs: {:?}", e);
            }
        }

        Ok(())
    }

    /// Send a gossip message to a peer
    pub(crate) async fn send_gossip_message(
        &self,
        msg: GossipMessage,
        target_url: Url,
    ) -> K2Result<()> {
        let Some(transport) = self.transport.upgrade() else {
            return Err(K2Error::other(
                "Transport has been dropped, can no longer send messages",
            ));
        };

        tracing::debug!(
            "Sending gossip message to {:?}: {:?}",
            target_url,
            msg
        );

        let msg = serialize_gossip_message(msg)?;
        transport
            .send_module(
                target_url,
                self.space_id.clone(),
                MOD_NAME.to_string(),
                msg,
            )
            .await
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
            match this.respond_to_msg(from_peer.clone(), msg).await {
                Ok(_) => {}
                Err(e @ K2GossipError::PeerBehaviorError { .. }) => {
                    tracing::error!("Peer behavior error: {:?}", e);

                    if let Err(e) = this
                        .peer_meta_store
                        .incr_peer_behavior_errors(from_peer)
                        .await
                    {
                        tracing::warn!(
                            "Could not record peer behavior error: {:?}",
                            e
                        );
                    }
                }
                Err(e) => {
                    tracing::error!(
                        "could not respond to gossip message: {:?}",
                        e
                    );

                    if let Err(e) =
                        this.peer_meta_store.incr_local_errors(from_peer).await
                    {
                        tracing::warn!("Could not record local error: {:?}", e);
                    }
                }
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

    fn get_state_summary(
        &self,
        request: GossipStateSummaryRequest,
    ) -> BoxFut<'_, K2Result<GossipStateSummary>> {
        Box::pin(async move { self.summary(request.include_dht_summary).await })
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

#[cfg(test)]
mod test {
    use super::*;
    use crate::harness::K2GossipFunctionalTestFactory;
    use kitsune2_core::factories::MemoryOp;
    use kitsune2_dht::{SECTOR_SIZE, UNIT_TIME};
    use kitsune2_test_utils::space::TEST_SPACE_ID;
    use kitsune2_test_utils::{enable_tracing, iter_check};

    #[tokio::test]
    async fn two_way_agent_sync() {
        enable_tracing();

        let space = TEST_SPACE_ID;
        let factory =
            K2GossipFunctionalTestFactory::create(space.clone(), false, None)
                .await;
        let harness_1 = factory.new_instance().await;
        let agent_info_1 = harness_1.join_local_agent(DhtArc::FULL).await;

        let harness_2 = factory.new_instance().await;
        let agent_info_2 = harness_2.join_local_agent(DhtArc::FULL).await;

        // Join extra agents for each peer. These will take a few seconds to be
        // found by bootstrap. Try to sync them with gossip.
        let secret_agent_1 = harness_1.join_local_agent(DhtArc::FULL).await;
        assert!(harness_2
            .space
            .peer_store()
            .get(secret_agent_1.agent.clone())
            .await
            .unwrap()
            .is_none());
        let secret_agent_2 = harness_2.join_local_agent(DhtArc::FULL).await;
        assert!(harness_1
            .space
            .peer_store()
            .get(secret_agent_2.agent.clone())
            .await
            .unwrap()
            .is_none());

        // Simulate peer discovery so that gossip can sync agents
        harness_2
            .space
            .peer_store()
            .insert(vec![agent_info_1.clone()])
            .await
            .unwrap();

        harness_1
            .wait_for_agent_in_peer_store(secret_agent_2.agent.clone())
            .await;
        harness_2
            .wait_for_agent_in_peer_store(secret_agent_1.agent.clone())
            .await;

        let completed_1 = harness_1
            .peer_meta_store
            .completed_rounds(agent_info_2.url.clone().unwrap())
            .await
            .unwrap()
            .unwrap_or_default();
        assert_eq!(1, completed_1);
        let completed_2 = harness_2
            .peer_meta_store
            .completed_rounds(agent_info_1.url.clone().unwrap())
            .await
            .unwrap()
            .unwrap_or_default();
        assert_eq!(1, completed_2);
    }

    #[tokio::test]
    async fn two_way_op_sync() {
        enable_tracing();

        let space = TEST_SPACE_ID;
        let factory =
            K2GossipFunctionalTestFactory::create(space.clone(), true, None)
                .await;
        let harness_1 = factory.new_instance().await;
        harness_1.join_local_agent(DhtArc::FULL).await;
        let op_1 = MemoryOp::new(Timestamp::now(), vec![1; 128]);
        let op_id_1 = op_1.compute_op_id();
        harness_1
            .space
            .op_store()
            .process_incoming_ops(vec![op_1.clone().into()])
            .await
            .unwrap();

        let harness_2 = factory.new_instance().await;
        harness_2.join_local_agent(DhtArc::FULL).await;
        let op_2 = MemoryOp::new(Timestamp::now(), vec![2; 128]);
        let op_id_2 = op_2.compute_op_id();
        harness_2
            .space
            .op_store()
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
        let factory =
            K2GossipFunctionalTestFactory::create(space.clone(), false, None)
                .await;

        let harness_1 = factory.new_instance().await;
        let agent_info_1 = harness_1.join_local_agent(DhtArc::FULL).await;
        let op_1 = MemoryOp::new(Timestamp::from_micros(100), vec![1; 128]);
        let op_id_1 = op_1.compute_op_id();
        harness_1
            .space
            .op_store()
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
            .space
            .op_store()
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
            .space
            .peer_store()
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
        let factory =
            K2GossipFunctionalTestFactory::create(space.clone(), false, None)
                .await;
        let harness_1 = factory.new_instance().await;
        let agent_info_1 = harness_1.join_local_agent(DhtArc::FULL).await;
        let op_1 = MemoryOp::new(
            (Timestamp::now() - 2 * UNIT_TIME).unwrap(),
            vec![1; 128],
        );
        let op_id_1 = op_1.compute_op_id();
        harness_1
            .space
            .op_store()
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
            .space
            .op_store()
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
            .space
            .peer_store()
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
    async fn respect_size_limit_for_new_ops() {
        enable_tracing();

        let space = TEST_SPACE_ID;
        let factory = K2GossipFunctionalTestFactory::create(
            space.clone(),
            true,
            Some(K2GossipConfig {
                max_gossip_op_bytes: 3 * 128,
                // Set the initiate interval low so that gossip will start quickly after
                // bootstrapping but leave the min initiate interval high so that we can
                // run a single gossip round during the test.
                initial_initiate_interval_ms: 10,
                initiate_interval_ms: 10,
                initiate_jitter_ms: 0,
                ..Default::default()
            }),
        )
        .await;
        let harness_1 = factory.new_instance().await;
        harness_1.join_local_agent(DhtArc::FULL).await;

        let mut ops = Vec::new();

        for i in 0u8..5 {
            let op = MemoryOp::new(Timestamp::now(), vec![i; 128]);
            ops.push(op);
        }

        harness_1
            .space
            .op_store()
            .process_incoming_ops(
                ops.clone().into_iter().map(Into::into).collect(),
            )
            .await
            .unwrap();
        harness_1
            .space
            .inform_ops_stored(
                ops.clone().into_iter().map(Into::into).collect(),
            )
            .await
            .unwrap();

        let harness_2 = factory.new_instance().await;
        harness_2.join_local_agent(DhtArc::FULL).await;

        // Just the first 3 ops should be received, the limit should prevent the next two from
        // being sent.
        harness_2
            .wait_for_ops(
                ops.iter().take(3).map(|op| op.compute_op_id()).collect(),
            )
            .await;

        // We received the ones we wanted above, make sure it's just those 3.
        let all_ops = harness_2
            .space
            .op_store()
            .retrieve_ops(ops.iter().map(|op| op.compute_op_id()).collect())
            .await
            .unwrap();
        assert_eq!(3, all_ops.len());
    }

    #[tokio::test]
    async fn respect_size_limit_for_new_ops_and_dht_disc_diff() {
        enable_tracing();

        let space = TEST_SPACE_ID;
        let factory = K2GossipFunctionalTestFactory::create(
            space.clone(),
            true,
            Some(K2GossipConfig {
                max_gossip_op_bytes: 7 * 128,
                // Set the initiate interval low so that gossip will start quickly after
                // bootstrapping but leave the min initiate interval high so that we can
                // run a single gossip round during the test.
                initial_initiate_interval_ms: 10,
                initiate_interval_ms: 10,
                initiate_jitter_ms: 0,
                ..Default::default()
            }),
        )
        .await;
        let harness_1 = factory.new_instance().await;
        let agent_info_1 = harness_1.join_local_agent(DhtArc::FULL).await;

        let mut ops = Vec::new();

        for i in 0u8..5 {
            let mut op_data = (i as u32 * SECTOR_SIZE).to_le_bytes().to_vec();
            op_data.resize(128, 0);

            let op =
                MemoryOp::new(Timestamp::from_micros(100 + i as i64), op_data);
            ops.push(op);
        }
        harness_1
            .space
            .op_store()
            .process_incoming_ops(
                ops.clone().into_iter().map(Into::into).collect(),
            )
            .await
            .unwrap();

        let bookmark = Timestamp::now();

        for i in 0u8..5 {
            let op = MemoryOp::new(Timestamp::now(), vec![100 + i; 128]);
            ops.push(op);
        }

        harness_1
            .space
            .op_store()
            .process_incoming_ops(
                ops.clone().into_iter().skip(5).map(Into::into).collect(),
            )
            .await
            .unwrap();
        harness_1
            .space
            .inform_ops_stored(
                ops.clone().into_iter().map(Into::into).collect(),
            )
            .await
            .unwrap();

        let harness_2 = factory.new_instance().await;

        // Set a bookmark so that the first 5 ops are behind the bookmark and need a disc sync and
        // the next 5 ops are ahead of the bookmark and a new ops sync
        harness_2
            .peer_meta_store
            .set_new_ops_bookmark(agent_info_1.url.clone().unwrap(), bookmark)
            .await
            .unwrap();

        // Now join an agent to the second harness so that we can send ops to it.
        harness_2.join_local_agent(DhtArc::FULL).await;

        // Just 2 historical and all 5 new ops should be sent.
        harness_2
            .wait_for_ops(
                ops.iter()
                    .take(2)
                    .chain(ops.iter().skip(5))
                    .map(|op| op.compute_op_id())
                    .collect(),
            )
            .await;

        // We received the ones we wanted above, make sure it's just those 7.
        let all_ops = harness_2
            .space
            .op_store()
            .retrieve_ops(ops.iter().map(|op| op.compute_op_id()).collect())
            .await
            .unwrap();
        assert_eq!(7, all_ops.len());
    }

    #[tokio::test]
    async fn respect_size_limit_for_new_ops_and_dht_ring_diff() {
        enable_tracing();

        let space = TEST_SPACE_ID;
        let factory = K2GossipFunctionalTestFactory::create(
            space.clone(),
            true,
            Some(K2GossipConfig {
                max_gossip_op_bytes: 7 * 128,
                // Set the initiate interval low so that gossip will start quickly after
                // bootstrapping but leave the min initiate interval high so that we can
                // run a single gossip round during the test.
                initial_initiate_interval_ms: 10,
                initiate_interval_ms: 10,
                initiate_jitter_ms: 0,
                ..Default::default()
            }),
        )
        .await;
        let harness_1 = factory.new_instance().await;
        let agent_info_1 = harness_1.join_local_agent(DhtArc::FULL).await;

        let mut ops = Vec::new();

        for i in 0u8..5 {
            let mut op_data = (i as u32 * SECTOR_SIZE).to_le_bytes().to_vec();
            op_data.resize(128, 0);

            let op = MemoryOp::new(
                (Timestamp::now() - 2 * UNIT_TIME).unwrap(),
                op_data,
            );
            ops.push(op);
        }
        harness_1
            .space
            .op_store()
            .process_incoming_ops(
                ops.clone().into_iter().map(Into::into).collect(),
            )
            .await
            .unwrap();

        let bookmark = Timestamp::now();

        for i in 0u8..5 {
            let op = MemoryOp::new(Timestamp::now(), vec![100 + i; 128]);
            ops.push(op);
        }

        harness_1
            .space
            .op_store()
            .process_incoming_ops(
                ops.clone().into_iter().skip(5).map(Into::into).collect(),
            )
            .await
            .unwrap();
        harness_1
            .space
            .inform_ops_stored(
                ops.clone().into_iter().map(Into::into).collect(),
            )
            .await
            .unwrap();

        let harness_2 = factory.new_instance().await;

        // Set a bookmark so that the first 5 ops are behind the bookmark and need a disc sync and
        // the next 5 ops are ahead of the bookmark and a new ops sync
        harness_2
            .peer_meta_store
            .set_new_ops_bookmark(agent_info_1.url.clone().unwrap(), bookmark)
            .await
            .unwrap();

        // Now join an agent to the second harness so that we can send ops to it.
        harness_2.join_local_agent(DhtArc::FULL).await;

        // Just 2 historical and all 5 new ops should be sent.
        harness_2
            .wait_for_ops(
                ops.iter()
                    .take(2)
                    .chain(ops.iter().skip(5))
                    .map(|op| op.compute_op_id())
                    .collect(),
            )
            .await;

        // We received the ones we wanted above, make sure it's just those 7.
        let all_ops = harness_2
            .space
            .op_store()
            .retrieve_ops(ops.iter().map(|op| op.compute_op_id()).collect())
            .await
            .unwrap();
        assert_eq!(7, all_ops.len());
    }

    #[tokio::test]
    async fn check_summary_after_round() {
        enable_tracing();

        let space = TEST_SPACE_ID;
        let factory =
            K2GossipFunctionalTestFactory::create(space.clone(), true, None)
                .await;
        let harness_1 = factory.new_instance().await;
        harness_1.join_local_agent(DhtArc::FULL).await;

        let harness_2 = factory.new_instance().await;
        let local_agent_2 = harness_2.join_local_agent(DhtArc::FULL).await;
        let op_2 = MemoryOp::new(Timestamp::now(), vec![2; 128]);
        let op_id_2 = op_2.compute_op_id();
        harness_2
            .space
            .op_store()
            .process_incoming_ops(vec![op_2.clone().into()])
            .await
            .unwrap();

        let received_ops = harness_1.wait_for_ops(vec![op_id_2]).await;
        assert_eq!(1, received_ops.len());
        assert_eq!(op_2, received_ops[0]);

        let summary = harness_1
            .gossip
            .get_state_summary(GossipStateSummaryRequest {
                include_dht_summary: true,
            })
            .await
            .unwrap();

        assert!(summary.dht_summary.contains_key("0..4294967295"));
        let meta = summary
            .peer_meta
            .get(&local_agent_2.url.clone().unwrap())
            .cloned()
            .unwrap();

        assert!(meta.completed_rounds.unwrap() > 0);
    }

    #[tokio::test]
    async fn force_initiate() {
        enable_tracing();

        let space = TEST_SPACE_ID;
        let factory = K2GossipFunctionalTestFactory::create(
            space.clone(),
            false,
            Some(K2GossipConfig {
                initial_initiate_interval_ms: 10,
                initiate_interval_ms: 5000,
                round_timeout_ms: 50,
                ..K2GossipConfig::default()
            }),
        )
        .await;

        // The second harness has an empty arc set, so it will be lower priority for target
        // selection in gossip initiate.
        let harness_2 = factory.new_instance().await;
        let agent_2 = harness_2.join_local_agent(DhtArc::Empty).await;

        // Create some agent info to use with the first harness so it has to work through some
        // unavailable agents.
        let mut junk_agents = vec![];
        for _ in 0..3 {
            // Create a new harness and join an agent to it.
            // We capture the agent info from this, but the harness is dropped immediately.
            let junk_harness = factory.new_instance().await;
            let junk_agent = junk_harness.join_local_agent(DhtArc::FULL).await;
            let junk_agent = junk_harness
                .force_storage_arc(junk_agent.agent.clone(), DhtArc::FULL)
                .await;

            println!("Created a junk agent: {:#?}", junk_agent);

            junk_agents.push(junk_agent);

            drop(junk_harness);
        }

        // The first harness is going to do our gossip initiation.
        let harness_1 = factory.new_instance().await;
        harness_1.join_local_agent(DhtArc::FULL).await;

        // Let the first harness know about all the junk agents
        harness_1
            .space
            .peer_store()
            .insert(junk_agents.clone())
            .await
            .unwrap();

        // Also let the first harness know about the second agent
        harness_1
            .space
            .peer_store()
            .insert(vec![agent_2.clone()])
            .await
            .unwrap();

        iter_check!(1000, 20, {
            let mut all_tried = true;

            for agent in &junk_agents {
                all_tried &= harness_1
                    .peer_meta_store
                    .last_gossip_timestamp(agent.url.clone().unwrap())
                    .await
                    .unwrap()
                    .is_some();
            }

            if all_tried {
                break;
            }

            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        });

        let good_agent_url = agent_2.url.clone().unwrap();
        iter_check!(1000, 20, {
            if harness_1
                .peer_meta_store
                .last_gossip_timestamp(good_agent_url.clone())
                .await
                .unwrap()
                .is_some()
            {
                break;
            }

            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        });

        // Note that the total time elapsed by this point must be less than the initiate
        // interval. So we must have force initiated after timeouts.
    }
}
