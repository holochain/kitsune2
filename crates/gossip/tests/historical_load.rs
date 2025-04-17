//! Tests that gossip will successfully sync historical data to a new peer joining a network.
//!
//! This test uses the default network transport and creates enough data to be realistic.

use bytes::Bytes;
use kitsune2_api::{DhtArc, StoredOp, Timestamp};
use kitsune2_core::factories::MemoryOp;
use kitsune2_gossip::harness::K2GossipFunctionalTestFactory;
use kitsune2_gossip::K2GossipConfig;
use kitsune2_test_utils::space::TEST_SPACE_ID;
use kitsune2_test_utils::{enable_tracing_with_default_level, random_bytes};
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
    // We want less logging in this test, it create a lot of output at debug level.
    enable_tracing_with_default_level(tracing::Level::INFO);

    let factory = K2GossipFunctionalTestFactory::create(
        TEST_SPACE_ID,
        true,
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
    )).expect("Please wait until your civilisation has completed its first year of existence.");

    // Stop time set to now
    let stop_time = Timestamp::now();

    tracing::info!("Creating test data");

    // Create an op every 12 hours for a year
    let mut ops = Vec::<Bytes>::with_capacity(2 * 365);
    while time < stop_time {
        let op_size = rand::random::<usize>()
            % (MAX_OP_SIZE_BYTES - MIN_OP_SIZE_BYTES)
            + MIN_OP_SIZE_BYTES;

        let op = MemoryOp::new(time, random_bytes(op_size as u16));
        ops.push(op.into());

        time += Duration::from_secs(60 * 60 * 12);
    }

    let total_size = ops.iter().map(|op| op.len()).sum::<usize>();
    tracing::info!(
        "Created {} ops, total size: {} bytes",
        ops.len(),
        total_size
    );

    // Store the ops in the op store for this first agent.
    let op_ids = harness_1
        .space
        .op_store()
        .process_incoming_ops(ops)
        .await
        .unwrap();

    let stored_ops = harness_1
        .space
        .op_store()
        .retrieve_ops(op_ids)
        .await
        .unwrap();

    harness_1
        .space
        .inform_ops_stored(
            stored_ops
                .into_iter()
                .map(|o| StoredOp {
                    op_id: o.op_id,
                    created_at: MemoryOp::from(o.op_data).created_at,
                })
                .collect(),
        )
        .await
        .unwrap();

    let harness_2 = factory.new_instance().await;
    let agent_2 = harness_2.join_local_agent(DhtArc::FULL).await;

    harness_1
        .wait_for_ops_discovered(&harness_2, Duration::from_secs(180))
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

    assert!(
        completed_rounds_with_agent_2.unwrap() <= 30,
        "Gossip took an unreasonable number of rounds to sync: {}",
        completed_rounds_with_agent_2.unwrap()
    );

    // Ensure the data syncs too.
    harness_1
        .wait_for_sync_with(&harness_2, Duration::from_secs(60))
        .await;
}
