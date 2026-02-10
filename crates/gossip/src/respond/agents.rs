use crate::error::{K2GossipError, K2GossipResult};
use crate::gossip::K2Gossip;
use crate::protocol::{GossipMessage, K2GossipAgentsMessage};
use crate::state::{GossipRoundState, RoundStage};
use kitsune2_api::{K2Error, Url};

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
                return Err(K2GossipError::peer_behavior(
                    "Unsolicited Agents message",
                ));
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
    ) -> K2GossipResult<()> {
        if self.session_with_peer != from_peer {
            return Err(K2Error::other(format!(
                "Agents message from wrong peer: {} != {}",
                self.session_with_peer, from_peer
            ))
            .into());
        }

        if self.session_id != agents.session_id {
            return Err(K2GossipError::peer_behavior(format!(
                "Session id mismatch: {:?} != {:?}",
                self.session_id, agents.session_id
            )));
        }

        match &self.stage {
            RoundStage::NoDiff => {
                tracing::trace!(?agents.session_id, "NoDiff round state found");
            }
            stage => {
                return Err(K2GossipError::peer_behavior(format!(
                    "Unexpected round stage for agents: NoDiff != {stage:?}"
                )));
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::error::K2GossipError;
    use crate::protocol::{K2GossipAgentsMessage, encode_agent_infos};
    use crate::respond::harness::{RespondTestHarness, test_session_id};
    use crate::state::RoundStage;
    use kitsune2_api::DhtArc;
    use kitsune2_test_utils::enable_tracing;

    #[tokio::test]
    async fn receive_agents() {
        enable_tracing();

        let harness = RespondTestHarness::create().await;

        let local_agent = harness.create_agent(DhtArc::FULL).await;
        let remote_agent = harness.create_agent(DhtArc::FULL).await;

        let session_id = harness
            .insert_initiated_round_state(&local_agent, &remote_agent)
            .await;
        harness
            .gossip
            .initiated_round_state
            .lock()
            .await
            .as_mut()
            .unwrap()
            .stage = RoundStage::NoDiff;

        let discovered_agent_1 = harness.create_agent(DhtArc::Empty).await;
        let discovered_agent_2 = harness.create_agent(DhtArc::FULL).await;

        // Should be nothing in the peer store yet.
        assert!(
            harness
                .gossip
                .peer_store
                .get_all()
                .await
                .unwrap()
                .is_empty()
        );

        let response = harness
            .gossip
            .respond_to_agents(
                remote_agent.url.clone().unwrap(),
                K2GossipAgentsMessage {
                    session_id,
                    provided_agents: encode_agent_infos([
                        discovered_agent_1.agent_info.clone(),
                        discovered_agent_2.agent_info.clone(),
                    ])
                    .unwrap(),
                },
            )
            .await;

        assert!(response.is_ok(), "Response is: {response:?}");

        // Check that the agents were added to the peer store.
        let all_agents = harness.gossip.peer_store.get_all().await.unwrap();
        assert_eq!(all_agents.len(), 2);
        assert!(all_agents.contains(&discovered_agent_1));
        assert!(all_agents.contains(&discovered_agent_2));
    }

    #[tokio::test]
    async fn receive_agents_from_wrong_peer() {
        enable_tracing();

        let harness = RespondTestHarness::create().await;

        let local_agent = harness.create_agent(DhtArc::FULL).await;
        let remote_agent = harness.create_agent(DhtArc::FULL).await;

        let session_id = harness
            .insert_initiated_round_state(&local_agent, &remote_agent)
            .await;
        harness
            .gossip
            .initiated_round_state
            .lock()
            .await
            .as_mut()
            .unwrap()
            .stage = RoundStage::NoDiff;

        let discovered_agent_1 = harness.create_agent(DhtArc::Empty).await;
        let discovered_agent_2 = harness.create_agent(DhtArc::FULL).await;

        let error = harness
            .gossip
            .respond_to_agents(
                harness
                    .create_agent(DhtArc::Empty)
                    .await
                    .url
                    .clone()
                    .unwrap(),
                K2GossipAgentsMessage {
                    session_id,
                    provided_agents: encode_agent_infos([
                        discovered_agent_1.agent_info.clone(),
                        discovered_agent_2.agent_info.clone(),
                    ])
                    .unwrap(),
                },
            )
            .await
            .unwrap_err();

        assert!(
            error.to_string().contains("Agents message from wrong peer"),
            "Expected error for agents message from wrong peer, got: {error:?}"
        );

        // Should not have added any agents to the peer store,
        let all_agents = harness.gossip.peer_store.get_all().await.unwrap();
        assert!(all_agents.is_empty());
    }

    #[tokio::test]
    async fn receive_agents_with_mismatched_session_id() {
        enable_tracing();

        let harness = RespondTestHarness::create().await;

        let local_agent = harness.create_agent(DhtArc::FULL).await;
        let remote_agent = harness.create_agent(DhtArc::FULL).await;

        harness
            .insert_initiated_round_state(&local_agent, &remote_agent)
            .await;
        harness
            .gossip
            .initiated_round_state
            .lock()
            .await
            .as_mut()
            .unwrap()
            .stage = RoundStage::NoDiff;

        let discovered_agent_1 = harness.create_agent(DhtArc::Empty).await;
        let discovered_agent_2 = harness.create_agent(DhtArc::FULL).await;

        let error = harness
            .gossip
            .respond_to_agents(
                remote_agent.url.clone().unwrap(),
                K2GossipAgentsMessage {
                    session_id: test_session_id(),
                    provided_agents: encode_agent_infos([
                        discovered_agent_1.agent_info.clone(),
                        discovered_agent_2.agent_info.clone(),
                    ])
                    .unwrap(),
                },
            )
            .await
            .unwrap_err();

        assert!(
            error.to_string().contains("Session id mismatch"),
            "Expected error for mismatched session id, got: {error:?}"
        );

        // Should not have added any agents to the peer store,
        let all_agents = harness.gossip.peer_store.get_all().await.unwrap();
        assert!(all_agents.is_empty());
    }

    #[tokio::test]
    async fn receive_agents_at_wrong_stage() {
        enable_tracing();

        let harness = RespondTestHarness::create().await;

        let local_agent = harness.create_agent(DhtArc::FULL).await;
        let remote_agent = harness.create_agent(DhtArc::FULL).await;

        let session_id = harness
            .insert_initiated_round_state(&local_agent, &remote_agent)
            .await;

        let discovered_agent_1 = harness.create_agent(DhtArc::Empty).await;
        let discovered_agent_2 = harness.create_agent(DhtArc::FULL).await;

        let error = harness
            .gossip
            .respond_to_agents(
                remote_agent.url.clone().unwrap(),
                K2GossipAgentsMessage {
                    session_id,
                    provided_agents: encode_agent_infos([
                        discovered_agent_1.agent_info.clone(),
                        discovered_agent_2.agent_info.clone(),
                    ])
                    .unwrap(),
                },
            )
            .await
            .unwrap_err();

        assert!(
            error.to_string().contains(
                "Unexpected round stage for agents: NoDiff != Initiated"
            ),
            "Expected error for wrong round stage, got: {error:?}"
        );

        // Should not have added any agents to the peer store,
        let all_agents = harness.gossip.peer_store.get_all().await.unwrap();
        assert!(all_agents.is_empty());
    }

    #[tokio::test]
    async fn receive_agents_with_no_initiated_session() {
        enable_tracing();

        let harness = RespondTestHarness::create().await;

        let remote_agent = harness.create_agent(DhtArc::FULL).await;

        let discovered_agent_1 = harness.create_agent(DhtArc::Empty).await;
        let discovered_agent_2 = harness.create_agent(DhtArc::FULL).await;

        let error = harness
            .gossip
            .respond_to_agents(
                remote_agent.url.clone().unwrap(),
                K2GossipAgentsMessage {
                    session_id: test_session_id(),
                    provided_agents: encode_agent_infos([
                        discovered_agent_1.agent_info.clone(),
                        discovered_agent_2.agent_info.clone(),
                    ])
                    .unwrap(),
                },
            )
            .await
            .unwrap_err();

        assert!(
            matches!(error, K2GossipError::PeerBehaviorError { .. }),
            "Expected PeerBehavior error for unsolicited Agents message, got: {error:?}"
        );

        // Should not have added any agents to the peer store,
        let all_agents = harness.gossip.peer_store.get_all().await.unwrap();
        assert!(all_agents.is_empty());
    }
}
