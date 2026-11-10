use crate::burst::AcceptBurstTracker;
use crate::gossip::K2Gossip;
use crate::peer_meta_store::K2PeerMetaStore;
use crate::protocol::{
    AcceptResponseMessage, GossipMessage, K2GossipNoDiffMessage,
    deserialize_gossip_message,
};
use crate::state::{GossipRoundState, RoundStage, RoundStageAccepted};
use crate::{K2GossipConfig, MOD_NAME};
use bytes::Bytes;
use kitsune2_api::*;
use kitsune2_core::{Ed25519LocalAgent, default_test_builder};
use kitsune2_dht::{ArcSet, Dht};
use kitsune2_test_utils::agent::AgentBuilder;
use kitsune2_test_utils::space::TEST_SPACE_ID;
#[cfg(test)]
use rand::Rng;
use std::ops::Deref;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};

/// Test-only harness for deterministically exercising gossip responses.
pub struct RespondTestHarness {
    pub(crate) gossip: K2Gossip,
    pub(crate) known_peers: DynKnownPeers,
    pub(crate) rx: tokio::sync::mpsc::Receiver<(Url, Bytes)>,
    pub(crate) _transport: DynTransport,
}

impl RespondTestHarness {
    /// Create a responder harness with the default gossip configuration.
    pub async fn create() -> Self {
        RespondTestHarness::create_with_config(Default::default()).await
    }

    pub(crate) async fn create_with_config(config: K2GossipConfig) -> Self {
        let builder =
            Arc::new(default_test_builder().with_default_config().unwrap());

        let op_store = builder
            .op_store
            .create(builder.clone(), TEST_SPACE_ID)
            .await
            .unwrap();

        let dht = Dht::try_from_store(Timestamp::now(), op_store.clone())
            .await
            .unwrap();

        let (tx, rx) = tokio::sync::mpsc::channel(100);
        let mut transport = MockTransport::new();
        transport.expect_send_module().returning(
            move |peer, space_id, module_id, data| {
                assert_eq!(space_id, TEST_SPACE_ID);
                assert_eq!(module_id.as_str(), MOD_NAME);

                let tx = tx.clone();
                Box::pin(async move {
                    tx.send((peer, data)).await.unwrap();
                    Ok(())
                })
            },
        );
        transport
            .expect_register_module_handler()
            .returning(|_, _, _| {});
        let transport: DynTransport = Arc::new(transport);
        let peer_meta_store = builder
            .peer_meta_store
            .create(builder.clone(), TEST_SPACE_ID)
            .await
            .unwrap();
        let blocks = builder
            .blocks
            .create(builder.clone(), TEST_SPACE_ID)
            .await
            .unwrap();
        let known_peers = builder
            .known_peers
            .create(builder.clone(), TEST_SPACE_ID)
            .await
            .unwrap();

        let config = Arc::new(config);
        Self {
            gossip: K2Gossip {
                config: config.clone(),
                initiated_round_state: Default::default(),
                accepted_round_states: Default::default(),
                dht: Arc::new(RwLock::new(dht)),
                space_id: TEST_SPACE_ID,
                peer_store: builder
                    .peer_store
                    .create(
                        builder.clone(),
                        TEST_SPACE_ID,
                        blocks,
                        known_peers.clone(),
                    )
                    .await
                    .unwrap(),
                local_agent_store: builder
                    .local_agent_store
                    .create(builder.clone())
                    .await
                    .unwrap(),
                peer_meta_store: Arc::new(K2PeerMetaStore::new(
                    peer_meta_store.clone(),
                )),
                op_store: op_store.clone(),
                fetch: builder
                    .fetch
                    .create(
                        builder.clone(),
                        TEST_SPACE_ID,
                        builder
                            .report
                            .create(builder.clone(), transport.clone())
                            .await
                            .unwrap(),
                        op_store.clone(),
                        peer_meta_store.clone(),
                        transport.clone(),
                    )
                    .await
                    .unwrap(),
                agent_verifier: builder.verifier.clone(),
                transport: Arc::downgrade(&transport),
                burst: AcceptBurstTracker::new(config),
                _initiate_task: Default::default(),
                _timeout_task: Default::default(),
                _dht_update_task: Default::default(),
            },
            known_peers,
            rx,
            _transport: transport,
        }
    }

    /// Create a signed test agent with the requested target storage arc.
    pub async fn create_agent(&self, tgt_storage_arc: DhtArc) -> TestAgent {
        let local_agent = Ed25519LocalAgent::default();
        local_agent.set_tgt_storage_arc_hint(tgt_storage_arc);

        let builder = AgentBuilder::default().with_url(Some(
            Url::from_str(format!("ws://test:80/{}", rand::random::<u64>()))
                .unwrap(),
        ));

        let local: DynLocalAgent = Arc::new(local_agent);
        TestAgent {
            local: local.clone(),
            agent_info: builder.build(local),
        }
    }

    pub(crate) async fn insert_initiated_round_state(
        &self,
        local_agent: &TestAgent,
        with_remote_agent: &TestAgent,
    ) -> Bytes {
        let mut round_state = self.gossip.initiated_round_state.lock().await;
        assert!(round_state.is_none());
        let state = GossipRoundState::new(
            with_remote_agent.url.clone().unwrap(),
            vec![local_agent.agent.clone()],
            ArcSet::new(vec![DhtArc::FULL]).unwrap(),
        );
        let session_id = state.session_id.clone();
        *round_state = Some(state);
        session_id
    }

    pub(crate) async fn insert_accepted_round_state(
        &self,
        local_agent: &TestAgent,
        with_remote_agent: &TestAgent,
    ) -> Bytes {
        let mut accepted = self.gossip.accepted_round_states.write().await;
        assert!(
            !accepted.contains_key(with_remote_agent.url.as_ref().unwrap())
        );
        let state = GossipRoundState::new(
            with_remote_agent.url.clone().unwrap(),
            vec![local_agent.agent.clone()],
            ArcSet::new(vec![DhtArc::FULL]).unwrap(),
        );
        let session_id = state.session_id.clone();
        accepted.insert(
            with_remote_agent.url.clone().unwrap(),
            Arc::new(Mutex::new(state)),
        );
        session_id
    }

    /// Return the peer store used by this gossip instance.
    pub fn peer_store(&self) -> DynPeerStore {
        self.gossip.peer_store.clone()
    }

    /// Return the URL index used by this gossip instance.
    pub fn known_peers(&self) -> DynKnownPeers {
        self.known_peers.clone()
    }

    /// Capture the agents response selected for a request.
    pub async fn prepare_agents_response(
        &mut self,
        responder: &TestAgent,
        requester: &TestAgent,
        requested_agent: AgentId,
        updated_new_since: Timestamp,
    ) -> (Bytes, Bytes) {
        let session_id =
            self.insert_accepted_round_state(responder, requester).await;
        {
            let accepted = self.gossip.accepted_round_states.read().await;
            let mut state = accepted
                .get(requester.url.as_ref().unwrap())
                .unwrap()
                .lock()
                .await;
            state.stage = RoundStage::Accepted(RoundStageAccepted {
                our_agents: vec![requested_agent.clone()],
                common_arc_set: ArcSet::new(vec![DhtArc::FULL]).unwrap(),
            });
        }
        self.gossip
            .respond_to_msg(
                requester.url.clone().unwrap(),
                GossipMessage::NoDiff(K2GossipNoDiffMessage {
                    session_id: session_id.clone(),
                    accept_response: Some(AcceptResponseMessage {
                        missing_agents: vec![requested_agent.0.into()],
                        provided_agents: vec![],
                        new_ops: vec![],
                        updated_new_since: updated_new_since.as_micros(),
                    }),
                    cannot_compare: false,
                }),
            )
            .await
            .unwrap();

        let (_, response) = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            self.rx.recv(),
        )
        .await
        .unwrap()
        .unwrap();
        (session_id, response)
    }

    /// Deliver a previously captured agents response.
    pub async fn receive_agents_response(
        &self,
        local_agent: &TestAgent,
        remote_agent: &TestAgent,
        session_id: Bytes,
        response: Bytes,
    ) {
        self.insert_initiated_round_state(local_agent, remote_agent)
            .await;
        {
            let mut state = self.gossip.initiated_round_state.lock().await;
            let state = state.as_mut().unwrap();
            state.session_id = session_id;
            state.stage = RoundStage::NoDiff;
        }
        self.gossip
            .respond_to_msg(
                remote_agent.url.clone().unwrap(),
                deserialize_gossip_message(response).unwrap(),
            )
            .await
            .unwrap();
    }

    #[cfg(test)]
    pub(crate) async fn wait_for_sent_response(&mut self) -> GossipMessage {
        let received = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            self.rx.recv(),
        )
        .await
        .unwrap()
        .unwrap();

        deserialize_gossip_message(received.1).unwrap()
    }
}

/// A local agent and its signed advertisement for responder tests.
#[derive(Debug)]
pub struct TestAgent {
    /// The signing local agent.
    pub local: DynLocalAgent,
    /// The signed agent advertisement.
    pub agent_info: Arc<AgentInfoSigned>,
}

impl Deref for TestAgent {
    type Target = Arc<AgentInfoSigned>;

    fn deref(&self) -> &Self::Target {
        &self.agent_info
    }
}

#[cfg(test)]
pub(crate) fn test_session_id() -> Bytes {
    let mut session_id = bytes::BytesMut::zeroed(12);
    rand::rng().fill_bytes(&mut session_id);

    session_id.freeze()
}
