use kitsune2_api::{
    builder::Builder,
    fetch::DynFetch,
    transport::{DynTransport, TxBaseHandler, TxHandler},
    BoxFut, DynOpStore, OpId, Timestamp, Url,
};
use kitsune2_core::{
    default_test_builder,
    factories::{
        core_fetch::config::{CoreFetchConfig, CoreFetchModConfig},
        MemoryOp,
    },
};
use kitsune2_test_utils::{
    enable_tracing, iter_check, random_bytes, space::TEST_SPACE_ID,
};
use std::{sync::Arc, time::Duration};

#[derive(Debug)]
struct MockTxHandler {
    peer_url: std::sync::Mutex<Url>,
}
impl TxBaseHandler for MockTxHandler {
    fn new_listening_address(&self, this_url: Url) -> BoxFut<'static, ()> {
        *(self.peer_url.lock().unwrap()) = this_url;
        Box::pin(async {})
    }
}
impl TxHandler for MockTxHandler {}

fn create_op_list(num_ops: u16) -> (Vec<MemoryOp>, Vec<OpId>) {
    let mut ops = Vec::new();
    let mut op_ids = Vec::new();
    for _ in 0..num_ops {
        let op = MemoryOp::new(Timestamp::now(), random_bytes(32));
        let op_id = op.compute_op_id();
        ops.push(op);
        op_ids.push(op_id);
    }
    (ops, op_ids)
}

struct Peer {
    builder: Arc<Builder>,
    op_store: DynOpStore,
    transport: DynTransport,
    peer_url: Url,
    fetch: Option<DynFetch>,
}

async fn make_peer(
    fetch_config: Option<CoreFetchModConfig>,
    create_fetch: bool,
) -> Peer {
    let builder =
        Arc::new(default_test_builder().with_default_config().unwrap());
    let op_store = builder
        .op_store
        .create(builder.clone(), TEST_SPACE_ID)
        .await
        .unwrap();
    let tx_handler = Arc::new(MockTxHandler {
        peer_url: std::sync::Mutex::new(
            // Placeholder URL which will be overwritten when the transport is created.
            Url::from_str("ws://127.0.0.1:80").unwrap(),
        ),
    });
    let transport = builder
        .transport
        .create(builder.clone(), tx_handler.clone())
        .await
        .unwrap();
    let peer_url = tx_handler.peer_url.lock().unwrap().clone();
    if let Some(config) = fetch_config {
        builder.config.set_module_config(&config).unwrap();
    }
    let fetch = if create_fetch {
        Some(
            builder
                .fetch
                .create(
                    builder.clone(),
                    TEST_SPACE_ID,
                    op_store.clone(),
                    transport.clone(),
                )
                .await
                .unwrap(),
        )
    } else {
        None
    };

    Peer {
        builder,
        op_store,
        transport,
        peer_url,
        fetch,
    }
}

async fn assert_ops_arrived_in_store(op_store: DynOpStore, op_ids: Vec<OpId>) {
    iter_check!({
        let ops_in_store = op_store.retrieve_ops(op_ids.clone()).await.unwrap();
        if ops_in_store.len() == op_ids.len() {
            break;
        }
    });
}

#[tokio::test(flavor = "multi_thread")]
async fn two_peer_fetch() {
    enable_tracing();
    let Peer {
        fetch: fetch_alice,
        op_store: op_store_alice,
        peer_url: peer_url_alice,
        ..
    } = make_peer(None, true).await;
    let fetch_alice = fetch_alice.unwrap();
    let Peer {
        fetch: fetch_bob,
        op_store: op_store_bob,
        peer_url: peer_url_bob,
        ..
    } = make_peer(None, true).await;
    let fetch_bob = fetch_bob.unwrap();

    let num_of_ops = 1;
    let (requested_ops_alice, requested_op_ids_alice) =
        create_op_list(num_of_ops);
    let (requested_ops_bob, requested_op_ids_bob) = create_op_list(num_of_ops);

    // Insert requested ops in Alice's and Bob's op store.
    op_store_alice
        .process_incoming_ops(
            requested_ops_alice
                .clone()
                .into_iter()
                .map(Into::into)
                .collect(),
        )
        .await
        .unwrap();
    op_store_bob
        .process_incoming_ops(
            requested_ops_bob
                .clone()
                .into_iter()
                .map(Into::into)
                .collect(),
        )
        .await
        .unwrap();

    // Alice and Bob should not hold any of the ops to be requested from each other.
    let ops_in_store_alice = op_store_alice
        .retrieve_ops(requested_op_ids_bob.clone())
        .await
        .unwrap();
    assert_eq!(ops_in_store_alice, vec![]);

    let ops_in_store_bob = op_store_bob
        .retrieve_ops(requested_op_ids_alice.clone())
        .await
        .unwrap();
    assert_eq!(ops_in_store_bob, vec![]);

    // Alice requests the ops from Bob.
    fetch_alice
        .request_ops(requested_op_ids_bob.clone(), peer_url_bob)
        .await
        .unwrap();
    // Bob requests the ops from Alice.
    fetch_bob
        .request_ops(requested_op_ids_alice.clone(), peer_url_alice)
        .await
        .unwrap();

    // Wait until Alice has fetched Bob's and Bob has fetched Alice's ops.
    futures::future::join_all(vec![
        assert_ops_arrived_in_store(op_store_alice, requested_op_ids_bob),
        assert_ops_arrived_in_store(op_store_bob, requested_op_ids_alice),
    ])
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn bob_comes_online_after_being_unresponsive() {
    enable_tracing();
    let fetch_config_alice = CoreFetchModConfig {
        core_fetch: CoreFetchConfig {
            first_back_off_interval_ms: 10,
            re_insert_outgoing_request_delay_ms: 10,
            ..Default::default()
        },
    };
    let Peer {
        fetch: fetch_alice,
        op_store: op_store_alice,
        transport: _transport_alice,
        ..
    } = make_peer(Some(fetch_config_alice), true).await;
    let fetch_alice = fetch_alice.unwrap();

    // Make Bob without fetch.
    let Peer {
        op_store: op_store_bob,
        transport: transport_bob,
        peer_url: peer_url_bob,
        builder: builder_bob,
        ..
    } = make_peer(None, false).await;

    let num_of_ops = 100;
    let (requested_ops_bob, requested_op_ids_bob) = create_op_list(num_of_ops);

    // Add Bob's ops to his store.
    op_store_bob
        .process_incoming_ops(
            requested_ops_bob
                .clone()
                .into_iter()
                .map(Into::into)
                .collect(),
        )
        .await
        .unwrap();

    // Alice requests the ops from Bob.
    fetch_alice
        .request_ops(requested_op_ids_bob.clone(), peer_url_bob)
        .await
        .unwrap();

    // Let some time pass for Alice to attempt sending a request to Bob
    // and put him on the back off list.
    tokio::time::sleep(Duration::from_millis(10)).await;

    // Alice could not fetch ops from Bob.
    let ops_in_store_alice = op_store_alice
        .retrieve_ops(requested_op_ids_bob.clone())
        .await
        .unwrap();
    assert_eq!(ops_in_store_alice, vec![]);

    // Bob comes online. Fetch is kept to keep message handler live.
    let _fetch_bob = builder_bob
        .fetch
        .create(
            builder_bob.clone(),
            TEST_SPACE_ID,
            op_store_bob.clone(),
            transport_bob,
        )
        .await
        .unwrap();

    // Wait until Alice has fetched Bob's ops.
    assert_ops_arrived_in_store(op_store_alice, requested_op_ids_bob).await;
}
