use crate::error::{K2GossipError, K2GossipResult};
use crate::gossip::K2Gossip;
use crate::protocol::{
    encode_agent_infos, GossipMessage, K2GossipAgentsMessage,
    K2GossipNoDiffMessage, K2GossipTerminateMessage,
};
use crate::state::{GossipRoundState, RoundStage, RoundStageAccepted};
use kitsune2_api::{AgentId, K2Error, Url};

impl K2Gossip {
    pub(super) async fn respond_to_no_diff(
        &self,
        from_peer: Url,
        no_diff: K2GossipNoDiffMessage,
    ) -> K2GossipResult<Option<GossipMessage>> {
        self.check_no_diff_state_and_remove(from_peer.clone(), &no_diff)
            .await?;

        // Unwrap because checked by validate_no_diff
        let accept_response = no_diff.accept_response.unwrap();

        let send_agents = self
            .handle_accept_response(&from_peer, accept_response)
            .await?;

        match send_agents {
            Some(agents) if !agents.is_empty() => {
                Ok(Some(GossipMessage::Agents(K2GossipAgentsMessage {
                    session_id: no_diff.session_id,
                    provided_agents: encode_agent_infos(agents)?,
                })))
            }
            _ => Ok(Some(GossipMessage::Terminate(K2GossipTerminateMessage {
                session_id: no_diff.session_id,
                reason: "No agents".to_string(),
            }))),
        }
    }

    async fn check_no_diff_state_and_remove(
        &self,
        from_peer: Url,
        no_diff: &K2GossipNoDiffMessage,
    ) -> K2GossipResult<()> {
        let mut accepted_states = self.accepted_round_states.write().await;
        if !accepted_states.contains_key(&from_peer) {
            return Err(K2GossipError::peer_behavior(format!(
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

impl GossipRoundState {
    fn validate_no_diff(
        &self,
        from_peer: Url,
        no_diff: &K2GossipNoDiffMessage,
    ) -> K2GossipResult<()> {
        if self.session_with_peer != from_peer {
            return Err(K2Error::other(format!(
                "NoDiff message from wrong peer: {} != {}",
                self.session_with_peer, from_peer
            ))
            .into());
        }

        if self.session_id != no_diff.session_id {
            return Err(K2GossipError::peer_behavior(format!(
                "Session id mismatch: {:?} != {:?}",
                self.session_id, no_diff.session_id
            )));
        }

        match &self.stage {
            RoundStage::Accepted(RoundStageAccepted { our_agents, .. }) => {
                let Some(accept_response) = &no_diff.accept_response else {
                    return Err(K2GossipError::peer_behavior(
                        "Received NoDiff message without accept response",
                    ));
                };

                if accept_response
                    .missing_agents
                    .iter()
                    .any(|a| !our_agents.contains(&AgentId::from(a.clone())))
                {
                    return Err(K2GossipError::peer_behavior(
                        "NoDiff message contains agents that we didn't declare",
                    ));
                }
            }
            stage => {
                return Err(K2GossipError::peer_behavior(format!(
                    "Unexpected round state for accept: Accepted != {:?}",
                    stage
                )));
            }
        }

        Ok(())
    }
}
