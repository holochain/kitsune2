use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bytes::Bytes;
use kitsune2_api::{AgentId, Id, LocalAgent, MessageBlockCount, SpaceId};
use kitsune2_api::{
    AgentInfoSigned, BlockTarget, BoxFut, Builder, DynSpace, DynTransport,
    K2Result, SpaceHandler, TxBaseHandler, TxHandler, TxModuleHandler, Url,
};
use kitsune2_core::factories::CoreGossipStubFactory;
use kitsune2_core::{Ed25519LocalAgent, Ed25519Verifier};
use kitsune2_test_utils::agent::AgentBuilder;
use kitsune2_test_utils::iter_check;
use kitsune2_test_utils::noop_bootstrap::NoopBootstrapFactory;
use kitsune2_test_utils::{enable_tracing, space::TEST_SPACE_ID};
#[cfg(all(
    not(feature = "transport-tx5-backend-libdatachannel"),
    not(feature = "transport-tx5-backend-go-pion"),
    not(feature = "transport-tx5-datachannel-vendored"),
    feature = "transport-iroh"
))]
use kitsune2_transport_iroh::{
    config::{IrohTransportConfig, IrohTransportModConfig},
    test_utils::{spawn_iroh_relay_server, Server},
    IrohTransportFactory,
};

use std::sync::mpsc::{Receiver, Sender};
use tokio::sync::OnceCell;
#[cfg(any(
    feature = "transport-tx5-backend-libdatachannel",
    feature = "transport-tx5-backend-go-pion",
    feature = "transport-tx5-datachannel-vendored"
))]
use {kitsune2_transport_tx5::Tx5TransportFactory, sbd_server::SbdServer};

/// A default test space handler that just drops received messages.
#[derive(Debug)]
struct TestSpaceHandler {
    recv_notify_sender: Sender<bytes::Bytes>,
}

impl TestSpaceHandler {
    fn create() -> (Self, Receiver<bytes::Bytes>) {
        let (send, recv) = std::sync::mpsc::channel();
        (
            Self {
                recv_notify_sender: send,
            },
            recv,
        )
    }
}

impl SpaceHandler for TestSpaceHandler {
    fn recv_notify(
        &self,
        _from_peer: Url,
        _space_id: kitsune2_api::SpaceId,
        data: bytes::Bytes,
    ) -> K2Result<()> {
        self.recv_notify_sender.send(data).unwrap();
        Ok(())
    }
}

/// A test TxHandler
#[derive(Debug)]
pub struct TestTxHandler {
    pub peer_url: std::sync::Mutex<Url>,
    space: Arc<OnceCell<DynSpace>>,
    recv_module_msg_sender: Sender<bytes::Bytes>,
    peer_disconnect_sender: Sender<Url>,
}

impl TestTxHandler {
    fn create(
        peer_url: Mutex<Url>,
        space: Arc<OnceCell<DynSpace>>,
    ) -> (Arc<Self>, Receiver<bytes::Bytes>, Receiver<Url>) {
        let (recv_module_msg_sender, recv_module_msg_recv) =
            std::sync::mpsc::channel();
        let (peer_disconnect_sender, peer_disconnect_recv) =
            std::sync::mpsc::channel();

        (
            Arc::new(Self {
                peer_url,
                space,
                recv_module_msg_sender,
                peer_disconnect_sender,
            }),
            recv_module_msg_recv,
            peer_disconnect_recv,
        )
    }
}

impl TxModuleHandler for TestTxHandler {
    fn recv_module_msg(
        &self,
        _peer: Url,
        _space_id: kitsune2_api::SpaceId,
        _module: String,
        data: bytes::Bytes,
    ) -> K2Result<()> {
        self.recv_module_msg_sender.send(data).unwrap();
        Ok(())
    }
}

impl TxBaseHandler for TestTxHandler {
    fn new_listening_address(&self, this_url: Url) -> BoxFut<'static, ()> {
        *(self.peer_url.lock().unwrap()) = this_url;
        Box::pin(async {})
    }

    fn peer_disconnect(&self, peer: Url, _reason: Option<String>) {
        if self.peer_disconnect_sender.send(peer).is_err() {
            tracing::error!("Failed to send peer disconnect. This is okay if it happens at the end of a test if the receiver has been dropped before the connection got dropped.");
        };
    }
}
impl TxHandler for TestTxHandler {
    fn preflight_gather_outgoing(
        &self,
        _peer_url: Url,
    ) -> BoxFut<'_, K2Result<bytes::Bytes>> {
        let space = self
            .space
            .get()
            .expect("Space OnceCell has not been initialized in time.")
            .clone();

        Box::pin(async move {
            let agents = space.peer_store().get_all().await.unwrap();
            let agents_encoded: Vec<String> =
                agents.into_iter().map(|a| a.encode().unwrap()).collect();
            Ok(serde_json::to_vec(&agents_encoded).unwrap().into())
        })
    }

    fn preflight_validate_incoming(
        &self,
        _peer_url: Url,
        data: bytes::Bytes,
    ) -> BoxFut<'_, K2Result<()>> {
        let agents_encoded: Vec<String> =
            serde_json::from_slice(&data).unwrap();

        let agents: Vec<Arc<AgentInfoSigned>> = agents_encoded
            .iter()
            .map(|a| {
                AgentInfoSigned::decode(&Ed25519Verifier, a.as_bytes()).unwrap()
            })
            .collect();

        let space = self
            .space
            .get()
            .expect("Space OnceCell has not been initialized in time.")
            .clone();

        Box::pin(async move {
            space.peer_store().insert(agents).await?;
            Ok(())
        })
    }
}

#[cfg(any(
    feature = "transport-tx5-backend-libdatachannel",
    feature = "transport-tx5-backend-go-pion",
    feature = "transport-tx5-datachannel-vendored"
))]
async fn builder_with_tx5() -> (Arc<Builder>, SbdServer) {
    let sbd_server = SbdServer::new(Arc::new(sbd_server::Config {
        bind: vec!["127.0.0.1:0".to_string()],
        ..Default::default()
    }))
    .await
    .unwrap();
    let signal_server_url = format!("ws://{}", sbd_server.bind_addrs()[0]);

    let builder = Builder {
        transport: Tx5TransportFactory::create(),
        gossip: CoreGossipStubFactory::create(),
        bootstrap: Arc::new(NoopBootstrapFactory {}),
        ..kitsune2_core::default_test_builder()
    }
    .with_default_config()
    .unwrap();

    builder
        .config
        .set_module_config(
            &kitsune2_transport_tx5::config::Tx5TransportModConfig {
                tx5_transport:
                    kitsune2_transport_tx5::config::Tx5TransportConfig {
                        signal_allow_plain_text: true,
                        server_url: signal_server_url,
                        ..Default::default()
                    },
            },
        )
        .unwrap();

    (Arc::new(builder), sbd_server)
}

#[cfg(all(
    not(feature = "transport-tx5-backend-libdatachannel"),
    not(feature = "transport-tx5-backend-go-pion"),
    not(feature = "transport-tx5-datachannel-vendored"),
    feature = "transport-iroh"
))]
async fn builder_with_iroh() -> (Arc<Builder>, Server) {
    let (_, relay_server_url, relay_server) = spawn_iroh_relay_server().await;
    let builder = Builder {
        transport: IrohTransportFactory::create(),
        gossip: CoreGossipStubFactory::create(),
        bootstrap: Arc::new(NoopBootstrapFactory {}),
        ..kitsune2_core::default_test_builder()
    }
    .with_default_config()
    .unwrap();

    builder
        .config
        .set_module_config(&IrohTransportModConfig {
            iroh_transport: IrohTransportConfig {
                relay_url: Some(relay_server_url.to_string()),
                ..Default::default()
            },
        })
        .unwrap();

    (Arc::new(builder), relay_server)
}

macro_rules! builder_with_relay {
    () => {{
        #[cfg(any(
            feature = "transport-tx5-backend-libdatachannel",
            feature = "transport-tx5-backend-go-pion",
            feature = "transport-tx5-datachannel-vendored"
        ))]
        {
            builder_with_tx5().await
        }

        #[cfg(all(
            not(feature = "transport-tx5-backend-libdatachannel"),
            not(feature = "transport-tx5-backend-go-pion"),
            not(feature = "transport-tx5-datachannel-vendored"),
            feature = "transport-iroh"
        ))]
        {
            builder_with_iroh().await
        }
    }};
}

pub struct TestPeer {
    space: DynSpace,
    transport: DynTransport,
    peer_url: Url,
    agent_id: AgentId,
    agent_info: Arc<AgentInfoSigned>,
    recv_notify_recv: Receiver<bytes::Bytes>,
    recv_module_msg_recv: Receiver<bytes::Bytes>,
    peer_disconnect_recv: Receiver<Url>,
}

pub async fn make_test_peer(builder: Arc<Builder>) -> TestPeer {
    let space_once_cell = Arc::new(OnceCell::new());

    let (tx_handler, recv_module_msg_recv, peer_disconnect_recv) =
        TestTxHandler::create(
            Mutex::new(
                // Placeholder URL which will be overwritten when the transport is created.
                Url::from_str("ws://127.0.0.1:80").unwrap(),
            ),
            space_once_cell.clone(),
        );

    let transport = builder
        .transport
        .create(builder.clone(), tx_handler.clone())
        .await
        .unwrap();

    transport.register_module_handler(TEST_SPACE_ID, "test".into(), tx_handler);

    // It may take a while until the peer url shows up in the transport stats
    let peer_url = iter_check!(5000, 100, {
        let stats = transport.dump_network_stats().await.unwrap();
        let peer_url = stats.transport_stats.peer_urls.first();
        if let Some(url) = peer_url {
            return url.clone();
        }
    });

    let (space_handler, recv_notify_recv) = TestSpaceHandler::create();

    let report = builder
        .report
        .create(builder.clone(), transport.clone())
        .await
        .unwrap();

    let space = builder
        .space
        .create(
            builder.clone(),
            None,
            Arc::new(space_handler),
            TEST_SPACE_ID,
            report,
            transport.clone(),
        )
        .await
        .unwrap();

    // join with one local agent
    let agent_info =
        join_new_local_agent_and_wait_for_agent_info(space.clone()).await;

    space_once_cell.set(space.clone()).unwrap();

    TestPeer {
        space,
        transport,
        agent_id: agent_info.agent.clone(),
        agent_info,
        peer_url,
        recv_notify_recv,
        recv_module_msg_recv,
        peer_disconnect_recv,
    }
}

const TEST_SPACE_ID_1: SpaceId =
    SpaceId(Id(Bytes::from_static(b"test_space_1")));
const TEST_SPACE_ID_2: SpaceId =
    SpaceId(Id(Bytes::from_static(b"test_space_2")));

pub struct TestPeerLight {
    space1: DynSpace,
    space2: DynSpace,
    dummy_agent_info_1: Arc<AgentInfoSigned>,
    dummy_agent_info_2: Arc<AgentInfoSigned>,
    transport: DynTransport,
    peer_url: Url,
}

pub async fn make_test_peer_light(builder: Arc<Builder>) -> TestPeerLight {
    #[derive(Debug)]
    struct NoopHandler;
    impl TxHandler for NoopHandler {}
    impl TxBaseHandler for NoopHandler {}
    impl SpaceHandler for NoopHandler {}

    let transport = builder
        .transport
        .create(builder.clone(), Arc::new(NoopHandler))
        .await
        .unwrap();

    // It may take a while until the peer url shows up in the transport stats
    let peer_url = iter_check!(5000, {
        let stats = transport.dump_network_stats().await.unwrap();
        let peer_url = stats.transport_stats.peer_urls.first();
        if let Some(url) = peer_url {
            return url.clone();
        }
    });

    let report = builder
        .report
        .create(builder.clone(), transport.clone())
        .await
        .unwrap();

    let space1 = builder
        .space
        .create(
            builder.clone(),
            None,
            Arc::new(NoopHandler),
            TEST_SPACE_ID_1,
            report.clone(),
            transport.clone(),
        )
        .await
        .unwrap();

    let space2 = builder
        .space
        .create(
            builder.clone(),
            None,
            Arc::new(NoopHandler),
            TEST_SPACE_ID_2,
            report,
            transport.clone(),
        )
        .await
        .unwrap();

    // Create two dummy agents that can be used to be added to the peer store
    // of a sending peer such that the message doesn't get blocked before
    // actually sending, due to missing peers in the peer store.
    //
    // Using the normal local_agent_join() method on a space would lead to
    // agent infos getting published via module messages and these module
    // messages would in turn get blocked due to missing agent infos in the
    // peer store which would consequently distort the blocks count that we
    // want to test the proper functioning of.
    let local_agent_1 = Ed25519LocalAgent::default();

    let dummy_agent_info_1 = AgentBuilder::default()
        .with_space(TEST_SPACE_ID_1)
        .with_url(Some(peer_url.clone()))
        .build(local_agent_1);

    let local_agent_2 = Ed25519LocalAgent::default();

    let dummy_agent_info_2 = AgentBuilder::default()
        .with_space(TEST_SPACE_ID_2)
        .with_url(Some(peer_url.clone()))
        .build(local_agent_2);

    TestPeerLight {
        space1,
        space2,
        dummy_agent_info_1,
        dummy_agent_info_2,
        transport,
        peer_url,
    }
}

/// A helper function that blocks an agent in the given space and also checks that:
/// - the agent block was not considered blocked in the blocks module before
/// - the agent is indeed considered blocked in the blocks module afterwards
async fn block_agent_in_space(agent: AgentId, space: DynSpace) {
    // Now we block Bob's agent
    let block_targets = vec![BlockTarget::Agent(agent.clone())];

    // Check that the Blocks module doesn't consider Bob's agent blocked prior to blocking
    let any_blocked = space
        .blocks()
        .is_any_blocked(block_targets.clone())
        .await
        .unwrap();
    assert!(!any_blocked);

    // Then block the agent
    space
        .blocks()
        .block(BlockTarget::Agent(agent.clone()))
        .await
        .unwrap();
    // After blocking, the agent must be removed from the peer store
    space.peer_store().remove(agent.clone()).await.unwrap();

    // Check that all agents are considered blocked now
    let any_blocked =
        space.blocks().is_any_blocked(block_targets).await.unwrap();
    assert!(any_blocked);
}

/// A helper function that joins with a new local agent to the given space,
/// then waits for the signed agent info to have been inserted into the peer
/// store and returns the signed agent info.
async fn join_new_local_agent_and_wait_for_agent_info(
    space: DynSpace,
) -> Arc<AgentInfoSigned> {
    let local_agent = Arc::new(Ed25519LocalAgent::default());
    space.local_agent_join(local_agent.clone()).await.unwrap();

    // Wait for the agent info to be published so we can get and return it
    tokio::time::timeout(std::time::Duration::from_secs(5), {
        let agent_id = local_agent.agent().clone();
        let peer_store = space.peer_store().clone();
        async move {
            while peer_store
                .get_all()
                .await
                .unwrap()
                .iter()
                .all(|a| a.agent.clone() != agent_id)
            {
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
        }
    })
    .await
    .unwrap();

    space
        .peer_store()
        .get(local_agent.agent().clone())
        .await
        .unwrap()
        .unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn incoming_message_block_count_increases_correctly() {
    enable_tracing();

    let (builder, _relay_server) = builder_with_relay!();

    let TestPeerLight {
        space1: space_alice_1,
        space2: space_alice_2,
        transport: transport_alice,
        peer_url: peer_url_alice,
        ..
    } = make_test_peer_light(builder.clone()).await;

    let TestPeerLight {
        space1: space_bob_1,
        space2: _space_bob_2, // Need to keep the space in memory here since it's being used to check for blocks
        transport: transport_bob,
        peer_url: peer_url_bob,
        ..
    } = make_test_peer_light(builder.clone()).await;

    let TestPeerLight {
        space1: _space_carol_1, // Need to keep the space in memory here since it's being used to check for blocks
        space2: _space_carol_2, // -- ditto --
        dummy_agent_info_1: dummy_agent_info_carol_1,
        dummy_agent_info_2: dummy_agent_info_carol_2,
        transport: transport_carol,
        peer_url: peer_url_carol,
    } = make_test_peer_light(builder.clone()).await;

    // Add Carol's dummy agent infos to Alice's peer stores in both spaces
    // to not have Alice consider Carol blocked when sending a message
    // due to missing agents in the peer store.
    space_alice_1
        .peer_store()
        .insert(vec![dummy_agent_info_carol_1.clone()])
        .await
        .unwrap();

    space_alice_2
        .peer_store()
        .insert(vec![dummy_agent_info_carol_2.clone()])
        .await
        .unwrap();

    // Verify that Carol's message blocks count is empty initially
    let stats = transport_carol.dump_network_stats().await.unwrap();
    assert_eq!(stats.blocked_message_counts.len(), 0);

    // Then have Alice send a message to Carol. Since we didn't add any agent
    // info of Alice to Carol's peer store, Alice is expected to be considered
    // blocked by Carol and Alice's message should be dropped by Carol.
    transport_alice
        .send_space_notify(
            peer_url_carol.clone(),
            TEST_SPACE_ID_1,
            Bytes::new(),
        )
        .await
        .unwrap();

    // Check that the incoming message blocks count goes up on Carol's side
    let expected_blocked_message_counts: HashMap<
        Url,
        HashMap<SpaceId, MessageBlockCount>,
    > = [(
        peer_url_alice.clone(),
        [(
            TEST_SPACE_ID_1,
            MessageBlockCount {
                incoming: 1,
                outgoing: 0,
            },
        )]
        .into(),
    )]
    .into();

    iter_check!(500, {
        let net_stats = transport_carol.dump_network_stats().await.unwrap();
        if net_stats.blocked_message_counts == expected_blocked_message_counts {
            break;
        }
    });

    // Send another one, the count should go up again
    transport_alice
        .send_space_notify(
            peer_url_carol.clone(),
            TEST_SPACE_ID_1,
            Bytes::new(),
        )
        .await
        .unwrap();
    let expected_blocked_message_counts: HashMap<
        Url,
        HashMap<SpaceId, MessageBlockCount>,
    > = [(
        peer_url_alice.clone(),
        [(
            TEST_SPACE_ID_1,
            MessageBlockCount {
                incoming: 2,
                outgoing: 0,
            },
        )]
        .into(),
    )]
    .into();
    iter_check!(500, {
        let net_stats = transport_carol.dump_network_stats().await.unwrap();
        if net_stats.blocked_message_counts == expected_blocked_message_counts {
            break;
        }
    });

    // Now send one to the second space
    transport_alice
        .send_space_notify(
            peer_url_carol.clone(),
            TEST_SPACE_ID_2,
            Bytes::new(),
        )
        .await
        .unwrap();
    let expected_blocked_message_counts: HashMap<
        Url,
        HashMap<SpaceId, MessageBlockCount>,
    > = [(
        peer_url_alice.clone(),
        [
            (
                TEST_SPACE_ID_1,
                MessageBlockCount {
                    incoming: 2,
                    outgoing: 0,
                },
            ),
            (
                TEST_SPACE_ID_2,
                MessageBlockCount {
                    incoming: 1,
                    outgoing: 0,
                },
            ),
        ]
        .into(),
    )]
    .into();
    iter_check!(500, {
        let net_stats = transport_carol.dump_network_stats().await.unwrap();
        if net_stats.blocked_message_counts == expected_blocked_message_counts {
            break;
        }
    });

    // Add Carol's dummy agent infos to Bob's peer store in space 1
    // to not have Bob consider Carol blocked when sending a message
    // due to missing agents in the peer store.
    space_bob_1
        .peer_store()
        .insert(vec![dummy_agent_info_carol_1.clone()])
        .await
        .unwrap();

    // And finally send one to Bob and verify that the HashMap gets updated correctly as well
    transport_bob
        .send_space_notify(
            peer_url_carol.clone(),
            TEST_SPACE_ID_1,
            Bytes::new(),
        )
        .await
        .unwrap();

    let expected_blocked_message_counts: HashMap<
        Url,
        HashMap<SpaceId, MessageBlockCount>,
    > = [
        (
            peer_url_alice.clone(),
            [
                (
                    TEST_SPACE_ID_1,
                    MessageBlockCount {
                        incoming: 2,
                        outgoing: 0,
                    },
                ),
                (
                    TEST_SPACE_ID_2,
                    MessageBlockCount {
                        incoming: 1,
                        outgoing: 0,
                    },
                ),
            ]
            .into(),
        ),
        (
            peer_url_bob,
            [(
                TEST_SPACE_ID_1,
                MessageBlockCount {
                    incoming: 1,
                    outgoing: 0,
                },
            )]
            .into(),
        ),
    ]
    .into();
    iter_check!(500, {
        let net_stats = transport_carol.dump_network_stats().await.unwrap();
        if net_stats.blocked_message_counts == expected_blocked_message_counts {
            break;
        }
    });
}

#[tokio::test(flavor = "multi_thread")]
async fn outgoing_message_block_count_increases_correctly() {
    enable_tracing();

    let (builder, _relay_server) = builder_with_relay!();

    let TestPeerLight {
        space1: _space_alice_1, // Need to keep the space in memory here since it's being used to check for blocks
        space2: _space_alice_2, // -- ditto --
        transport: transport_alice,
        ..
    } = make_test_peer_light(builder.clone()).await;

    let TestPeerLight {
        space1: _space_bob_1, // Need to keep the space in memory here since it's being used to check for blocks
        space2: _space_bob_2, // -- ditto --
        peer_url: peer_url_bob,
        ..
    } = make_test_peer_light(builder.clone()).await;

    let TestPeerLight {
        space1: _space_carol_1, // Need to keep the space in memory here since it's being used to check for blocks
        space2: _space_carol_2, // -- ditto --
        peer_url: peer_url_carol,
        ..
    } = make_test_peer_light(builder.clone()).await;

    // Verify that Alice's message blocks count is empty initially
    let stats = transport_alice.dump_network_stats().await.unwrap();
    assert_eq!(stats.blocked_message_counts.len(), 0);

    // Now have Alice send a message to Carol. Since we didn't add any agent
    // info of Carol to Alice's peer store, Carol is expected to be considered
    // blocked and the message should be dropped before being sent.
    transport_alice
        .send_space_notify(
            peer_url_carol.clone(),
            TEST_SPACE_ID_1,
            Bytes::new(),
        )
        .await
        .unwrap();

    // Check that the blocks count goes up on Alice's side
    let expected_blocked_message_counts: HashMap<
        Url,
        HashMap<SpaceId, MessageBlockCount>,
    > = [(
        peer_url_carol.clone(),
        [(
            TEST_SPACE_ID_1,
            MessageBlockCount {
                incoming: 0,
                outgoing: 1,
            },
        )]
        .into(),
    )]
    .into();
    iter_check!(500, {
        let net_stats = transport_alice.dump_network_stats().await.unwrap();
        if net_stats.blocked_message_counts == expected_blocked_message_counts {
            break;
        }
    });

    // Send another one, the count should go up
    transport_alice
        .send_space_notify(
            peer_url_carol.clone(),
            TEST_SPACE_ID_1,
            Bytes::new(),
        )
        .await
        .unwrap();
    let expected_blocked_message_counts: HashMap<
        Url,
        HashMap<SpaceId, MessageBlockCount>,
    > = [(
        peer_url_carol.clone(),
        [(
            TEST_SPACE_ID_1,
            MessageBlockCount {
                incoming: 0,
                outgoing: 2,
            },
        )]
        .into(),
    )]
    .into();
    iter_check!(500, {
        let net_stats = transport_alice.dump_network_stats().await.unwrap();
        if net_stats.blocked_message_counts == expected_blocked_message_counts {
            break;
        }
    });

    // Now send one to the second space
    transport_alice
        .send_space_notify(
            peer_url_carol.clone(),
            TEST_SPACE_ID_2,
            Bytes::new(),
        )
        .await
        .unwrap();
    let expected_blocked_message_counts: HashMap<
        Url,
        HashMap<SpaceId, MessageBlockCount>,
    > = [(
        peer_url_carol.clone(),
        [
            (
                TEST_SPACE_ID_1,
                MessageBlockCount {
                    incoming: 0,
                    outgoing: 2,
                },
            ),
            (
                TEST_SPACE_ID_2,
                MessageBlockCount {
                    incoming: 0,
                    outgoing: 1,
                },
            ),
        ]
        .into(),
    )]
    .into();
    iter_check!(500, {
        let net_stats = transport_alice.dump_network_stats().await.unwrap();
        if net_stats.blocked_message_counts == expected_blocked_message_counts {
            break;
        }
    });

    // And finally send one to Bob and verify that the HashMap gets updated correctly as well
    transport_alice
        .send_space_notify(peer_url_bob.clone(), TEST_SPACE_ID_1, Bytes::new())
        .await
        .unwrap();
    let expected_blocked_message_counts: HashMap<
        Url,
        HashMap<SpaceId, MessageBlockCount>,
    > = [
        (
            peer_url_carol.clone(),
            [
                (
                    TEST_SPACE_ID_1,
                    MessageBlockCount {
                        incoming: 0,
                        outgoing: 2,
                    },
                ),
                (
                    TEST_SPACE_ID_2,
                    MessageBlockCount {
                        incoming: 0,
                        outgoing: 1,
                    },
                ),
            ]
            .into(),
        ),
        (
            peer_url_bob,
            [(
                TEST_SPACE_ID_1,
                MessageBlockCount {
                    incoming: 0,
                    outgoing: 1,
                },
            )]
            .into(),
        ),
    ]
    .into();
    iter_check!(500, {
        let net_stats = transport_alice.dump_network_stats().await.unwrap();
        if net_stats.blocked_message_counts == expected_blocked_message_counts {
            break;
        }
    });
}

#[tokio::test(flavor = "multi_thread")]
async fn incoming_notify_messages_from_blocked_peers_are_dropped() {
    enable_tracing();

    let (builder, _relay_server) = builder_with_relay!();

    let TestPeer {
        space: space_alice,
        transport: transport_alice,
        peer_url: peer_url_alice,
        agent_id: agent_id_alice,
        peer_disconnect_recv: peer_disconnect_recv_alice,
        ..
    } = make_test_peer(builder.clone()).await;

    let TestPeer {
        space: space_bob,
        transport: transport_bob,
        peer_url: peer_url_bob,
        agent_info: agent_info_bob,
        recv_notify_recv: recv_notify_recv_bob,
        peer_disconnect_recv: _peer_disconnect_recv_bob,
        ..
    } = make_test_peer(builder).await;

    // Add Bob's agent to Alice's peer store.
    space_alice
        .peer_store()
        .insert(vec![agent_info_bob])
        .await
        .unwrap();

    // Alice sends a space message that should go through normally and establish
    // a connection with Bob
    let payload = Bytes::from("Hello world");

    // This send here fails relatively often in CI, presumably due to a race
    // condition in tx5: https://github.com/holochain/tx5/issues/193
    // The working hypothesis is that agent info gossip which starts in the background
    // after the local agent join may lead to an incoming connection being established
    // more or less simultaneously, thereby evoking the race condition.
    iter_check!(2_000, 500, {
        if transport_alice
            .send_space_notify(
                peer_url_bob.clone(),
                TEST_SPACE_ID,
                payload.clone(),
            )
            .await
            .is_ok()
        {
            break;
        }
    });

    // Verify that the space message has been handled by Bob's recv_space_notify hook
    let payload_received = recv_notify_recv_bob
        .recv_timeout(std::time::Duration::from_secs(2))
        .expect("timed out waiting for space message");
    assert_eq!(payload, payload_received);

    // Bob should have one open connection now and hasn't blocked any messages
    let net_stats_bob = transport_bob.dump_network_stats().await.unwrap();
    assert_eq!(net_stats_bob.transport_stats.connections.len(), 1);
    assert_eq!(net_stats_bob.blocked_message_counts.len(), 0);

    // Verify that Bob's peer store contains 2 agents now, his own agent as well as
    // Alice's agent that should have been added via the preflight.
    let agents_in_peer_store = space_bob.peer_store().get_all().await.unwrap();
    assert_eq!(agents_in_peer_store.len(), 2);

    // Now block Alice's agent.
    block_agent_in_space(agent_id_alice, space_bob.clone()).await;

    // Now send another message from Alice to Bob. Bob should reject it now and
    // close the connection.
    transport_alice
        .send_space_notify(
            peer_url_bob.clone(),
            TEST_SPACE_ID,
            Bytes::from("Sending to blocker"),
        )
        .await
        .unwrap();

    // Verify that Bob has blocked the message
    iter_check!(500, {
        let net_stats = transport_bob.dump_network_stats().await.unwrap();
        if net_stats.blocked_message_counts.len() == 1 {
            break;
        }
    });

    // Now have Bob close the connection so that we can verify later that
    // resending a message from Alice succeeds after she joins with a new
    // agent.
    transport_bob.disconnect(peer_url_alice, None).await;

    // Wait for Alice to disconnect as well. Otherwise, Alice may send the next
    // message over the old WebRTC connection still that Bob has already
    // closed on his end and Bob won't ever receive it.
    iter_check!(500, {
        if let Ok(peer_url) = peer_disconnect_recv_alice.try_recv() {
            if peer_url == peer_url_bob {
                break;
            }
        }
    });

    // To double-check, verify additionally that no space message has been
    // handled by Bob.
    assert!(recv_notify_recv_bob
        .recv_timeout(std::time::Duration::from_millis(50))
        .is_err());

    // Now have Alice join the space with a second agent which Bob should then
    // receive via the preflight and consequently let further messages through
    // again.
    let alice_local_agent_2 = Arc::new(Ed25519LocalAgent::default());
    space_alice
        .local_agent_join(alice_local_agent_2.clone())
        .await
        .unwrap();

    // Now send yet another message that should get through again since the preflight
    // should get through and Bob consequently add the new agent to its peer store
    // and not consider all agents blocked anymore at Alice's peer url.
    let payload_unblocked = Bytes::from("Sending to unblocked");

    // Sending too shortly after a disconnect can lead to a tx5 send error
    // sporadically and would leave the test flaky. Therefore, we wrap the
    // send into an iter_check in order to make sure it makes it through.
    iter_check!(2_000, 500, {
        if transport_alice
            .send_space_notify(
                peer_url_bob.clone(),
                TEST_SPACE_ID,
                payload_unblocked.clone(),
            )
            .await
            .is_ok()
        {
            break;
        }
    });

    // Verify that the message is being received correctly by Bob
    let payload_unblocked_received = recv_notify_recv_bob
        .recv_timeout(std::time::Duration::from_secs(2))
        .expect("timed out waiting for space message");
    assert_eq!(payload_unblocked, payload_unblocked_received);
}

#[tokio::test(flavor = "multi_thread")]
async fn incoming_module_messages_from_blocked_peers_are_dropped() {
    enable_tracing();

    let (builder, _relay_server) = builder_with_relay!();

    let TestPeer {
        space: space_alice,
        transport: transport_alice,
        peer_url: peer_url_alice,
        agent_id: agent_id_alice,
        peer_disconnect_recv: peer_disconnect_recv_alice,
        ..
    } = make_test_peer(builder.clone()).await;

    let TestPeer {
        space: space_bob,
        transport: transport_bob,
        peer_url: peer_url_bob,
        agent_info: agent_info_bob,
        recv_module_msg_recv: recv_module_msg_recv_bob,
        peer_disconnect_recv: _peer_disconnect_recv_bob,
        ..
    } = make_test_peer(builder).await;

    // Add Bob's agent to Alice's peer store.
    space_alice
        .peer_store()
        .insert(vec![agent_info_bob])
        .await
        .unwrap();

    // Alice sends a module message that should go through normally and establish
    // a connection with Bob
    let payload_module = Bytes::from("Hello module world");

    // This send here fails relatively often in CI, presumably due to a race
    // condition in tx5: https://github.com/holochain/tx5/issues/193
    // The working hypothesis is that agent info gossip which starts in the background
    // after the local agent join may lead to an incoming connection being established
    // more or less simultaneously, thereby evoking the race condition.
    iter_check!(2_000, 500, {
        if transport_alice
            .send_module(
                peer_url_bob.clone(),
                TEST_SPACE_ID,
                "test".into(),
                payload_module.clone(),
            )
            .await
            .is_ok()
        {
            break;
        }
    });

    let payload_module_received = recv_module_msg_recv_bob
        .recv_timeout(Duration::from_secs(2))
        .expect("timed out waiting for module message");
    assert_eq!(payload_module, payload_module_received);

    // Bob should have one open connection now and hasn't blocked any messages
    let net_stats_bob = transport_bob.dump_network_stats().await.unwrap();
    assert_eq!(net_stats_bob.transport_stats.connections.len(), 1);
    assert_eq!(net_stats_bob.blocked_message_counts.len(), 0);

    // Verify that Bob's peer store contains 2 agents now, his own agent as well as
    // Alice's agent that should have been added via the preflight.
    let agents_in_peer_store = space_bob.peer_store().get_all().await.unwrap();
    assert_eq!(agents_in_peer_store.len(), 2);

    // Now block Alice's agent.
    block_agent_in_space(agent_id_alice, space_bob.clone()).await;

    // Now send another message from Alice to Bob. Bob should reject it now and
    // close the connection.
    transport_alice
        .send_module(
            peer_url_bob.clone(),
            TEST_SPACE_ID,
            "test".into(),
            Bytes::from("Sending to blocker"),
        )
        .await
        .unwrap();

    // Verify that Bob has blocked the message
    iter_check!(500, {
        let net_stats = transport_bob.dump_network_stats().await.unwrap();
        if net_stats.blocked_message_counts.len() == 1 {
            break;
        }
    });

    // Now have Bob close the connection so that we can verify later that
    // resending a message from Alice succeeds after she joins with a new
    // agent.
    transport_bob.disconnect(peer_url_alice, None).await;

    // Wait for Alice to disconnect as well. Otherwise, Alice may send the next
    // message over the old WebRTC connection still that Bob has already
    // closed on his end and Bob won't ever receive it.
    iter_check!(500, {
        if let Ok(peer_url) = peer_disconnect_recv_alice.try_recv() {
            if peer_url == peer_url_bob {
                break;
            }
        }
    });

    // To double-check, verify additionally that no module message has been
    // handled by Bob.
    assert!(recv_module_msg_recv_bob
        .recv_timeout(std::time::Duration::from_millis(50))
        .is_err());

    // Now have Alice join the space with a second agent which Bob should then
    // receive via the preflight and consequently let further messages through
    // again.
    let alice_local_agent_2 = Arc::new(Ed25519LocalAgent::default());
    space_alice
        .local_agent_join(alice_local_agent_2.clone())
        .await
        .unwrap();

    // Now send yet another message that should get through again since the preflight
    // should get through and Bob consequently add the new agent to its peer store
    // and not consider all agents blocked anymore at Alice's peer url.
    let payload_unblocked = Bytes::from("Sending to unblocked");

    // Sending too shortly after a disconnect can lead to a tx5 send error
    // sporadically and would leave the test flaky. Therefore, we wrap the
    // send into an iter_check in order to make sure it makes it through.
    iter_check!(2_000, 500, {
        if transport_alice
            .send_module(
                peer_url_bob.clone(),
                TEST_SPACE_ID,
                "test".into(),
                payload_unblocked.clone(),
            )
            .await
            .is_ok()
        {
            break;
        }
    });

    let payload_unblocked_received = recv_module_msg_recv_bob
        .recv_timeout(std::time::Duration::from_secs(2))
        .expect("timed out waiting for module message");
    assert_eq!(payload_unblocked, payload_unblocked_received);
}

#[tokio::test(flavor = "multi_thread")]
async fn outgoing_notify_messages_to_blocked_peers_are_dropped() {
    enable_tracing();

    let (builder, _relay_server) = builder_with_relay!();

    let TestPeer {
        space: space_alice,
        transport: transport_alice,
        peer_url: peer_url_alice,
        peer_disconnect_recv: peer_disconnect_recv_alice,
        ..
    } = make_test_peer(builder.clone()).await;

    let TestPeer {
        space: space_bob,
        transport: transport_bob,
        peer_url: peer_url_bob,
        agent_id: agent_id_bob,
        agent_info: agent_info_bob,
        recv_notify_recv: recv_notify_recv_bob,
        peer_disconnect_recv: _peer_disconnect_recv_bob,
        ..
    } = make_test_peer(builder).await;

    space_alice
        .peer_store()
        .insert(vec![agent_info_bob.clone()])
        .await
        .unwrap();

    // Alice sends a space message that should go through normally
    let payload = Bytes::from("Hello world");

    // This send here fails relatively often in CI, presumably due to a race
    // condition in tx5: https://github.com/holochain/tx5/issues/193
    // The working hypothesis is that agent info gossip which starts in the background
    // after the local agent join may lead to an incoming connection being established
    // more or less simultaneously, thereby evoking the race condition.
    iter_check!(2_000, 500, {
        if transport_alice
            .send_space_notify(
                peer_url_bob.clone(),
                TEST_SPACE_ID,
                payload.clone(),
            )
            .await
            .is_ok()
        {
            break;
        }
    });

    // Verify that the space message has been handled by Bob's recv_module_msg hook
    let payload_received = recv_notify_recv_bob
        .recv_timeout(std::time::Duration::from_secs(2))
        .expect("timed out waiting for space message");
    assert_eq!(payload, payload_received);

    // Verify that Alice's peer store contains 2 agents now, her own agent as well as
    // Bob's agent that should have been added via the preflight.
    let agents_in_peer_store =
        space_alice.peer_store().get_all().await.unwrap();
    assert_eq!(agents_in_peer_store.len(), 2);

    // Now Alice blocks Bob's agent
    block_agent_in_space(agent_id_bob, space_alice.clone()).await;

    // Now Alice tries to send another message to Bob. It should get dropped on
    // her own end before being sent to Bob.
    transport_alice
        .send_space_notify(
            peer_url_bob.clone(),
            TEST_SPACE_ID,
            Bytes::from("Sending to blocker"),
        )
        .await
        .unwrap();

    // Verify that Alice has blocked the message
    iter_check!(500, {
        let net_stats = transport_alice.dump_network_stats().await.unwrap();
        if let Some(space_blocks) =
            net_stats.blocked_message_counts.get(&peer_url_bob)
        {
            if let Some(c) = space_blocks.get(&TEST_SPACE_ID) {
                if c.outgoing == 1 {
                    break;
                }
            }
        }
    });

    // Now have Bob close the connection so that we can verify later that
    // resending a message from Alice succeeds after she joins with a new
    // agent.
    transport_bob.disconnect(peer_url_alice.clone(), None).await;

    // Wait for Alice to disconnect as well. Otherwise, Alice may send the next
    // message over the old WebRTC connection still that Bob has already
    // closed on his end and Bob won't ever receive it.
    iter_check!(500, {
        if let Ok(peer_url) = peer_disconnect_recv_alice.try_recv() {
            if peer_url == peer_url_bob {
                break;
            }
        }
    });

    // To double-check, verify additionally that no space message has been
    // handled by Bob in the meantime.
    assert!(recv_notify_recv_bob
        .recv_timeout(std::time::Duration::from_millis(50))
        .is_err());

    // Now have Bob join the space with a second agent and insert it to Alice's
    // peer store to simulate bootstrapping.
    let agent_info_bob_2 =
        join_new_local_agent_and_wait_for_agent_info(space_bob).await;

    space_alice
        .peer_store()
        .insert(vec![agent_info_bob_2])
        .await
        .unwrap();

    // Now send yet another message that should get through again since not
    // all of Bob's agents are blocked anymore by Alice.
    let payload_unblocked = Bytes::from("Sending to unblocked");

    // Sending too shortly after a disconnect can lead to a tx5 send error
    // sporadically and would leave the test flaky. Therefore, we wrap the
    // send into an iter_check in order to make sure it makes it through.
    iter_check!(2_000, 500, {
        if transport_alice
            .send_space_notify(
                peer_url_bob.clone(),
                TEST_SPACE_ID,
                payload_unblocked.clone(),
            )
            .await
            .is_ok()
        {
            break;
        }
    });

    // Verify that the message is being received correctly by Bob
    let payload_unblocked_received = recv_notify_recv_bob
        .recv_timeout(std::time::Duration::from_secs(2))
        .expect("timed out waiting for space message");
    assert_eq!(payload_unblocked, payload_unblocked_received);

    // And that the message blocks count on Alice's side is still 1
    iter_check!(500, {
        let net_stats = transport_alice.dump_network_stats().await.unwrap();
        if let Some(space_blocks) =
            net_stats.blocked_message_counts.get(&peer_url_bob)
        {
            if let Some(c) = space_blocks.get(&TEST_SPACE_ID) {
                if c.outgoing == 1 {
                    break;
                }
            }
        }
    });
}

#[tokio::test(flavor = "multi_thread")]
async fn outgoing_module_messages_to_blocked_peers_are_dropped() {
    enable_tracing();

    let (builder, _relay_server) = builder_with_relay!();

    let TestPeer {
        space: space_alice,
        transport: transport_alice,
        peer_url: peer_url_alice,
        peer_disconnect_recv: peer_disconnect_recv_alice,
        ..
    } = make_test_peer(builder.clone()).await;

    let TestPeer {
        space: space_bob,
        transport: transport_bob,
        peer_url: peer_url_bob,
        agent_id: agent_id_bob,
        agent_info: agent_info_bob,
        recv_module_msg_recv: recv_module_msg_recv_bob,
        peer_disconnect_recv: _peer_disconnect_recv_bob,
        ..
    } = make_test_peer(builder).await;

    space_alice
        .peer_store()
        .insert(vec![agent_info_bob.clone()])
        .await
        .unwrap();

    // Alice sends a module message that should go through normally
    let payload = Bytes::from("Hello module world");

    // This send here fails relatively often in CI, presumably due to a race
    // condition in tx5: https://github.com/holochain/tx5/issues/193
    // The working hypothesis is that agent info gossip which starts in the background
    // after the local agent join may lead to an incoming connection being established
    // more or less simultaneously, thereby evoking the race condition.
    iter_check!(2_000, 500, {
        if transport_alice
            .send_module(
                peer_url_bob.clone(),
                TEST_SPACE_ID,
                "test".into(),
                payload.clone(),
            )
            .await
            .is_ok()
        {
            break;
        }
    });

    // Verify that the module message has been handled by Bob's recv_module_msg hook
    let payload_received = recv_module_msg_recv_bob
        .recv_timeout(std::time::Duration::from_secs(2))
        .expect("timed out waiting for module message");
    assert_eq!(payload, payload_received);

    // Verify that Alice's peer store contains 2 agents now, her own agent as well as
    // Bob's agent that should have been added via the preflight.
    let agents_in_peer_store =
        space_alice.peer_store().get_all().await.unwrap();
    assert_eq!(agents_in_peer_store.len(), 2);

    // Now Alice blocks Bob's agent
    block_agent_in_space(agent_id_bob, space_alice.clone()).await;

    // And tries to send another message from to Bob. It should get dropped on
    // her own end before being sent to Bob.
    transport_alice
        .send_module(
            peer_url_bob.clone(),
            TEST_SPACE_ID,
            "test".into(),
            Bytes::from("Sending to blocker"),
        )
        .await
        .unwrap();

    // Verify that Alice has blocked the message
    iter_check!(500, {
        let net_stats = transport_alice.dump_network_stats().await.unwrap();
        if let Some(space_blocks) =
            net_stats.blocked_message_counts.get(&peer_url_bob)
        {
            if let Some(c) = space_blocks.get(&TEST_SPACE_ID) {
                if c.outgoing == 1 {
                    break;
                }
            }
        }
    });

    // Now have Bob close the connection so that we can verify later that
    // re-sending a message from Alice succeeds after she joins with a new
    // agent.
    transport_bob.disconnect(peer_url_alice, None).await;

    // Wait for Alice to disconnect as well. Otherwise, Alice may send the next
    // message over the old WebRTC connection still that Bob has already
    // closed on his end and Bob won't ever receive it.
    iter_check!(500, {
        if let Ok(peer_url) = peer_disconnect_recv_alice.try_recv() {
            if peer_url == peer_url_bob {
                break;
            }
        }
    });

    // To double-check, verify additionally that no module message has been
    // handled by Bob in the meantime.
    assert!(recv_module_msg_recv_bob
        .recv_timeout(std::time::Duration::from_millis(50))
        .is_err());

    // Now have Bob join the space with a second agent and insert it to Alice's
    // peer store to simulate bootstrapping.
    let agent_info_bob_2 =
        join_new_local_agent_and_wait_for_agent_info(space_bob).await;

    space_alice
        .peer_store()
        .insert(vec![agent_info_bob_2])
        .await
        .unwrap();

    // Now send yet another message that should get through again since not
    // all of Bob's agents are blocked anymore by Alice.
    let payload_unblocked = Bytes::from("Sending to unblocked");

    // Sending too shortly after a disconnect can lead to a tx5 send error
    // sporadically and would leave the test flaky. Therefore, we wrap the
    // send into an iter_check in order to make sure it makes it through.
    iter_check!(2_000, 500, {
        if transport_alice
            .send_module(
                peer_url_bob.clone(),
                TEST_SPACE_ID,
                "test".into(),
                payload_unblocked.clone(),
            )
            .await
            .is_ok()
        {
            break;
        }
    });

    // Verify that the message is being received correctly by Bob
    let payload_unblocked_received = recv_module_msg_recv_bob
        .recv_timeout(std::time::Duration::from_secs(2))
        .expect("timed out waiting for module message");
    assert_eq!(payload_unblocked, payload_unblocked_received);

    // And that the message blocks count on Alice's side is still 1
    iter_check!(500, {
        let net_stats = transport_alice.dump_network_stats().await.unwrap();
        if let Some(space_blocks) =
            net_stats.blocked_message_counts.get(&peer_url_bob)
        {
            if let Some(c) = space_blocks.get(&TEST_SPACE_ID) {
                if c.outgoing == 1 {
                    break;
                }
            }
        }
    });
}
