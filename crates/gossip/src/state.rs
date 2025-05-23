use bytes::Bytes;
use kitsune2_api::{AgentId, Url};
use kitsune2_dht::ArcSet;
use kitsune2_dht::DhtSnapshot;
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

    /// The maximum number of bytes of [kitsune2_api::Op]s the peer is willing to accept.
    ///
    /// Note that it's actually [kitsune2_api::OpId]s that are exchanged. So this is a hint in
    /// terms of op data about how many op ids we should send back to the other peer.
    ///
    /// The value is an `i32` because it's a soft limit and in order to send complete slices,
    /// gossip may exceed the limit. That is recorded here as a negative number.
    pub peer_max_op_data_bytes: i32,

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
            // Initial value, to be updated when the round is accepted.
            peer_max_op_data_bytes: 0,
            stage: RoundStage::Initiated(RoundStageInitiated {
                our_agents,
                our_arc_set,
                tie_breaker: rand::thread_rng().next_u32().saturating_add(1),
            }),
        }
    }

    pub(crate) fn new_accepted(
        session_with_peer: Url,
        session_id: Bytes,
        peer_max_op_data_bytes: u32,
        our_agents: Vec<AgentId>,
        common_arc_set: ArcSet,
    ) -> Self {
        Self {
            session_with_peer,
            started_at: tokio::time::Instant::now(),
            session_id,
            peer_max_op_data_bytes: peer_max_op_data_bytes as i32,
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
    pub tie_breaker: u32,
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
