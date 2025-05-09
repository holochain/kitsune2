use bytes::Bytes;
use kitsune2::default_builder;
use kitsune2_api::{
    BoxFut, DhtArc, DynKitsune, DynSpace, DynSpaceHandler, K2Result,
    KitsuneHandler, LocalAgent, OpId, SpaceHandler, SpaceId, Timestamp,
};
use kitsune2_core::{
    factories::{
        config::{CoreBootstrapConfig, CoreBootstrapModConfig},
        MemoryOp,
    },
    Ed25519LocalAgent,
};
use kitsune2_gossip::{K2GossipConfig, K2GossipModConfig};
use kitsune2_test_utils::{
    bootstrap::TestBootstrapSrv, enable_tracing, iter_check, random_bytes,
    space::TEST_SPACE_ID,
};
use kitsune2_transport_tx5::config::{
    Tx5TransportConfig, Tx5TransportModConfig,
};
use sbd_server::SbdServer;
use std::sync::Arc;

fn create_op_list(num_ops: u16) -> (Vec<Bytes>, Vec<OpId>) {
    let mut ops = Vec::new();
    let mut op_ids = Vec::new();
    for _ in 0..num_ops {
        let op = MemoryOp::new(Timestamp::from_micros(0), random_bytes(256));
        let op_id = op.compute_op_id();
        ops.push(op.into());
        op_ids.push(op_id);
    }
    (ops, op_ids)
}

#[derive(Debug)]
struct TestKitsuneHandler;
impl KitsuneHandler for TestKitsuneHandler {
    fn create_space(
        &self,
        _space_id: SpaceId,
    ) -> BoxFut<'_, K2Result<DynSpaceHandler>> {
        Box::pin(async {
            let space_handler: DynSpaceHandler = Arc::new(TestSpaceHandler);
            Ok(space_handler)
        })
    }
}

#[derive(Debug)]
struct TestSpaceHandler;
impl SpaceHandler for TestSpaceHandler {}

async fn make_kitsune_node(
    signal_server_url: &str,
    bootstrap_server_url: &str,
) -> DynKitsune {
    let kitsune_builder = default_builder().with_default_config().unwrap();
    kitsune_builder
        .config
        .set_module_config(&CoreBootstrapModConfig {
            core_bootstrap: CoreBootstrapConfig {
                server_url: bootstrap_server_url.to_owned(),
                backoff_max_ms: 1000,
                ..Default::default()
            },
        })
        .unwrap();
    kitsune_builder
        .config
        .set_module_config(&Tx5TransportModConfig {
            tx5_transport: Tx5TransportConfig {
                server_url: signal_server_url.to_owned(),
                signal_allow_plain_text: true,
                timeout_s: 5,
                ..Default::default()
            },
        })
        .unwrap();
    kitsune_builder
        .config
        .set_module_config(&K2GossipModConfig {
            k2_gossip: K2GossipConfig {
                initiate_interval_ms: 100,
                min_initiate_interval_ms: 75,
                initiate_jitter_ms: 10,
                round_timeout_ms: 10_000,
                ..Default::default()
            },
        })
        .unwrap();

    let kitsune_handler = Arc::new(TestKitsuneHandler);
    let kitsune = kitsune_builder.build().await.unwrap();
    kitsune
        .register_handler(kitsune_handler.clone())
        .await
        .unwrap();

    kitsune
}

async fn start_space(kitsune: &DynKitsune) -> DynSpace {
    let space = kitsune.space(TEST_SPACE_ID).await.unwrap();

    // Create an agent.
    let local_agent = Arc::new(Ed25519LocalAgent::default());
    local_agent.set_tgt_storage_arc_hint(DhtArc::FULL);

    // Join agent to local space.
    space.local_agent_join(local_agent.clone()).await.unwrap();

    // Wait for agent to publish their info to the bootstrap & peer store.
    iter_check!(5000, 100, {
        if space
            .peer_store()
            .get(local_agent.agent().clone())
            .await
            .unwrap()
            .is_some()
        {
            break;
        }
    });

    space
}

#[tokio::test]
async fn two_node_gossip() {
    enable_tracing();

    let signal_server = SbdServer::new(Arc::new(sbd_server::Config {
        bind: vec!["127.0.0.1:0".to_string()],
        ..Default::default()
    }))
    .await
    .unwrap();
    let signal_server_url = format!("ws://{}", signal_server.bind_addrs()[0]);

    let bootstrap_server = TestBootstrapSrv::new(false).await;
    let bootstrap_server_url = bootstrap_server.addr().to_string();

    // Create 2 Kitsune instances...
    let kitsune_1 =
        make_kitsune_node(&signal_server_url, &bootstrap_server_url).await;
    let kitsune_2 =
        make_kitsune_node(&signal_server_url, &bootstrap_server_url).await;

    // and 1 space with 1 joined agent each.
    let space_1 = start_space(&kitsune_1).await;
    let space_2 = start_space(&kitsune_2).await;

    // Wait for Windows runner to catch up with establishing the connection.
    #[cfg(target_os = "windows")]
    tokio::time::sleep(std::time::Duration::from_secs(5)).await;

    // Insert ops into both spaces' op stores.
    let (ops_1, op_ids_1) = create_op_list(1000);
    space_1
        .op_store()
        .process_incoming_ops(ops_1.clone())
        .await
        .unwrap();
    let (ops_2, op_ids_2) = create_op_list(1000);
    space_2
        .op_store()
        .process_incoming_ops(ops_2.clone())
        .await
        .unwrap();

    // Wait for gossip to exchange all ops.
    iter_check!(5000, 500, {
        let actual_ops_1 = space_1
            .op_store()
            .retrieve_ops(op_ids_2.clone())
            .await
            .unwrap();
        let actual_ops_2 = space_2
            .op_store()
            .retrieve_ops(op_ids_1.clone())
            .await
            .unwrap();
        if actual_ops_1.len() == ops_2.len()
            && actual_ops_2.len() == ops_1.len()
        {
            break;
        } else {
            println!(
                "space 1 actual ops received {}/expected {}",
                actual_ops_1.len(),
                ops_2.len()
            );
            println!(
                "space 2 actual ops received {}/expected {}",
                actual_ops_2.len(),
                ops_1.len()
            );
        }
    });
}

/// Test that space shutdown is reasonably clean:
/// - Start two Kitsune2 instances
/// - Record the initial number of Tokio tasks
/// - Start two spaces, one on each instance
/// - Join an agent to each space
/// - Create some ops in each space
/// - Wait for gossip to exchange all ops
/// - Have local agents leave the spaces
/// - Wait for all peers to declare a tombstone
/// - Shut down the spaces
/// - Wait for the spaces' tasks to be cleaned up
///
/// This isn't a perfect check for shutdown, but it's a reasonable expectation that if all the
/// Tokio tasks for a space are gone, then it's not actively doing work in the background.
#[tokio::test]
async fn shutdown_space() {
    enable_tracing();

    let signal_server = SbdServer::new(Arc::new(sbd_server::Config {
        bind: vec!["127.0.0.1:0".to_string()],
        ..Default::default()
    }))
    .await
    .unwrap();
    let signal_server_url = format!("ws://{}", signal_server.bind_addrs()[0]);

    let bootstrap_server = TestBootstrapSrv::new(false).await;
    let bootstrap_server_url = bootstrap_server.addr().to_string();

    // Create 2 Kitsune instances..
    let kitsune_1 =
        make_kitsune_node(&signal_server_url, &bootstrap_server_url).await;
    let kitsune_2 =
        make_kitsune_node(&signal_server_url, &bootstrap_server_url).await;

    let metrics = tokio::runtime::Handle::current().metrics();
    let initial_tasks = metrics.num_alive_tasks();

    // and 1 space with 1 joined agent each.
    let space_1 = start_space(&kitsune_1).await;
    let space_2 = start_space(&kitsune_2).await;

    // Create some data for each agent
    let (ops_1, op_ids_1) = create_op_list(10);
    space_1
        .op_store()
        .process_incoming_ops(ops_1.clone())
        .await
        .unwrap();
    let (ops_2, op_ids_2) = create_op_list(10);
    space_2
        .op_store()
        .process_incoming_ops(ops_2.clone())
        .await
        .unwrap();

    // Wait for gossip to exchange all ops.
    iter_check!(15000, 500, {
        let actual_ops_1 = space_1
            .op_store()
            .retrieve_ops(op_ids_2.clone())
            .await
            .unwrap();
        let actual_ops_2 = space_2
            .op_store()
            .retrieve_ops(op_ids_1.clone())
            .await
            .unwrap();
        if actual_ops_1.len() == ops_2.len()
            && actual_ops_2.len() == ops_1.len()
        {
            break;
        } else {
            println!(
                "space 1 actual ops received {}/expected {}",
                actual_ops_1.len(),
                ops_2.len()
            );
            println!(
                "space 2 actual ops received {}/expected {}",
                actual_ops_2.len(),
                ops_1.len()
            );
        }
    });

    // Attempt to shut down a space while there are still agents joined.
    let err = kitsune_1.remove_space(TEST_SPACE_ID).await.unwrap_err();
    assert!(err.to_string().contains("Cannot remove space with local agents"), 
        "Got error: {err}");
    
    // Leave the spaces.
    for local_agent in space_1.local_agent_store().get_all().await.unwrap() {
        space_1.local_agent_leave(local_agent.agent().clone()).await;
    }
    for local_agent in space_2.local_agent_store().get_all().await.unwrap() {
        space_2.local_agent_leave(local_agent.agent().clone()).await;
    }

    // Wait for all peers to declare a tombstone.
    iter_check!(5000, 500, {
        let all_peers_tombstone_1 = space_1
            .peer_store()
            .get_all()
            .await
            .unwrap()
            .iter()
            .all(|a| a.url.is_none());
        let all_peers_tombstone_2 = space_2
            .peer_store()
            .get_all()
            .await
            .unwrap()
            .iter()
            .all(|a| a.url.is_none());

        if all_peers_tombstone_1 && all_peers_tombstone_2 {
            break;
        } else {
            println!(
                "space 1 peers: {:?}",
                space_1.peer_store().get_all().await.unwrap()
            );
            println!(
                "space 2 peers: {:?}",
                space_2.peer_store().get_all().await.unwrap()
            );
        }
    });

    // Now that the spaces have been active and messaging each other, shut them down.
    drop(space_1);
    drop(space_2);
    kitsune_1.remove_space(TEST_SPACE_ID).await.unwrap();
    kitsune_2.remove_space(TEST_SPACE_ID).await.unwrap();

    // Wait for the space's tasks to be cleaned up.
    // This includes connection tasks, otherwise the task count would stay higher than the initial
    // count.
    iter_check!(30000, 100, {
        let current_tasks = metrics.num_alive_tasks();
        if current_tasks == initial_tasks {
            break;
        } else {
            println!("Current tasks: {current_tasks}, Initial tasks: {initial_tasks}");
        }
    });

    // The spaces should be gone.
    assert!(kitsune_1.space_if_exists(TEST_SPACE_ID).await.is_none());
    assert!(kitsune_2.space_if_exists(TEST_SPACE_ID).await.is_none());
}
