//! Kitsune2 - 2nd generation peer-to-peer communication framework.
//!
//! This is the top-level crate of the Kitsune2 framework. It only contains a
//! production builder for creating instances using the factory pattern. The
//! individual components of Kitsune2 provide more information about its functionality
//! and types.
//!
//! Kitsune2 is the reference implementation of the [Kitsune2 API](kitsune2_api)
//!
//! [DHT](https://docs.rs/kitsune2_dht/latest/kitsune2_dht/)  
//! [Gossip protocol](https://github.com/holochain/kitsune2/blob/main/crates/gossip/README.md)  
//! [Bootstrap server](https://docs.rs/kitsune2_bootstrap_srv/latest/kitsune2_bootstrap_srv/)  
//! [Core modules](kitsune2_core)  

use kitsune2_api::*;
use kitsune2_core::{
    factories::{self, MemOpStoreFactory},
    Ed25519Verifier,
};
use kitsune2_gossip::K2GossipFactory;
use kitsune2_transport_tx5::Tx5TransportFactory;

/// Construct a default production builder for Kitsune2.
///
/// - `verifier` - The default verifier is [Ed25519Verifier].
/// - `kitsune` - The default top-level kitsune module is [factories::CoreKitsuneFactory].
/// - `space` - The default space module is [factories::CoreSpaceFactory].
/// - `peer_store` - The default peer store is [factories::MemPeerStoreFactory].
/// - `bootstrap` - The default bootstrap is [factories::CoreBootstrapFactory].
/// - `fetch` - The default fetch module is [factories::CoreFetchFactory].
/// - `transport` - The default transport is [Tx5TransportFactory].
/// - `op_store` - The default op store is [MemOpStoreFactory].
///                Note: you will likely want to implement your own op store.
/// - `peer_meta_store` - The default peer meta store is [factories::MemPeerMetaStoreFactory].
///                       Note: you will likely want to implement your own peer meta store.
/// - `gossip` - The default gossip module is [K2GossipFactory].
/// - `local_agent_store` - The default local agent store is [factories::CoreLocalAgentStoreFactory].
/// - `publish` - The default publish module is [factories::CorePublishFactory].
pub fn default_builder() -> Builder {
    Builder {
        config: Config::default(),
        verifier: std::sync::Arc::new(Ed25519Verifier),
        kitsune: factories::CoreKitsuneFactory::create(),
        space: factories::CoreSpaceFactory::create(),
        peer_store: factories::MemPeerStoreFactory::create(),
        bootstrap: factories::CoreBootstrapFactory::create(),
        fetch: factories::CoreFetchFactory::create(),
        transport: Tx5TransportFactory::create(),
        op_store: MemOpStoreFactory::create(),
        peer_meta_store: factories::MemPeerMetaStoreFactory::create(),
        gossip: K2GossipFactory::create(),
        local_agent_store: factories::CoreLocalAgentStoreFactory::create(),
        publish: factories::CorePublishFactory::create(),
    }
}

#[cfg(test)]
mod test {
    use crate::default_builder;
    use bytes::Bytes;
    use kitsune2_api::{
        BoxFut, DhtArc, DynKitsune, DynSpace, DynSpaceHandler, Id, K2Result,
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
            let op =
                MemoryOp::new(Timestamp::from_micros(0), random_bytes(256));
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
        space_id: SpaceId,
    ) -> (DynSpace, DynKitsune) {
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
        let space = kitsune.space(space_id).await.unwrap();

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

        (space, kitsune)
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
        let signal_server_url =
            format!("ws://{}", signal_server.bind_addrs()[0]);

        let bootstrap_server = TestBootstrapSrv::new(false).await;
        let bootstrap_server_url = bootstrap_server.addr().to_string();

        // Create 2 Kitsune instances and 1 space with 1 joined agent each.
        let (space_1, _kitsune_1) = make_kitsune_node(
            &signal_server_url,
            &bootstrap_server_url,
            TEST_SPACE_ID,
        )
        .await;
        let (space_2, _kitsune_2) = make_kitsune_node(
            &signal_server_url,
            &bootstrap_server_url,
            TEST_SPACE_ID,
        )
        .await;

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

    #[tokio::test]
    async fn track_sent_bytes_same_space() {
        use std::time::Duration;
        use tokio::time::sleep;

        // Create a signaling server to help nodes discover each other
        let signal_server = SbdServer::new(Arc::new(sbd_server::Config {
            bind: vec!["127.0.0.1:0".to_string()],
            ..Default::default()
        }))
        .await
        .unwrap();
        let signal_server_url =
            format!("ws://{}", signal_server.bind_addrs()[0]);

        // Create a bootstrap server for node discovery
        let bootstrap_server = TestBootstrapSrv::new(false).await;
        let bootstrap_server_url = bootstrap_server.addr().to_string();

        // Create two nodes in the same space
        let (space_1, _kitsune_1) = make_kitsune_node(
            &signal_server_url,
            &bootstrap_server_url,
            TEST_SPACE_ID,
        )
        .await;
        let (space_2, _kitsune_2) = make_kitsune_node(
            &signal_server_url,
            &bootstrap_server_url,
            TEST_SPACE_ID,
        )
        .await;

        let tracker1 = space_1.get_bandwidth_tracker();
        let tracker2 = space_2.get_bandwidth_tracker();

        // Prepare a 10-byte payload
        let payload =
            bytes::Bytes::from_static(&[0, 1, 2, 3, 4, 5, 6, 7, 8, 9]);

        // Send 2 messages from space_1 to space_2 (2 * 10 bytes = 20)
        let _ = space_1
            .send_notify(space_2.current_url().unwrap(), payload.clone())
            .await;
        let _ = space_1
            .send_notify(space_2.current_url().unwrap(), payload.clone())
            .await;

        // Send 1 message from space_2 to space_1 (1 * 10 bytes = 10)
        let _ = space_2
            .send_notify(space_1.current_url().unwrap(), payload.clone())
            .await;

        // Allow time for all messages to be processed
        sleep(Duration::from_secs(2)).await;

        // Validate bandwidth usage after 3 messages
        assert_eq!(
            20,
            tracker1
                .get_space_stats(TEST_SPACE_ID)
                .unwrap()
                .bytes_sent()
        );
        assert_eq!(
            10,
            tracker2
                .get_space_stats(TEST_SPACE_ID)
                .unwrap()
                .bytes_sent()
        );
        assert_eq!(
            10,
            tracker1
                .get_space_stats(TEST_SPACE_ID)
                .unwrap()
                .bytes_received()
        );
        assert_eq!(
            20,
            tracker2
                .get_space_stats(TEST_SPACE_ID)
                .unwrap()
                .bytes_received()
        );

        // Send one more message from space_1 to space_2
        let _ = space_1
            .send_notify(space_2.current_url().unwrap(), payload.clone())
            .await;

        // Wait again for processing
        sleep(Duration::from_secs(2)).await;

        // Final expected bandwidth stats:
        // space_1: sent 30 (3 messages), received 10 (1 message)
        // space_2: sent 10 (1 message), received 30 (3 messages)
        assert_eq!(
            30,
            tracker1
                .get_space_stats(TEST_SPACE_ID)
                .unwrap()
                .bytes_sent()
        );
        assert_eq!(
            30,
            tracker2
                .get_space_stats(TEST_SPACE_ID)
                .unwrap()
                .bytes_received()
        );
        assert_eq!(
            10,
            tracker1
                .get_space_stats(TEST_SPACE_ID)
                .unwrap()
                .bytes_received()
        );
        assert_eq!(
            10,
            tracker2
                .get_space_stats(TEST_SPACE_ID)
                .unwrap()
                .bytes_sent()
        );
    }

    #[tokio::test]
    async fn track_sent_bytes_different_space() {
        use std::time::Duration;
        use tokio::time::sleep;

        // Create signaling and bootstrap servers
        let signal_server = SbdServer::new(Arc::new(sbd_server::Config {
            bind: vec!["127.0.0.1:0".to_string()],
            ..Default::default()
        }))
        .await
        .unwrap();
        let signal_server_url =
            format!("ws://{}", signal_server.bind_addrs()[0]);
        let bootstrap_server = TestBootstrapSrv::new(false).await;
        let bootstrap_server_url = bootstrap_server.addr().to_string();

        // Create three nodes in different spaces
        let space_id1 = SpaceId(Id(Bytes::from_static(b"space1")));
        let space_id2 = SpaceId(Id(Bytes::from_static(b"space2")));
        let space_id3 = SpaceId(Id(Bytes::from_static(b"space3")));
        let (space_1, _kitsune_1) = make_kitsune_node(
            &signal_server_url,
            &bootstrap_server_url,
            space_id1.clone(),
        )
        .await;
        let (space_2, _kitsune_2) = make_kitsune_node(
            &signal_server_url,
            &bootstrap_server_url,
            space_id2.clone(),
        )
        .await;
        let (space_3, _kitsune_3) = make_kitsune_node(
            &signal_server_url,
            &bootstrap_server_url,
            space_id3.clone(),
        )
        .await;

        let tracker1 = space_1.get_bandwidth_tracker();
        let tracker2 = space_2.get_bandwidth_tracker();
        let tracker3 = space_3.get_bandwidth_tracker();

        // Create a 10-byte payload
        let payload =
            bytes::Bytes::from_static(&[0, 1, 2, 3, 4, 5, 6, 7, 8, 9]);

        // Send 10 bytes from:
        // - space_1 → space_3
        // - space_2 → space_3
        // - space_3 → space_2
        let _ = space_1
            .send_notify(space_3.current_url().unwrap(), payload.clone())
            .await;
        let _ = space_2
            .send_notify(space_3.current_url().unwrap(), payload.clone())
            .await;
        let _ = space_3
            .send_notify(space_2.current_url().unwrap(), payload.clone())
            .await;

        // Wait to ensure delivery and processing
        sleep(Duration::from_secs(5)).await;

        // Expected:
        // space_1: sent 10, received 0
        // space_2: sent 10, received 10
        // space_3: sent 10, received 20
        assert_eq!(
            10,
            tracker1
                .get_space_stats(space_id1.clone())
                .unwrap()
                .bytes_sent()
        );
        assert_eq!(
            10,
            tracker2
                .get_space_stats(space_id2.clone())
                .unwrap()
                .bytes_sent()
        );
        assert_eq!(
            10,
            tracker3
                .get_space_stats(space_id3.clone())
                .unwrap()
                .bytes_sent()
        );

        assert_eq!(
            0,
            tracker1
                .get_space_stats(space_id1)
                .unwrap()
                .bytes_received()
        );
        assert_eq!(
            0,
            tracker2
                .get_space_stats(space_id2)
                .unwrap()
                .bytes_received()
        );
        assert_eq!(
            0,
            tracker3
                .get_space_stats(space_id3)
                .unwrap()
                .bytes_received()
        );
    }
}
