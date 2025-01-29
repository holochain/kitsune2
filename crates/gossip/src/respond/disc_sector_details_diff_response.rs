use crate::gossip::K2Gossip;
use crate::protocol::{
    encode_op_ids, GossipMessage, K2GossipDiscSectorDetailsDiffResponseMessage,
    K2GossipHashesMessage,
};
use crate::state::{GossipRoundState, RoundStageDiscSectorDetailsDiff};
use kitsune2_api::id::decode_ids;
use kitsune2_api::{K2Error, K2Result, Url};
use kitsune2_dht::DhtSnapshotNextAction;
use tokio::sync::OwnedMutexGuard;

impl K2Gossip {
    pub(super) async fn respond_to_disc_sector_details_diff_response(
        &self,
        from_peer: Url,
        response: K2GossipDiscSectorDetailsDiffResponseMessage,
    ) -> K2Result<Option<GossipMessage>> {
        let (_state, disc_sector_details) = self
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

        let next_action = self
            .dht
            .read()
            .await
            .handle_snapshot(
                &their_snapshot,
                Some(disc_sector_details.snapshot.clone()),
                &disc_sector_details.common_arc_set,
            )
            .await?;

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
