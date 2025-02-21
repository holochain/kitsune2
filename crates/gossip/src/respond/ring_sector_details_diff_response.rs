use crate::error::{K2GossipError, K2GossipResult};
use crate::gossip::K2Gossip;
use crate::protocol::{
    GossipMessage, K2GossipHashesMessage,
    K2GossipRingSectorDetailsDiffResponseMessage, K2GossipTerminateMessage,
    encode_op_ids,
};
use crate::state::{
    GossipRoundState, RoundStage, RoundStageRingSectorDetailsDiff,
};
use kitsune2_api::decode_ids;
use kitsune2_api::{K2Error, Url};
use kitsune2_dht::DhtSnapshot;
use kitsune2_dht::DhtSnapshotNextAction;
use tokio::sync::MutexGuard;

impl K2Gossip {
    pub(super) async fn respond_to_ring_sector_details_diff_response(
        &self,
        from_peer: Url,
        response: K2GossipRingSectorDetailsDiffResponseMessage,
    ) -> K2GossipResult<Option<GossipMessage>> {
        let (mut state, ring_sector_details) = self
            .check_ring_sector_details_diff_response_state(
                from_peer.clone(),
                &response,
            )
            .await?;

        self.fetch
            .request_ops(decode_ids(response.missing_ids), from_peer.clone())
            .await?;

        let their_snapshot: DhtSnapshot =
            response.snapshot.unwrap().try_into()?;

        let peer_max_op_data_bytes = state
            .as_ref()
            .map(|s| s.peer_max_op_data_bytes)
            .unwrap_or(0);

        let (next_action, used_bytes) = self
            .dht
            .read()
            .await
            .handle_snapshot(
                their_snapshot.clone(),
                Some(ring_sector_details.snapshot.clone()),
                ring_sector_details.common_arc_set.clone(),
                peer_max_op_data_bytes,
            )
            .await?;

        if let Some(state) = state.as_mut() {
            state.peer_max_op_data_bytes -= used_bytes as i32;
        }

        self.update_storage_arcs(
            &next_action,
            &their_snapshot,
            ring_sector_details.common_arc_set,
        )
        .await?;

        match next_action {
            DhtSnapshotNextAction::CannotCompare
            | DhtSnapshotNextAction::Identical => {
                tracing::info!(
                    "Received a ring sector details diff response that we can't respond to, terminating gossip round"
                );

                // Terminating the session, so remove the state.
                state.take();

                Ok(Some(GossipMessage::Terminate(K2GossipTerminateMessage {
                    session_id: response.session_id,
                    reason: "Nothing to compare".to_string(),
                })))
            }
            DhtSnapshotNextAction::HashList(op_ids) => {
                // This is the final message we're going to send, remove state
                state.take();

                Ok(Some(GossipMessage::Hashes(K2GossipHashesMessage {
                    session_id: response.session_id,
                    missing_ids: encode_op_ids(op_ids),
                })))
            }
            a => {
                tracing::error!("Unexpected next action: {:?}", a);

                // Remove the round state.
                state.take();

                Ok(Some(GossipMessage::Terminate(K2GossipTerminateMessage {
                    session_id: response.session_id,
                    reason: "Unexpected next action".to_string(),
                })))
            }
        }
    }

    async fn check_ring_sector_details_diff_response_state<'a>(
        &'a self,
        from_peer: Url,
        ring_sector_details_diff_response: &K2GossipRingSectorDetailsDiffResponseMessage,
    ) -> K2GossipResult<(
        MutexGuard<'a, Option<GossipRoundState>>,
        RoundStageRingSectorDetailsDiff,
    )> {
        let lock = self.initiated_round_state.lock().await;
        let ring_sector_details_diff = match lock.as_ref() {
            Some(state) => state
                .validate_ring_sector_details_diff_response(
                    from_peer.clone(),
                    ring_sector_details_diff_response,
                )?
                .clone(),
            None => {
                return Err(K2GossipError::peer_behavior(
                    "Unsolicited RingSectorDetailsDiffResponse message",
                ));
            }
        };

        Ok((lock, ring_sector_details_diff))
    }
}

impl GossipRoundState {
    fn validate_ring_sector_details_diff_response(
        &self,
        from_peer: Url,
        ring_sector_details_diff_response: &K2GossipRingSectorDetailsDiffResponseMessage,
    ) -> K2GossipResult<&RoundStageRingSectorDetailsDiff> {
        if self.session_with_peer != from_peer {
            return Err(K2Error::other(format!(
                "RingSectorDetailsDiffResponse message from wrong peer: {} != {}",
                self.session_with_peer, from_peer
            )).into());
        }

        if self.session_id != ring_sector_details_diff_response.session_id {
            return Err(K2GossipError::peer_behavior(format!(
                "Session id mismatch: {:?} != {:?}",
                self.session_id, ring_sector_details_diff_response.session_id
            )));
        }

        let Some(snapshot) = &ring_sector_details_diff_response.snapshot else {
            return Err(K2GossipError::peer_behavior(
                "Received RingSectorDetailsDiffResponse message without snapshot",
            ));
        };

        match &self.stage {
            RoundStage::RingSectorDetailsDiff(
                state @ RoundStageRingSectorDetailsDiff {
                    common_arc_set, ..
                },
            ) => {
                for sector in snapshot
                    .ring_sector_hashes
                    .iter()
                    .flat_map(|sh| sh.sector_indices.iter())
                {
                    if !common_arc_set.includes_sector_index(*sector) {
                        return Err(K2GossipError::peer_behavior(
                            "RingSectorDetailsDiffResponse message contains sector that isn't in the common arc set",
                        ));
                    }
                }

                Ok(state)
            }
            stage => Err(K2GossipError::peer_behavior(format!(
                "Unexpected round state for ring sector details diff response: RingSectorDetailsDiff != {:?}",
                stage
            ))),
        }
    }
}
