use crate::gossip::K2Gossip;
use crate::protocol::{
    encode_agent_infos, encode_op_ids, GossipMessage, K2GossipAgentsMessage,
    K2GossipRingSectorDetailsDiffMessage,
    K2GossipRingSectorDetailsDiffResponseMessage,
};
use crate::state::{
    GossipRoundState, RoundStage, RoundStageAccepted,
    RoundStageRingSectorDetailsDiff,
};
use kitsune2_api::{AgentId, K2Error, K2Result, Url};
use kitsune2_dht::snapshot::DhtSnapshot;
use kitsune2_dht::DhtSnapshotNextAction;
use tokio::sync::OwnedMutexGuard;

impl K2Gossip {
    pub(super) async fn respond_to_ring_sector_details_diff(
        &self,
        from_peer: Url,
        ring_sector_details_diff: K2GossipRingSectorDetailsDiffMessage,
    ) -> K2Result<Option<GossipMessage>> {
        let (mut state, accepted) = self
            .check_ring_sector_details_diff_state(
                from_peer.clone(),
                &ring_sector_details_diff,
            )
            .await?;

        let accept_response_message =
            ring_sector_details_diff.accept_response.unwrap();
        self.handle_accept_response(
            &from_peer,
            accept_response_message.clone(),
        )
        .await?;

        let their_snapshot: DhtSnapshot = ring_sector_details_diff
            .snapshot
            .expect(
                "Snapshot present checked by validate_ring_sector_details_diff",
            )
            .try_into()?;

        let (next_action, used_bytes) = self
            .dht
            .read()
            .await
            .handle_snapshot(
                their_snapshot.clone(),
                None,
                accepted.common_arc_set.clone(),
                state.peer_max_op_data_bytes,
            )
            .await?;

        state.peer_max_op_data_bytes -= used_bytes as i32;

        self.update_storage_arcs(
            &next_action,
            &their_snapshot,
            accepted.common_arc_set.clone(),
        )
        .await?;

        match next_action {
            DhtSnapshotNextAction::CannotCompare
            | DhtSnapshotNextAction::Identical => {
                tracing::info!("Received a ring sector details diff but no diff to send back, responding with agents");

                // Terminating the session, so remove the state.
                self.accepted_round_states.write().await.remove(&from_peer);

                let send_agents = self
                    .load_agent_infos(accept_response_message.missing_agents)
                    .await;
                if !send_agents.is_empty() {
                    Ok(Some(GossipMessage::Agents(K2GossipAgentsMessage {
                        session_id: ring_sector_details_diff.session_id,
                        provided_agents: encode_agent_infos(send_agents)?,
                    })))
                } else {
                    Ok(None)
                }
            }
            DhtSnapshotNextAction::NewSnapshotAndHashList(snapshot, op_ids) => {
                state.stage = RoundStage::RingSectorDetailsDiff(
                    RoundStageRingSectorDetailsDiff {
                        common_arc_set: accepted.common_arc_set.clone(),
                        snapshot: snapshot.clone(),
                    },
                );

                Ok(Some(GossipMessage::RingSectorDetailsDiffResponse(
                    K2GossipRingSectorDetailsDiffResponseMessage {
                        session_id: ring_sector_details_diff.session_id,
                        missing_ids: encode_op_ids(op_ids),
                        snapshot: Some(snapshot.try_into()?),
                    },
                )))
            }
            _ => {
                unreachable!("unexpected next action")
            }
        }
    }

    async fn check_ring_sector_details_diff_state(
        &self,
        from_peer: Url,
        ring_sector_details_diff: &K2GossipRingSectorDetailsDiffMessage,
    ) -> K2Result<(OwnedMutexGuard<GossipRoundState>, RoundStageAccepted)> {
        match self.accepted_round_states.read().await.get(&from_peer) {
            Some(state) => {
                let state = state.clone().lock_owned().await;
                let out = state
                    .validate_ring_sector_details_diff(
                        from_peer.clone(),
                        ring_sector_details_diff,
                    )?
                    .clone();

                Ok((state, out))
            }
            None => Err(K2Error::other(format!(
                "Unsolicited RingSectorDetailsDiff message from peer: {:?}",
                from_peer
            ))),
        }
    }
}

impl GossipRoundState {
    fn validate_ring_sector_details_diff(
        &self,
        from_peer: Url,
        ring_sector_details_diff: &K2GossipRingSectorDetailsDiffMessage,
    ) -> K2Result<&RoundStageAccepted> {
        if self.session_with_peer != from_peer {
            return Err(K2Error::other(format!(
                "RingSectorDetailsDiff message from wrong peer: {} != {}",
                self.session_with_peer, from_peer
            )));
        }

        if self.session_id != ring_sector_details_diff.session_id {
            return Err(K2Error::other(format!(
                "Session id mismatch: {:?} != {:?}",
                self.session_id, ring_sector_details_diff.session_id
            )));
        }

        let Some(snapshot) = &ring_sector_details_diff.snapshot else {
            return Err(K2Error::other(
                "Received RingSectorDetailsDiff message without snapshot",
            ));
        };

        match &self.stage {
            RoundStage::Accepted(stage @ RoundStageAccepted { our_agents, common_arc_set, .. }) => {
                let Some(accept_response) = &ring_sector_details_diff.accept_response
                else {
                    return Err(K2Error::other(
                        "Received RingSectorDetailsDiff message without accept response",
                    ));
                };

                if accept_response
                    .missing_agents
                    .iter()
                    .any(|a| !our_agents.contains(&AgentId::from(a.clone())))
                {
                    return Err(K2Error::other(
                        "RingSectorDetailsDiff message contains agents that we didn't declare",
                    ));
                }

                for sector in snapshot.ring_sector_hashes.iter().flat_map(|sh| sh.sector_indices.iter()) {
                    if !common_arc_set.includes_sector_index(*sector) {
                        return Err(K2Error::other(
                            "RingSectorDetailsDiff message contains sector that isn't in the common arc set",
                        ));
                    }
                }

                Ok(stage)
            }
            stage => {
                Err(K2Error::other(format!(
                    "Unexpected round state for ring sector details diff: Accepted != {:?}",
                    stage
                )))
            }
        }
    }
}
