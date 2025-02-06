use crate::gossip::K2Gossip;
use crate::protocol::{
    encode_agent_infos, encode_op_ids, AcceptResponseMessage, GossipMessage,
    K2GossipAcceptMessage, K2GossipDiscSectorsDiffMessage,
    K2GossipNoDiffMessage, K2GossipRingSectorDetailsDiffMessage,
};
use crate::state::{
    GossipRoundState, RoundStage, RoundStageDiscSectorsDiff,
    RoundStageInitiated, RoundStageRingSectorDetailsDiff,
};
use kitsune2_api::id::decode_ids;
use kitsune2_api::{AgentId, K2Error, K2Result, Timestamp, Url};
use kitsune2_dht::snapshot::DhtSnapshot;
use kitsune2_dht::{ArcSet, DhtSnapshotNextAction};
use tokio::sync::MutexGuard;

impl K2Gossip {
    pub(super) async fn respond_to_accept(
        &self,
        from_peer: Url,
        accept: K2GossipAcceptMessage,
    ) -> K2Result<Option<GossipMessage>> {
        // Validate the incoming accept against our own state.
        let (mut lock, initiated) =
            self.check_accept_state(&from_peer, &accept).await?;

        // Only once the other peer has accepted should we record that we've tried to
        // gossip with them. Otherwise, we risk lock each other out if we both record
        // a last gossip timestamp and try to initiate at the same time.
        self.peer_meta_store
            .set_last_gossip_timestamp(from_peer.clone(), Timestamp::now())
            .await?;

        let common_arc_set = Self::get_common_arc_set(&initiated, &accept)?;

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

        // TODO Ideally we'd reset this if their arc set changes, to avoid missing ops in new
        //      sectors. Do this by updating bookmark to `Timestamp::now() - UNIT_TIME` when
        //      receiving or creating an agent id which has a peer URL in the meta store.
        self.peer_meta_store
            .set_new_ops_bookmark(
                from_peer.clone(),
                Timestamp::from_micros(accept.updated_new_since),
            )
            .await?;

        let (send_new_ops, used_bytes, send_new_bookmark) = self
            .retrieve_new_op_ids(
                &common_arc_set,
                Timestamp::from_micros(accept.new_since),
                accept.max_op_data_bytes,
            )
            .await?;

        // Update the peer's max op data bytes to reflect the amount of data we're sending ids for.
        // The remaining limit will be used for the DHT diff as required.
        if let Some(state) = lock.as_mut() {
            tracing::debug!(
                "Used {}/{} op budget to send {} op ids",
                used_bytes,
                accept.max_op_data_bytes,
                send_new_ops.len()
            );

            // Note that this value will have been initialised to 0 here when we created the
            // initial state. So we need to initialise and subtract here.
            state.peer_max_op_data_bytes = (std::cmp::min(
                self.config.max_request_gossip_op_bytes,
                accept.max_op_data_bytes,
            ) - used_bytes) as i32;
        }

        // The common part
        let accept_response = AcceptResponseMessage {
            missing_agents,
            provided_agents: encode_agent_infos(send_agent_infos)?,
            new_ops: encode_op_ids(send_new_ops),
            updated_new_since: send_new_bookmark.as_micros(),
        };

        match accept.snapshot {
            Some(their_snapshot) => {
                let (next_action, _) = self
                    .dht
                    .read()
                    .await
                    .handle_snapshot(
                        their_snapshot.into(),
                        None,
                        common_arc_set.clone(),
                        // Zero because this cannot return op ids
                        0,
                    )
                    .await?;

                // Then pick an appropriate response message based on the snapshot
                match next_action {
                    DhtSnapshotNextAction::Identical => {
                        if let Some(state) = lock.as_mut() {
                            state.stage = RoundStage::NoDiff;
                        }

                        Ok(Some(GossipMessage::NoDiff(K2GossipNoDiffMessage {
                            session_id: accept.session_id,
                            accept_response: Some(accept_response),
                            cannot_compare: false,
                        })))
                    }
                    DhtSnapshotNextAction::CannotCompare => {
                        if let Some(state) = lock.as_mut() {
                            state.stage = RoundStage::NoDiff;
                        }

                        Ok(Some(GossipMessage::NoDiff(K2GossipNoDiffMessage {
                            session_id: accept.session_id,
                            accept_response: Some(accept_response),
                            cannot_compare: true,
                        })))
                    }
                    DhtSnapshotNextAction::NewSnapshot(snapshot) => {
                        match snapshot {
                            DhtSnapshot::DiscSectors { .. } => {
                                if let Some(state) = lock.as_mut() {
                                    state.stage = RoundStage::DiscSectorsDiff(
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
                            DhtSnapshot::RingSectorDetails { .. } => {
                                if let Some(state) = lock.as_mut() {
                                    state.stage =
                                        RoundStage::RingSectorDetailsDiff(
                                            RoundStageRingSectorDetailsDiff {
                                                common_arc_set,
                                                snapshot: snapshot.clone(),
                                            },
                                        );
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
                                // TODO while this would require a local inconsistency between
                                //      the DHT and the gossip crates, we should probably still
                                //      handle this without a panic.
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
                if let Some(state) = lock.as_mut() {
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

    async fn check_accept_state<'a>(
        &'a self,
        from_peer: &Url,
        accept: &K2GossipAcceptMessage,
    ) -> K2Result<(
        MutexGuard<'a, Option<GossipRoundState>>,
        RoundStageInitiated,
    )> {
        let round_state = self.initiated_round_state.lock().await;
        let initiated = match round_state.as_ref() {
            Some(state) => {
                state.validate_accept(from_peer.clone(), accept)?.clone()
            }
            None => {
                return Err(K2Error::other("Unsolicited Accept message"));
            }
        };

        Ok((round_state, initiated))
    }

    fn get_common_arc_set(
        initiated: &RoundStageInitiated,
        accept: &K2GossipAcceptMessage,
    ) -> K2Result<ArcSet> {
        let other_arc_set = match &accept.arc_set {
            Some(message) => ArcSet::decode(&message.value)?,
            None => {
                return Err(K2Error::other("no arc set in accept message"));
            }
        };

        Ok(other_arc_set.intersection(&initiated.our_arc_set))
    }
}

impl GossipRoundState {
    fn validate_accept(
        &self,
        from_peer: Url,
        accept: &K2GossipAcceptMessage,
    ) -> K2Result<&RoundStageInitiated> {
        if self.session_with_peer != from_peer {
            return Err(K2Error::other(format!(
                "Accept message from wrong peer: {} != {}",
                self.session_with_peer, from_peer
            )));
        }

        if self.session_id != accept.session_id {
            return Err(K2Error::other(format!(
                "Session id mismatch: {:?} != {:?}",
                self.session_id, accept.session_id
            )));
        }

        match &self.stage {
            RoundStage::Initiated(
                stage @ RoundStageInitiated { our_agents, .. },
            ) => {
                tracing::trace!("Initiated round state found");

                if accept
                    .missing_agents
                    .iter()
                    .any(|a| !our_agents.contains(&AgentId::from(a.clone())))
                {
                    return Err(K2Error::other(
                        "Accept message contains agents that we didn't declare",
                    ));
                }

                Ok(stage)
            }
            stage => Err(K2Error::other(format!(
                "Unexpected round state for accept: Initiated != {:?}",
                stage
            ))),
        }
    }
}
