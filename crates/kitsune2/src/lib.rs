use kitsune2_api::{builder::Builder, config::Config};
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
///                Note: you will likely want to implement your own op_store.
/// - `peer_meta_store` - The default peer meta store is [factories::MemPeerMetaStoreFactory].
/// - `gossip` - The default gossip module is [K2GossipFactory].
/// - `local_agent_store` - The default local agent store is [factories::CoreLocalAgentStoreFactory].
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
    }
}

#[cfg(test)]
mod test {
    use crate::default_builder;
    use bytes::Bytes;
    use kitsune2_api::{
        agent::LocalAgent,
        kitsune::KitsuneHandler,
        space::{DynSpace, DynSpaceHandler, SpaceHandler},
        BoxFut, K2Result, OpId, SpaceId, Timestamp,
    };
    use kitsune2_core::{
        factories::{
            core_bootstrap::config::{
                CoreBootstrapConfig, CoreBootstrapModConfig,
            },
            MemOpStoreFactory, MemoryOp,
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
            let op = MemoryOp::new(Timestamp::now(), random_bytes(32));
            let op_id = op.compute_op_id();
            ops.push(op.into());
            op_ids.push(op_id);
        }
        (ops, op_ids)
    }

    #[derive(Debug)]
    struct HolochainKitsuneHandler;
    impl KitsuneHandler for HolochainKitsuneHandler {
        fn create_space(
            &self,
            _space_id: SpaceId,
        ) -> BoxFut<'_, K2Result<DynSpaceHandler>> {
            Box::pin(async {
                let space_handler: DynSpaceHandler =
                    Arc::new(HolochainSpaceHandler);
                Ok(space_handler)
            })
        }
    }

    #[derive(Debug)]
    struct HolochainSpaceHandler;
    impl SpaceHandler for HolochainSpaceHandler {}

    async fn make_kitsune_space(
        signal_server_url: &str,
        bootstrap_server_url: &str,
    ) -> DynSpace {
        let mut kitsune_builder =
            default_builder().with_default_config().unwrap();
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
        kitsune_builder.op_store = MemOpStoreFactory::create();
        kitsune_builder
            .config
            .set_module_config(&Tx5TransportModConfig {
                tx5_transport: Tx5TransportConfig {
                    server_url: signal_server_url.to_owned(),
                    signal_allow_plain_text: true,
                },
            })
            .unwrap();
        kitsune_builder
            .config
            .set_module_config(&K2GossipModConfig {
                k2_gossip: K2GossipConfig {
                    initiate_interval_ms: 100,
                    min_initiate_interval_ms: 75,
                    round_timeout_ms: 1000,
                    ..Default::default()
                },
            })
            .unwrap();

        let kitsune_handler = Arc::new(HolochainKitsuneHandler);
        let kitsune = kitsune_builder
            .build(kitsune_handler.clone())
            .await
            .unwrap();
        kitsune.space(TEST_SPACE_ID).await.unwrap()
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

        // Create 2 Kitsune instances and 1 space each.
        let space_1 =
            make_kitsune_space(&signal_server_url, &bootstrap_server_url).await;
        let space_2 =
            make_kitsune_space(&signal_server_url, &bootstrap_server_url).await;

        // Insert ops into both spaces' op stores.
        let (ops_1, op_ids_1) = create_op_list(1);
        space_1
            .op_store()
            .process_incoming_ops(ops_1.clone())
            .await
            .unwrap();
        let (ops_2, op_ids_2) = create_op_list(1);
        space_2
            .op_store()
            .process_incoming_ops(ops_2.clone())
            .await
            .unwrap();

        // Create 2 agents.
        let local_agent_1 = Arc::new(Ed25519LocalAgent::default());
        let local_agent_2 = Arc::new(Ed25519LocalAgent::default());

        // Join agent 1 to local space.
        space_1
            .local_agent_join(local_agent_1.clone())
            .await
            .unwrap();

        // Wait for agent_1 to publish their info to the bootstrap & peer store.
        iter_check!(5000, 100, {
            let space_1_agents = space_1.peer_store().get_all().await.unwrap();
            if space_1_agents
                .iter()
                .any(|agent| agent.agent == *local_agent_1.agent())
            {
                break;
            }
        });

        // Join agent 2 to local space.
        space_2
            .local_agent_join(local_agent_2.clone())
            .await
            .unwrap();

        // Wait for agent 2 to publish their info and agent 1 to be polled from
        // bootstrap.
        iter_check!(5000, 100, {
            let space_2_agents = space_2.peer_store().get_all().await.unwrap();
            if space_2_agents.len() == 2 {
                break;
            }
        });

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
                    "space 1 actual ops {}/expected {}",
                    actual_ops_1.len(),
                    ops_2.len()
                );
                println!(
                    "space 2 actual ops {}/expected {}",
                    actual_ops_2.len(),
                    ops_1.len()
                );
            }
        });
    }
}
