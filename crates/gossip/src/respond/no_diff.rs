use crate::gossip::K2Gossip;
use crate::protocol::{
    encode_agent_infos, GossipMessage, K2GossipAgentsMessage,
    K2GossipNoDiffMessage,
};
use kitsune2_api::{K2Error, K2Result, Url};

impl K2Gossip {
    pub(super) async fn respond_to_no_diff(
        &self,
        from_peer: Url,
        no_diff: K2GossipNoDiffMessage,
    ) -> K2Result<Option<GossipMessage>> {
        self.check_no_diff_state_and_remove(from_peer.clone(), &no_diff)
            .await?;

        // Unwrap because checked by validate_no_diff
        let accept_response = no_diff.accept_response.unwrap();

        let send_agents = self
            .handle_accept_response(&from_peer, accept_response)
            .await?;

        match send_agents {
            Some(agents) => {
                Ok(Some(GossipMessage::Agents(K2GossipAgentsMessage {
                    session_id: no_diff.session_id,
                    provided_agents: encode_agent_infos(agents)?,
                })))
            }
            None => Ok(None),
        }
    }

    async fn check_no_diff_state_and_remove(
        &self,
        from_peer: Url,
        no_diff: &K2GossipNoDiffMessage,
    ) -> K2Result<()> {
        let mut accepted_states = self.accepted_round_states.write().await;
        if !accepted_states.contains_key(&from_peer) {
            return Err(K2Error::other(format!(
                "Unsolicited NoDiff message from peer: {:?}",
                from_peer
            )));
        }

        accepted_states[&from_peer]
            .lock()
            .await
            .validate_no_diff(from_peer.clone(), no_diff)?;

        // We're at the end of the round. We might send back an Agents message, but we shouldn't
        // get any further messages from the other peer.
        accepted_states.remove(&from_peer);

        Ok(())
    }
}
