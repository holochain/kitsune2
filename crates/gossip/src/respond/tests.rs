//! Tests that span multiple message responders.

use crate::protocol::k2_gossip_accept_message::SnapshotMinimalMessage;
use crate::protocol::{
    encode_agent_ids, ArcSetMessage, DiscSliceHashes, GossipMessage,
    K2GossipAcceptMessage, K2GossipDiscSectorDetailsDiffMessage,
    K2GossipDiscSectorDetailsDiffResponseMessage, K2GossipInitiateMessage,
    SnapshotDiscSectorDetailsMessage,
};
use crate::respond::harness::{test_session_id, RespondTestHarness};
use crate::state::{
    RoundStage, RoundStageDiscSectorDetailsDiff, RoundStageDiscSectorsDiff,
};
use crate::K2GossipConfig;
use bytes::Bytes;
use kitsune2_api::{
    decode_ids, DhtArc, Gossip, OpId, Timestamp, UNIX_TIMESTAMP,
};
use kitsune2_core::factories::MemoryOp;
use kitsune2_dht::{ArcSet, DhtSnapshot, SECTOR_SIZE};
use kitsune2_test_utils::enable_tracing;
use rand::RngCore;
use std::collections::HashMap;

#[tokio::test]
async fn initiate_respect_size_limit_for_new_ops_and_disc() {
    enable_tracing();

    let max_ops_per_round = 7usize;
    let available_new_ops = 5usize;
    let available_disc_ops = 5usize;
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

    let mut ops = Vec::new();
    for i in 0u8..(available_new_ops as u8) {
        let op = MemoryOp::new(Timestamp::now(), vec![i; op_size]);
        ops.push(op);
    }

    harness
        .gossip
        .op_store
        .process_incoming_ops(ops.clone().into_iter().map(Into::into).collect())
        .await
        .unwrap();
    harness
        .gossip
        .inform_ops_stored(ops.clone().into_iter().map(Into::into).collect())
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
    let no_diff = harness
        .gossip
        .respond_to_accept(
            remote_agent.url.clone().unwrap(),
            K2GossipAcceptMessage {
                session_id: session_id.clone(),
                participating_agents: encode_agent_ids([remote_agent
                    .agent
                    .clone()]),
                arc_set: Some(ArcSetMessage {
                    value: ArcSet::new(vec![
                        local_agent.local.get_tgt_storage_arc(),
                        remote_agent.local.get_tgt_storage_arc(),
                    ])
                    .unwrap()
                    .encode(),
                }),
                missing_agents: vec![],
                new_since: UNIX_TIMESTAMP.as_micros(),
                max_op_data_bytes: harness.gossip.config.max_gossip_op_bytes,
                new_ops: vec![],
                updated_new_since: Timestamp::now().as_micros(),
                snapshot: Some(SnapshotMinimalMessage {
                    disc_boundary: disc_boundary.as_micros(),
                    disc_top_hash: Bytes::new(),
                    ring_top_hashes: vec![],
                }),
            },
        )
        .await
        .unwrap();

    assert!(
        no_diff.is_some(),
        "Should have responded to the accept message"
    );
    let no_diff = no_diff.unwrap();
    let no_diff = match no_diff {
        GossipMessage::NoDiff(accept) => accept,
        _ => panic!("Expected a NoDiff message, got: {:?}", no_diff),
    };
    let sent_ops = decode_ids::<OpId>(no_diff.accept_response.unwrap().new_ops);
    assert_eq!(
        available_new_ops,
        sent_ops.len(),
        "Should have sent all 5 ops"
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
        ((max_ops_per_round - available_new_ops) * op_size) as i32,
        remaining_budget,
        "Should have remaining budget for 2 more ops"
    );

    // Update the round state to DiscSectorDetailsDiff, so that we're further along in
    // the gossip round but should still have the same remaining budget.
    {
        let mut initiated = harness.gossip.initiated_round_state.lock().await;
        initiated.as_mut().unwrap().stage =
            RoundStage::DiscSectorsDiff(RoundStageDiscSectorsDiff {
                common_arc_set: ArcSet::new(vec![DhtArc::FULL]).unwrap(),
            });
    }

    // Create some disc ops
    let mut ops = Vec::new();
    for i in 0u8..(available_disc_ops as u8) {
        let mut op_data = (i as u32 * SECTOR_SIZE).to_le_bytes().to_vec();
        op_data.resize(op_size, 0);

        let op = MemoryOp::new(Timestamp::from_micros(100 + i as i64), op_data);
        ops.push(op);
    }

    harness
        .gossip
        .op_store
        .process_incoming_ops(ops.clone().into_iter().map(Into::into).collect())
        .await
        .unwrap();
    harness
        .gossip
        .inform_ops_stored(ops.clone().into_iter().map(Into::into).collect())
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
            "Expected a DiscSectorDetailsDiffResponse message, got: {:?}",
            diff_details
        ),
    };
    let sent_ops = decode_ids::<OpId>(diff_details.missing_ids);
    assert_eq!(
        max_ops_per_round - available_new_ops,
        sent_ops.len(),
        "Should have sent only 2 ops due to size limit"
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

#[tokio::test]
async fn accept_respect_size_limit_for_new_ops_and_disc() {
    enable_tracing();

    let max_ops_per_round = 7usize;
    let available_new_ops = 5usize;
    let available_disc_ops = 5usize;
    let op_size = 128usize;

    let harness = RespondTestHarness::create_with_config(K2GossipConfig {
        max_gossip_op_bytes: (max_ops_per_round * op_size) as u32,
        ..Default::default()
    })
    .await;

    let mut ops = Vec::new();
    for i in 0u8..(available_new_ops as u8) {
        let op = MemoryOp::new(Timestamp::now(), vec![i; op_size]);
        ops.push(op);
    }

    harness
        .gossip
        .op_store
        .process_incoming_ops(ops.clone().into_iter().map(Into::into).collect())
        .await
        .unwrap();
    harness
        .gossip
        .inform_ops_stored(ops.clone().into_iter().map(Into::into).collect())
        .await
        .unwrap();

    let local_agent = harness.create_agent(DhtArc::FULL).await;
    harness
        .gossip
        .local_agent_store
        .add(local_agent.local.clone())
        .await
        .unwrap();

    let remote_agent = harness.create_agent(DhtArc::FULL).await;

    let accept = harness
        .gossip
        .respond_to_initiate(
            remote_agent.url.clone().unwrap(),
            K2GossipInitiateMessage {
                session_id: test_session_id(),
                participating_agents: encode_agent_ids([remote_agent
                    .agent
                    .clone()]),
                arc_set: Some(ArcSetMessage {
                    value: ArcSet::new(vec![remote_agent
                        .local
                        .get_tgt_storage_arc()])
                    .unwrap()
                    .encode(),
                }),
                tie_breaker: rand::thread_rng().next_u32().saturating_add(1),
                new_since: UNIX_TIMESTAMP.as_micros(),
                max_op_data_bytes: harness.gossip.config.max_gossip_op_bytes,
            },
        )
        .await
        .unwrap();

    assert!(
        accept.is_some(),
        "Should have accepted the initiate message"
    );
    let accept = accept.unwrap();
    let accept = match accept {
        GossipMessage::Accept(accept) => accept,
        _ => panic!("Expected an Accept message, got: {:?}", accept),
    };
    let sent_ops = decode_ids::<OpId>(accept.new_ops);
    assert_eq!(
        available_new_ops,
        sent_ops.len(),
        "Should have sent all 5 ops"
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
        ((max_ops_per_round - available_new_ops) * op_size) as i32,
        remaining_budget,
        "Should have remaining budget for 2 more ops"
    );

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

    // Update the round state to DiscSectorDetailsDiff, so that we're further along in
    // the gossip round but should still have the same remaining budget.
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
    }

    // Create some disc ops
    let mut ops = Vec::new();
    for i in 0u8..(available_disc_ops as u8) {
        let mut op_data = (i as u32 * SECTOR_SIZE).to_le_bytes().to_vec();
        op_data.resize(op_size, 0);

        let op = MemoryOp::new(Timestamp::from_micros(100 + i as i64), op_data);
        ops.push(op);
    }

    harness
        .gossip
        .op_store
        .process_incoming_ops(ops.clone().into_iter().map(Into::into).collect())
        .await
        .unwrap();
    harness
        .gossip
        .inform_ops_stored(ops.clone().into_iter().map(Into::into).collect())
        .await
        .unwrap();

    let session_id = harness
        .gossip
        .accepted_round_states
        .read()
        .await
        .get(remote_agent.url.as_ref().unwrap())
        .unwrap()
        .lock()
        .await
        .session_id
        .clone();

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
        max_ops_per_round - available_new_ops,
        sent_ops.len(),
        "Should have sent only 2 ops due to size limit"
    );

    // End of the round, so the state should be removed
    assert!(!harness
        .gossip
        .accepted_round_states
        .read()
        .await
        .contains_key(remote_agent.url.as_ref().unwrap()));
}
