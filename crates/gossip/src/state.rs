use crate::protocol::{
    K2GossipAcceptMessage, K2GossipAgentsMessage, K2GossipNoDiffMessage,
};
use bytes::Bytes;
use kitsune2_api::{AgentId, K2Error, K2Result, Url};
use rand::RngCore;

/// The state of a gossip round.
#[derive(Debug)]
pub(crate) struct GossipRoundState {
    /// The agent id of the other party who is participating in this round.
    pub session_with_peer: Url,

    /// The time at which this round was initiated.
    ///
    /// This is used to apply a timeout to the round.
    pub started_at: tokio::time::Instant,

    /// The session id of this round.
    ///
    /// Must be randomly chosen and unique for each initiated round.
    pub session_id: Bytes,

    /// The current stage of the round.
    ///
    /// Store the current stage, so that the next stage can be validated.
    pub stage: RoundStage,
}

impl GossipRoundState {
    /// Create a new gossip round state.
    pub(crate) fn new(
        session_with_peer: Url,
        our_agents: Vec<AgentId>,
    ) -> Self {
        let mut session_id = bytes::BytesMut::zeroed(12);
        rand::thread_rng().fill_bytes(&mut session_id);

        Self {
            session_with_peer,
            started_at: tokio::time::Instant::now(),
            session_id: session_id.freeze(),
            stage: RoundStage::Initiated { our_agents },
        }
    }

    pub(crate) fn new_accepted(
        session_with_peer: Url,
        session_id: Bytes,
        our_agents: Vec<AgentId>,
    ) -> Self {
        Self {
            session_with_peer,
            started_at: tokio::time::Instant::now(),
            session_id,
            stage: RoundStage::Accepted { our_agents },
        }
    }

    pub(crate) fn validate_accept(
        &self,
        from_peer: Url,
        accept: &K2GossipAcceptMessage,
    ) -> K2Result<()> {
        if self.session_with_peer != from_peer {
            return Err(K2Error::other(format!(
                "Accept message from wrong peer: {} != {}",
                self.session_with_peer, from_peer
            )));
        }

        if self.session_id != accept.session_id {
            return Err(K2Error::other(format!(
                "Session id mismatch: {:?} != {:?}",
                self.session_id, accept.session_id
            )));
        }

        match &self.stage {
            RoundStage::Initiated { our_agents } => {
                tracing::trace!("Initiated round state found");

                if accept
                    .missing_agents
                    .iter()
                    .any(|a| !our_agents.contains(&AgentId::from(a.clone())))
                {
                    return Err(K2Error::other(
                        "Accept message contains agents that we didn't declare",
                    ));
                }
            }
            stage => {
                return Err(K2Error::other(format!(
                    "Unexpected round state for accept: Initiated != {:?}",
                    stage
                )));
            }
        }

        Ok(())
    }

    pub(crate) fn validate_no_diff(
        &self,
        from_peer: Url,
        no_diff: &K2GossipNoDiffMessage,
    ) -> K2Result<()> {
        if self.session_with_peer != from_peer {
            return Err(K2Error::other(format!(
                "NoDiff message from wrong peer: {} != {}",
                self.session_with_peer, from_peer
            )));
        }

        if self.session_id != no_diff.session_id {
            return Err(K2Error::other(format!(
                "Session id mismatch: {:?} != {:?}",
                self.session_id, no_diff.session_id
            )));
        }

        match &self.stage {
            RoundStage::Accepted { our_agents } => {
                tracing::trace!("Accepted round state found");

                if no_diff
                    .missing_agents
                    .iter()
                    .any(|a| !our_agents.contains(&AgentId::from(a.clone())))
                {
                    return Err(K2Error::other(
                        "NoDiff message contains agents that we didn't declare",
                    ));
                }
            }
            stage => {
                return Err(K2Error::other(format!(
                    "Unexpected round state for accept: Accepted != {:?}",
                    stage
                )));
            }
        }

        Ok(())
    }

    pub(crate) fn validate_agents(
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

/// The state of a gossip round.
#[derive(Debug)]
pub(crate) enum RoundStage {
    Initiated {
        our_agents: Vec<AgentId>,
    },
    Accepted {
        #[allow(dead_code)]
        our_agents: Vec<AgentId>,
    },
    NoDiff,
}

#[cfg(test)]
mod tests {
    use crate::state::GossipRoundState;
    use bytes::Bytes;
    use kitsune2_api::{AgentId, Url};

    #[test]
    fn create_round_state() {
        let state = GossipRoundState::new(
            Url::from_str("ws://test:80").unwrap(),
            vec![AgentId::from(Bytes::from_static(b"test-agent"))],
        );

        assert_eq!(12, state.session_id.len());
        assert_ne!(
            0,
            state.session_id[0]
                ^ state.session_id[1]
                ^ state.session_id[2]
                ^ state.session_id[3]
        );
    }
}
