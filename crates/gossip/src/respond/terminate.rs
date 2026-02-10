use crate::error::{K2GossipError, K2GossipResult};
use crate::gossip::K2Gossip;
use crate::protocol::{GossipMessage, K2GossipTerminateMessage};
use kitsune2_api::Url;

impl K2Gossip {
    pub(super) async fn respond_to_terminate(
        &self,
        from_peer: Url,
        terminate: K2GossipTerminateMessage,
    ) -> K2GossipResult<Option<GossipMessage>> {
        tracing::info!(
            ?terminate.session_id,
            "Peer {from_peer} is attempting to terminate gossip session with reason: {}",
            terminate.reason
        );

        let terminated_initiated_session = {
            let mut initiate_lock = self.initiated_round_state.lock().await;
            if let Some(state) = initiate_lock.as_ref() {
                if state.session_with_peer == from_peer {
                    if state.session_id == terminate.session_id {
                        initiate_lock.take();
                    } else {
                        initiate_lock.take();
                        return Err(K2GossipError::peer_behavior(format!(
                            "Unsolicited terminate message from: {from_peer}"
                        )));
                    }

                    true
                } else {
                    false
                }
            } else {
                false
            }
        };

        let terminated_accepted_session = if !terminated_initiated_session {
            let read_guard = self.accepted_round_states.read().await;
            let accepted = read_guard.get(&from_peer);

            if let Some(state) = accepted {
                let state = state.clone();

                let session_id = state.lock().await.session_id.clone();

                drop(read_guard);
                self.accepted_round_states.write().await.remove(&from_peer);

                if session_id != terminate.session_id {
                    return Err(K2GossipError::peer_behavior(format!(
                        "Unsolicited terminate message from: {from_peer}"
                    )));
                }

                true
            } else {
                false
            }
        } else {
            false
        };

        if terminated_initiated_session || terminated_accepted_session {
            self.peer_meta_store.incr_peer_terminated(from_peer).await?;
        } else {
            return Err(K2GossipError::peer_behavior(format!(
                "Unsolicited termination message from: {from_peer}"
            )));
        }

        // If there was no error then always respond with no message, the other side terminated
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use crate::protocol::{
        ArcSetMessage, GossipMessage, K2GossipInitiateMessage,
        K2GossipTerminateMessage, encode_agent_ids,
    };
    use crate::respond::harness::{RespondTestHarness, test_session_id};
    use kitsune2_api::DhtArc;
    use kitsune2_dht::ArcSet;
    use kitsune2_test_utils::enable_tracing;

    #[tokio::test]
    async fn reject_message_that_matches_nothing() {
        enable_tracing();

        let harness = RespondTestHarness::create().await;

        let remote_agent = harness.create_agent(DhtArc::Empty).await;

        let response = harness
            .gossip
            .respond_to_terminate(
                remote_agent.url.clone().unwrap(),
                K2GossipTerminateMessage {
                    session_id: test_session_id(),
                    reason: "Test".to_string(),
                },
            )
            .await;

        assert!(response.is_err());
        let err = response.unwrap_err();

        assert!(
            err.to_string()
                .contains("Unsolicited termination message from")
        );
    }

    #[tokio::test]
    async fn terminate_initiated_session() {
        enable_tracing();

        let mut harness = RespondTestHarness::create().await;

        let remote_agent = harness.create_agent(DhtArc::Empty).await;

        // Initiate a session with the remote agent
        let initiated = harness
            .gossip
            .initiate_gossip(remote_agent.url.clone().unwrap())
            .await
            .unwrap();
        assert!(initiated);

        // Wait for us to send the initiate message
        let response = harness.wait_for_sent_response().await;
        let initiate = match response {
            GossipMessage::Initiate(initiate) => initiate,
            other => panic!("Expected initiate message, got: {other:?}"),
        };

        {
            assert!(
                harness.gossip.initiated_round_state.lock().await.is_some(),
                "Session should be initiated"
            );
        }

        // Now simulate receiving a terminate message
        let response = harness
            .gossip
            .respond_to_terminate(
                remote_agent.url.clone().unwrap(),
                K2GossipTerminateMessage {
                    session_id: initiate.session_id,
                    reason: "Test".to_string(),
                },
            )
            .await;

        // We should accept the terminate but not send a response
        assert!(response.is_ok(), "Wanted ok response, got: {response:?}");
        assert!(response.unwrap().is_none());

        // Check that the peer terminated count was incremented
        let peer_terminated_count = harness
            .gossip
            .peer_meta_store
            .peer_terminated(remote_agent.url.clone().unwrap())
            .await
            .unwrap();
        assert_eq!(Some(1), peer_terminated_count);

        // Check that the session is no longer in the initiated state
        let initiated = harness.gossip.initiated_round_state.lock().await;
        assert!(initiated.is_none(), "Session should be terminated");
    }

    #[tokio::test]
    async fn attempt_terminate_initiated_session_from_wrong_peer() {
        enable_tracing();

        let mut harness = RespondTestHarness::create().await;

        let remote_agent = harness.create_agent(DhtArc::Empty).await;

        // Initiate a session with the remote agent
        let initiated = harness
            .gossip
            .initiate_gossip(remote_agent.url.clone().unwrap())
            .await
            .unwrap();
        assert!(initiated);

        // Wait for us to send the initiate message
        let response = harness.wait_for_sent_response().await;
        match response {
            GossipMessage::Initiate(_) => {}
            other => panic!("Expected initiate message, got: {other:?}"),
        };

        // Now simulate receiving a terminate message from some other peer
        let other_agent = harness.create_agent(DhtArc::Empty).await;
        let response = harness
            .gossip
            .respond_to_terminate(
                other_agent.url.clone().unwrap(),
                K2GossipTerminateMessage {
                    session_id: test_session_id(),
                    reason: "Test".to_string(),
                },
            )
            .await;

        // We should reject the termination
        assert!(
            response.is_err(),
            "Wanted error response, got: {response:?}"
        );
        let err = response.unwrap_err();
        assert!(
            err.to_string()
                .contains("Unsolicited termination message from")
        );

        // Check that the session is still in the initiated state
        let initiated = harness.gossip.initiated_round_state.lock().await;
        assert!(initiated.is_some(), "Session should still be initiated");
    }

    #[tokio::test]
    async fn terminate_initiated_session_with_wrong_session_id() {
        enable_tracing();

        let mut harness = RespondTestHarness::create().await;

        let remote_agent = harness.create_agent(DhtArc::Empty).await;

        // Initiate a session with the remote agent
        let initiated = harness
            .gossip
            .initiate_gossip(remote_agent.url.clone().unwrap())
            .await
            .unwrap();
        assert!(initiated);

        // Wait for us to send the initiate message
        let response = harness.wait_for_sent_response().await;
        match response {
            GossipMessage::Initiate(_) => {}
            other => panic!("Expected initiate message, got: {other:?}"),
        };

        // Now simulate receiving a terminate message with a different session id
        let response = harness
            .gossip
            .respond_to_terminate(
                remote_agent.url.clone().unwrap(),
                K2GossipTerminateMessage {
                    session_id: test_session_id(),
                    reason: "Test".to_string(),
                },
            )
            .await;

        // We should accept the terminate and send an error response
        assert!(
            response.is_err(),
            "Wanted error response, got: {response:?}"
        );
        let err = response.unwrap_err();
        assert!(
            err.to_string()
                .contains("Unsolicited terminate message from"),
            "Error is: {err:?}"
        );

        // Check that the session is no longer in the initiated state
        let initiated = harness.gossip.initiated_round_state.lock().await;
        assert!(initiated.is_none(), "Session should be terminated");
    }

    #[tokio::test]
    async fn terminate_accepted_session() {
        enable_tracing();

        let mut harness = RespondTestHarness::create().await;

        let remote_agent = harness.create_agent(DhtArc::Empty).await;

        // Simulate receiving an initiate message from the remote agent
        harness
            .gossip
            .respond_to_msg(
                remote_agent.url.clone().unwrap(),
                GossipMessage::Initiate(K2GossipInitiateMessage {
                    session_id: test_session_id(),
                    participating_agents: encode_agent_ids([remote_agent
                        .agent
                        .clone()]),
                    arc_set: Some(ArcSetMessage {
                        value: ArcSet::new(vec![
                            remote_agent.local.get_tgt_storage_arc(),
                        ])
                        .unwrap()
                        .encode(),
                    }),
                    tie_breaker: 0,
                    new_since: 0,
                    max_op_data_bytes: 30_000,
                }),
            )
            .await
            .unwrap();

        // Check that we sent an accept message
        let our_accept = harness.wait_for_sent_response().await;
        let our_accept = match our_accept {
            GossipMessage::Accept(accept) => accept,
            other => panic!("Expected accept message, got: {other:?}"),
        };

        {
            assert!(
                harness
                    .gossip
                    .accepted_round_states
                    .read()
                    .await
                    .contains_key(&remote_agent.url.clone().unwrap()),
                "Session should be accepted"
            );
            tracing::info!("Checked that we accepted the session");
        }

        // Now simulate receiving a terminate message from the same peer
        let response = harness
            .gossip
            .respond_to_terminate(
                remote_agent.url.clone().unwrap(),
                K2GossipTerminateMessage {
                    session_id: our_accept.session_id,
                    reason: "Test".to_string(),
                },
            )
            .await;

        assert!(response.is_ok(), "Wanted ok response, got: {response:?}");
        assert!(response.unwrap().is_none());

        // Check that the peer terminated count was incremented
        let peer_terminated_count = harness
            .gossip
            .peer_meta_store
            .peer_terminated(remote_agent.url.clone().unwrap())
            .await
            .unwrap();
        assert_eq!(Some(1), peer_terminated_count);

        // Check that the session is no longer in the accepted state
        let accepted = harness
            .gossip
            .accepted_round_states
            .read()
            .await
            .get(&remote_agent.url.clone().unwrap())
            .cloned();
        assert!(accepted.is_none(), "Session should be terminated");
    }

    #[tokio::test]
    async fn terminate_accepted_session_with_wrong_session_id() {
        enable_tracing();

        let mut harness = RespondTestHarness::create().await;

        let remote_agent = harness.create_agent(DhtArc::Empty).await;

        // Simulate receiving an initiate message from the remote agent
        harness
            .gossip
            .respond_to_msg(
                remote_agent.url.clone().unwrap(),
                GossipMessage::Initiate(K2GossipInitiateMessage {
                    session_id: test_session_id(),
                    participating_agents: encode_agent_ids([remote_agent
                        .agent
                        .clone()]),
                    arc_set: Some(ArcSetMessage {
                        value: ArcSet::new(vec![
                            remote_agent.local.get_tgt_storage_arc(),
                        ])
                        .unwrap()
                        .encode(),
                    }),
                    tie_breaker: 0,
                    new_since: 0,
                    max_op_data_bytes: 30_000,
                }),
            )
            .await
            .unwrap();

        // Check that we sent an accept message
        let our_accept = harness.wait_for_sent_response().await;
        match our_accept {
            GossipMessage::Accept(_) => {}
            other => panic!("Expected accept message, got: {other:?}"),
        };

        {
            assert!(
                harness
                    .gossip
                    .accepted_round_states
                    .read()
                    .await
                    .contains_key(&remote_agent.url.clone().unwrap()),
                "Session should be accepted"
            );
            tracing::info!("Checked that we accepted the session");
        }

        // Now simulate receiving a terminate message with the wrong session id
        let response = harness
            .gossip
            .respond_to_terminate(
                remote_agent.url.clone().unwrap(),
                K2GossipTerminateMessage {
                    session_id: test_session_id(),
                    reason: "Test".to_string(),
                },
            )
            .await;

        assert!(response.is_err(), "Wanted ok response, got: {response:?}");
        let err = response.unwrap_err();
        assert!(
            err.to_string()
                .contains("Unsolicited terminate message from"),
            "Error is: {err:?}"
        );

        // Check that the session is no longer in the accepted state
        let accepted = harness
            .gossip
            .accepted_round_states
            .read()
            .await
            .get(&remote_agent.url.clone().unwrap())
            .cloned();
        assert!(accepted.is_none(), "Session should be terminated");
    }
}
