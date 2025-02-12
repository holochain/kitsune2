use crate::error::{K2GossipError, K2GossipResult};
use crate::gossip::K2Gossip;
use crate::protocol::{GossipMessage, K2GossipTerminateMessage};
use kitsune2_api::Url;

impl K2Gossip {
    pub(super) async fn respond_to_terminate(
        &self,
        from_peer: Url,
        terminate: K2GossipTerminateMessage,
    ) -> K2GossipResult<Option<GossipMessage>> {
        tracing::info!("Peer {from_peer} is attempting to terminate gossip session with reason: {}", terminate.reason);

        let terminated_initiated_session = {
            let mut initiate_lock = self.initiated_round_state.lock().await;
            if let Some(state) = initiate_lock.as_ref() {
                if state.session_with_peer == from_peer {
                    if state.session_id == terminate.session_id {
                        initiate_lock.take();
                    } else {
                        initiate_lock.take();
                        return Err(K2GossipError::peer_behavior(format!(
                            "Unsolicited terminate message from: {from_peer}"
                        )));
                    }
                }

                true
            } else {
                false
            }
        };

        let terminated_accepted_session = if !terminated_initiated_session {
            let read_guard = self.accepted_round_states.write().await;
            let accepted = read_guard.get(&from_peer);

            if let Some(state) = accepted.as_ref() {
                let lock = state.lock().await;

                self.accepted_round_states.write().await.remove(&from_peer);
                if lock.session_id != terminate.session_id {
                    return Err(K2GossipError::peer_behavior(format!(
                        "Unsolicited terminate message from: {from_peer}"
                    )));
                }

                true
            } else {
                false
            }
        } else {
            false
        };

        if terminated_initiated_session || terminated_accepted_session {
            self.peer_meta_store.incr_peer_terminated(from_peer).await?;
        } else {
            return Err(K2GossipError::peer_behavior(format!(
                "Unsolicited termination message from: {from_peer}"
            )));
        }

        // If there was no error then always respond with no message, the other side terminated
        Ok(None)
    }
}
