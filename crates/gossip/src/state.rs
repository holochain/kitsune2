use crate::protocol::{
    K2GossipAcceptMessage, K2GossipAgentsMessage,
    K2GossipDiscSectorDetailsDiffMessage,
    K2GossipDiscSectorDetailsDiffResponseMessage,
    K2GossipDiscSectorsDiffMessage, K2GossipNoDiffMessage,
};
use bytes::Bytes;
use kitsune2_api::{AgentId, K2Error, K2Result, Url};
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
            stage: RoundStage::Accepted {
                our_agents,
                common_arc_set,
            },
        }
    }

    pub(crate) fn validate_accept(
        &self,
        from_peer: Url,
        accept: &K2GossipAcceptMessage,
    ) -> K2Result<&RoundStageInitiated> {
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
            RoundStage::Initiated(
                stage @ RoundStageInitiated { our_agents, .. },
            ) => {
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

                Ok(stage)
            }
            stage => Err(K2Error::other(format!(
                "Unexpected round state for accept: Initiated != {:?}",
                stage
            ))),
        }
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
            RoundStage::Accepted { our_agents, .. } => {
                let Some(accept_response) = &no_diff.accept_response else {
                    return Err(K2Error::other(
                        "Received NoDiff message without accept response",
                    ));
                };

                if accept_response
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

    pub(crate) fn validate_disc_sectors_diff(
        &self,
        from_peer: Url,
        disc_sectors_diff: &K2GossipDiscSectorsDiffMessage,
    ) -> K2Result<()> {
        if self.session_with_peer != from_peer {
            return Err(K2Error::other(format!(
                "DiscSectorsDiff message from wrong peer: {} != {}",
                self.session_with_peer, from_peer
            )));
        }

        if self.session_id != disc_sectors_diff.session_id {
            return Err(K2Error::other(format!(
                "Session id mismatch: {:?} != {:?}",
                self.session_id, disc_sectors_diff.session_id
            )));
        }

        let Some(snapshot) = &disc_sectors_diff.snapshot else {
            return Err(K2Error::other(
                "Received DiscSectorsDiff message without snapshot",
            ));
        };

        match &self.stage {
            RoundStage::Accepted {
                our_agents,
                common_arc_set,
            } => {
                let Some(accept_response) = &disc_sectors_diff.accept_response
                else {
                    return Err(K2Error::other(
                        "Received NoDiff message without accept response",
                    ));
                };

                if accept_response
                    .missing_agents
                    .iter()
                    .any(|a| !our_agents.contains(&AgentId::from(a.clone())))
                {
                    return Err(K2Error::other(
                        "NoDiff message contains agents that we didn't declare",
                    ));
                }

                for sector in &snapshot.disc_sectors {
                    if !common_arc_set.includes_sector_index(*sector) {
                        return Err(K2Error::other(
                            "DiscSectorsDiff message contains sector that isn't in the common arc set",
                        ));
                    }
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

    pub(crate) fn validate_disc_sector_details_diff(
        &self,
        from_peer: Url,
        disc_sector_details_diff: &K2GossipDiscSectorDetailsDiffMessage,
    ) -> K2Result<&RoundStageDiscSectorsDiff> {
        if self.session_with_peer != from_peer {
            return Err(K2Error::other(format!(
                "DiscSectorDetailsDiff message from wrong peer: {} != {}",
                self.session_with_peer, from_peer
            )));
        }

        if self.session_id != disc_sector_details_diff.session_id {
            return Err(K2Error::other(format!(
                "Session id mismatch: {:?} != {:?}",
                self.session_id, disc_sector_details_diff.session_id
            )));
        }

        let Some(snapshot) = &disc_sector_details_diff.snapshot else {
            return Err(K2Error::other(
                "Received DiscSectorDetailsDiff message without snapshot",
            ));
        };

        match &self.stage {
            RoundStage::DiscSectorsDiff(stage @ RoundStageDiscSectorsDiff { common_arc_set }) => {
                for sector in &snapshot.sector_indices {
                    if !common_arc_set.includes_sector_index(*sector) {
                        return Err(K2Error::other(
                            "DiscSectorDetailsDiff message contains sector that isn't in the common arc set",
                        ));
                    }
                }

                Ok(stage)
            }
            stage => {
                Err(K2Error::other(format!(
                    "Unexpected round state for disc sector details diff: DiscSectorsDiff != {:?}",
                    stage
                )))
            }
        }
    }

    pub(crate) fn validate_disc_sector_details_diff_response(
        &self,
        from_peer: Url,
        disc_sector_details_diff: &K2GossipDiscSectorDetailsDiffResponseMessage,
    ) -> K2Result<()> {
        if self.session_with_peer != from_peer {
            return Err(K2Error::other(format!(
                "DiscSectorDetailsDiffResponse message from wrong peer: {} != {}",
                self.session_with_peer, from_peer
            )));
        }

        if self.session_id != disc_sector_details_diff.session_id {
            return Err(K2Error::other(format!(
                "Session id mismatch: {:?} != {:?}",
                self.session_id, disc_sector_details_diff.session_id
            )));
        }

        let Some(snapshot) = &disc_sector_details_diff.snapshot else {
            return Err(K2Error::other(
                "Received DiscSectorDetailsDiffResponse message without snapshot",
            ));
        };

        match &self.stage {
            RoundStage::DiscSectorDetailsDiff(RoundStageDiscSectorDetailsDiff { common_arc_set, .. }) => {
                for sector in &snapshot.sector_indices {
                    if !common_arc_set.includes_sector_index(*sector) {
                        return Err(K2Error::other(
                            "DiscSectorDetailsDiffResponse message contains sector that isn't in the common arc set",
                        ));
                    }
                }

                Ok(())
            }
            stage => {
                Err(K2Error::other(format!(
                    "Unexpected round state for disc sector details diff response: DiscSectorDetailsDiff != {:?}",
                    stage
                )))
            }
        }
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
    Initiated(RoundStageInitiated),
    Accepted {
        our_agents: Vec<AgentId>,
        common_arc_set: ArcSet,
    },
    NoDiff,
    DiscSectorsDiff(RoundStageDiscSectorsDiff),
    DiscSectorDetailsDiff(RoundStageDiscSectorDetailsDiff),
    RingSectorDetailsDiff(RoundStageRingSectorDetailsDiff),
}

/// The state of a gossip round that has been initiated.
#[derive(Debug)]
pub(crate) struct RoundStageInitiated {
    pub our_agents: Vec<AgentId>,
    pub our_arc_set: ArcSet,
}

#[derive(Debug)]
pub(crate) struct RoundStageDiscSectorsDiff {
    pub common_arc_set: ArcSet,
}

#[derive(Debug)]
pub(crate) struct RoundStageDiscSectorDetailsDiff {
    pub common_arc_set: ArcSet,
    pub snapshot: DhtSnapshot,
}

#[derive(Debug)]
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
