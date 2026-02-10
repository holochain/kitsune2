use crate::error::{K2GossipError, K2GossipResult};
use crate::gossip::K2Gossip;
use crate::protocol::{
    GossipMessage, K2GossipAgentsMessage, K2GossipDiscSectorDetailsDiffMessage,
    K2GossipDiscSectorsDiffMessage, K2GossipTerminateMessage,
    encode_agent_infos,
};
use crate::state::{
    GossipRoundState, RoundStage, RoundStageAccepted,
    RoundStageDiscSectorDetailsDiff,
};
use kitsune2_api::{AgentId, K2Error, Url};
use kitsune2_dht::DhtSnapshot;
use kitsune2_dht::DhtSnapshotNextAction;
use tokio::sync::OwnedMutexGuard;

impl K2Gossip {
    pub(super) async fn respond_to_disc_sectors_diff(
        &self,
        from_peer: Url,
        disc_sectors_diff: K2GossipDiscSectorsDiffMessage,
    ) -> K2GossipResult<Option<GossipMessage>> {
        let (mut state, accepted) = self
            .check_disc_sectors_diff_state(
                from_peer.clone(),
                &disc_sectors_diff,
            )
            .await?;

        // Unwrap because checked by validate_disc_sectors_diff
        let accept_response = disc_sectors_diff.accept_response.unwrap();

        let send_agents = self
            .handle_accept_response(&from_peer, accept_response)
            .await?;

        let their_snapshot: DhtSnapshot = disc_sectors_diff
            .snapshot
            .expect("Snapshot present checked by validate_disc_sectors_diff")
            .try_into()?;

        let next_action = self
            .dht
            .read()
            .await
            .handle_snapshot(
                their_snapshot,
                None,
                accepted.common_arc_set.clone(),
                // Zero because this cannot return op ids
                0,
            )
            .await?
            .0;

        match next_action {
            DhtSnapshotNextAction::CannotCompare
            | DhtSnapshotNextAction::Identical => {
                tracing::info!(
                    ?disc_sectors_diff.session_id,
                    "Received a disc sectors diff but no diff to send back, responding with agents"
                );

                // Terminating the session, so remove the state.
                self.accepted_round_states.write().await.remove(&from_peer);

                if let Some(send_agents) = send_agents {
                    Ok(Some(GossipMessage::Agents(K2GossipAgentsMessage {
                        session_id: disc_sectors_diff.session_id,
                        provided_agents: encode_agent_infos(send_agents)?,
                    })))
                } else {
                    Ok(None)
                }
            }
            DhtSnapshotNextAction::NewSnapshot(snapshot) => {
                state.stage = RoundStage::DiscSectorDetailsDiff(
                    RoundStageDiscSectorDetailsDiff {
                        common_arc_set: accepted.common_arc_set.clone(),
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
            a => {
                tracing::error!(?disc_sectors_diff.session_id, "Unexpected next action: {:?}", a);

                // Remove round state
                self.accepted_round_states.write().await.remove(&from_peer);

                Ok(Some(GossipMessage::Terminate(K2GossipTerminateMessage {
                    session_id: disc_sectors_diff.session_id,
                    reason: "Unexpected next action".to_string(),
                })))
            }
        }
    }

    async fn check_disc_sectors_diff_state(
        &self,
        from_peer: Url,
        disc_sectors_diff: &K2GossipDiscSectorsDiffMessage,
    ) -> K2GossipResult<(OwnedMutexGuard<GossipRoundState>, RoundStageAccepted)>
    {
        match self.accepted_round_states.read().await.get(&from_peer) {
            Some(state) => {
                let state = state.clone().lock_owned().await;
                let accepted = state
                    .validate_disc_sectors_diff(
                        from_peer.clone(),
                        disc_sectors_diff,
                    )?
                    .clone();

                Ok((state, accepted))
            }
            None => Err(K2GossipError::peer_behavior(format!(
                "Unsolicited DiscSectorsDiff message from peer: {from_peer:?}"
            ))),
        }
    }
}

impl GossipRoundState {
    fn validate_disc_sectors_diff(
        &self,
        from_peer: Url,
        disc_sectors_diff: &K2GossipDiscSectorsDiffMessage,
    ) -> K2GossipResult<&RoundStageAccepted> {
        if self.session_with_peer != from_peer {
            return Err(K2Error::other(format!(
                "DiscSectorsDiff message from wrong peer: {} != {}",
                self.session_with_peer, from_peer
            ))
            .into());
        }

        if self.session_id != disc_sectors_diff.session_id {
            return Err(K2GossipError::peer_behavior(format!(
                "Session id mismatch: {:?} != {:?}",
                self.session_id, disc_sectors_diff.session_id
            )));
        }

        let Some(snapshot) = &disc_sectors_diff.snapshot else {
            return Err(K2GossipError::peer_behavior(
                "Received DiscSectorsDiff message without snapshot",
            ));
        };

        match &self.stage {
            RoundStage::Accepted(
                out @ RoundStageAccepted {
                    our_agents,
                    common_arc_set,
                },
            ) => {
                let Some(accept_response) = &disc_sectors_diff.accept_response
                else {
                    return Err(K2GossipError::peer_behavior(
                        "Received NoDiff message without accept response",
                    ));
                };

                if accept_response
                    .missing_agents
                    .iter()
                    .any(|a| !our_agents.contains(&AgentId::from(a.clone())))
                {
                    return Err(K2GossipError::peer_behavior(
                        "NoDiff message contains agents that we didn't declare",
                    ));
                }

                for sector in &snapshot.disc_sectors {
                    if !common_arc_set.includes_sector_index(*sector) {
                        return Err(K2GossipError::peer_behavior(
                            "DiscSectorsDiff message contains sector that isn't in the common arc set",
                        ));
                    }
                }

                Ok(out)
            }
            stage => Err(K2GossipError::peer_behavior(format!(
                "Unexpected round state for accept: Accepted != {stage:?}"
            ))),
        }
    }
}
