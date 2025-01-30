use crate::gossip::K2Gossip;
use crate::protocol::{
    encode_op_ids, GossipMessage, K2GossipDiscSectorDetailsDiffMessage,
    K2GossipDiscSectorDetailsDiffResponseMessage,
};
use crate::state::{
    GossipRoundState, RoundStage, RoundStageDiscSectorDetailsDiff,
    RoundStageDiscSectorsDiff,
};
use kitsune2_api::{K2Error, K2Result, Url};
use kitsune2_dht::DhtSnapshotNextAction;
use tokio::sync::MutexGuard;

impl K2Gossip {
    pub(super) async fn respond_to_disc_sector_details_diff(
        &self,
        from_peer: Url,
        disc_sector_details_diff: K2GossipDiscSectorDetailsDiffMessage,
    ) -> K2Result<Option<GossipMessage>> {
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

        let next_action = self
            .dht
            .read()
            .await
            .handle_snapshot(
                their_snapshot,
                None,
                disc_sector_details.common_arc_set.clone(),
            )
            .await?;

        match next_action {
            DhtSnapshotNextAction::CannotCompare
            | DhtSnapshotNextAction::Identical => {
                tracing::info!("Received a disc sector details diff but no diff to send back, responding with agents");

                // TODO These cases where we terminate don't notify the remote so they'll
                //      end up timing out. Should do something differently here.
                // Terminating the session, so remove the state.
                state.take();

                Ok(None)
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
                unreachable!("unexpected next action")
            }
        }
    }

    async fn check_disc_sector_details_diff_state<'a>(
        &'a self,
        from_peer: Url,
        disc_sector_details_diff: &K2GossipDiscSectorDetailsDiffMessage,
    ) -> K2Result<(
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
                return Err(K2Error::other(
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
    ) -> K2Result<&RoundStageDiscSectorsDiff> {
        if self.session_with_peer != from_peer {
            return Err(K2Error::other(format!(
                "DiscSectorDetailsDiff message from wrong peer: {} != {}",
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
                "Received DiscSectorDetailsDiff message without snapshot",
            ));
        };

        match &self.stage {
            RoundStage::DiscSectorsDiff(stage @ RoundStageDiscSectorsDiff { common_arc_set }) => {
                for sector in &snapshot.sector_indices {
                    if !common_arc_set.includes_sector_index(*sector) {
                        return Err(K2Error::other(
                            "DiscSectorDetailsDiff message contains sector that isn't in the common arc set",
                        ));
                    }
                }

                Ok(stage)
            }
            stage => {
                Err(K2Error::other(format!(
                    "Unexpected round state for disc sector details diff: DiscSectorsDiff != {:?}",
                    stage
                )))
            }
        }
    }
}
