use bytes::Bytes;
use kitsune2_api::{AgentId, Url};
use kitsune2_dht::snapshot::DhtSnapshot;
use kitsune2_dht::ArcSet;
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
        our_arc_set: ArcSet,
    ) -> Self {
        let mut session_id = bytes::BytesMut::zeroed(12);
        rand::thread_rng().fill_bytes(&mut session_id);

        Self {
            session_with_peer,
            started_at: tokio::time::Instant::now(),
            session_id: session_id.freeze(),
            stage: RoundStage::Initiated(RoundStageInitiated {
                our_agents,
                our_arc_set,
            }),
        }
    }

    pub(crate) fn new_accepted(
        session_with_peer: Url,
        session_id: Bytes,
        our_agents: Vec<AgentId>,
        common_arc_set: ArcSet,
    ) -> Self {
        Self {
            session_with_peer,
            started_at: tokio::time::Instant::now(),
            session_id,
            stage: RoundStage::Accepted(RoundStageAccepted {
                our_agents,
                common_arc_set,
            }),
        }
    }
}

/// The state of a gossip round.
#[derive(Debug)]
pub(crate) enum RoundStage {
    Initiated(RoundStageInitiated),
    Accepted(RoundStageAccepted),
    NoDiff,
    DiscSectorsDiff(RoundStageDiscSectorsDiff),
    DiscSectorDetailsDiff(RoundStageDiscSectorDetailsDiff),
    RingSectorDetailsDiff(RoundStageRingSectorDetailsDiff),
}

/// The state of a gossip round that has been initiated.
#[derive(Debug, Clone)]
pub(crate) struct RoundStageInitiated {
    pub our_agents: Vec<AgentId>,
    pub our_arc_set: ArcSet,
}

#[derive(Debug, Clone)]
pub(crate) struct RoundStageAccepted {
    pub our_agents: Vec<AgentId>,
    pub common_arc_set: ArcSet,
}

#[derive(Debug, Clone)]
pub(crate) struct RoundStageDiscSectorsDiff {
    pub common_arc_set: ArcSet,
}

#[derive(Debug, Clone)]
pub(crate) struct RoundStageDiscSectorDetailsDiff {
    pub common_arc_set: ArcSet,
    pub snapshot: DhtSnapshot,
}

#[derive(Debug, Clone)]
pub(crate) struct RoundStageRingSectorDetailsDiff {
    pub common_arc_set: ArcSet,
    pub snapshot: DhtSnapshot,
}

#[cfg(test)]
mod tests {
    use crate::state::GossipRoundState;
    use bytes::Bytes;
    use kitsune2_api::{AgentId, DhtArc, Url};
    use kitsune2_dht::ArcSet;

    #[test]
    fn create_round_state() {
        let state = GossipRoundState::new(
            Url::from_str("ws://test:80").unwrap(),
            vec![AgentId::from(Bytes::from_static(b"test-agent"))],
            ArcSet::new(vec![DhtArc::FULL]).unwrap(),
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
