use crate::error::{K2GossipError, K2GossipResult};
use crate::gossip::K2Gossip;
use crate::protocol::{
    GossipMessage, K2GossipAgentsMessage, K2GossipRingSectorDetailsDiffMessage,
    K2GossipRingSectorDetailsDiffResponseMessage, K2GossipTerminateMessage,
    encode_agent_infos, encode_op_ids,
};
use crate::state::{
    GossipRoundState, RoundStage, RoundStageAccepted,
    RoundStageRingSectorDetailsDiff,
};
use kitsune2_api::{AgentId, Url};
use kitsune2_dht::DhtSnapshot;
use kitsune2_dht::DhtSnapshotNextAction;
use tokio::sync::OwnedMutexGuard;

impl K2Gossip {
    pub(super) async fn respond_to_ring_sector_details_diff(
        &self,
        from_peer: Url,
        ring_sector_details_diff: K2GossipRingSectorDetailsDiffMessage,
    ) -> K2GossipResult<Option<GossipMessage>> {
        let (mut state, accepted) = self
            .check_ring_sector_details_diff_state(
                from_peer.clone(),
                &ring_sector_details_diff,
            )
            .await?;

        let accept_response_message =
            ring_sector_details_diff.accept_response.unwrap();
        self.handle_accept_response(
            &from_peer,
            accept_response_message.clone(),
        )
        .await?;

        let their_snapshot: DhtSnapshot = ring_sector_details_diff
            .snapshot
            .expect(
                "Snapshot present checked by validate_ring_sector_details_diff",
            )
            .try_into()?;

        let (next_action, used_bytes) = self
            .dht
            .read()
            .await
            .handle_snapshot(
                their_snapshot.clone(),
                None,
                accepted.common_arc_set.clone(),
                state.peer_max_op_data_bytes,
            )
            .await?;

        state.peer_max_op_data_bytes -= used_bytes as i32;

        self.update_storage_arcs(
            &next_action,
            &their_snapshot,
            accepted.common_arc_set.clone(),
        )
        .await?;

        match next_action {
            DhtSnapshotNextAction::CannotCompare
            | DhtSnapshotNextAction::Identical => {
                tracing::info!(
                    ?ring_sector_details_diff.session_id,
                    "Received a ring sector details diff but no diff to send back, responding with agents"
                );

                // Terminating the session, so remove the state.
                self.accepted_round_states.write().await.remove(&from_peer);

                let send_agents = self
                    .load_agent_infos(accept_response_message.missing_agents)
                    .await;
                if !send_agents.is_empty() {
                    Ok(Some(GossipMessage::Agents(K2GossipAgentsMessage {
                        session_id: ring_sector_details_diff.session_id,
                        provided_agents: encode_agent_infos(send_agents)?,
                    })))
                } else {
                    Ok(Some(GossipMessage::Terminate(
                        K2GossipTerminateMessage {
                            session_id: ring_sector_details_diff.session_id,
                            reason: "Nothing to compare and no agents"
                                .to_string(),
                        },
                    )))
                }
            }
            DhtSnapshotNextAction::NewSnapshotAndHashList(snapshot, op_ids) => {
                state.stage = RoundStage::RingSectorDetailsDiff(
                    RoundStageRingSectorDetailsDiff {
                        common_arc_set: accepted.common_arc_set.clone(),
                        snapshot: snapshot.clone(),
                    },
                );

                Ok(Some(GossipMessage::RingSectorDetailsDiffResponse(
                    K2GossipRingSectorDetailsDiffResponseMessage {
                        session_id: ring_sector_details_diff.session_id,
                        missing_ids: encode_op_ids(op_ids),
                        snapshot: Some(snapshot.try_into()?),
                    },
                )))
            }
            a => {
                tracing::error!(?ring_sector_details_diff.session_id, "Unexpected next action: {:?}", a);

                // Remove round state
                self.accepted_round_states.write().await.remove(&from_peer);

                Ok(Some(GossipMessage::Terminate(K2GossipTerminateMessage {
                    session_id: ring_sector_details_diff.session_id,
                    reason: "Unexpected next action".to_string(),
                })))
            }
        }
    }

    async fn check_ring_sector_details_diff_state(
        &self,
        from_peer: Url,
        ring_sector_details_diff: &K2GossipRingSectorDetailsDiffMessage,
    ) -> K2GossipResult<(OwnedMutexGuard<GossipRoundState>, RoundStageAccepted)>
    {
        match self.accepted_round_states.read().await.get(&from_peer) {
            Some(state) => {
                let state = state.clone().lock_owned().await;
                let out = state
                    .validate_ring_sector_details_diff(
                        from_peer.clone(),
                        ring_sector_details_diff,
                    )?
                    .clone();

                Ok((state, out))
            }
            None => Err(K2GossipError::peer_behavior(format!(
                "Unsolicited RingSectorDetailsDiff message from peer: {from_peer:?}"
            ))),
        }
    }
}

impl GossipRoundState {
    fn validate_ring_sector_details_diff(
        &self,
        from_peer: Url,
        ring_sector_details_diff: &K2GossipRingSectorDetailsDiffMessage,
    ) -> K2GossipResult<&RoundStageAccepted> {
        if self.session_with_peer != from_peer {
            return Err(K2GossipError::peer_behavior(format!(
                "RingSectorDetailsDiff message from wrong peer: {} != {}",
                self.session_with_peer, from_peer
            )));
        }

        if self.session_id != ring_sector_details_diff.session_id {
            return Err(K2GossipError::peer_behavior(format!(
                "Session id mismatch: {:?} != {:?}",
                self.session_id, ring_sector_details_diff.session_id
            )));
        }

        let Some(snapshot) = &ring_sector_details_diff.snapshot else {
            return Err(K2GossipError::peer_behavior(
                "Received RingSectorDetailsDiff message without snapshot",
            ));
        };

        match &self.stage {
            RoundStage::Accepted(
                stage @ RoundStageAccepted {
                    our_agents,
                    common_arc_set,
                    ..
                },
            ) => {
                let Some(accept_response) =
                    &ring_sector_details_diff.accept_response
                else {
                    return Err(K2GossipError::peer_behavior(
                        "Received RingSectorDetailsDiff message without accept response",
                    ));
                };

                if accept_response
                    .missing_agents
                    .iter()
                    .any(|a| !our_agents.contains(&AgentId::from(a.clone())))
                {
                    return Err(K2GossipError::peer_behavior(
                        "RingSectorDetailsDiff message contains agents that we didn't declare",
                    ));
                }

                for sector in snapshot
                    .ring_sector_hashes
                    .iter()
                    .flat_map(|sh| sh.sector_indices.iter())
                {
                    if !common_arc_set.includes_sector_index(*sector) {
                        return Err(K2GossipError::peer_behavior(
                            "RingSectorDetailsDiff message contains sector that isn't in the common arc set",
                        ));
                    }
                }

                Ok(stage)
            }
            stage => Err(K2GossipError::peer_behavior(format!(
                "Unexpected round state for ring sector details diff: Accepted != {stage:?}"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::K2GossipConfig;
    use crate::protocol::{
        AcceptResponseMessage, GossipMessage,
        K2GossipRingSectorDetailsDiffMessage, RingSectorHashes,
        SnapshotRingSectorDetailsMessage,
    };
    use crate::respond::harness::RespondTestHarness;
    use crate::state::{RoundStage, RoundStageAccepted};
    use bytes::Bytes;
    use kitsune2_api::{DhtArc, Gossip, OpId, Timestamp, decode_ids};
    use kitsune2_core::factories::MemoryOp;
    use kitsune2_dht::{ArcSet, DhtSnapshot, SECTOR_SIZE};
    use kitsune2_test_utils::enable_tracing;

    #[tokio::test]
    async fn respect_size_limit() {
        enable_tracing();

        let max_ops_per_round = 3usize;
        let available_ops = 5;
        let op_size = 128usize;

        let harness = RespondTestHarness::create_with_config(K2GossipConfig {
            max_gossip_op_bytes: (max_ops_per_round * op_size) as u32,
            ..Default::default()
        })
        .await;

        let local_agent = harness.create_agent(DhtArc::FULL).await;
        harness
            .gossip
            .local_agent_store
            .add(local_agent.local.clone())
            .await
            .unwrap();

        let remote_agent = harness.create_agent(DhtArc::FULL).await;

        let session_id = harness
            .insert_accepted_round_state(&local_agent, &remote_agent)
            .await;
        {
            let accepted = harness.gossip.accepted_round_states.read().await;
            let mut round_state = accepted
                .get(remote_agent.url.as_ref().unwrap())
                .unwrap()
                .lock()
                .await;
            round_state.stage = RoundStage::Accepted(RoundStageAccepted {
                our_agents: vec![local_agent.agent.clone()],
                common_arc_set: ArcSet::new(vec![DhtArc::FULL]).unwrap(),
            });
            round_state.peer_max_op_data_bytes =
                harness.gossip.config.max_gossip_op_bytes as i32;
        }

        let disc_boundary = match harness
            .gossip
            .dht
            .read()
            .await
            .snapshot_minimal(ArcSet::new(vec![DhtArc::FULL]).unwrap())
            .await
            .unwrap()
        {
            DhtSnapshot::Minimal { disc_boundary, .. } => disc_boundary,
            _ => panic!("Expected a minimal snapshot"),
        };

        let mut ops = Vec::new();
        for i in 0u8..available_ops {
            let mut op_data = (i as u32 * SECTOR_SIZE).to_le_bytes().to_vec();
            op_data.resize(op_size, 0);

            let op = MemoryOp::new(
                // Ops created just after the disc boundary so that they'll be in the first ring
                disc_boundary + std::time::Duration::from_secs(30 + i as u64),
                op_data,
            );
            ops.push(op);
        }

        harness
            .gossip
            .op_store
            .process_incoming_ops(
                ops.clone().into_iter().map(Into::into).collect(),
            )
            .await
            .unwrap();
        harness
            .gossip
            .inform_ops_stored(
                ops.clone().into_iter().map(Into::into).collect(),
            )
            .await
            .unwrap();

        let diff_response = harness
            .gossip
            .respond_to_ring_sector_details_diff(
                remote_agent.url.clone().unwrap(),
                K2GossipRingSectorDetailsDiffMessage {
                    session_id,
                    accept_response: Some(AcceptResponseMessage {
                        missing_agents: vec![],
                        provided_agents: vec![],
                        new_ops: vec![],
                        updated_new_since: Timestamp::now().as_micros(),
                    }),
                    snapshot: Some(SnapshotRingSectorDetailsMessage {
                        disc_boundary: disc_boundary.as_micros(),
                        ring_indices: vec![0],
                        ring_sector_hashes: vec![RingSectorHashes {
                            sector_indices: vec![0, 1, 2, 3, 4],
                            hashes: vec![
                                Bytes::from_static(b"hash1"),
                                Bytes::from_static(b"hash2"),
                                Bytes::from_static(b"hash3"),
                                Bytes::from_static(b"hash4"),
                                Bytes::from_static(b"hash5"),
                            ],
                        }],
                    }),
                },
            )
            .await
            .unwrap();

        assert!(
            diff_response.is_some(),
            "Should have responded to the diff message"
        );
        let diff_response = diff_response.unwrap();
        let diff_response = match diff_response {
            GossipMessage::RingSectorDetailsDiffResponse(diff_details) => {
                diff_details
            }
            _ => panic!(
                "Expected a RingSectorDetailsDiffResponse message, got: {diff_response:?}"
            ),
        };
        let sent_ops = decode_ids::<OpId>(diff_response.missing_ids);
        assert_eq!(
            max_ops_per_round,
            sent_ops.len(),
            "Should have sent only 3 ops due to size limit"
        );

        let remaining_budget = harness
            .gossip
            .accepted_round_states
            .read()
            .await
            .get(remote_agent.url.as_ref().unwrap())
            .unwrap()
            .lock()
            .await
            .peer_max_op_data_bytes;
        assert_eq!(
            0, remaining_budget,
            "Should have used up the entire budget for the response"
        );
    }
}
