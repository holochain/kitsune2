use crate::error::{K2GossipError, K2GossipResult};
use crate::gossip::K2Gossip;
use crate::protocol::{
    encode_op_ids, GossipMessage, K2GossipDiscSectorDetailsDiffMessage,
    K2GossipDiscSectorDetailsDiffResponseMessage, K2GossipTerminateMessage,
};
use crate::state::{
    GossipRoundState, RoundStage, RoundStageDiscSectorDetailsDiff,
    RoundStageDiscSectorsDiff,
};
use kitsune2_api::{K2Error, Url};
use kitsune2_dht::DhtSnapshotNextAction;
use tokio::sync::MutexGuard;

impl K2Gossip {
    pub(super) async fn respond_to_disc_sector_details_diff(
        &self,
        from_peer: Url,
        disc_sector_details_diff: K2GossipDiscSectorDetailsDiffMessage,
    ) -> K2GossipResult<Option<GossipMessage>> {
        // Validate the incoming disc sector details diff against our own state.
        let (mut state, disc_sector_details) = self
            .check_disc_sector_details_diff_state(
                from_peer.clone(),
                &disc_sector_details_diff,
            )
            .await?;

        let their_snapshot = disc_sector_details_diff
            .snapshot
            .expect(
                "Snapshot present checked by validate_disc_sector_details_diff",
            )
            .try_into()?;

        let peer_max_op_data_bytes = state
            .as_ref()
            .map(|s| s.peer_max_op_data_bytes)
            .unwrap_or(0);

        let (next_action, used_bytes) = self
            .dht
            .read()
            .await
            .handle_snapshot(
                their_snapshot,
                None,
                disc_sector_details.common_arc_set.clone(),
                peer_max_op_data_bytes,
            )
            .await?;

        if let Some(state) = state.as_mut() {
            tracing::debug!(
                ?disc_sector_details_diff.session_id,
                "Used {}/{} op budget to send disc ops",
                used_bytes,
                state.peer_max_op_data_bytes,
            );
            state.peer_max_op_data_bytes -= used_bytes as i32;
        }

        match next_action {
            DhtSnapshotNextAction::CannotCompare
            | DhtSnapshotNextAction::Identical => {
                tracing::info!(
                    ?disc_sector_details_diff.session_id,
                    "Received a disc sector details diff but no diff to send back, responding with agents"
                );

                // Terminating the session, so remove the state.
                state.take();

                Ok(Some(GossipMessage::Terminate(K2GossipTerminateMessage {
                    session_id: disc_sector_details_diff.session_id,
                    reason: "Nothing to compare".to_string(),
                })))
            }
            DhtSnapshotNextAction::NewSnapshotAndHashList(snapshot, ops) => {
                if let Some(state) = state.as_mut() {
                    state.stage = RoundStage::DiscSectorDetailsDiff(
                        RoundStageDiscSectorDetailsDiff {
                            common_arc_set: disc_sector_details
                                .common_arc_set
                                .clone(),
                            snapshot: snapshot.clone(),
                        },
                    );
                }

                Ok(Some(GossipMessage::DiscSectorDetailsDiffResponse(
                    K2GossipDiscSectorDetailsDiffResponseMessage {
                        session_id: disc_sector_details_diff.session_id,
                        missing_ids: encode_op_ids(ops),
                        snapshot: Some(snapshot.try_into()?),
                    },
                )))
            }
            _ => {
                tracing::error!(?disc_sector_details_diff.session_id, "Unexpected next action: {:?}", next_action);

                // Remove round state.
                state.take();

                Ok(Some(GossipMessage::Terminate(K2GossipTerminateMessage {
                    session_id: disc_sector_details_diff.session_id,
                    reason: "Unexpected next action".into(),
                })))
            }
        }
    }

    async fn check_disc_sector_details_diff_state<'a>(
        &'a self,
        from_peer: Url,
        disc_sector_details_diff: &K2GossipDiscSectorDetailsDiffMessage,
    ) -> K2GossipResult<(
        MutexGuard<'a, Option<GossipRoundState>>,
        RoundStageDiscSectorsDiff,
    )> {
        let lock = self.initiated_round_state.lock().await;
        let disc_sectors_diff = match lock.as_ref() {
            Some(state) => state
                .validate_disc_sector_details_diff(
                    from_peer.clone(),
                    disc_sector_details_diff,
                )?
                .clone(),
            None => {
                return Err(K2GossipError::peer_behavior(
                    "Unsolicited DiscSectorDetailsDiff message",
                ));
            }
        };

        Ok((lock, disc_sectors_diff))
    }
}

impl GossipRoundState {
    fn validate_disc_sector_details_diff(
        &self,
        from_peer: Url,
        disc_sector_details_diff: &K2GossipDiscSectorDetailsDiffMessage,
    ) -> K2GossipResult<&RoundStageDiscSectorsDiff> {
        if self.session_with_peer != from_peer {
            return Err(K2Error::other(format!(
                "DiscSectorDetailsDiff message from wrong peer: {} != {}",
                self.session_with_peer, from_peer
            ))
            .into());
        }

        if self.session_id != disc_sector_details_diff.session_id {
            return Err(K2GossipError::peer_behavior(format!(
                "Session id mismatch: {:?} != {:?}",
                self.session_id, disc_sector_details_diff.session_id
            )));
        }

        let Some(snapshot) = &disc_sector_details_diff.snapshot else {
            return Err(K2GossipError::peer_behavior(
                "Received DiscSectorDetailsDiff message without snapshot",
            ));
        };

        match &self.stage {
            RoundStage::DiscSectorsDiff(
                stage @ RoundStageDiscSectorsDiff { common_arc_set },
            ) => {
                for sector in &snapshot.sector_indices {
                    if !common_arc_set.includes_sector_index(*sector) {
                        return Err(K2GossipError::peer_behavior(
                            "DiscSectorDetailsDiff message contains sector that isn't in the common arc set",
                        ));
                    }
                }

                Ok(stage)
            }
            stage => Err(K2GossipError::peer_behavior(format!(
                "Unexpected round state for disc sector details diff: DiscSectorsDiff != {stage:?}"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::protocol::{
        DiscSliceHashes, GossipMessage, K2GossipDiscSectorDetailsDiffMessage,
        SnapshotDiscSectorDetailsMessage,
    };
    use crate::respond::harness::RespondTestHarness;
    use crate::state::{RoundStage, RoundStageDiscSectorsDiff};
    use crate::K2GossipConfig;
    use bytes::Bytes;
    use kitsune2_api::{decode_ids, DhtArc, Gossip, OpId, Timestamp};
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
            .insert_initiated_round_state(&local_agent, &remote_agent)
            .await;
        {
            let mut round_state =
                harness.gossip.initiated_round_state.lock().await;
            round_state.as_mut().unwrap().stage =
                RoundStage::DiscSectorsDiff(RoundStageDiscSectorsDiff {
                    common_arc_set: ArcSet::new(vec![DhtArc::FULL]).unwrap(),
                });
            round_state.as_mut().unwrap().peer_max_op_data_bytes =
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
        let diff_details = harness
            .gossip
            .respond_to_disc_sector_details_diff(
                remote_agent.url.clone().unwrap(),
                K2GossipDiscSectorDetailsDiffMessage {
                    session_id,
                    provided_agents: vec![],
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
            GossipMessage::DiscSectorDetailsDiffResponse(diff_details) => {
                diff_details
            }
            _ => panic!(
                "Expected a DiscSectorDetailsDiffResponse message, got: {diff_details:?}"
            ),
        };
        let sent_ops = decode_ids::<OpId>(diff_details.missing_ids);
        assert_eq!(
            max_ops_per_round,
            sent_ops.len(),
            "Should have sent only 3 ops due to size limit"
        );

        let remaining_budget = harness
            .gossip
            .initiated_round_state
            .lock()
            .await
            .as_ref()
            .unwrap()
            .peer_max_op_data_bytes;
        assert_eq!(
            0, remaining_budget,
            "Should have used up the entire budget for the response"
        );
    }
}
