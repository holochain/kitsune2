use crate::error::{K2GossipError, K2GossipResult};
use crate::gossip::K2Gossip;
use crate::protocol::{
    encode_op_ids, GossipMessage, K2GossipDiscSectorDetailsDiffMessage,
    K2GossipDiscSectorDetailsDiffResponseMessage, K2GossipTerminateMessage,
};
use crate::state::{
    GossipRoundState, RoundStage, RoundStageDiscSectorDetailsDiff,
    RoundStageDiscSectorsDiff,
};
use kitsune2_api::{K2Error, Url};
use kitsune2_dht::DhtSnapshotNextAction;
use tokio::sync::MutexGuard;

impl K2Gossip {
    pub(super) async fn respond_to_disc_sector_details_diff(
        &self,
        from_peer: Url,
        disc_sector_details_diff: K2GossipDiscSectorDetailsDiffMessage,
    ) -> K2GossipResult<Option<GossipMessage>> {
        // Validate the incoming disc sector details diff against our own state.
        let (mut state, disc_sector_details) = self
            .check_disc_sector_details_diff_state(
                from_peer.clone(),
                &disc_sector_details_diff,
            )
            .await?;

        let their_snapshot = disc_sector_details_diff
            .snapshot
            .expect(
                "Snapshot present checked by validate_disc_sector_details_diff",
            )
            .try_into()?;

        let peer_max_op_data_bytes = state
            .as_ref()
            .map(|s| s.peer_max_op_data_bytes)
            .unwrap_or(0);

        let (next_action, used_bytes) = self
            .dht
            .read()
            .await
            .handle_snapshot(
                their_snapshot,
                None,
                disc_sector_details.common_arc_set.clone(),
                peer_max_op_data_bytes,
            )
            .await?;

        if let Some(state) = state.as_mut() {
            tracing::debug!(
                "Used {}/{} op budget to send disc ops",
                used_bytes,
                state.peer_max_op_data_bytes,
            );
            state.peer_max_op_data_bytes -= used_bytes as i32;
        }

        match next_action {
            DhtSnapshotNextAction::CannotCompare
            | DhtSnapshotNextAction::Identical => {
                tracing::info!("Received a disc sector details diff but no diff to send back, responding with agents");

                // Terminating the session, so remove the state.
                state.take();

                Ok(Some(GossipMessage::Terminate(K2GossipTerminateMessage {
                    session_id: disc_sector_details_diff.session_id,
                    reason: "Nothing to compare".to_string(),
                })))
            }
            DhtSnapshotNextAction::NewSnapshotAndHashList(snapshot, ops) => {
                if let Some(state) = state.as_mut() {
                    state.stage = RoundStage::DiscSectorDetailsDiff(
                        RoundStageDiscSectorDetailsDiff {
                            common_arc_set: disc_sector_details
                                .common_arc_set
                                .clone(),
                            snapshot: snapshot.clone(),
                        },
                    );
                }

                Ok(Some(GossipMessage::DiscSectorDetailsDiffResponse(
                    K2GossipDiscSectorDetailsDiffResponseMessage {
                        session_id: disc_sector_details_diff.session_id,
                        missing_ids: encode_op_ids(ops),
                        snapshot: Some(snapshot.try_into()?),
                    },
                )))
            }
            _ => {
                tracing::error!("Unexpected next action: {:?}", next_action);

                // Remove round state.
                state.take();

                Ok(Some(GossipMessage::Terminate(K2GossipTerminateMessage {
                    session_id: disc_sector_details_diff.session_id,
                    reason: "Unexpected next action".into(),
                })))
            }
        }
    }

    async fn check_disc_sector_details_diff_state<'a>(
        &'a self,
        from_peer: Url,
        disc_sector_details_diff: &K2GossipDiscSectorDetailsDiffMessage,
    ) -> K2GossipResult<(
        MutexGuard<'a, Option<GossipRoundState>>,
        RoundStageDiscSectorsDiff,
    )> {
        let lock = self.initiated_round_state.lock().await;
        let disc_sectors_diff = match lock.as_ref() {
            Some(state) => state
                .validate_disc_sector_details_diff(
                    from_peer.clone(),
                    disc_sector_details_diff,
                )?
                .clone(),
            None => {
                return Err(K2GossipError::peer_behavior(
                    "Unsolicited DiscSectorDetailsDiff message",
                ));
            }
        };

        Ok((lock, disc_sectors_diff))
    }
}

impl GossipRoundState {
    fn validate_disc_sector_details_diff(
        &self,
        from_peer: Url,
        disc_sector_details_diff: &K2GossipDiscSectorDetailsDiffMessage,
    ) -> K2GossipResult<&RoundStageDiscSectorsDiff> {
        if self.session_with_peer != from_peer {
            return Err(K2Error::other(format!(
                "DiscSectorDetailsDiff message from wrong peer: {} != {}",
                self.session_with_peer, from_peer
            ))
            .into());
        }

        if self.session_id != disc_sector_details_diff.session_id {
            return Err(K2GossipError::peer_behavior(format!(
                "Session id mismatch: {:?} != {:?}",
                self.session_id, disc_sector_details_diff.session_id
            )));
        }

        let Some(snapshot) = &disc_sector_details_diff.snapshot else {
            return Err(K2GossipError::peer_behavior(
                "Received DiscSectorDetailsDiff message without snapshot",
            ));
        };

        match &self.stage {
            RoundStage::DiscSectorsDiff(stage @ RoundStageDiscSectorsDiff { common_arc_set }) => {
                for sector in &snapshot.sector_indices {
                    if !common_arc_set.includes_sector_index(*sector) {
                        return Err(K2GossipError::peer_behavior(
                            "DiscSectorDetailsDiff message contains sector that isn't in the common arc set",
                        ));
                    }
                }

                Ok(stage)
            }
            stage => {
                Err(K2GossipError::peer_behavior(format!(
                    "Unexpected round state for disc sector details diff: DiscSectorsDiff != {:?}",
                    stage
                )))
            }
        }
    }
}
