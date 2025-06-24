use crate::error::{K2GossipError, K2GossipResult};
use crate::gossip::K2Gossip;
use crate::protocol::K2GossipBusyMessage;
use crate::state::GossipRoundState;
use kitsune2_api::{K2Error, Timestamp, Url};

impl K2Gossip {
    pub(crate) async fn respond_to_busy(
        &self,
        from_peer: Url,
        busy: K2GossipBusyMessage,
    ) -> K2GossipResult<()> {
        self.check_busy_state_and_remove(&from_peer, busy).await?;

        self.peer_meta_store
            .incr_peer_busy(from_peer.clone())
            .await?;

        // Mark that we've gossiped with this peer recently. We haven't been successful, but we've
        // tried, and we shouldn't immediately try again because they were busy.
        self.peer_meta_store
            .set_last_gossip_timestamp(from_peer, Timestamp::now())
            .await?;

        Ok(())
    }

    async fn check_busy_state_and_remove(
        &self,
        from_peer: &Url,
        busy: K2GossipBusyMessage,
    ) -> K2GossipResult<()> {
        let mut round_state = self.initiated_round_state.lock().await;
        match round_state.as_ref() {
            Some(state) => {
                state.validate_busy(from_peer.clone(), busy)?;
            }
            None => {
                return Err(K2GossipError::peer_behavior(
                    "Unsolicited Busy message",
                ));
            }
        };

        round_state.take();

        Ok(())
    }
}

impl GossipRoundState {
    fn validate_busy(
        &self,
        from_peer: Url,
        accept: K2GossipBusyMessage,
    ) -> K2GossipResult<()> {
        if self.session_with_peer != from_peer {
            return Err(K2Error::other("Busy message from wrong peer").into());
        }

        if self.session_id != accept.session_id {
            return Err(K2GossipError::peer_behavior(
                "Busy message with wrong session id",
            ));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::error::K2GossipError;
    use crate::protocol::K2GossipBusyMessage;
    use crate::respond::harness::{test_session_id, RespondTestHarness};
    use kitsune2_api::DhtArc;
    use kitsune2_test_utils::enable_tracing;

    #[tokio::test]
    async fn remove_session_on_busy() {
        enable_tracing();

        let harness = RespondTestHarness::create().await;

        let local_agent = harness.create_agent(DhtArc::FULL).await;
        let remote_agent = harness.create_agent(DhtArc::FULL).await;

        let session_id = harness
            .insert_initiated_round_state(&local_agent, &remote_agent)
            .await;

        let response = harness
            .gossip
            .respond_to_busy(
                remote_agent.url.clone().unwrap(),
                K2GossipBusyMessage { session_id },
            )
            .await;

        assert!(response.is_ok());

        let busy = harness
            .gossip
            .peer_meta_store
            .peer_busy(remote_agent.url.clone().unwrap())
            .await
            .unwrap();
        assert!(busy.is_some());
        assert_eq!(busy.unwrap(), 1);

        assert!(
            harness.gossip.initiated_round_state.lock().await.is_none(),
            "Expected initiated round state to be removed after busy response"
        );
    }

    #[tokio::test]
    async fn busy_from_wrong_peer() {
        enable_tracing();

        let harness = RespondTestHarness::create().await;

        let local_agent = harness.create_agent(DhtArc::FULL).await;
        let remote_agent = harness.create_agent(DhtArc::FULL).await;

        let session_id = harness
            .insert_initiated_round_state(&local_agent, &remote_agent)
            .await;

        let response = harness
            .gossip
            .respond_to_busy(
                harness
                    .create_agent(DhtArc::Empty)
                    .await
                    .url
                    .clone()
                    .unwrap(),
                K2GossipBusyMessage { session_id },
            )
            .await;

        let error = response.unwrap_err();
        assert!(
            error.to_string().contains("Busy message from wrong peer"),
            "Expected error for busy message from wrong peer, got: {}",
            error
        );

        let busy = harness
            .gossip
            .peer_meta_store
            .peer_busy(remote_agent.url.clone().unwrap())
            .await
            .unwrap();
        assert!(busy.is_none());
    }

    #[tokio::test]
    async fn busy_with_mismatched_session_id() {
        enable_tracing();

        let harness = RespondTestHarness::create().await;

        let local_agent = harness.create_agent(DhtArc::FULL).await;
        let remote_agent = harness.create_agent(DhtArc::FULL).await;

        harness
            .insert_initiated_round_state(&local_agent, &remote_agent)
            .await;

        let response = harness
            .gossip
            .respond_to_busy(
                remote_agent.url.clone().unwrap(),
                K2GossipBusyMessage {
                    session_id: test_session_id(),
                },
            )
            .await;

        let error = response.unwrap_err();
        assert!(
            error
                .to_string()
                .contains(" Busy message with wrong session id"),
            "Expected error for busy message with wrong session id, got: {}",
            error
        );

        let busy = harness
            .gossip
            .peer_meta_store
            .peer_busy(remote_agent.url.clone().unwrap())
            .await
            .unwrap();
        assert!(busy.is_none());
    }

    #[tokio::test]
    async fn busy_with_no_initiated_session() {
        enable_tracing();

        let harness = RespondTestHarness::create().await;

        let remote_agent = harness.create_agent(DhtArc::FULL).await;

        let response = harness
            .gossip
            .respond_to_busy(
                remote_agent.url.clone().unwrap(),
                K2GossipBusyMessage {
                    session_id: test_session_id(),
                },
            )
            .await;

        let error = response.unwrap_err();
        assert!(
            matches!(error, K2GossipError::PeerBehaviorError { .. }),
            "Expected PeerBehaviorError for unsolicited Busy message, got: {}",
            error
        );
    }
}
