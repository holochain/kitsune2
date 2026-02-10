use crate::error::{K2GossipError, K2GossipResult};
use crate::gossip::K2Gossip;
use crate::protocol::{GossipMessage, K2GossipHashesMessage};
use kitsune2_api::Url;
use kitsune2_api::decode_ids;

impl K2Gossip {
    pub(super) async fn respond_to_hashes(
        &self,
        from_peer: Url,
        hashes: K2GossipHashesMessage,
    ) -> K2GossipResult<Option<GossipMessage>> {
        // This could be received from either the initiator or the acceptor.
        // So we have to check in both places!

        let handled_as_initiator = {
            let mut initiated_state = self.initiated_round_state.lock().await;
            if let Some(state) = initiated_state.as_ref() {
                if state.session_with_peer == from_peer
                    && state.session_id == hashes.session_id
                {
                    // Session is complete, remove state
                    initiated_state.take();

                    self.fetch
                        .request_ops(
                            decode_ids(hashes.missing_ids.clone()),
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
                    self.accepted_round_states.write().await.remove(&from_peer);

                    self.fetch
                        .request_ops(
                            decode_ids(hashes.missing_ids),
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
            return Err(K2GossipError::peer_behavior(
                "Unsolicited Hashes message",
            ));
        }

        Ok(None)
    }
}
