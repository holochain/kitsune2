use crate::error::{K2GossipError, K2GossipResult};
use crate::gossip::K2Gossip;
use crate::protocol::K2GossipBusyMessage;
use crate::state::GossipRoundState;
use kitsune2_api::{K2Error, Timestamp, Url};

impl K2Gossip {
    pub(crate) async fn respond_to_busy(
        &self,
        from_peer: Url,
        busy: K2GossipBusyMessage,
    ) -> K2GossipResult<()> {
        self.check_busy_state_and_remove(&from_peer, busy).await?;

        self.peer_meta_store
            .incr_peer_busy(from_peer.clone())
            .await?;

        // Mark that we've gossiped with this peer recently. We haven't been successful, but we've
        // tried, and we shouldn't immediately try again because they were busy.
        self.peer_meta_store
            .set_last_gossip_timestamp(from_peer, Timestamp::now())
            .await?;

        Ok(())
    }

    async fn check_busy_state_and_remove(
        &self,
        from_peer: &Url,
        busy: K2GossipBusyMessage,
    ) -> K2GossipResult<()> {
        let mut round_state = self.initiated_round_state.lock().await;
        match round_state.as_ref() {
            Some(state) => {
                state.validate_busy(from_peer.clone(), busy)?;
            }
            None => {
                return Err(K2GossipError::peer_behavior(
                    "Unsolicited Busy message",
                ));
            }
        };

        round_state.take();

        Ok(())
    }
}

impl GossipRoundState {
    fn validate_busy(
        &self,
        from_peer: Url,
        accept: K2GossipBusyMessage,
    ) -> K2GossipResult<()> {
        if self.session_with_peer != from_peer {
            return Err(K2Error::other("Busy message from wrong peer").into());
        }

        if self.session_id != accept.session_id {
            return Err(K2GossipError::peer_behavior(
                "Busy message with wrong session id",
            ));
        }

        Ok(())
    }
}
