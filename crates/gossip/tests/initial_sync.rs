//! Tests that initial sync is reasonably fast with the default configuration.
//!
//! This is a UX test more than a functional test.

use kitsune2_api::{DhtArc, Timestamp};
use kitsune2_core::factories::MemoryOp;
use kitsune2_gossip::harness::{K2GossipFunctionalTestFactory, MemoryOpRecord};
use kitsune2_test_utils::space::TEST_SPACE_ID;
use kitsune2_test_utils::{enable_tracing_with_default_level, random_bytes};
use std::time::Duration;

#[tokio::test(flavor = "multi_thread")]
async fn two_new_agents_sync() {
    // We want less logging in this test, it creates a lot of output at debug level.
    enable_tracing_with_default_level(tracing::Level::INFO);

    let factory = K2GossipFunctionalTestFactory::create(
        TEST_SPACE_ID,
        true,
        // Use default config for this test
        Some(Default::default()),
    )
    .await;

    let harness_1 = factory.new_instance().await;
    let agent_info_1 = harness_1.join_local_agent(DhtArc::FULL).await;

    const NUM_OPS: usize = 20;
    let mut ops = Vec::<MemoryOpRecord>::with_capacity(NUM_OPS);
    for _ in 0..NUM_OPS {
        let op_size = rand::random::<usize>() % 1000 + 500;

        let op = MemoryOp::new(Timestamp::now(), random_bytes(op_size as u16));
        ops.push(MemoryOpRecord {
            op_id: op.compute_op_id(),
            op_data: op.op_data,
            created_at: op.created_at,
            stored_at: Timestamp::now(),
            processed: false,
        });
    }

    // Store the ops in the op store for this first agent.
    harness_1
        .op_store
        .write()
        .await
        .op_list
        .extend(ops.iter().map(|op| (op.op_id.clone(), op.clone())));

    let harness_2 = factory.new_instance().await;
    let agent_info_2 = harness_2.join_local_agent(DhtArc::FULL).await;

    // Simulate peer discovery so that gossip messages won't be dropped
    // due to no associated agents in the peer store when checking for
    // blocks.
    harness_1
        .space
        .peer_store()
        .insert(vec![agent_info_2.clone()])
        .await
        .unwrap();

    harness_2
        .space
        .peer_store()
        .insert(vec![agent_info_1.clone()])
        .await
        .unwrap();

    // Wait for data to be synced.
    harness_1
        .wait_for_sync_with(&harness_2, Duration::from_secs(10))
        .await;

    // Then both agents should have reached full arc.
    harness_2
        .wait_for_full_arc_for_all(Duration::from_secs(10))
        .await;
    harness_1
        .wait_for_full_arc_for_all(Duration::from_secs(10))
        .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn new_agent_joins_existing_network() {
    // We want less logging in this test, it creates a lot of output at debug level.
    enable_tracing_with_default_level(tracing::Level::INFO);

    let factory = K2GossipFunctionalTestFactory::create(
        TEST_SPACE_ID,
        true,
        // Use default config for this test
        Some(Default::default()),
    )
    .await;

    let harness_1 = factory.new_instance().await;
    let agent_info_1 = harness_1.join_local_agent(DhtArc::FULL).await;

    // Force the first agent to have a full arc, to simulate an existing network where there are
    // agents that are already synced.
    harness_1
        .force_storage_arc(agent_info_1.agent.clone(), DhtArc::FULL)
        .await;

    const NUM_OPS: usize = 20;
    let mut ops = Vec::<MemoryOpRecord>::with_capacity(NUM_OPS);
    for _ in 0..NUM_OPS {
        let op_size = rand::random::<usize>() % 1000 + 500;

        let op = MemoryOp::new(Timestamp::now(), random_bytes(op_size as u16));
        ops.push(MemoryOpRecord {
            op_id: op.compute_op_id(),
            op_data: op.op_data,
            created_at: op.created_at,
            stored_at: Timestamp::now(),
            processed: false,
        });
    }

    // Store the ops in the op store for this first agent.
    harness_1
        .op_store
        .write()
        .await
        .op_list
        .extend(ops.iter().map(|op| (op.op_id.clone(), op.clone())));

    // Join a new agent that wants to reach full arc, and let them try to sync with the first agent.
    let harness_2 = factory.new_instance().await;
    let agent_info_2 = harness_2.join_local_agent(DhtArc::FULL).await;

    // Simulate peer discovery so that gossip messages won't be dropped
    // due to no associated agents in the peer store when checking for
    // blocks.
    harness_1
        .space
        .peer_store()
        .insert(vec![agent_info_2.clone()])
        .await
        .unwrap();

    harness_2
        .space
        .peer_store()
        .insert(vec![agent_info_1.clone()])
        .await
        .unwrap();

    // Wait for data to be synced.
    harness_1
        .wait_for_sync_with(&harness_2, Duration::from_secs(10))
        .await;

    // Then both agents should have reached full arc.
    harness_1
        .wait_for_full_arc_for_all(Duration::from_secs(5))
        .await;
    harness_2
        .wait_for_full_arc_for_all(Duration::from_secs(5))
        .await;
}
