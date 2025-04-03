//! Tests that gossip will successfully sync historical data to a new peer joining a network.
//!
//! This test uses the default network transport and creates enough data to be realistic.

use bytes::Bytes;
use kitsune2_api::{DhtArc, Timestamp};
use kitsune2_core::factories::MemoryOp;
use kitsune2_gossip::harness::K2GossipFunctionalTestFactory;
use kitsune2_gossip::K2GossipConfig;
use kitsune2_test_utils::space::TEST_SPACE_ID;
use kitsune2_test_utils::{enable_tracing, random_bytes};
use std::time::Duration;

/// The minimum size of an op in bytes.
///
/// 0.5 KiB
const MIN_OP_SIZE_BYTES: usize = 512;
/// The maximum size of an op in bytes.
///
/// 1 MiB
const MAX_OP_SIZE_BYTES: usize = 1024 * 1024;

#[tokio::test(flavor = "multi_thread")]
async fn historical_load() {
    enable_tracing();

    let factory = K2GossipFunctionalTestFactory::create(
        TEST_SPACE_ID,
        true,
        Some(K2GossipConfig {
            initiate_interval_ms: 500,
            min_initiate_interval_ms: 500,
            initiate_jitter_ms: 0,
            ..Default::default()
        }),
    )
    .await;

    let harness_1 = factory.new_instance().await;
    harness_1.join_local_agent(DhtArc::FULL).await;

    // Start time set to roughly 1 year ago
    let mut time = (Timestamp::now() - Duration::from_secs(
        60 * 60 * 24 * 365,
    )).expect("Please wait until your civilisation has completed its first hour of existence.");

    // Stop time set to now
    let stop_time = Timestamp::now();

    tracing::info!("Creating test data");

    // Create an op every 6 hours for a year
    let mut ops = Vec::<Bytes>::with_capacity(4 * 365);
    while time < stop_time {
        let op_size = rand::random::<usize>()
            % (MAX_OP_SIZE_BYTES - MIN_OP_SIZE_BYTES)
            + MIN_OP_SIZE_BYTES;

        let op = MemoryOp::new(time, random_bytes(op_size as u16));
        ops.push(op.into());

        time += Duration::from_secs(60 * 60 * 6);
    }

    let total_size = ops.iter().map(|op| op.len()).sum::<usize>();
    tracing::info!(
        "Created {} ops, total size: {} bytes",
        ops.len(),
        total_size
    );

    // Store the ops in the op store for this first agent.
    harness_1
        .space
        .op_store()
        .process_incoming_ops(ops)
        .await
        .unwrap();

    let harness_2 = factory.new_instance().await;
    let agent_2 = harness_2.join_local_agent(DhtArc::FULL).await;

    harness_1
        .wait_for_ops_discovered(&harness_2, Duration::from_secs(60))
        .await;

    let completed_rounds_with_agent_2 = harness_1
        .peer_meta_store
        .completed_rounds(agent_2.url.clone().unwrap())
        .await
        .unwrap();

    assert!(
        completed_rounds_with_agent_2.is_some(),
        "Completed rounds with agent 2 should have a value"
    );

    let required_rounds =
        total_size / K2GossipConfig::default().max_gossip_op_bytes as usize + 1;
    assert!(
        completed_rounds_with_agent_2.unwrap() <= required_rounds as u32,
        "Completed rounds with agent 2 should be no more than {}",
        required_rounds
    );

    // Ensure the data syncs too.
    harness_1
        .wait_for_sync_with(&harness_2, Duration::from_secs(60))
        .await;
}
