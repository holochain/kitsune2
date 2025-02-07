use crate::gossip::K2Gossip;
use crate::protocol::{GossipMessage, K2GossipAgentsMessage};
use crate::state::{GossipRoundState, RoundStage};
use kitsune2_api::{K2Error, K2Result, Url};
use crate::error::K2GossipResult;

impl K2Gossip {
    pub(super) async fn respond_to_agents(
        &self,
        from_peer: Url,
        agents: K2GossipAgentsMessage,
    ) -> K2GossipResult<Option<GossipMessage>> {
        // Validate the incoming agents message against our own state.
        let mut initiated_lock = self.initiated_round_state.lock().await;
        match initiated_lock.as_ref() {
            Some(state) => {
                state.validate_agents(from_peer.clone(), &agents)?;
                // The session is finished, remove the state.
                initiated_lock.take();
            }
            None => {
                return Err(K2Error::other("Unsolicited Agents message").into());
            }
        }

        self.receive_agent_infos(agents.provided_agents).await?;

        Ok(None)
    }
}

impl GossipRoundState {
    fn validate_agents(
        &self,
        from_peer: Url,
        agents: &K2GossipAgentsMessage,
    ) -> K2Result<()> {
        if self.session_with_peer != from_peer {
            return Err(K2Error::other(format!(
                "Agents message from wrong peer: {} != {}",
                self.session_with_peer, from_peer
            )));
        }

        if self.session_id != agents.session_id {
            return Err(K2Error::other(format!(
                "Session id mismatch: {:?} != {:?}",
                self.session_id, agents.session_id
            )));
        }

        match &self.stage {
            RoundStage::NoDiff { .. } => {
                tracing::trace!("NoDiff round state found");
            }
            stage => {
                return Err(K2Error::other(format!(
                    "Unexpected round state for agents: NoDiff != {:?}",
                    stage
                )));
            }
        }

        Ok(())
    }
}
