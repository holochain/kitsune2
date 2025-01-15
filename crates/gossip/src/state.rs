use crate::protocol::K2GossipAcceptMessage;
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
    #[allow(dead_code)]
    started_at: std::time::Instant,

    /// The session id of this round.
    ///
    /// Must be randomly chosen and unique for each initiated round.
    pub session_id: bytes::Bytes,

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
        let mut session_id = bytes::BytesMut::with_capacity(96);
        rand::thread_rng().fill_bytes(&mut session_id);

        Self {
            session_with_peer,
            started_at: std::time::Instant::now(),
            session_id: session_id.freeze(),
            stage: RoundStage::Initiated { our_agents },
        }
    }

    pub(crate) fn new_accepted(
        session_with_peer: Url,
        session_id: bytes::Bytes,
        our_agents: Vec<AgentId>,
    ) -> Self {
        Self {
            session_with_peer,
            started_at: std::time::Instant::now(),
            session_id,
            stage: RoundStage::Accepted { our_agents },
        }
    }

    pub(crate) fn validate_accept(
        this: &Option<Self>,
        from: Url,
        accept: &K2GossipAcceptMessage,
    ) -> K2Result<()> {
        match this {
            Some(state) => {
                if state.session_with_peer != from {
                    return Err(K2Error::other(format!(
                        "Accept message from wrong peer: {} != {}",
                        state.session_with_peer, from
                    )));
                }

                match &state.stage {
                    RoundStage::Initiated { our_agents } => {
                        tracing::trace!("Initiated round state found");

                        if accept.missing_agents.iter().any(|a| {
                            !our_agents.contains(&AgentId::from(a.clone()))
                        }) {
                            return Err(K2Error::other("Accept message contains agents that we didn't declare"));
                        }
                    }
                    stage => {
                        return Err(K2Error::other(format!("Unexpected round state for accept: Initiated != {:?}", stage)));
                    }
                }

                if state.session_id != accept.session_id {
                    return Err(K2Error::other(format!(
                        "Session id mismatch: {:?} != {:?}",
                        state.session_id, accept.session_id
                    )));
                }
            }
            None => {
                return Err(K2Error::other("No initiated round state"));
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
}
