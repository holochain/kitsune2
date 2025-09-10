<<<<<<< HEAD
use std::collections::HashMap;
=======
>>>>>>> 47ca19c (refactor: switch to tokio's OnceCell in MockTxHandler of the blocks integration tests)
use std::sync::{Arc, Mutex};

use bytes::Bytes;
use kitsune2_api::{AgentId, Id, LocalAgent, SpaceId};
use kitsune2_api::{
    AgentInfoSigned, BlockTarget, BoxFut, Builder, DynSpace, DynTransport,
    K2Result, SpaceHandler, TxBaseHandler, TxHandler, TxModuleHandler, Url,
};
use kitsune2_core::{Ed25519LocalAgent, Ed25519Verifier};
use kitsune2_test_utils::iter_check;
use kitsune2_test_utils::{enable_tracing, space::TEST_SPACE_ID};
use kitsune2_transport_tx5::harness::Tx5TransportTestHarness;
use std::sync::mpsc::{Receiver, Sender};
use tokio::sync::OnceCell;

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
        self.peer_disconnect_sender.send(peer).unwrap();
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
            for agent in agents {
                space.peer_store().insert(vec![agent]).await?;
            }
            Ok(())
        })
    }
}

pub struct TestPeer {
    space: DynSpace,
    transport: DynTransport,
    peer_url: Url,
    agent_id: AgentId,
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
    let peer_url = iter_check!(200, {
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
            Arc::new(space_handler),
            TEST_SPACE_ID,
            report,
            transport.clone(),
        )
        .await
        .unwrap();

    // join with one local agent
    let local_agent = Arc::new(Ed25519LocalAgent::default());
    let agent_id = local_agent.agent().clone();
    space.local_agent_join(local_agent).await.unwrap();

    space_once_cell.set(space.clone()).unwrap();

    TestPeer {
        space,
        transport,
        agent_id,
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
    let peer_url = iter_check!(200, {
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
            Arc::new(NoopHandler),
            TEST_SPACE_ID_2,
            report,
            transport.clone(),
        )
        .await
        .unwrap();

    TestPeerLight {
        space1,
        space2,
        transport,
        peer_url,
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn message_block_count_increases_correctly() {
    enable_tracing();

    let tx5_harness = Tx5TransportTestHarness::new(None, Some(5)).await;

    let TestPeerLight {
        transport: transport_alice,
        peer_url: peer_url_alice,
        ..
    } = make_test_peer_light(tx5_harness.builder.clone()).await;

    let TestPeerLight {
        transport: transport_bob,
        peer_url: peer_url_bob,
        ..
    } = make_test_peer_light(tx5_harness.builder.clone()).await;

    let TestPeerLight {
        space1: _space_carol_1, // Need to keep the space in memory here since it's being used to check for blocks
        space2: _space_carol_2, // -- ditto --
        transport: transport_carol,
        peer_url: peer_url_carol,
    } = make_test_peer_light(tx5_harness.builder.clone()).await;

    // Verify that Carol's message blocks count is empty initially
    let stats = transport_carol.dump_network_stats().await.unwrap();
    assert_eq!(stats.blocked_message_counts.len(), 0);

    // Now have Alice send a message to Carol. Since Alice didn't join with a local
    // agent, there shouldn't be any agent infos be sent in the preflight and
    // the message should be dropped as a result since Carol can't check for
    // blocks without agent infos.
    transport_alice
        .send_space_notify(
            peer_url_carol.clone(),
            TEST_SPACE_ID_1,
            Bytes::new(),
        )
        .await
        .unwrap();

    // Check that the blocks count goes up on Carols side
    let expected_blocked_message_counts: HashMap<Url, HashMap<SpaceId, u32>> =
        [(peer_url_alice.clone(), [(TEST_SPACE_ID_1, 1)].into())].into();
    iter_check!(500, {
        let net_stats = transport_carol.dump_network_stats().await.unwrap();
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
    let expected_blocked_message_counts: HashMap<Url, HashMap<SpaceId, u32>> =
        [(peer_url_alice.clone(), [(TEST_SPACE_ID_1, 2)].into())].into();
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
    let expected_blocked_message_counts: HashMap<Url, HashMap<SpaceId, u32>> =
        [(
            peer_url_alice.clone(),
            [(TEST_SPACE_ID_1, 2), (TEST_SPACE_ID_2, 1)].into(),
        )]
        .into();
    iter_check!(500, {
        let net_stats = transport_carol.dump_network_stats().await.unwrap();
        if net_stats.blocked_message_counts == expected_blocked_message_counts {
            break;
        }
    });

    // And finally have Bob send one also and verify that the HashMap gets updated correctly
    transport_bob
        .send_space_notify(
            peer_url_carol.clone(),
            TEST_SPACE_ID_1,
            Bytes::new(),
        )
        .await
        .unwrap();
    let expected_blocked_message_counts: HashMap<Url, HashMap<SpaceId, u32>> =
        [
            (
                peer_url_alice.clone(),
                [(TEST_SPACE_ID_1, 2), (TEST_SPACE_ID_2, 1)].into(),
            ),
            (peer_url_bob, [(TEST_SPACE_ID_1, 1)].into()),
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
async fn notify_messages_of_blocked_peers_are_dropped() {
    enable_tracing();

    let tx5_harness = Tx5TransportTestHarness::new(None, Some(5)).await;

    let TestPeer {
        space: space_alice,
        transport: transport_alice,
        peer_url: peer_url_alice,
        agent_id: agent_id_alice,
        peer_disconnect_recv: peer_disconnect_recv_alice,
        ..
    } = make_test_peer(tx5_harness.builder.clone()).await;

    let TestPeer {
        space: space_bob,
        transport: transport_bob,
        peer_url: peer_url_bob,
        recv_notify_recv: recv_notify_recv_bob,
        peer_disconnect_recv: _peer_disconnect_recv_bob,
        ..
    } = make_test_peer(tx5_harness.builder).await;

    // Alice sends a space message that should go through normally and establish
    // a connection with Bob
    let payload = Bytes::from("Hello world");
    transport_alice
        .send_space_notify(peer_url_bob.clone(), TEST_SPACE_ID, payload.clone())
        .await
        .unwrap();

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

    let block_targets = vec![BlockTarget::Agent(agent_id_alice.clone())];

    // Check that the Blocks module doesn't consider Alice's agent blocked prior to blocking
    let all_blocked = space_bob
        .blocks()
        .are_all_blocked(block_targets.clone())
        .await
        .unwrap();
    assert!(!all_blocked);

    // Then block Alice's agent
    space_bob
        .blocks()
        .block(BlockTarget::Agent(agent_id_alice.clone()))
        .await
        .unwrap();

    // Check that all agents at Alice's peer URL are indeed blocked now
    // according to Bob's Blocks module
    let all_blocked = space_bob
        .blocks()
        .are_all_blocked(block_targets)
        .await
        .unwrap();
    assert!(all_blocked);

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
    transport_alice
        .send_space_notify(
            peer_url_bob.clone(),
            TEST_SPACE_ID,
            payload_unblocked.clone(),
        )
        .await
        .unwrap();

    // Verify that the message is being received correctly by Bob
    let payload_unblocked_received = recv_notify_recv_bob
        .recv_timeout(std::time::Duration::from_secs(2))
        .expect("timed out waiting for space message");
    assert_eq!(payload_unblocked, payload_unblocked_received);
}

#[tokio::test(flavor = "multi_thread")]
async fn module_messages_of_blocked_peers_are_dropped() {
    enable_tracing();

    let tx5_harness = Tx5TransportTestHarness::new(None, Some(5)).await;

    let TestPeer {
        space: space_alice,
        transport: transport_alice,
        peer_url: peer_url_alice,
        agent_id: agent_id_alice,
        peer_disconnect_recv: peer_disconnect_recv_alice,
        ..
    } = make_test_peer(tx5_harness.builder.clone()).await;

    let TestPeer {
        space: space_bob,
        transport: transport_bob,
        peer_url: peer_url_bob,
        recv_module_msg_recv: recv_module_msg_recv_bob,
        peer_disconnect_recv: _peer_disconnect_recv_bob,
        ..
    } = make_test_peer(tx5_harness.builder).await;

    // Alice sends a space message that should go through normally and establish
    // a connection with Bob
    let payload_module = Bytes::from("Hello module world");
    transport_alice
        .send_module(
            peer_url_bob.clone(),
            TEST_SPACE_ID,
            "test".into(),
            payload_module.clone(),
        )
        .await
        .unwrap();

    let payload_module_received = recv_module_msg_recv_bob
        .recv_timeout(std::time::Duration::from_secs(2))
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

    let block_targets = vec![BlockTarget::Agent(agent_id_alice.clone())];

    // Check that the Blocks module doesn't consider Alice's agent blocked prior to blocking
    let all_blocked = space_bob
        .blocks()
        .are_all_blocked(block_targets.clone())
        .await
        .unwrap();
    assert!(!all_blocked);

    // Then block Alice's agent
    space_bob
        .blocks()
        .block(BlockTarget::Agent(agent_id_alice.clone()))
        .await
        .unwrap();

    // Check that all agents at Alice's peer URL are indeed blocked now
    // according to Bob's Blocks module
    let all_blocked = space_bob
        .blocks()
        .are_all_blocked(block_targets)
        .await
        .unwrap();
    assert!(all_blocked);

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
    transport_alice
        .send_module(
            peer_url_bob.clone(),
            TEST_SPACE_ID,
            "test".into(),
            payload_unblocked.clone(),
        )
        .await
        .unwrap();

    let payload_unblocked_received = recv_module_msg_recv_bob
        .recv_timeout(std::time::Duration::from_secs(2))
        .expect("timed out waiting for module message");
    assert_eq!(payload_unblocked, payload_unblocked_received);
}
