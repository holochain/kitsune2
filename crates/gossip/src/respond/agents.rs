use crate::gossip::K2Gossip;
use crate::protocol::{GossipMessage, K2GossipAgentsMessage};
use kitsune2_api::{K2Error, K2Result, Url};

impl K2Gossip {
    pub(super) async fn respond_to_agents(
        &self,
        from_peer: Url,
        agents: K2GossipAgentsMessage,
    ) -> K2Result<Option<GossipMessage>> {
        // Validate the incoming agents message against our own state.
        let mut initiated_lock = self.initiated_round_state.lock().await;
        match initiated_lock.as_ref() {
            Some(state) => {
                state.validate_agents(from_peer.clone(), &agents)?;
                // The session is finished, remove the state.
                initiated_lock.take();
            }
            None => {
                return Err(K2Error::other("Unsolicited Agents message"));
            }
        }

        self.receive_agent_infos(agents.provided_agents).await?;

        Ok(None)
    }
}
