use std::cell::OnceCell;
use std::sync::{Arc, Mutex};

use bytes::Bytes;
use kitsune2_api::LocalAgent;
use kitsune2_api::{
    AgentInfoSigned, BlockTarget, BoxFut, Builder, DynSpace, DynTransport,
    K2Result, SpaceHandler, TxBaseHandler, TxHandler, TxModuleHandler, Url,
};
use kitsune2_test_utils::agent::{AgentBuilder, TestVerifier};
use kitsune2_test_utils::iter_check;
use kitsune2_test_utils::{
    agent::TestLocalAgent, enable_tracing, space::TEST_SPACE_ID,
};
use kitsune2_transport_tx5::harness::Tx5TransportTestHarness;
use std::sync::mpsc::{Receiver, Sender};

/// A default mock space handler that just drops received messages.
#[derive(Debug)]
struct MockSpaceHandler {
    recv_notify_sender: Sender<bytes::Bytes>,
}

impl MockSpaceHandler {
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

impl SpaceHandler for MockSpaceHandler {
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

/// A mock TxHandler
#[derive(Debug)]
pub struct MockTxHandler {
    pub peer_url: std::sync::Mutex<Url>,
    space: Arc<Mutex<OnceCell<DynSpace>>>,
    recv_module_msg_sender: Sender<bytes::Bytes>,
}

impl MockTxHandler {
    fn create(
        peer_url: Mutex<Url>,
        space: Arc<Mutex<OnceCell<DynSpace>>>,
    ) -> (Arc<Self>, Receiver<bytes::Bytes>) {
        let (recv_module_msg_sender, recv) = std::sync::mpsc::channel();

        (
            Arc::new(Self {
                peer_url,
                space,
                recv_module_msg_sender,
            }),
            recv,
        )
    }
}

impl TxModuleHandler for MockTxHandler {
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

impl TxBaseHandler for MockTxHandler {
    fn new_listening_address(&self, this_url: Url) -> BoxFut<'static, ()> {
        *(self.peer_url.lock().unwrap()) = this_url;
        Box::pin(async {})
    }
}
impl TxHandler for MockTxHandler {
    fn preflight_gather_outgoing(
        &self,
        _peer_url: Url,
    ) -> BoxFut<'_, K2Result<bytes::Bytes>> {
        let space = self.space.lock().expect("poisoned");
        let space = space
            .get()
            .expect("Space OnceCell has not been initialized in time.")
            .clone();

        Box::pin(async move {
            let agents = space.peer_store().get_all().await.unwrap();
            let agents_encoded: Vec<String> =
                agents.into_iter().map(|a| a.encode().unwrap()).collect();
            let mut b = bytes::BufMut::writer(bytes::BytesMut::new());
            rmp_serde::encode::write_named(&mut b, &agents_encoded).unwrap();
            Ok(b.into_inner().freeze())
        })
    }

    fn preflight_validate_incoming(
        &self,
        _peer_url: Url,
        data: bytes::Bytes,
    ) -> BoxFut<'_, K2Result<()>> {
        let agents_encoded: Vec<String> =
            rmp_serde::decode::from_slice(&data).unwrap();

        let agents: Vec<Arc<AgentInfoSigned>> = agents_encoded
            .iter()
            .map(|a| {
                AgentInfoSigned::decode(&TestVerifier, a.as_bytes()).unwrap()
            })
            .collect();

        let space = self
            .space
            .lock()
            .expect("poisoned")
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
    recv_notify_recv: Receiver<bytes::Bytes>,
    recv_module_msg_recv: Receiver<bytes::Bytes>,
}

pub async fn make_test_peer(builder: Arc<Builder>) -> TestPeer {
    let space_once_cell = Arc::new(Mutex::new(OnceCell::new()));

    let (tx_handler, recv_module_msg_recv) = MockTxHandler::create(
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
    let peer_url =
        tokio::time::timeout(std::time::Duration::from_millis(200), async {
            loop {
                let transport_stats =
                    transport.dump_network_stats().await.unwrap();
                let peer_url = transport_stats.peer_urls.first();
                if let Some(url) = peer_url {
                    return url.clone();
                }
            }
        })
        .await
        .unwrap();

    let (space_handler, recv_notify_recv) = MockSpaceHandler::create();

    let space = builder
        .space
        .create(
            builder.clone(),
            Arc::new(space_handler),
            TEST_SPACE_ID,
            transport.clone(),
        )
        .await
        .unwrap();

    let space_once_cell_lock = space_once_cell.lock().unwrap();
    space_once_cell_lock.set(space.clone()).unwrap();

    TestPeer {
        space,
        transport,
        peer_url,
        recv_notify_recv,
        recv_module_msg_recv,
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn space_messages_of_blocked_peers_are_rejected() {
    enable_tracing();

    let tx5_harness = Tx5TransportTestHarness::new(None, Some(5)).await;

    let TestPeer {
        space: space_alice,
        transport: transport_alice,
        peer_url: peer_url_alice,
        ..
    } = make_test_peer(tx5_harness.builder.clone()).await;

    let TestPeer {
        space: space_bob,
        transport: transport_bob,
        peer_url: peer_url_bob,
        recv_notify_recv: recv_notify_recv_bob,
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
    let payload_received = recv_notify_recv_bob.recv().unwrap();
    assert_eq!(payload, payload_received);

    // Bob should have one open connection to Alice now.
    let net_stats_bob = transport_bob.dump_network_stats().await.unwrap();
    assert!(net_stats_bob.connections.len() == 1);

    // Verify that Bob's peer store is currently empty
    let num_peers_in_store = space_bob.peer_store().get_all().await.unwrap();
    assert_eq!(num_peers_in_store.len(), 0);

    // Now insert a dummy local agent into Bob's peer store at Alice's URL and block it.
    let dummy_local_agent = TestLocalAgent::default();
    let dummy_agent_id = dummy_local_agent.agent().clone();
    let dummy_agent = AgentBuilder::default()
        .with_url(Some(peer_url_alice.clone()))
        .build(dummy_local_agent);

    space_bob
        .peer_store()
        .insert(vec![dummy_agent])
        .await
        .unwrap();

    // Verify that Bob's peer store has now one agent
    let agents_in_store = space_bob.peer_store().get_all().await.unwrap();
    assert_eq!(agents_in_store.len(), 1);

    let block_targets_in_store: Vec<BlockTarget> = agents_in_store
        .iter()
        .map(|a| BlockTarget::Agent(a.get_agent_info().agent.clone()))
        .collect();

    // Check that the Blocks module doesn't consider all agents blocked prior to blocking
    let all_blocked = space_bob
        .blocks()
        .are_all_blocked(block_targets_in_store.clone())
        .await
        .unwrap();
    assert!(!all_blocked);

    // Then block the dummy agent
    space_bob
        .blocks()
        .block(BlockTarget::Agent(dummy_agent_id.clone()))
        .await
        .unwrap();

    // Check that all agents at Alice's peer URL are indeed blocked now
    // according to Bob's Blocks module
    let all_blocked = space_bob
        .blocks()
        .are_all_blocked(block_targets_in_store)
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

    // Verify that Bob has closed the connection to Alice after the message has been rejected
    iter_check!(500, {
        let net_stats_bob_1 = transport_bob.dump_network_stats().await.unwrap();
        if net_stats_bob_1.connections.is_empty() {
            break;
        }
    });

    // Wait for Alice to disconnect as well. Otherwise, Alice may send the next
    // message over the old WebRTC connection still that Bob has already
    // closed on his end and Bob won't ever receive it.
    tokio::time::timeout(std::time::Duration::from_millis(300), async {
        loop {
            let net_stats_alice =
                transport_alice.dump_network_stats().await.unwrap();
            if net_stats_alice.connections.is_empty() {
                break;
            }
        }
    })
    .await
    .unwrap();

    // Verify that no message has been handled in the recv_notify() hook of
    // Bob. This is not a strictly reliable check since the receiver could
    // be delayed here and we check it momentarily with try_recv(). But since
    // we check it after the connection had been closed above, we can probably
    // assume that we would at least sometimes catch it here if the
    // message had been handled nevertheless.
    assert!(recv_notify_recv_bob.try_recv().is_err());

    // Now add an agent to Alice's peer store which Bob should then receive via
    // the preflight and consequently let further messages through again.
    let dummy_local_agent2 = TestLocalAgent::default();
    let dummy_agent2 = AgentBuilder::default()
        .with_url(Some(peer_url_alice.clone()))
        .build(dummy_local_agent2);

    space_alice
        .peer_store()
        .insert(vec![dummy_agent2])
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
    let payload_unblocked_received = recv_notify_recv_bob.recv().unwrap();
    assert_eq!(payload_unblocked, payload_unblocked_received);
}

#[tokio::test(flavor = "multi_thread")]
async fn module_messages_of_blocked_peers_are_rejected() {
    enable_tracing();

    let tx5_harness = Tx5TransportTestHarness::new(None, Some(5)).await;

    let TestPeer {
        space: space_alice,
        transport: transport_alice,
        peer_url: peer_url_alice,
        ..
    } = make_test_peer(tx5_harness.builder.clone()).await;

    let TestPeer {
        space: space_bob,
        transport: transport_bob,
        peer_url: peer_url_bob,
        recv_module_msg_recv: recv_module_msg_recv_bob,
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

    let payload_module_received = recv_module_msg_recv_bob.recv().unwrap();
    assert_eq!(payload_module, payload_module_received);

    // Bob should have one open connection now.
    let net_stats_bob = transport_bob.dump_network_stats().await.unwrap();
    assert!(net_stats_bob.connections.len() == 1);

    // Now insert a dummy local agent into Bob's peer store at Alice's URL and block it.
    let dummy_local_agent = TestLocalAgent::default();
    let dummy_agent_id = dummy_local_agent.agent().clone();
    let dummy_agent = AgentBuilder::default()
        .with_url(Some(peer_url_alice.clone()))
        .build(dummy_local_agent);

    space_bob
        .peer_store()
        .insert(vec![dummy_agent])
        .await
        .unwrap();

    // Verify that Bob's peer store has now one agent
    let agents_in_store = space_bob.peer_store().get_all().await.unwrap();
    assert_eq!(agents_in_store.len(), 1);

    let block_targets_in_store: Vec<BlockTarget> = agents_in_store
        .iter()
        .map(|a| BlockTarget::Agent(a.get_agent_info().agent.clone()))
        .collect();

    // Check that the Blocks module doesn't consider all agents blocked prior to blocking
    let all_blocked = space_bob
        .blocks()
        .are_all_blocked(block_targets_in_store.clone())
        .await
        .unwrap();
    assert!(!all_blocked);

    // Then block the dummy agent
    space_bob
        .blocks()
        .block(BlockTarget::Agent(dummy_agent_id.clone()))
        .await
        .unwrap();

    // Check that all agents at Alice's peer URL are indeed blocked now
    // according to Bob's Blocks module
    let all_blocked = space_bob
        .blocks()
        .are_all_blocked(block_targets_in_store)
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

    // Verify that Bob has closed the connection to Alice after the message has been rejected
    iter_check!(500, {
        let net_stats_bob_1 = transport_bob.dump_network_stats().await.unwrap();
        if net_stats_bob_1.connections.is_empty() {
            break;
        }
    });

    // Wait for Alice to disconnect as well. Otherwise, Alice may send the next
    // message over the old WebRTC connection still that Bob has already
    // closed on his end and Bob won't ever receive it.
    tokio::time::timeout(std::time::Duration::from_millis(300), async {
        loop {
            let net_stats_alice =
                transport_alice.dump_network_stats().await.unwrap();
            if net_stats_alice.connections.is_empty() {
                break;
            }
        }
    })
    .await
    .unwrap();

    // Verify that no module message has been handled by Bob. This is not a
    // strictly reliable check since the receiver could be delayed here
    // and we check it momentarily with try_recv(). But since we check
    // it after the connection had been closed above, we can probably
    // assume that we would at least sometimes catch it here if the
    // message had been handled nevertheless.
    assert!(recv_module_msg_recv_bob.try_recv().is_err());

    // Now add an agent to Alice's peer store which Bob should then receive via
    // the preflight and consequently let further messages through again.
    let dummy_local_agent2 = TestLocalAgent::default();
    let dummy_agent2 = AgentBuilder::default()
        .with_url(Some(peer_url_alice.clone()))
        .build(dummy_local_agent2);

    space_alice
        .peer_store()
        .insert(vec![dummy_agent2])
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

    let payload_unblocked_received = recv_module_msg_recv_bob.recv().unwrap();
    assert_eq!(payload_unblocked, payload_unblocked_received);
}
