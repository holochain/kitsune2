use crate::gossip::K2Gossip;
use crate::protocol::{
    encode_agent_infos, GossipMessage, K2GossipAgentsMessage,
    K2GossipDiscSectorDetailsDiffMessage, K2GossipDiscSectorsDiffMessage,
};
use crate::state::{
    GossipRoundState, RoundStage, RoundStageAccepted,
    RoundStageDiscSectorDetailsDiff,
};
use kitsune2_api::{K2Error, K2Result, Url};
use kitsune2_dht::snapshot::DhtSnapshot;
use kitsune2_dht::DhtSnapshotNextAction;
use tokio::sync::OwnedMutexGuard;

impl K2Gossip {
    pub(super) async fn respond_to_disc_sectors_diff(
        &self,
        from_peer: Url,
        disc_sectors_diff: K2GossipDiscSectorsDiffMessage,
    ) -> K2Result<Option<GossipMessage>> {
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
            .handle_snapshot(&their_snapshot, None, &accepted.common_arc_set)
            .await?;

        match next_action {
            DhtSnapshotNextAction::CannotCompare
            | DhtSnapshotNextAction::Identical => {
                tracing::info!("Received a disc sectors diff but no diff to send back, responding with agents");

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
            _ => {
                unreachable!("unexpected next action")
            }
        }
    }

    async fn check_disc_sectors_diff_state(
        &self,
        from_peer: Url,
        disc_sectors_diff: &K2GossipDiscSectorsDiffMessage,
    ) -> K2Result<(OwnedMutexGuard<GossipRoundState>, RoundStageAccepted)> {
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
            None => Err(K2Error::other(format!(
                "Unsolicited DiscSectorsDiff message from peer: {:?}",
                from_peer
            ))),
        }
    }
}
