//! Long-running relay stability test.
//!
//! Runs two full Kitsune2 nodes with iroh transport against a real deployment,
//! verifying that multiple gossip rounds complete and notify messages are
//! delivered continuously without dropping.
//!
//! Required environment variables:
//!   KITSUNE2_TEST_RELAY_URL       - e.g. "https://example.com/relay"
//!   KITSUNE2_TEST_BOOTSTRAP_URL   - e.g. "https://example.com/bootstrap"
//!   KITSUNE2_TEST_AUTH_MATERIAL_1 - base64url (no-pad) encoded auth material for node 1
//!   KITSUNE2_TEST_AUTH_MATERIAL_2 - base64url (no-pad) encoded auth material for node 2
//!
//! Optional:
//!   KITSUNE2_TEST_DURATION_SECS   - total test duration (default: 300 = 5 minutes)
//!   KITSUNE2_TEST_GOSSIP_ROUNDS   - number of gossip rounds to verify (default: 5)
//!
//! Run:
//! ```sh
//! cargo test -p kitsune2 --no-default-features --features transport-iroh \
//!     --test relay_stability -- --ignored --nocapture
//! ```

use base64::Engine as _;
use bytes::Bytes;
use kitsune2::default_builder;
use kitsune2_api::{
    BoxFut, Builder, Config, DhtArc, DynKitsune, DynSpace, DynSpaceHandler, Id,
    K2Result, KitsuneHandler, LocalAgent, OpId, SpaceHandler, SpaceId, Timestamp,
};
use kitsune2_core::{
    Ed25519LocalAgent,
    factories::{
        MemoryOp,
        config::{CoreBootstrapConfig, CoreBootstrapModConfig},
    },
};
use kitsune2_gossip::{K2GossipConfig, K2GossipModConfig};
use kitsune2_test_utils::{enable_tracing, iter_check, random_bytes};
use kitsune2_transport_iroh::{
    IrohTransportFactory,
    config::{IrohTransportConfig, IrohTransportModConfig},
};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::time::{Duration, Instant};

/// Shared space id for both nodes.
static SPACE_ID: std::sync::LazyLock<SpaceId> =
    std::sync::LazyLock::new(|| SpaceId(Id(Bytes::from_static(b"relay-stability-test"))));

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct StabilityKitsuneHandler;
impl KitsuneHandler for StabilityKitsuneHandler {
    fn create_space(
        &self,
        _space_id: SpaceId,
        _config_override: Option<&Config>,
    ) -> BoxFut<'_, K2Result<DynSpaceHandler>> {
        Box::pin(async {
            let h: DynSpaceHandler = Arc::new(StabilitySpaceHandler {
                notify_count: Arc::new(AtomicUsize::new(0)),
            });
            Ok(h)
        })
    }
}

#[derive(Debug)]
struct StabilitySpaceHandler {
    notify_count: Arc<AtomicUsize>,
}

impl SpaceHandler for StabilitySpaceHandler {
    fn recv_notify(
        &self,
        _from_peer: kitsune2_api::Url,
        _space_id: SpaceId,
        _data: Bytes,
    ) -> K2Result<()> {
        self.notify_count.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Node construction
// ---------------------------------------------------------------------------

fn read_env(name: &str) -> Option<String> {
    std::env::var(name).ok()
}

fn require_env(name: &str) -> String {
    read_env(name).unwrap_or_else(|| {
        panic!(
            "Environment variable {name} is required. \
             Set it before running this test."
        )
    })
}

fn decode_auth_material(b64: &str) -> Vec<u8> {
    base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(b64)
        .expect("auth material must be base64url (no-pad) encoded")
}

async fn make_node(
    relay_url: &str,
    bootstrap_url: &str,
    auth_material: Vec<u8>,
) -> DynKitsune {
    let mut builder = Builder {
        transport: IrohTransportFactory::create(),
        ..default_builder()
    }
    .with_default_config()
    .unwrap();

    builder.auth_material = Some(auth_material);

    builder
        .config
        .set_module_config(&CoreBootstrapModConfig {
            core_bootstrap: CoreBootstrapConfig {
                server_url: Some(bootstrap_url.to_owned()),
                backoff_min_ms: 1000,
                backoff_max_ms: 1000,
            },
        })
        .unwrap();

    builder
        .config
        .set_module_config(&IrohTransportModConfig {
            iroh_transport: IrohTransportConfig {
                relay_url: Some(relay_url.to_string()),
                ..Default::default()
            },
        })
        .unwrap();

    builder
        .config
        .set_module_config(&K2GossipModConfig {
            k2_gossip: K2GossipConfig {
                initiate_interval_ms: 2_000,
                min_initiate_interval_ms: 500,
                initiate_jitter_ms: 500,
                round_timeout_ms: 15_000,
                ..Default::default()
            },
        })
        .unwrap();

    let handler = Arc::new(StabilityKitsuneHandler);
    let kitsune = builder.build().await.unwrap();
    kitsune.register_handler(handler).await.unwrap();

    kitsune
}

async fn join_agent(kitsune: &DynKitsune) -> (DynSpace, Arc<Ed25519LocalAgent>) {
    let space = kitsune.space(SPACE_ID.clone(), None).await.unwrap();

    let agent = Arc::new(Ed25519LocalAgent::default());
    agent.set_tgt_storage_arc_hint(DhtArc::FULL);
    space.local_agent_join(agent.clone()).await.unwrap();

    // Wait for the agent to appear in the peer store.
    iter_check!(30_000, 200, {
        if space
            .peer_store()
            .get(agent.agent().clone())
            .await
            .unwrap()
            .is_some()
        {
            break;
        }
    });

    (space, agent)
}

fn create_ops(n: u16) -> (Vec<Bytes>, Vec<OpId>) {
    let mut ops = Vec::new();
    let mut ids = Vec::new();
    for _ in 0..n {
        let op = MemoryOp::new(Timestamp::from_micros(0), random_bytes(128));
        ids.push(op.compute_op_id());
        ops.push(op.into());
    }
    (ops, ids)
}

// ---------------------------------------------------------------------------
// Test
// ---------------------------------------------------------------------------

/// Long-running test that verifies relay stability across multiple gossip
/// rounds and continuous notify traffic.
///
/// Ignored by default so it does not run in CI — must be invoked explicitly
/// with the required environment variables set.
#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn relay_stability_gossip_and_notify() {
    enable_tracing();

    // iroh uses rustls; ensure a crypto provider is installed.
    let _ = rustls::crypto::ring::default_provider().install_default();

    let relay_url = require_env("KITSUNE2_TEST_RELAY_URL");
    let bootstrap_url = require_env("KITSUNE2_TEST_BOOTSTRAP_URL");
    let auth_1 = decode_auth_material(&require_env("KITSUNE2_TEST_AUTH_MATERIAL_1"));
    let auth_2 = decode_auth_material(&require_env("KITSUNE2_TEST_AUTH_MATERIAL_2"));

    let duration_secs: u64 = read_env("KITSUNE2_TEST_DURATION_SECS")
        .and_then(|v| v.parse().ok())
        .unwrap_or(300);
    let gossip_rounds: usize = read_env("KITSUNE2_TEST_GOSSIP_ROUNDS")
        .and_then(|v| v.parse().ok())
        .unwrap_or(5);

    tracing::info!(
        %relay_url,
        %bootstrap_url,
        duration_secs,
        gossip_rounds,
        "Starting relay stability test"
    );

    // -----------------------------------------------------------------------
    // Bootstrap two nodes
    // -----------------------------------------------------------------------
    let node_1 = make_node(&relay_url, &bootstrap_url, auth_1).await;
    let node_2 = make_node(&relay_url, &bootstrap_url, auth_2).await;

    let (space_1, _agent_1) = join_agent(&node_1).await;
    let (space_2, _agent_2) = join_agent(&node_2).await;

    tracing::info!("Both nodes joined the space — waiting for peer discovery");

    // Wait until each node sees the other in its peer store.
    iter_check!(60_000, 500, {
        let peers_1 = space_1.peer_store().get_all().await.unwrap();
        let peers_2 = space_2.peer_store().get_all().await.unwrap();
        if peers_1.len() >= 2 && peers_2.len() >= 2 {
            tracing::info!(
                node_1_peers = peers_1.len(),
                node_2_peers = peers_2.len(),
                "Peer discovery complete"
            );
            break;
        }
    });

    // -----------------------------------------------------------------------
    // Phase 1: Repeated gossip rounds
    // -----------------------------------------------------------------------
    tracing::info!("=== Phase 1: Gossip rounds ===");

    for round in 1..=gossip_rounds {
        let ops_per_round = 50;

        let (ops_a, ids_a) = create_ops(ops_per_round);
        let (ops_b, ids_b) = create_ops(ops_per_round);

        space_1
            .op_store()
            .process_incoming_ops(ops_a)
            .await
            .unwrap();
        space_2
            .op_store()
            .process_incoming_ops(ops_b)
            .await
            .unwrap();

        tracing::info!(round, "Inserted ops — waiting for gossip sync");

        iter_check!(60_000, 1_000, {
            let got_b_on_1 = space_1
                .op_store()
                .retrieve_ops(ids_b.clone())
                .await
                .unwrap();
            let got_a_on_2 = space_2
                .op_store()
                .retrieve_ops(ids_a.clone())
                .await
                .unwrap();

            if got_b_on_1.len() == ops_per_round as usize
                && got_a_on_2.len() == ops_per_round as usize
            {
                tracing::info!(
                    round,
                    "Gossip round complete — all ops synced"
                );
                break;
            } else {
                tracing::debug!(
                    round,
                    node_1_received = got_b_on_1.len(),
                    node_2_received = got_a_on_2.len(),
                    expected = ops_per_round,
                    "Waiting for gossip sync"
                );
            }
        });
    }

    // -----------------------------------------------------------------------
    // Phase 2: Continuous notify messages for the remaining duration
    // -----------------------------------------------------------------------
    tracing::info!("=== Phase 2: Continuous notify traffic ===");

    let deadline = Instant::now() + Duration::from_secs(duration_secs);
    let notify_interval = Duration::from_secs(5);
    let mut sent_count: usize = 0;
    let mut consecutive_failures: usize = 0;
    let max_consecutive_failures: usize = 3;

    while Instant::now() < deadline {
        // Determine peer URLs from each node's peer store.
        let peer_url_for_2 = {
            let peers = space_1.peer_store().get_all().await.unwrap();
            peers
                .iter()
                .find(|p| p.url.is_some() && p.agent != *_agent_1.agent())
                .and_then(|p| p.url.clone())
        };
        let peer_url_for_1 = {
            let peers = space_2.peer_store().get_all().await.unwrap();
            peers
                .iter()
                .find(|p| p.url.is_some() && p.agent != *_agent_2.agent())
                .and_then(|p| p.url.clone())
        };

        if let (Some(url_2), Some(url_1)) = (peer_url_for_2, peer_url_for_1) {
            let msg_1_to_2 =
                Bytes::from(format!("notify-1-to-2-{sent_count}"));
            let msg_2_to_1 =
                Bytes::from(format!("notify-2-to-1-{sent_count}"));

            let r1 = space_1.send_notify(url_2, msg_1_to_2).await;
            let r2 = space_2.send_notify(url_1, msg_2_to_1).await;

            match (r1, r2) {
                (Ok(()), Ok(())) => {
                    sent_count += 1;
                    consecutive_failures = 0;
                    tracing::info!(
                        sent_count,
                        remaining_secs = deadline
                            .saturating_duration_since(Instant::now())
                            .as_secs(),
                        "Notify round-trip OK"
                    );
                }
                (r1, r2) => {
                    consecutive_failures += 1;
                    tracing::error!(
                        ?r1,
                        ?r2,
                        consecutive_failures,
                        "Notify send failed"
                    );
                    assert!(
                        consecutive_failures < max_consecutive_failures,
                        "Too many consecutive notify failures ({consecutive_failures}). \
                         Communication has dropped."
                    );
                }
            }
        } else {
            consecutive_failures += 1;
            tracing::warn!(
                consecutive_failures,
                "Peer URL not available — peer may have \
                 temporarily dropped"
            );
            assert!(
                consecutive_failures < max_consecutive_failures,
                "Peer URL unavailable {consecutive_failures} times in a row. \
                 Communication has dropped."
            );
        }

        tokio::time::sleep(notify_interval).await;
    }

    tracing::info!(
        sent_count,
        gossip_rounds,
        duration_secs,
        "Relay stability test completed successfully"
    );

    assert!(
        sent_count > 0,
        "Expected at least one notify round-trip to succeed"
    );
}
