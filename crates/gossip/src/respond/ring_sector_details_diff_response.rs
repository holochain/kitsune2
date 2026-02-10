use crate::error::{K2GossipError, K2GossipResult};
use crate::gossip::K2Gossip;
use crate::protocol::{
    GossipMessage, K2GossipHashesMessage,
    K2GossipRingSectorDetailsDiffResponseMessage, K2GossipTerminateMessage,
    encode_op_ids,
};
use crate::state::{
    GossipRoundState, RoundStage, RoundStageRingSectorDetailsDiff,
};
use kitsune2_api::decode_ids;
use kitsune2_api::{K2Error, Url};
use kitsune2_dht::DhtSnapshot;
use kitsune2_dht::DhtSnapshotNextAction;
use tokio::sync::MutexGuard;

impl K2Gossip {
    pub(super) async fn respond_to_ring_sector_details_diff_response(
        &self,
        from_peer: Url,
        response: K2GossipRingSectorDetailsDiffResponseMessage,
    ) -> K2GossipResult<Option<GossipMessage>> {
        let (mut state, ring_sector_details) = self
            .check_ring_sector_details_diff_response_state(
                from_peer.clone(),
                &response,
            )
            .await?;

        self.fetch
            .request_ops(decode_ids(response.missing_ids), from_peer.clone())
            .await?;

        let their_snapshot: DhtSnapshot =
            response.snapshot.unwrap().try_into()?;

        let peer_max_op_data_bytes = state
            .as_ref()
            .map(|s| s.peer_max_op_data_bytes)
            .unwrap_or(0);

        let (next_action, used_bytes) = self
            .dht
            .read()
            .await
            .handle_snapshot(
                their_snapshot.clone(),
                Some(ring_sector_details.snapshot.clone()),
                ring_sector_details.common_arc_set.clone(),
                peer_max_op_data_bytes,
            )
            .await?;

        if let Some(state) = state.as_mut() {
            state.peer_max_op_data_bytes -= used_bytes as i32;
        }

        self.update_storage_arcs(
            &next_action,
            &their_snapshot,
            ring_sector_details.common_arc_set,
        )
        .await?;

        match next_action {
            DhtSnapshotNextAction::CannotCompare
            | DhtSnapshotNextAction::Identical => {
                tracing::info!(
                    ?response.session_id,
                    "Received a ring sector details diff response that we can't respond to, terminating gossip round"
                );

                // Terminating the session, so remove the state.
                state.take();

                Ok(Some(GossipMessage::Terminate(K2GossipTerminateMessage {
                    session_id: response.session_id,
                    reason: "Nothing to compare".to_string(),
                })))
            }
            DhtSnapshotNextAction::HashList(op_ids) => {
                // This is the final message we're going to send, remove state
                state.take();

                Ok(Some(GossipMessage::Hashes(K2GossipHashesMessage {
                    session_id: response.session_id,
                    missing_ids: encode_op_ids(op_ids),
                })))
            }
            a => {
                tracing::error!(?response.session_id, "Unexpected next action: {:?}", a);

                // Remove the round state.
                state.take();

                Ok(Some(GossipMessage::Terminate(K2GossipTerminateMessage {
                    session_id: response.session_id,
                    reason: "Unexpected next action".to_string(),
                })))
            }
        }
    }

    async fn check_ring_sector_details_diff_response_state<'a>(
        &'a self,
        from_peer: Url,
        ring_sector_details_diff_response: &K2GossipRingSectorDetailsDiffResponseMessage,
    ) -> K2GossipResult<(
        MutexGuard<'a, Option<GossipRoundState>>,
        RoundStageRingSectorDetailsDiff,
    )> {
        let lock = self.initiated_round_state.lock().await;
        let ring_sector_details_diff = match lock.as_ref() {
            Some(state) => state
                .validate_ring_sector_details_diff_response(
                    from_peer.clone(),
                    ring_sector_details_diff_response,
                )?
                .clone(),
            None => {
                return Err(K2GossipError::peer_behavior(
                    "Unsolicited RingSectorDetailsDiffResponse message",
                ));
            }
        };

        Ok((lock, ring_sector_details_diff))
    }
}

impl GossipRoundState {
    fn validate_ring_sector_details_diff_response(
        &self,
        from_peer: Url,
        ring_sector_details_diff_response: &K2GossipRingSectorDetailsDiffResponseMessage,
    ) -> K2GossipResult<&RoundStageRingSectorDetailsDiff> {
        if self.session_with_peer != from_peer {
            return Err(K2Error::other(format!(
                "RingSectorDetailsDiffResponse message from wrong peer: {} != {}",
                self.session_with_peer, from_peer
            )).into());
        }

        if self.session_id != ring_sector_details_diff_response.session_id {
            return Err(K2GossipError::peer_behavior(format!(
                "Session id mismatch: {:?} != {:?}",
                self.session_id, ring_sector_details_diff_response.session_id
            )));
        }

        let Some(snapshot) = &ring_sector_details_diff_response.snapshot else {
            return Err(K2GossipError::peer_behavior(
                "Received RingSectorDetailsDiffResponse message without snapshot",
            ));
        };

        match &self.stage {
            RoundStage::RingSectorDetailsDiff(
                state @ RoundStageRingSectorDetailsDiff {
                    common_arc_set, ..
                },
            ) => {
                for sector in snapshot
                    .ring_sector_hashes
                    .iter()
                    .flat_map(|sh| sh.sector_indices.iter())
                {
                    if !common_arc_set.includes_sector_index(*sector) {
                        return Err(K2GossipError::peer_behavior(
                            "RingSectorDetailsDiffResponse message contains sector that isn't in the common arc set",
                        ));
                    }
                }

                Ok(state)
            }
            stage => Err(K2GossipError::peer_behavior(format!(
                "Unexpected round state for ring sector details diff response: RingSectorDetailsDiff != {stage:?}"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::K2GossipConfig;
    use crate::protocol::{
        GossipMessage, K2GossipRingSectorDetailsDiffResponseMessage,
        RingSectorHashes, SnapshotRingSectorDetailsMessage,
    };
    use crate::respond::harness::RespondTestHarness;
    use crate::state::{RoundStage, RoundStageRingSectorDetailsDiff};
    use bytes::Bytes;
    use kitsune2_api::{DhtArc, Gossip, OpId, decode_ids};
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
            .insert_initiated_round_state(&local_agent, &remote_agent)
            .await;
        {
            let mut initiated =
                harness.gossip.initiated_round_state.lock().await;
            let mut ring_sector_hashes = HashMap::new();
            let mut sector_hashes = HashMap::new();
            sector_hashes.insert(0, Bytes::from_static(b"other-hash1"));
            sector_hashes.insert(1, Bytes::from_static(b"other-hash2"));
            sector_hashes.insert(2, Bytes::from_static(b"other-hash3"));
            sector_hashes.insert(3, Bytes::from_static(b"other-hash4"));
            sector_hashes.insert(4, Bytes::from_static(b"other-hash5"));
            ring_sector_hashes.insert(0, sector_hashes);
            initiated.as_mut().unwrap().stage =
                RoundStage::RingSectorDetailsDiff(
                    RoundStageRingSectorDetailsDiff {
                        common_arc_set: ArcSet::new(vec![DhtArc::FULL])
                            .unwrap(),
                        snapshot: DhtSnapshot::RingSectorDetails {
                            disc_boundary,
                            ring_sector_hashes,
                        },
                    },
                );
            initiated.as_mut().unwrap().peer_max_op_data_bytes =
                harness.gossip.config.max_gossip_op_bytes as i32;
        }

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

        let hashes = harness
            .gossip
            .respond_to_ring_sector_details_diff_response(
                remote_agent.url.clone().unwrap(),
                K2GossipRingSectorDetailsDiffResponseMessage {
                    session_id,
                    missing_ids: vec![],
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
            hashes.is_some(),
            "Should have responded to the diff message"
        );
        let hashes = hashes.unwrap();
        let hashes = match hashes {
            GossipMessage::Hashes(diff_details) => diff_details,
            _ => panic!("Expected a Hashes message, got: {hashes:?}"),
        };
        let sent_ops = decode_ids::<OpId>(hashes.missing_ids);
        assert_eq!(
            max_ops_per_round,
            sent_ops.len(),
            "Should have sent only 3 ops due to size limit"
        );

        // The gossip round is finished, so the initiated round state should be cleared
        assert!(harness.gossip.initiated_round_state.lock().await.is_none());
    }
}
