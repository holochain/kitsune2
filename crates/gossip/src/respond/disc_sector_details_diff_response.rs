use crate::gossip::K2Gossip;
use crate::protocol::{
    encode_op_ids, GossipMessage, K2GossipDiscSectorDetailsDiffResponseMessage,
    K2GossipHashesMessage,
};
use crate::state::{
    GossipRoundState, RoundStage, RoundStageDiscSectorDetailsDiff,
};
use kitsune2_api::decode_ids;
use kitsune2_api::{K2Error, K2Result, Url};
use kitsune2_dht::DhtSnapshotNextAction;
use tokio::sync::OwnedMutexGuard;

impl K2Gossip {
    pub(super) async fn respond_to_disc_sector_details_diff_response(
        &self,
        from_peer: Url,
        response: K2GossipDiscSectorDetailsDiffResponseMessage,
    ) -> K2Result<Option<GossipMessage>> {
        let (mut state, disc_sector_details) = self
            .check_disc_sectors_diff_response_state(
                from_peer.clone(),
                &response,
            )
            .await?;

        self.fetch
            .request_ops(decode_ids(response.missing_ids), from_peer.clone())
            .await?;

        let their_snapshot = response
            .snapshot
            .expect(
                "Snapshot present checked by validate_disc_sector_details_diff",
            )
            .try_into()?;

        let (next_action, used_bytes) = self
            .dht
            .read()
            .await
            .handle_snapshot(
                their_snapshot,
                Some(disc_sector_details.snapshot.clone()),
                disc_sector_details.common_arc_set,
                state.peer_max_op_data_bytes,
            )
            .await?;

        tracing::debug!(
            "Used {}/{} op budget to send disc ops",
            used_bytes,
            state.peer_max_op_data_bytes,
        );
        state.peer_max_op_data_bytes -= used_bytes as i32;

        match next_action {
            DhtSnapshotNextAction::CannotCompare
            | DhtSnapshotNextAction::Identical => {
                tracing::info!("Received a disc sector details diff response that we can't respond to, terminating gossip round");

                // Terminating the session, so remove the state.
                self.accepted_round_states.write().await.remove(&from_peer);

                Ok(None)
            }
            DhtSnapshotNextAction::HashList(op_ids) => {
                // This is the final message we're going to send, remove state
                self.accepted_round_states.write().await.remove(&from_peer);

                Ok(Some(GossipMessage::Hashes(K2GossipHashesMessage {
                    session_id: response.session_id,
                    missing_ids: encode_op_ids(op_ids),
                })))
            }
            _ => {
                unreachable!("unexpected next action")
            }
        }
    }

    async fn check_disc_sectors_diff_response_state(
        &self,
        from_peer: Url,
        disc_sector_details_diff_response: &K2GossipDiscSectorDetailsDiffResponseMessage,
    ) -> K2Result<(
        OwnedMutexGuard<GossipRoundState>,
        RoundStageDiscSectorDetailsDiff,
    )> {
        match self.accepted_round_states.read().await.get(&from_peer) {
            Some(state) => {
                let state = state.clone().lock_owned().await;
                let disc_sector_details = state.validate_disc_sector_details_diff_response(
                    from_peer.clone(),
                    disc_sector_details_diff_response,
                )?.clone();

                Ok((state, disc_sector_details))
            }
            None => Err(K2Error::other(format!(
                "Unsolicited DiscSectorDetailsDiffResponse message from peer: {:?}",
                from_peer
            )))
        }
    }
}

impl GossipRoundState {
    fn validate_disc_sector_details_diff_response(
        &self,
        from_peer: Url,
        disc_sector_details_diff: &K2GossipDiscSectorDetailsDiffResponseMessage,
    ) -> K2Result<&RoundStageDiscSectorDetailsDiff> {
        if self.session_with_peer != from_peer {
            return Err(K2Error::other(format!(
                "DiscSectorDetailsDiffResponse message from wrong peer: {} != {}",
                self.session_with_peer, from_peer
            )));
        }

        if self.session_id != disc_sector_details_diff.session_id {
            return Err(K2Error::other(format!(
                "Session id mismatch: {:?} != {:?}",
                self.session_id, disc_sector_details_diff.session_id
            )));
        }

        let Some(snapshot) = &disc_sector_details_diff.snapshot else {
            return Err(K2Error::other(
                "Received DiscSectorDetailsDiffResponse message without snapshot",
            ));
        };

        match &self.stage {
            RoundStage::DiscSectorDetailsDiff(state @ RoundStageDiscSectorDetailsDiff { common_arc_set, .. }) => {
                for sector in &snapshot.sector_indices {
                    if !common_arc_set.includes_sector_index(*sector) {
                        return Err(K2Error::other(
                            "DiscSectorDetailsDiffResponse message contains sector that isn't in the common arc set",
                        ));
                    }
                }

                Ok(state)
            }
            stage => {
                Err(K2Error::other(format!(
                    "Unexpected round state for disc sector details diff response: DiscSectorDetailsDiff != {:?}",
                    stage
                )))
            }
        }
    }
}
