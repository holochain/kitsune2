use crate::initiate::spawn_initiate_task;
use crate::peer_meta_store::K2PeerMetaStore;
use crate::protocol::k2_gossip_accept_message::SnapshotMinimalMessage;
use crate::protocol::{
    deserialize_gossip_message, encode_agent_ids, encode_agent_infos,
    encode_op_ids, serialize_gossip_message, AcceptResponseMessage,
    ArcSetMessage, GossipMessage, K2GossipAcceptMessage, K2GossipAgentsMessage,
    K2GossipDiscSectorDetailsDiffMessage,
    K2GossipDiscSectorDetailsDiffResponseMessage,
    K2GossipDiscSectorsDiffMessage, K2GossipHashesMessage,
    K2GossipInitiateMessage, K2GossipNoDiffMessage,
    K2GossipRingSectorDetailsDiffMessage,
};
use crate::state::{
    GossipRoundState, RoundStage, RoundStageDiscSectorDetailsDiff,
    RoundStageDiscSectorsDiff,
};
use crate::timeout::spawn_timeout_task;
use crate::{K2GossipConfig, K2GossipModConfig, MOD_NAME};
use bytes::Bytes;
use kitsune2_api::agent::{AgentInfoSigned, DynVerifier, LocalAgent};
use kitsune2_api::fetch::DynFetch;
use kitsune2_api::id::decode_ids;
use kitsune2_api::peer_store::DynPeerStore;
use kitsune2_api::transport::{DynTransport, TxBaseHandler, TxModuleHandler};
use kitsune2_api::{
    AgentId, DynGossip, DynGossipFactory, DynLocalAgentStore, DynOpStore,
    DynPeerMetaStore, Gossip, GossipFactory, K2Error, K2Result, SpaceId,
    Timestamp, Url, UNIX_TIMESTAMP,
};
use kitsune2_dht::snapshot::DhtSnapshot;
use kitsune2_dht::{ArcSet, Dht, DhtSnapshotNextAction};
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
            let config: K2GossipConfig = builder.config.get_module_config()?;

            let gossip: DynGossip = Arc::new(
                K2Gossip::create(
                    config,
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
pub(crate) struct K2Gossip {
    config: Arc<K2GossipConfig>,
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
    dht: Arc<RwLock<Dht>>,
    space_id: SpaceId,
    // This is a weak reference because we need to call the space, but we do not create and own it.
    // Only a problem in this case because we register the gossip module with the transport and
    // create a cycle.
    pub(crate) peer_store: DynPeerStore,
    pub(crate) local_agent_store: DynLocalAgentStore,
    pub(crate) peer_meta_store: Arc<K2PeerMetaStore>,
    op_store: DynOpStore,
    fetch: DynFetch,
    agent_verifier: DynVerifier,
    response_tx: Sender<GossipResponse>,
    _response_task: Arc<DropAbortHandle>,
    _initiate_task: Arc<Option<DropAbortHandle>>,
    _timeout_task: Arc<Option<DropAbortHandle>>,
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

        Ok(gossip)
    }
}

impl K2Gossip {
    pub(crate) async fn initiate_gossip(
        &self,
        target_peer_url: Url,
    ) -> K2Result<bool> {
        let state = self.initiated_round_state.clone();
        let mut initiated_lock = state.lock().await;
        if initiated_lock.is_some() {
            tracing::debug!("initiate_gossip: already initiated");
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

        // Right before we send the initiate message, check whether the target has already
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
            "Initiate gossip with {:?}: {:?}",
            target_peer_url,
            initiate
        );

        send_gossip_message(
            &self.response_tx,
            target_peer_url,
            GossipMessage::Initiate(initiate),
        )?;
        *initiated_lock = Some(round_state);

        Ok(true)
    }

    /// Handle a gossip message.
    ///
    /// Produces a response message if the input is acceptable and a response is required.
    /// Otherwise, returns None.
    fn handle_gossip_message(
        &self,
        from_peer: Url,
        msg: GossipMessage,
    ) -> K2Result<()> {
        tracing::debug!(
            "handle_gossip_message from: {:?}, msg: {:?}",
            from_peer,
            msg
        );

        let this = self.clone();
        tokio::task::spawn(async move {
            if let Err(e) = this.respond_to_msg(from_peer, msg).await {
                tracing::error!("could not respond to gossip message: {:?}", e);
            }
        });

        Ok(())
    }

    async fn respond_to_msg(
        &self,
        from_peer: Url,
        msg: GossipMessage,
    ) -> K2Result<()> {
        let res = match msg {
            GossipMessage::Initiate(initiate) => {
                // Rate limit incoming gossip messages by peer
                if let Some(timestamp) = self
                    .peer_meta_store
                    .last_gossip_timestamp(from_peer.clone())
                    .await?
                {
                    let elapsed =
                        (Timestamp::now() - timestamp).map_err(|_| {
                            K2Error::other("could not calculate elapsed time")
                        })?;

                    if elapsed < self.config.min_initiate_interval() {
                        tracing::info!("peer {:?} attempted to initiate too soon {:?} < {:?}", from_peer, elapsed, self.config.min_initiate_interval());
                        return Err(K2Error::other("initiate too soon"));
                    }
                }

                // Note the gap between the read and write here. It's possible that both peers
                // could initiate at the same time. This is slightly wasteful but shouldn't be a
                // problem.
                self.peer_meta_store
                    .set_last_gossip_timestamp(
                        from_peer.clone(),
                        Timestamp::now(),
                    )
                    .await?;

                let other_arc_set = match &initiate.arc_set {
                    Some(message) => ArcSet::decode(&message.value)?,
                    None => {
                        return Err(K2Error::other(
                            "no arc set in initiate message",
                        ));
                    }
                };

                let (our_agents, our_arc_set) =
                    self.local_agent_state().await?;
                let common_arc_set = our_arc_set.intersection(&other_arc_set);

                let snapshot: Option<SnapshotMinimalMessage> = if common_arc_set
                    .covered_sector_count()
                    > 0
                {
                    let snapshot = self
                        .dht
                        .read()
                        .await
                        .snapshot_minimal(&common_arc_set)
                        .await?;
                    Some(snapshot.try_into()?)
                } else {
                    // TODO Need to decide what to do here. It's useful for now and it's reasonable
                    //      to need to initiate to discover this but we do want to minimize work
                    //      in this case.
                    tracing::info!("no common arc set, continue to sync agents but not ops");
                    None
                };

                // There's no validation to be done with an accept beyond what's been done above
                // to check how recently this peer initiated with us. We'll just record that they
                // have initiated and that we plan to accept.
                self.create_accept_state(
                    &from_peer,
                    &initiate,
                    our_agents.clone(),
                    common_arc_set,
                )
                .await?;

                let missing_agents = self
                    .filter_known_agents(&initiate.participating_agents)
                    .await?;

                let new_since = self
                    .peer_meta_store
                    .new_ops_bookmark(from_peer.clone())
                    .await?
                    .unwrap_or(UNIX_TIMESTAMP);

                let (new_ops, new_bookmark) = self
                    .op_store
                    .retrieve_op_ids_bounded(
                        Timestamp::from_micros(initiate.new_since),
                        initiate.max_new_bytes as usize,
                    )
                    .await?;

                Ok(Some(GossipMessage::Accept(K2GossipAcceptMessage {
                    session_id: initiate.session_id,
                    participating_agents: encode_agent_ids(our_agents),
                    arc_set: Some(ArcSetMessage {
                        value: our_arc_set.encode(),
                    }),
                    missing_agents,
                    new_since: new_since.as_micros(),
                    max_new_bytes: self.config.max_gossip_op_bytes,
                    new_ops: encode_op_ids(new_ops),
                    updated_new_since: new_bookmark.as_micros(),
                    snapshot,
                })))
            }
            GossipMessage::Accept(accept) => {
                // Validate the incoming accept against our own state.
                let mut initiated_lock =
                    self.initiated_round_state.lock().await;
                let initiated = match initiated_lock.as_ref() {
                    Some(state) => {
                        state.validate_accept(from_peer.clone(), &accept)?
                    }
                    None => {
                        return Err(K2Error::other(
                            "Unsolicited Accept message",
                        ));
                    }
                };

                // Only once the other peer has accepted should we record that we've tried to
                // gossip with them. Otherwise, we risk blocking each other if we both record
                // a last gossip timestamp and try to initiate at the same time.
                self.peer_meta_store
                    .set_last_gossip_timestamp(
                        from_peer.clone(),
                        Timestamp::now(),
                    )
                    .await?;

                let missing_agents = self
                    .filter_known_agents(&accept.participating_agents)
                    .await?;

                let send_agent_infos =
                    self.load_agent_infos(accept.missing_agents).await;

                self.update_new_ops_bookmark(
                    from_peer.clone(),
                    Timestamp::from_micros(accept.updated_new_since),
                )
                .await?;

                // Send discovered ops to the fetch queue
                self.fetch
                    .request_ops(decode_ids(accept.new_ops), from_peer.clone())
                    .await?;

                self.peer_meta_store
                    .set_new_ops_bookmark(
                        from_peer.clone(),
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

                // The common part
                let accept_response = AcceptResponseMessage {
                    missing_agents,
                    provided_agents: encode_agent_infos(send_agent_infos)?,
                    new_ops: encode_op_ids(new_ops),
                    updated_new_since: new_bookmark.as_micros(),
                };

                match accept.snapshot {
                    Some(their_snapshot) => {
                        let other_arc_set = match &accept.arc_set {
                            Some(message) => ArcSet::decode(&message.value)?,
                            None => {
                                return Err(K2Error::other(
                                    "no arc set in accept message",
                                ));
                            }
                        };
                        let common_arc_set =
                            other_arc_set.intersection(&initiated.our_arc_set);
                        let next_action = self
                            .dht
                            .read()
                            .await
                            .handle_snapshot(
                                &their_snapshot.into(),
                                None,
                                &common_arc_set,
                            )
                            .await?;

                        match next_action {
                            DhtSnapshotNextAction::Identical => {
                                if let Some(state) = initiated_lock.as_mut() {
                                    state.stage = RoundStage::NoDiff;
                                }

                                Ok(Some(GossipMessage::NoDiff(
                                    K2GossipNoDiffMessage {
                                        session_id: accept.session_id,
                                        accept_response: Some(accept_response),
                                        cannot_compare: false,
                                    },
                                )))
                            }
                            DhtSnapshotNextAction::CannotCompare => {
                                if let Some(state) = initiated_lock.as_mut() {
                                    state.stage = RoundStage::NoDiff;
                                }

                                Ok(Some(GossipMessage::NoDiff(
                                    K2GossipNoDiffMessage {
                                        session_id: accept.session_id,
                                        accept_response: Some(accept_response),
                                        cannot_compare: true,
                                    },
                                )))
                            }
                            DhtSnapshotNextAction::NewSnapshot(snapshot) => {
                                match snapshot {
                                    DhtSnapshot::DiscSectors { .. } => {
                                        if let Some(state) =
                                            initiated_lock.as_mut()
                                        {
                                            state.stage =
                                                RoundStage::DiscSectorsDiff(
                                                    RoundStageDiscSectorsDiff {
                                                        common_arc_set,
                                                    },
                                                );
                                        }

                                        Ok(Some(GossipMessage::DiscSectorsDiff(
                                            K2GossipDiscSectorsDiffMessage {
                                                session_id: accept.session_id,
                                                accept_response: Some(accept_response),
                                                snapshot: Some(snapshot.try_into()?),
                                            },
                                        )))
                                    }
                                    DhtSnapshot::RingSectorDetails {
                                        ..
                                    } => {
                                        if let Some(state) =
                                            initiated_lock.as_mut()
                                        {
                                            state.stage =
                                                RoundStage::RingSectorDetailsDiff;
                                        }

                                        Ok(Some(GossipMessage::RingSectorDetailsDiff(
                                            K2GossipRingSectorDetailsDiffMessage {
                                                session_id: accept.session_id,
                                                accept_response: Some(accept_response),
                                                snapshot: Some(snapshot.try_into()?),
                                            },
                                        )))
                                    }
                                    _ => {
                                        unreachable!("unexpected snapshot type")
                                    }
                                }
                            }
                            _ => {
                                // The other action types are not reachable from a minimal
                                // snapshot
                                unreachable!("unexpected next action")
                            }
                        }
                    }
                    None => {
                        if let Some(state) = initiated_lock.as_mut() {
                            state.stage = RoundStage::NoDiff;
                        }

                        // They didn't send us a diff, presumably because we have an empty common
                        // arc set, but we can still send new ops to them and agents.
                        Ok(Some(GossipMessage::NoDiff(K2GossipNoDiffMessage {
                            session_id: accept.session_id,
                            accept_response: Some(accept_response),
                            cannot_compare: false,
                        })))
                    }
                }
            }
            GossipMessage::NoDiff(no_diff) => {
                self.update_incoming_no_diff_state(from_peer.clone(), &no_diff)
                    .await?;

                // Unwrap because checked by validate_no_diff
                let accept_response = no_diff.accept_response.unwrap();

                let send_agents = self
                    .handle_accept_response(&from_peer, accept_response)
                    .await?;

                match send_agents {
                    Some(agents) => {
                        Ok(Some(GossipMessage::Agents(K2GossipAgentsMessage {
                            session_id: no_diff.session_id,
                            provided_agents: encode_agent_infos(agents)?,
                        })))
                    }
                    None => Ok(None),
                }
            }
            GossipMessage::DiscSectorsDiff(disc_sectors_diff) => {
                let state = self
                    .update_incoming_disc_sectors_diff_state(
                        from_peer.clone(),
                        &disc_sectors_diff,
                    )
                    .await?;
                let mut state_lock = state.lock().await;

                // Unwrap because checked by validate_no_diff
                let accept_response =
                    disc_sectors_diff.accept_response.unwrap();

                let send_agents = self
                    .handle_accept_response(&from_peer, accept_response)
                    .await?;

                let their_snapshot: DhtSnapshot = disc_sectors_diff.snapshot.expect(
                    "Snapshot present checked by validate_disc_sectors_diff",
                ).try_into()?;

                // TODO untidy, there is a better way of getting this from the validate check
                let common_arc_set = match &state_lock.stage {
                    RoundStage::Accepted { common_arc_set, .. } => {
                        common_arc_set
                    }
                    _ => {
                        unreachable!()
                    }
                };

                let next_action = self
                    .dht
                    .read()
                    .await
                    .handle_snapshot(&their_snapshot, None, common_arc_set)
                    .await?;

                match next_action {
                    DhtSnapshotNextAction::CannotCompare
                    | DhtSnapshotNextAction::Identical => {
                        tracing::info!("Received a disc sectors diff but no diff to send back, responding with agents");

                        // Terminating the session, so remove the state.
                        self.accepted_round_states
                            .write()
                            .await
                            .remove(&from_peer);

                        if let Some(send_agents) = send_agents {
                            Ok(Some(GossipMessage::Agents(
                                K2GossipAgentsMessage {
                                    session_id: disc_sectors_diff.session_id,
                                    provided_agents: encode_agent_infos(
                                        send_agents,
                                    )?,
                                },
                            )))
                        } else {
                            Ok(None)
                        }
                    }
                    DhtSnapshotNextAction::NewSnapshot(snapshot) => {
                        state_lock.stage = RoundStage::DiscSectorDetailsDiff(
                            RoundStageDiscSectorDetailsDiff {
                                common_arc_set: common_arc_set.clone(),
                                snapshot: snapshot.clone(),
                            },
                        );

                        Ok(Some(GossipMessage::DiscSectorDetailsDiff(
                            K2GossipDiscSectorDetailsDiffMessage {
                                session_id: disc_sectors_diff.session_id,
                                provided_agents: encode_agent_infos(
                                    send_agents.unwrap_or_default(),
                                )?,
                                snapshot: Some(snapshot.try_into()?),
                            },
                        )))
                    }
                    _ => {
                        unreachable!("unexpected next action")
                    }
                }
            }
            GossipMessage::DiscSectorDetailsDiff(disc_sector_details_diff) => {
                // Validate the incoming disc sector details diff against our own state.
                let mut disc_sectors_diff_lock =
                    self.initiated_round_state.lock().await;
                // TODO messy borrow handling
                let common_arc_set = {
                    let disc_sectors_diff =
                        match disc_sectors_diff_lock.as_ref() {
                            Some(state) => state
                                .validate_disc_sector_details_diff(
                                    from_peer.clone(),
                                    &disc_sector_details_diff,
                                )?,
                            None => {
                                return Err(K2Error::other(
                                    "Unsolicited DiscSectorDetailsDiff message",
                                ));
                            }
                        };
                    disc_sectors_diff.common_arc_set.clone()
                };

                let their_snapshot = disc_sector_details_diff.snapshot.expect(
                    "Snapshot present checked by validate_disc_sector_details_diff",
                ).try_into()?;

                let next_action = self
                    .dht
                    .read()
                    .await
                    .handle_snapshot(&their_snapshot, None, &common_arc_set)
                    .await?;

                match next_action {
                    DhtSnapshotNextAction::CannotCompare
                    | DhtSnapshotNextAction::Identical => {
                        tracing::info!("Received a disc sector details diff but no diff to send back, responding with agents");

                        // TODO These cases where we terminate don't notify the remote so they'll
                        //      end up timing out. Should do something differently here.
                        // Terminating the session, so remove the state.
                        disc_sectors_diff_lock.take();

                        Ok(None)
                    }
                    DhtSnapshotNextAction::NewSnapshotAndHashList(
                        snapshot,
                        ops,
                    ) => {
                        if let Some(state) = disc_sectors_diff_lock.as_mut() {
                            state.stage = RoundStage::DiscSectorDetailsDiff(
                                RoundStageDiscSectorDetailsDiff {
                                    common_arc_set: common_arc_set.clone(),
                                    snapshot: snapshot.clone(),
                                },
                            );
                        }

                        Ok(Some(GossipMessage::DiscSectorDetailsResponseDiff(
                            K2GossipDiscSectorDetailsDiffResponseMessage {
                                session_id: disc_sector_details_diff.session_id,
                                maybe_missing_ids: encode_op_ids(ops),
                                snapshot: Some(snapshot.try_into()?),
                            },
                        )))
                    }
                    _ => {
                        unreachable!("unexpected next action")
                    }
                }
            }
            GossipMessage::DiscSectorDetailsResponseDiff(
                disc_sector_details_response_diff,
            ) => {
                let state = self
                    .update_incoming_disc_sectors_diff_response_state(
                        from_peer.clone(),
                        &disc_sector_details_response_diff,
                    )
                    .await?;

                let state_lock = state.lock().await;
                let (common_arc_set, our_snapshot) = match &state_lock.stage {
                    RoundStage::DiscSectorDetailsDiff(
                        RoundStageDiscSectorDetailsDiff {
                            common_arc_set,
                            snapshot,
                        },
                    ) => (common_arc_set, snapshot),
                    _ => {
                        unreachable!()
                    }
                };

                self.fetch
                    .request_ops(
                        decode_ids(
                            disc_sector_details_response_diff.maybe_missing_ids,
                        ),
                        from_peer.clone(),
                    )
                    .await?;

                let their_snapshot = disc_sector_details_response_diff.snapshot.expect(
                    "Snapshot present checked by validate_disc_sector_details_diff",
                ).try_into()?;

                let next_action = self
                    .dht
                    .read()
                    .await
                    .handle_snapshot(
                        &their_snapshot,
                        Some(our_snapshot.clone()),
                        common_arc_set,
                    )
                    .await?;

                match next_action {
                    DhtSnapshotNextAction::CannotCompare
                    | DhtSnapshotNextAction::Identical => {
                        tracing::info!("Received a disc sector details diff response that we can't respond to, terminating gossip round");

                        // Terminating the session, so remove the state.
                        self.accepted_round_states
                            .write()
                            .await
                            .remove(&from_peer);

                        Ok(None)
                    }
                    DhtSnapshotNextAction::HashList(op_ids) => {
                        // This is the final message we're going to send, remove state
                        self.accepted_round_states
                            .write()
                            .await
                            .remove(&from_peer);

                        Ok(Some(GossipMessage::Hashes(K2GossipHashesMessage {
                            session_id: disc_sector_details_response_diff
                                .session_id,
                            maybe_missing_ids: encode_op_ids(op_ids),
                        })))
                    }
                    _ => {
                        unreachable!("unexpected next action")
                    }
                }
            }
            GossipMessage::RingSectorDetailsDiff(_ring_sector_details_diff) => {
                unimplemented!()
            }
            GossipMessage::Hashes(hashes) => {
                // This could be received from either the initiator or the acceptor.
                // So we have to check in both places!

                let handled_as_initiator = {
                    let mut initiated_state =
                        self.initiated_round_state.lock().await;
                    if let Some(state) = initiated_state.as_ref() {
                        if state.session_with_peer == from_peer
                            && state.session_id == hashes.session_id
                        {
                            // Session is complete, remove state
                            initiated_state.take();

                            self.fetch
                                .request_ops(
                                    decode_ids(
                                        hashes.maybe_missing_ids.clone(),
                                    ),
                                    from_peer.clone(),
                                )
                                .await?;

                            true
                        } else {
                            false
                        }
                    } else {
                        false
                    }
                };

                let handled_as_acceptor = if !handled_as_initiator {
                    let accepted = self
                        .accepted_round_states
                        .read()
                        .await
                        .get(&from_peer)
                        .cloned();

                    if let Some(accepted) = accepted {
                        let accepted_state = accepted.lock().await;
                        if accepted_state.session_id == hashes.session_id {
                            // Session is complete, remove state
                            self.accepted_round_states
                                .write()
                                .await
                                .remove(&from_peer);

                            self.fetch
                                .request_ops(
                                    decode_ids(hashes.maybe_missing_ids),
                                    from_peer.clone(),
                                )
                                .await?;

                            true
                        } else {
                            false
                        }
                    } else {
                        false
                    }
                } else {
                    false
                };

                if !handled_as_initiator && !handled_as_acceptor {
                    return Err(K2Error::other("Unsolicited Hashes message"));
                }

                Ok(None)
            }
            GossipMessage::Agents(agents) => {
                // Validate the incoming agents message against our own state.
                let mut initiated_lock =
                    self.initiated_round_state.lock().await;
                match initiated_lock.as_ref() {
                    Some(state) => {
                        state.validate_agents(from_peer.clone(), &agents)?;
                        // The session is finished, remove the state.
                        initiated_lock.take();
                    }
                    None => {
                        return Err(K2Error::other(
                            "Unsolicited Agents message",
                        ));
                    }
                }

                self.receive_agent_infos(agents.provided_agents).await?;

                Ok(None)
            }
        }?;

        if let Some(msg) = res {
            send_gossip_message(&self.response_tx, from_peer, msg)?;
        }

        Ok(())
    }

    async fn create_accept_state(
        &self,
        from_peer: &Url,
        initiate: &K2GossipInitiateMessage,
        our_agents: Vec<AgentId>,
        common_arc_set: ArcSet,
    ) -> K2Result<()> {
        let mut accepted_states = self.accepted_round_states.write().await;
        let accepted_entry = accepted_states.entry(from_peer.clone());
        match accepted_entry {
            std::collections::hash_map::Entry::Occupied(_) => {
                return Err(K2Error::other(format!(
                    "peer {:?} already accepted",
                    from_peer
                )));
            }
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert(Arc::new(Mutex::new(
                    GossipRoundState::new_accepted(
                        from_peer.clone(),
                        initiate.session_id.clone(),
                        our_agents,
                        common_arc_set,
                    ),
                )));
            }
        }

        Ok(())
    }

    async fn update_incoming_no_diff_state(
        &self,
        from_peer: Url,
        no_diff: &K2GossipNoDiffMessage,
    ) -> K2Result<()> {
        let mut accepted_states = self.accepted_round_states.write().await;
        if !accepted_states.contains_key(&from_peer) {
            return Err(K2Error::other(format!(
                "Unsolicited NoDiff message from peer: {:?}",
                from_peer
            )));
        }

        accepted_states[&from_peer]
            .lock()
            .await
            .validate_no_diff(from_peer.clone(), no_diff)?;

        // We're at the end of the round. We might send back an Agents message, but we shouldn't
        // get any further messages from the other peer.
        accepted_states.remove(&from_peer);

        Ok(())
    }

    async fn update_incoming_disc_sectors_diff_state(
        &self,
        from_peer: Url,
        disc_sectors_diff: &K2GossipDiscSectorsDiffMessage,
    ) -> K2Result<Arc<Mutex<GossipRoundState>>> {
        let accepted_states = self.accepted_round_states.read().await;
        let Some(state) = accepted_states.get(&from_peer) else {
            return Err(K2Error::other(format!(
                "Unsolicited DiscSectorsDiff message from peer: {:?}",
                from_peer
            )));
        };

        let state_lock = state.lock().await;
        state_lock
            .validate_disc_sectors_diff(from_peer.clone(), disc_sectors_diff)?;

        Ok(state.clone())
    }

    async fn update_incoming_disc_sectors_diff_response_state(
        &self,
        from_peer: Url,
        disc_sector_details_diff_response: &K2GossipDiscSectorDetailsDiffResponseMessage,
    ) -> K2Result<Arc<Mutex<GossipRoundState>>> {
        let Some(state) = self
            .accepted_round_states
            .read()
            .await
            .get(&from_peer)
            .cloned()
        else {
            return Err(K2Error::other(format!(
                "Unsolicited DiscSectorDetailsDiffResponse message from peer: {:?}",
                from_peer
            )));
        };

        let state_lock = state.lock().await;
        state_lock.validate_disc_sector_details_diff_response(
            from_peer.clone(),
            disc_sector_details_diff_response,
        )?;

        Ok(state.clone())
    }

    async fn handle_accept_response(
        &self,
        from_peer: &Url,
        accept_response: AcceptResponseMessage,
    ) -> K2Result<Option<Vec<Arc<AgentInfoSigned>>>> {
        self.receive_agent_infos(accept_response.provided_agents)
            .await?;

        self.update_new_ops_bookmark(
            from_peer.clone(),
            Timestamp::from_micros(accept_response.updated_new_since),
        )
        .await?;

        self.fetch
            .request_ops(decode_ids(accept_response.new_ops), from_peer.clone())
            .await?;
        self.peer_meta_store
            .set_new_ops_bookmark(
                from_peer.clone(),
                Timestamp::from_micros(accept_response.updated_new_since),
            )
            .await?;

        if accept_response.missing_agents.is_empty() {
            Ok(None)
        } else {
            let send_agent_infos =
                self.load_agent_infos(accept_response.missing_agents).await;

            Ok(Some(send_agent_infos))
        }
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
        provided_agents: Vec<Bytes>,
    ) -> K2Result<()> {
        if provided_agents.is_empty() {
            return Ok(());
        }

        // TODO check that the incoming agents are the one we requested
        let mut agents = Vec::with_capacity(provided_agents.len());
        for agent in provided_agents {
            let agent_info =
                AgentInfoSigned::decode(&self.agent_verifier, &agent)?;
            agents.push(agent_info);
        }
        tracing::info!("Storing agents: {:?}", agents);
        self.peer_store.insert(agents).await?;

        Ok(())
    }

    async fn local_agent_state(&self) -> K2Result<(Vec<AgentId>, ArcSet)> {
        let local_agents = self.local_agent_store.get_all().await?;
        let (send_agents, our_arcs) = local_agents
            .iter()
            .map(|a| (a.agent().clone(), a.get_tgt_storage_arc()))
            .collect::<(Vec<_>, Vec<_>)>();

        let our_arc_set = ArcSet::new(our_arcs)?;

        Ok((send_agents, our_arc_set))
    }

    async fn update_new_ops_bookmark(
        &self,
        from_peer: Url,
        updated_bookmark: Timestamp,
    ) -> K2Result<()> {
        let previous_bookmark = self
            .peer_meta_store
            .new_ops_bookmark(from_peer.clone())
            .await?;

        if previous_bookmark
            .map(|previous_bookmark| previous_bookmark <= updated_bookmark)
            .unwrap_or(true)
        {
            self.peer_meta_store
                .set_new_ops_bookmark(from_peer.clone(), updated_bookmark)
                .await?;
        } else {
            // This could happen due to a clock issue. If it happens frequently, or by a
            // large margin, it could be a sign of malicious activity.
            tracing::warn!(
                "new bookmark is older than previous bookmark from peer: {:?}",
                from_peer
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
    use kitsune2_api::agent::LocalAgent;
    use kitsune2_api::builder::Builder;
    use kitsune2_api::space::{DynSpace, SpaceHandler};
    use kitsune2_api::transport::{TxHandler, TxSpaceHandler};
    use kitsune2_api::{DhtArc, OpId};
    use kitsune2_core::factories::MemoryOp;
    use kitsune2_core::{default_test_builder, Ed25519LocalAgent};
    use kitsune2_test_utils::enable_tracing;
    use std::time::Duration;

    #[derive(Debug, Clone)]
    struct GossipTestHarness {
        gossip: K2Gossip,
        peer_store: DynPeerStore,
        op_store: DynOpStore,
        space: DynSpace,
    }

    impl GossipTestHarness {
        async fn join_local_agent(
            &self,
            target_arc: DhtArc,
        ) -> Arc<AgentInfoSigned> {
            let local_agent = Arc::new(Ed25519LocalAgent::default());
            local_agent.set_tgt_storage_arc_hint(target_arc.clone());

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
                space.peer_store().clone(),
                space.local_agent_store().clone(),
                self.builder
                    .peer_meta_store
                    .create(self.builder.clone())
                    .await
                    .unwrap(),
                op_store.clone(),
                transport.clone(),
                self.builder
                    .fetch
                    .create(
                        self.builder.clone(),
                        self.space_id.clone(),
                        op_store.clone(),
                        transport.clone(),
                    )
                    .await
                    .unwrap(),
                self.builder.verifier.clone(),
            )
            .await
            .unwrap();

            GossipTestHarness {
                gossip,
                peer_store: space.peer_store().clone(),
                op_store,
                space,
            }
        }
    }

    fn test_space() -> SpaceId {
        SpaceId::from(Bytes::from_static(b"test-space"))
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
        let agent_1 = harness_1.join_local_agent(DhtArc::FULL).await;
        let agent_info_1 = harness_1
            .peer_store
            .get(agent_1.agent.clone())
            .await
            .unwrap()
            .unwrap();

        let harness_2 = factory.new_instance().await;
        harness_2.join_local_agent(DhtArc::FULL).await;
        harness_2
            .peer_store
            .insert(vec![agent_info_1.clone()])
            .await
            .unwrap();

        // Join extra agents for each peer. These will take a few seconds to be
        // found by bootstrap. Try to sync them with gossip.
        let secret_agent_1 = harness_1.join_local_agent(DhtArc::FULL).await;
        let secret_agent_2 = harness_2.join_local_agent(DhtArc::FULL).await;

        assert!(harness_1
            .peer_store
            .get(secret_agent_2.agent.clone())
            .await
            .unwrap()
            .is_none());
        assert!(harness_2
            .peer_store
            .get(secret_agent_1.agent.clone())
            .await
            .unwrap()
            .is_none());

        harness_2
            .gossip
            .initiate_gossip(agent_info_1.url.clone().unwrap())
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

        let space = test_space();
        let factory = TestGossipFactory::create(space.clone()).await;
        let harness_1 = factory.new_instance().await;
        let agent_1 = harness_1.join_local_agent(DhtArc::FULL).await;
        let op_1 = MemoryOp::new(Timestamp::now(), vec![1; 128]);
        let op_id_1 = op_1.compute_op_id();
        harness_1
            .op_store
            .process_incoming_ops(vec![op_1.clone().into()])
            .await
            .unwrap();
        let agent_info_1 = harness_1
            .peer_store
            .get(agent_1.agent().clone())
            .await
            .unwrap()
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

        harness_2
            .gossip
            .initiate_gossip(agent_info_1.url.clone())
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

        let space = test_space();
        let factory = TestGossipFactory::create(space.clone()).await;
        let harness_1 = factory.new_instance().await;
        let agent_1 = harness_1.join_local_agent(DhtArc::FULL).await;
        let op_1 = MemoryOp::new(Timestamp::from_micros(100), vec![1; 128]);
        let op_id_1 = op_1.compute_op_id();
        harness_1
            .op_store
            .process_incoming_ops(vec![op_1.clone().into()])
            .await
            .unwrap();

        harness_1
            .gossip
            .dht
            .write()
            .await
            .inform_ops_stored(vec![op_1.clone().into()])
            .await
            .unwrap();

        let harness_2 = factory.new_instance().await;
        let agent_2 = harness_2.join_local_agent(DhtArc::FULL).await;
        let op_2 = MemoryOp::new(Timestamp::from_micros(500), vec![2; 128]);
        let op_id_2 = op_2.compute_op_id();
        harness_2
            .op_store
            .process_incoming_ops(vec![op_2.clone().into()])
            .await
            .unwrap();

        harness_2
            .gossip
            .dht
            .write()
            .await
            .inform_ops_stored(vec![op_2.clone().into()])
            .await
            .unwrap();

        // Inform the harnesses that we've already synced up to now. This will force the
        // ops we just created to be treated as historical disc ops.
        harness_1
            .gossip
            .peer_meta_store
            .set_new_ops_bookmark(
                agent_2.url.clone().unwrap(),
                Timestamp::now(),
            )
            .await
            .unwrap();
        harness_2
            .gossip
            .peer_meta_store
            .set_new_ops_bookmark(
                agent_1.url.clone().unwrap(),
                Timestamp::now(),
            )
            .await
            .unwrap();

        harness_2
            .gossip
            .initiate_gossip(agent_1.agent.clone())
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
