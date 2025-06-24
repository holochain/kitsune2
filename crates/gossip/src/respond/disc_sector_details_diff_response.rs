use crate::error::{K2GossipError, K2GossipResult};
use crate::gossip::K2Gossip;
use crate::protocol::{
    encode_op_ids, GossipMessage, K2GossipDiscSectorDetailsDiffResponseMessage,
    K2GossipHashesMessage, K2GossipTerminateMessage,
};
use crate::state::{
    GossipRoundState, RoundStage, RoundStageDiscSectorDetailsDiff,
};
use kitsune2_api::decode_ids;
use kitsune2_api::{K2Error, Url};
use kitsune2_dht::DhtSnapshotNextAction;
use tokio::sync::OwnedMutexGuard;

impl K2Gossip {
    pub(super) async fn respond_to_disc_sector_details_diff_response(
        &self,
        from_peer: Url,
        response: K2GossipDiscSectorDetailsDiffResponseMessage,
    ) -> K2GossipResult<Option<GossipMessage>> {
        let (mut state, disc_sector_details) = self
            .check_disc_sectors_diff_response_state(
                from_peer.clone(),
                &response,
            )
            .await?;

        self.fetch
            .request_ops(decode_ids(response.missing_ids), from_peer.clone())
            .await?;

        let their_snapshot = response
            .snapshot
            .expect(
                "Snapshot present checked by validate_disc_sector_details_diff",
            )
            .try_into()?;

        let (next_action, used_bytes) = self
            .dht
            .read()
            .await
            .handle_snapshot(
                their_snapshot,
                Some(disc_sector_details.snapshot.clone()),
                disc_sector_details.common_arc_set,
                state.peer_max_op_data_bytes,
            )
            .await?;

        tracing::debug!(
            ?response.session_id,
            "Used {}/{} op budget to send disc ops",
            used_bytes,
            state.peer_max_op_data_bytes,
        );
        state.peer_max_op_data_bytes -= used_bytes as i32;

        match next_action {
            DhtSnapshotNextAction::CannotCompare
            | DhtSnapshotNextAction::Identical => {
                tracing::info!(
                    ?response.session_id,
                    "Received a disc sector details diff response that we can't respond to, terminating gossip round"
                );

                // Terminating the session, so remove the state.
                self.accepted_round_states.write().await.remove(&from_peer);

                Ok(Some(GossipMessage::Terminate(K2GossipTerminateMessage {
                    session_id: response.session_id,
                    reason: "Nothing to compare".to_string(),
                })))
            }
            DhtSnapshotNextAction::HashList(op_ids) => {
                // This is the final message we're going to send, remove state
                self.accepted_round_states.write().await.remove(&from_peer);

                Ok(Some(GossipMessage::Hashes(K2GossipHashesMessage {
                    session_id: response.session_id,
                    missing_ids: encode_op_ids(op_ids),
                })))
            }
            a => {
                tracing::error!(?response.session_id, "Unexpected next action: {:?}", a);

                // Terminating the session, so remove the state.
                self.accepted_round_states.write().await.remove(&from_peer);

                Ok(Some(GossipMessage::Terminate(K2GossipTerminateMessage {
                    session_id: response.session_id,
                    reason: "Unexpected next action".to_string(),
                })))
            }
        }
    }

    async fn check_disc_sectors_diff_response_state(
        &self,
        from_peer: Url,
        disc_sector_details_diff_response: &K2GossipDiscSectorDetailsDiffResponseMessage,
    ) -> K2GossipResult<(
        OwnedMutexGuard<GossipRoundState>,
        RoundStageDiscSectorDetailsDiff,
    )> {
        match self.accepted_round_states.read().await.get(&from_peer) {
            Some(state) => {
                let state = state.clone().lock_owned().await;
                let disc_sector_details = state
                    .validate_disc_sector_details_diff_response(
                        from_peer.clone(),
                        disc_sector_details_diff_response,
                    )?
                    .clone();

                Ok((state, disc_sector_details))
            }
            None => Err(K2GossipError::peer_behavior(format!(
                "Unsolicited DiscSectorDetailsDiffResponse message from peer: {:?}",
                from_peer
            ))),
        }
    }
}

impl GossipRoundState {
    fn validate_disc_sector_details_diff_response(
        &self,
        from_peer: Url,
        disc_sector_details_diff: &K2GossipDiscSectorDetailsDiffResponseMessage,
    ) -> K2GossipResult<&RoundStageDiscSectorDetailsDiff> {
        if self.session_with_peer != from_peer {
            return Err(K2Error::other(format!(
                "DiscSectorDetailsDiffResponse message from wrong peer: {} != {}",
                self.session_with_peer, from_peer
            )).into());
        }

        if self.session_id != disc_sector_details_diff.session_id {
            return Err(K2GossipError::peer_behavior(format!(
                "Session id mismatch: {:?} != {:?}",
                self.session_id, disc_sector_details_diff.session_id
            )));
        }

        let Some(snapshot) = &disc_sector_details_diff.snapshot else {
            return Err(K2GossipError::peer_behavior(
                "Received DiscSectorDetailsDiffResponse message without snapshot",
            ));
        };

        match &self.stage {
            RoundStage::DiscSectorDetailsDiff(
                state @ RoundStageDiscSectorDetailsDiff {
                    common_arc_set, ..
                },
            ) => {
                for sector in &snapshot.sector_indices {
                    if !common_arc_set.includes_sector_index(*sector) {
                        return Err(K2GossipError::peer_behavior(
                            "DiscSectorDetailsDiffResponse message contains sector that isn't in the common arc set",
                        ));
                    }
                }

                Ok(state)
            }
            stage => Err(K2GossipError::peer_behavior(format!(
                "Unexpected round state for disc sector details diff response: DiscSectorDetailsDiff != {:?}",
                stage
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::protocol::{
        DiscSliceHashes, GossipMessage,
        K2GossipDiscSectorDetailsDiffResponseMessage,
        SnapshotDiscSectorDetailsMessage,
    };
    use crate::respond::harness::RespondTestHarness;
    use crate::state::{RoundStage, RoundStageDiscSectorDetailsDiff};
    use crate::K2GossipConfig;
    use bytes::Bytes;
    use kitsune2_api::{decode_ids, DhtArc, Gossip, OpId, Timestamp};
    use kitsune2_core::factories::MemoryOp;
    use kitsune2_dht::{ArcSet, DhtSnapshot, SECTOR_SIZE};
    use kitsune2_test_utils::enable_tracing;
    use std::collections::HashMap;

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
            round_state.stage = RoundStage::DiscSectorDetailsDiff(
                RoundStageDiscSectorDetailsDiff {
                    common_arc_set: ArcSet::new(vec![DhtArc::FULL]).unwrap(),
                    snapshot: DhtSnapshot::DiscSectorDetails {
                        disc_boundary,
                        disc_sector_hashes: HashMap::new(),
                    },
                },
            );
            round_state.peer_max_op_data_bytes =
                harness.gossip.config.max_gossip_op_bytes as i32;
        }

        let mut ops = Vec::new();
        for i in 0u8..available_ops {
            let mut op_data = (i as u32 * SECTOR_SIZE).to_le_bytes().to_vec();
            op_data.resize(128, 0);

            let op =
                MemoryOp::new(Timestamp::from_micros(100 + i as i64), op_data);
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

        let diff_details = harness
            .gossip
            .respond_to_disc_sector_details_diff_response(
                remote_agent.url.clone().unwrap(),
                K2GossipDiscSectorDetailsDiffResponseMessage {
                    session_id,
                    missing_ids: vec![],
                    snapshot: Some(SnapshotDiscSectorDetailsMessage {
                        disc_boundary: disc_boundary.as_micros(),
                        sector_indices: vec![0, 1, 2, 3, 4],
                        disc_slice_hashes: vec![
                            DiscSliceHashes {
                                slice_indices: vec![0],
                                hashes: vec![Bytes::from_static(b"hash1")],
                            },
                            DiscSliceHashes {
                                slice_indices: vec![0],
                                hashes: vec![Bytes::from_static(b"hash2")],
                            },
                            DiscSliceHashes {
                                slice_indices: vec![0],
                                hashes: vec![Bytes::from_static(b"hash3")],
                            },
                            DiscSliceHashes {
                                slice_indices: vec![0],
                                hashes: vec![Bytes::from_static(b"hash4")],
                            },
                            DiscSliceHashes {
                                slice_indices: vec![0],
                                hashes: vec![Bytes::from_static(b"hash5")],
                            },
                        ],
                    }),
                },
            )
            .await
            .unwrap();

        assert!(
            diff_details.is_some(),
            "Should have responded to the diff message"
        );
        let diff_details = diff_details.unwrap();
        let diff_details = match diff_details {
            GossipMessage::Hashes(diff_details) => diff_details,
            _ => panic!("Expected a Hashes message, got: {:?}", diff_details),
        };
        let sent_ops = decode_ids::<OpId>(diff_details.missing_ids);
        assert_eq!(
            max_ops_per_round,
            sent_ops.len(),
            "Should have sent only 3 ops due to size limit"
        );

        // End of the round, so the state should be removed
        assert!(!harness
            .gossip
            .accepted_round_states
            .read()
            .await
            .contains_key(remote_agent.url.as_ref().unwrap()));
    }
}
