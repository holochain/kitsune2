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

/// A TxHandler that sleeps during preflight validation to expose the race condition
/// between preflight processing and regular message handling.
///
/// In tx5, preflight and regular messages are processed by separate concurrent tasks.
/// If we slow down preflight processing, regular messages may arrive and be checked
/// for blocking BEFORE the preflight has inserted agents into the peer store and
/// computed the access decision.
#[derive(Debug)]
pub struct SlowPreflightTxHandler {
    pub peer_url: std::sync::Mutex<Url>,
    space: Arc<OnceCell<DynSpace>>,
    preflight_delay_ms: u64,
}

impl SlowPreflightTxHandler {
    fn create(
        peer_url: Mutex<Url>,
        space: Arc<OnceCell<DynSpace>>,
        preflight_delay_ms: u64,
    ) -> Arc<Self> {
        Arc::new(Self {
            peer_url,
            space,
            preflight_delay_ms,
        })
    }
}

impl TxBaseHandler for SlowPreflightTxHandler {
    fn new_listening_address(&self, this_url: Url) -> BoxFut<'static, ()> {
        *(self.peer_url.lock().unwrap()) = this_url;
        Box::pin(async {})
    }

    fn peer_disconnect(&self, _peer: Url, _reason: Option<String>) {}
}

impl TxHandler for SlowPreflightTxHandler {
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

        let delay_ms = self.preflight_delay_ms;

        Box::pin(async move {
            // Sleep FIRST, then insert agents.
            // This simulates slow preflight processing and creates a race window:
            //
            // 1. WebRTC data channel is established
            // 2. Bob's pre_task receives Alice's preflight and starts processing
            // 3. We sleep here (simulating slow processing)
            // 4. Meanwhile, Alice sends the actual message
            // 5. Bob's evt_task receives the message (separate concurrent task!)
            // 6. Bob checks if Alice is blocked - NO AGENTS IN PEER STORE YET
            // 7. Message gets blocked (incorrectly, since Alice is not actually blocked)
            // 8. Eventually we wake up and insert agents (too late)
            //
            // This demonstrates the race between pre_task (preflight) and evt_task (messages)
            // in tx5's concurrent task model.
            tracing::info!(
                "SlowPreflightTxHandler: sleeping {}ms BEFORE inserting agents",
                delay_ms
            );
            tokio::time::sleep(Duration::from_millis(delay_ms)).await;

            tracing::info!(
                "SlowPreflightTxHandler: now inserting agents after delay"
            );
            space.peer_store().insert(agents).await?;
            tracing::info!("SlowPreflightTxHandler: preflight complete");
            Ok(())
        })
    }
}

/// Test peer setup using the slow preflight handler
#[allow(dead_code)]
pub struct TestPeerWithSlowPreflight {
    space: DynSpace,
    transport: DynTransport,
    peer_url: Url,
    agent_info: Arc<AgentInfoSigned>,
    recv_notify_recv: Receiver<bytes::Bytes>,
}

pub async fn make_test_peer_with_slow_preflight(
    builder: Arc<Builder>,
    preflight_delay_ms: u64,
) -> TestPeerWithSlowPreflight {
    let space_once_cell = Arc::new(OnceCell::new());

    let tx_handler = SlowPreflightTxHandler::create(
        Mutex::new(Url::from_str("ws://127.0.0.1:80").unwrap()),
        space_once_cell.clone(),
        preflight_delay_ms,
    );

    let transport = builder
        .transport
        .create(builder.clone(), tx_handler.clone())
        .await
        .unwrap();

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

    TestPeerWithSlowPreflight {
        space,
        transport,
        peer_url,
        agent_info,
        recv_notify_recv,
    }
}

/// This test verifies that messages are NOT blocked due to race conditions
/// between preflight processing and access decision computation.
///
/// The test sets up:
/// 1. Alice: normal peer with Bob's agent info (can send to Bob)
/// 2. Bob: peer with slow preflight handler (delays after inserting agents)
///
/// When Alice sends to Bob:
/// 1. Connection is established, preflight exchanged
/// 2. Bob inserts Alice's agents into peer store
/// 3. Access decision listener is triggered (async)
/// 4. Bob's preflight handler sleeps (simulating slow processing)
/// 5. Preflight completes, connection is ready
/// 6. Message arrives at Bob
///
/// The message SHOULD get through because:
/// - Alice's agents are in Bob's peer store
/// - Even if access decision isn't cached yet, the fallback logic should
///   check the peer store and blocks module directly
///
/// If this test FAILS (message is blocked), it means the race condition
/// is occurring and the fix is needed.
#[tokio::test(flavor = "multi_thread")]
async fn messages_should_not_be_blocked_during_slow_preflight() {
    enable_tracing();

    let (builder, _relay_server) = builder_with_relay!();

    // Alice: normal peer that will send messages
    let TestPeer {
        space: space_alice,
        transport: transport_alice,
        agent_info: _agent_info_alice,
        peer_url: peer_url_alice,
        ..
    } = make_test_peer(builder.clone()).await;

    // Bob: peer with slow preflight handler
    // The handler inserts agents THEN sleeps, creating a race window
    // where agents exist but access decision may not be computed yet.
    let TestPeerWithSlowPreflight {
        space: _space_bob,
        transport: transport_bob,
        peer_url: peer_url_bob,
        agent_info: agent_info_bob,
        recv_notify_recv: recv_notify_recv_bob,
    } = make_test_peer_with_slow_preflight(builder.clone(), 2000).await;

    // Insert Bob's agent info into Alice's peer store so Alice can send to Bob
    space_alice
        .peer_store()
        .insert(vec![agent_info_bob.clone()])
        .await
        .unwrap();

    // Wait for Alice's access decision for Bob to be computed
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Verify Alice won't block outgoing messages to Bob
    let stats_alice_before =
        transport_alice.dump_network_stats().await.unwrap();
    assert!(
        stats_alice_before.blocked_message_counts.is_empty(),
        "Alice should not have any blocked messages before sending"
    );

    tracing::info!("Alice sending message to Bob...");

    // Alice sends a message to Bob.
    // This will:
    // 1. Establish connection and exchange preflights
    // 2. Bob's slow preflight handler sleeps, then inserts Alice's agents
    // 3. Message arrives at Bob after preflight completes
    // 4. Bob checks if Alice is blocked
    //
    // The message SHOULD get through. If it's blocked, the race condition is occurring.
    let payload = Bytes::from("Hello Bob, this should get through!");

    transport_alice
        .send_space_notify(peer_url_bob.clone(), TEST_SPACE_ID, payload.clone())
        .await
        .unwrap();

    // Wait for Bob's preflight to complete plus some buffer for message processing
    // The preflight delay is passed to make_test_peer_with_slow_preflight (2000ms)
    // We need to wait at least that long, plus extra for connection establishment
    tracing::info!("Waiting for Bob's slow preflight to complete...");
    tokio::time::sleep(Duration::from_millis(3000)).await;

    // Check Bob's blocked message counts
    let stats_bob = transport_bob.dump_network_stats().await.unwrap();
    tracing::info!(
        "Bob's blocked message counts: {:?}",
        stats_bob.blocked_message_counts
    );

    // Check if Bob blocked any incoming messages from Alice
    let bob_blocked_from_alice = stats_bob
        .blocked_message_counts
        .get(&peer_url_alice)
        .map(|space_counts| {
            space_counts
                .get(&TEST_SPACE_ID)
                .map(|c| c.incoming > 0)
                .unwrap_or(false)
        })
        .unwrap_or(false);

    // The message SHOULD have been received by Bob
    let message_received = recv_notify_recv_bob
        .recv_timeout(std::time::Duration::from_secs(2))
        .ok();

    if bob_blocked_from_alice {
        tracing::error!(
            "RACE CONDITION BUG: Bob blocked incoming message from Alice! \
             This should not happen - Alice's agents were inserted into Bob's peer store \
             during preflight, so the message should not be blocked. \
             Blocked counts: {:?}",
            stats_bob.blocked_message_counts
        );
    }

    if message_received.is_none() {
        tracing::error!(
            "Message was NOT received by Bob! \
             Either it was blocked, or there was a transport error."
        );
    }

    // ASSERT: No messages should be blocked
    assert!(
        !bob_blocked_from_alice,
        "RACE CONDITION BUG: Bob blocked {} incoming message(s) from Alice. \
         The access decision computation raced with message arrival. \
         Alice's agents were in Bob's peer store but the access decision \
         wasn't cached yet when the message arrived. \
         Fix needed: are_all_agents_at_url_blocked should fall back to checking \
         the peer store and blocks module directly when no access decision exists.",
        stats_bob
            .blocked_message_counts
            .get(&peer_url_alice)
            .and_then(|s| s.get(&TEST_SPACE_ID))
            .map(|c| c.incoming)
            .unwrap_or(0)
    );

    // ASSERT: Message should have been received
    assert!(
        message_received.is_some(),
        "Message should have been received by Bob"
    );
    assert_eq!(
        message_received.unwrap(),
        payload,
        "Received message should match sent payload"
    );

    tracing::info!("Test passed: message was received without being blocked");
}

/// Test peer that delays joining the local agent.
/// This allows testing the scenario where a connection is established
/// and preflight is exchanged BEFORE the local agent joins the space.
pub struct TestPeerDelayedJoin {
    pub space: DynSpace,
    pub transport: DynTransport,
    pub peer_url: Url,
    pub recv_notify_recv: Receiver<bytes::Bytes>,
    space_once_cell: Arc<OnceCell<DynSpace>>,
}

impl TestPeerDelayedJoin {
    /// Join with a local agent after the peer has been created.
    /// Returns the agent info after it's been published to the peer store.
    pub async fn join_local_agent(&self) -> Arc<AgentInfoSigned> {
        // Set the space in OnceCell so TxHandler can access it
        let _ = self.space_once_cell.set(self.space.clone());

        join_new_local_agent_and_wait_for_agent_info(self.space.clone()).await
    }
}

/// Creates a test peer WITHOUT joining a local agent.
/// The preflight handler will return an EMPTY agent list until join_local_agent() is called.
pub async fn make_test_peer_delayed_join(
    builder: Arc<Builder>,
) -> TestPeerDelayedJoin {
    let space_once_cell = Arc::new(OnceCell::new());

    let (tx_handler, _recv_module_msg_recv, _peer_disconnect_recv) =
        TestTxHandler::create(
            Mutex::new(Url::from_str("ws://127.0.0.1:80").unwrap()),
            space_once_cell.clone(),
        );

    let transport = builder
        .transport
        .create(builder.clone(), tx_handler.clone())
        .await
        .unwrap();

    transport.register_module_handler(TEST_SPACE_ID, "test".into(), tx_handler);

    // Wait for peer URL to be available
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

    // NOTE: We do NOT join a local agent here!
    // The space_once_cell is also NOT set yet, so preflight_gather_outgoing
    // will return an empty list.

    TestPeerDelayedJoin {
        space,
        transport,
        peer_url,
        recv_notify_recv,
        space_once_cell,
    }
}

/// This test reproduces the production issue where messages get blocked
/// when a connection is established BEFORE the local agent joins the space.
///
/// Scenario (based on PR #417 discussion):
/// 1. Alice creates a space but hasn't joined with a local agent yet
/// 2. Alice connects to Bob (preflight is exchanged)
/// 3. Alice's preflight contains NO agents (she hasn't joined yet!)
/// 4. Bob receives empty preflight - no agents to insert, no access decision made
/// 5. Alice joins with a local agent
/// 6. Alice sends messages to Bob
/// 7. Bob checks are_all_agents_at_url_blocked(alice_url) → NO ACCESS DECISION
/// 8. Messages get blocked incorrectly!
///
/// This matches ThetaSinner's hypothesis from PR #417:
/// > "Is it possible that network messages are being sent before the local agent
/// > joins the space? ... If the preflight gets sent without a local agent info
/// > then the connection has no way to recover until bootstrap happens."
#[tokio::test(flavor = "multi_thread")]
async fn messages_blocked_when_preflight_sent_before_local_agent_joins() {
    enable_tracing();

    let (builder, _relay_server) = builder_with_relay!();

    // Bob: normal peer with a local agent already joined
    let TestPeer {
        space: space_bob,
        transport: transport_bob,
        peer_url: peer_url_bob,
        agent_info: agent_info_bob,
        recv_notify_recv: recv_notify_recv_bob,
        ..
    } = make_test_peer(builder.clone()).await;

    // Alice: peer WITHOUT a local agent joined yet
    let alice = make_test_peer_delayed_join(builder.clone()).await;

    tracing::info!("Alice peer_url: {:?}", alice.peer_url);
    tracing::info!("Bob peer_url: {:?}", peer_url_bob);

    // Insert Bob's agent info into Alice's peer store so Alice can send to Bob
    // (This prevents sender-side blocking)
    alice
        .space
        .peer_store()
        .insert(vec![agent_info_bob.clone()])
        .await
        .unwrap();

    // Wait for access decision to be computed
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Now Alice needs to set up her space_once_cell so the TxHandler can work
    // But we'll do it in a way that the preflight will still be empty
    // because there's no local agent yet
    let _ = alice.space_once_cell.set(alice.space.clone());

    tracing::info!(
        "Step 1: Alice triggers connection to Bob (preflight will be EMPTY - no local agent yet)"
    );

    // Alice sends a message to Bob to trigger connection establishment.
    // At this point:
    // - Alice's preflight_gather_outgoing will return an EMPTY agent list
    //   (because no local agent has joined yet)
    // - Bob will receive empty preflight and insert nothing
    // - Bob will have NO access decision for Alice's URL
    let first_message_result = alice
        .transport
        .send_space_notify(
            peer_url_bob.clone(),
            TEST_SPACE_ID,
            Bytes::from("First message - during empty preflight"),
        )
        .await;

    // The first message might fail or succeed depending on transport behavior
    tracing::info!("First message result: {:?}", first_message_result);

    // Give time for connection establishment and preflight exchange
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Check Bob's peer store - it should NOT have Alice's agents
    let bob_peers = space_bob.peer_store().get_all().await.unwrap();
    let bob_has_alice_agents = bob_peers
        .iter()
        .any(|a| a.url.as_ref() == Some(&alice.peer_url));

    tracing::info!(
        "Bob's peer store has Alice's agents: {} (peers: {:?})",
        bob_has_alice_agents,
        bob_peers.iter().map(|a| &a.agent).collect::<Vec<_>>()
    );

    tracing::info!("Step 2: Now Alice joins with a local agent");

    // Now Alice joins with a local agent
    let alice_agent_info = alice.join_local_agent().await;
    tracing::info!("Alice joined with agent: {:?}", alice_agent_info.agent);

    // Wait a moment for the agent info to be available
    tokio::time::sleep(Duration::from_millis(100)).await;

    tracing::info!("Step 3: Alice sends another message to Bob");

    // Alice sends another message to Bob
    // This message SHOULD go through, but if Bob has no access decision for Alice,
    // it will be blocked!
    let payload = Bytes::from("Second message - after local agent joined");

    alice
        .transport
        .send_space_notify(peer_url_bob.clone(), TEST_SPACE_ID, payload.clone())
        .await
        .unwrap();

    // Give time for message processing
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Check Bob's blocked message counts
    let stats_bob = transport_bob.dump_network_stats().await.unwrap();
    tracing::info!(
        "Bob's blocked message counts: {:?}",
        stats_bob.blocked_message_counts
    );

    // Check if Bob blocked any incoming messages from Alice
    let blocked_from_alice = stats_bob
        .blocked_message_counts
        .get(&alice.peer_url)
        .map(|space_counts| {
            space_counts
                .get(&TEST_SPACE_ID)
                .map(|c| c.incoming)
                .unwrap_or(0)
        })
        .unwrap_or(0);

    // Try to receive the message
    let message_received = recv_notify_recv_bob
        .recv_timeout(std::time::Duration::from_secs(1))
        .ok();

    tracing::info!(
        "Messages blocked from Alice: {}, Message received: {:?}",
        blocked_from_alice,
        message_received.is_some()
    );

    // Expected behavior after the fix:
    // - The FIRST message (sent before Alice joined) may be blocked - this is unavoidable
    //   because Bob had no access decision for Alice at that point
    // - The SECOND message (sent after Alice joins) should NOT be blocked because:
    //   1. When Alice joins, preflight is re-sent to all connected peers
    //   2. Bob receives the updated preflight with Alice's agent info
    //   3. Bob computes an access decision for Alice
    //   4. Subsequent messages are allowed through

    // We expect at most 1 blocked message (the first one sent before join)
    // Note: In some cases the first message might not be blocked depending on timing
    assert!(
        blocked_from_alice <= 1,
        "At most 1 message should be blocked (the first one sent before Alice joined). \
         Blocked count: {}",
        blocked_from_alice
    );

    // The SECOND message MUST have been received - this proves the fix works
    assert!(
        message_received.is_some(),
        "Second message should have been received by Bob after Alice joined with a local agent. \
         This proves that resending preflight after local_agent_join works correctly."
    );

    assert_eq!(
        message_received.unwrap(),
        payload,
        "Received message should match sent payload"
    );

    tracing::info!(
        "Test passed: second message was received after local agent joined and preflight was re-sent. \
         {} message(s) were blocked (expected: the first message sent before Alice joined)",
        blocked_from_alice
    );
}
