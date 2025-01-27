use kitsune2_api::{builder::Builder, config::Config};
use kitsune2_core::{
    factories::{self, MemOpStoreFactory},
    Ed25519Verifier,
};
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
/// - `peer_meta_store` - The default peer meta store is [factories::MemPeerMetaStoreFactory].
/// - `gossip` - The default gossip module is [factories::CoreGossipStubFactory].
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
        gossip: factories::CoreGossipStubFactory::create(),
        local_agent_store: factories::CoreLocalAgentStoreFactory::create(),
    }
}

#[cfg(test)]
mod test {
    use crate::default_builder;
    use bytes::Bytes;
    use kitsune2_api::{
        kitsune::KitsuneHandler,
        space::{DynSpaceHandler, SpaceHandler},
        AgentId, BoxFut, K2Result, SpaceId, Url,
    };
    use kitsune2_core::factories::MemOpStoreFactory;
    use kitsune2_test_utils::{
        agent::{AgentBuilder, TestLocalAgent},
        space::TEST_SPACE_ID,
    };
    use kitsune2_transport_tx5::config::{
        Tx5TransportConfig, Tx5TransportModConfig,
    };
    use sbd_server::SbdServer;
    use std::{
        sync::{Arc, Mutex},
        time::Duration,
    };
    use tokio::sync::mpsc::{Receiver, Sender};

    #[derive(Debug)]
    struct HolochainKitsuneHandler {
        url: Mutex<Url>,
        notify_received_receiver: Mutex<Option<Receiver<Bytes>>>,
    }
    impl KitsuneHandler for HolochainKitsuneHandler {
        fn create_space(
            &self,
            _space_id: SpaceId,
        ) -> BoxFut<'_, K2Result<DynSpaceHandler>> {
            Box::pin(async {
                let (tx, rx) = tokio::sync::mpsc::channel(1);
                let space_handler: DynSpaceHandler =
                    Arc::new(HolochainSpaceHandler {
                        notify_received_sender: tx,
                    });
                *self.notify_received_receiver.lock().unwrap() = Some(rx);
                Ok(space_handler)
            })
        }
        fn new_listening_address(&self, this_url: Url) -> BoxFut<'static, ()> {
            *(self.url.lock().unwrap()) = this_url;
            Box::pin(async {})
        }
    }

    #[derive(Debug)]
    struct HolochainSpaceHandler {
        notify_received_sender: Sender<Bytes>,
    }
    impl SpaceHandler for HolochainSpaceHandler {
        fn recv_notify(
            &self,
            _to_agent: AgentId,
            _from_agent: AgentId,
            _space: SpaceId,
            data: Bytes,
        ) -> K2Result<()> {
            self.notify_received_sender.try_send(data).unwrap();
            Ok(())
        }
    }

    #[tokio::test]
    async fn create_kitsune_instance() {
        let signal_server = SbdServer::new(Arc::new(sbd_server::Config {
            bind: vec!["127.0.0.1:0".to_string()],
            ..Default::default()
        }))
        .await
        .unwrap();
        let server_url =
            format!("ws://{}", signal_server.bind_addrs()[0].to_string());

        // Create 2 Kitsune instances and 1 space each.
        let mut kitsune_builder_1 =
            default_builder().with_default_config().unwrap();
        kitsune_builder_1.op_store = MemOpStoreFactory::create();
        let kitsune_builder_1 = Arc::new(kitsune_builder_1);
        kitsune_builder_1
            .config
            .set_module_config(&Tx5TransportModConfig {
                tx5_transport: Tx5TransportConfig {
                    server_url: server_url.clone().to_string(),
                    signal_allow_plain_text: true,
                },
            })
            .unwrap();

        let kitsune_handler_1 = Arc::new(HolochainKitsuneHandler {
            url: Mutex::new(Url::from_str("ws://placeholder.url:1").unwrap()),
            notify_received_receiver: Mutex::new(None),
        });
        let kitsune_1 = kitsune_builder_1
            .kitsune
            .create(kitsune_builder_1.clone(), kitsune_handler_1.clone())
            .await
            .unwrap();
        let agent_1_url = kitsune_handler_1.url.lock().unwrap().clone();

        let agent_1 = AgentBuilder {
            url: Some(Some(agent_1_url.clone())),
            ..Default::default()
        }
        .build(TestLocalAgent::default());

        let space_1 = kitsune_1.space(TEST_SPACE_ID).await.unwrap();

        let kitsune_builder_2 =
            Arc::new(default_builder().with_default_config().unwrap());
        kitsune_builder_2
            .config
            .set_module_config(&Tx5TransportModConfig {
                tx5_transport: Tx5TransportConfig {
                    server_url: server_url.to_string(),
                    signal_allow_plain_text: true,
                },
            })
            .unwrap();

        let kitsune_handler_2 = Arc::new(HolochainKitsuneHandler {
            url: Mutex::new(Url::from_str("ws://placeholder.url:2").unwrap()),
            notify_received_receiver: Mutex::new(None),
        });
        let kitsune_2 = kitsune_builder_2
            .kitsune
            .create(kitsune_builder_2.clone(), kitsune_handler_2.clone())
            .await
            .unwrap();
        let agent_2_url = kitsune_handler_2.url.lock().unwrap().clone();

        let agent_2 = AgentBuilder {
            url: Some(Some(agent_2_url.clone())),
            ..Default::default()
        }
        .build(TestLocalAgent::default());

        let _space_2 = kitsune_2.space(TEST_SPACE_ID).await.unwrap();

        // Inject agent 2 into agent 1's peer store.
        space_1
            .peer_store()
            .insert(vec![agent_2.clone()])
            .await
            .unwrap();

        // Send a space notification and await it being received.
        let expected_data = Bytes::from_static(b"space_man");
        space_1
            .send_notify(
                agent_2.agent.clone(),
                agent_1.agent.clone(),
                expected_data.clone(),
            )
            .await
            .unwrap();

        tokio::time::timeout(Duration::from_millis(100), async {
            let actual_data = kitsune_handler_2
                .notify_received_receiver
                .lock()
                .unwrap()
                .as_mut()
                .unwrap()
                .recv()
                .await
                .unwrap();
            assert_eq!(actual_data, expected_data);
        })
        .await
        .unwrap();
    }
}
