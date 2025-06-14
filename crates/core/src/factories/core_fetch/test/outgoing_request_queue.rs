use super::test_utils::random_peer_url;
use crate::{
    default_test_builder,
    factories::core_fetch::{
        test::test_utils::make_op, CoreFetch, CoreFetchConfig,
    },
};
use kitsune2_api::*;
use kitsune2_test_utils::{
    id::{create_op_id_list, random_op_id},
    iter_check,
    space::TEST_SPACE_ID,
};
use prost::Message;
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

type RequestsSent = Vec<(OpId, Url)>;

struct TestCase {
    fetch: CoreFetch,
    requests_sent: Arc<Mutex<RequestsSent>>,
    op_store: DynOpStore,
    peer_meta_store: DynPeerMetaStore,
    _transport: DynTransport,
}

async fn setup_test(config: &CoreFetchConfig) -> TestCase {
    let builder =
        Arc::new(default_test_builder().with_default_config().unwrap());
    let op_store = builder
        .op_store
        .create(builder.clone(), TEST_SPACE_ID)
        .await
        .unwrap();
    let peer_meta_store = builder
        .peer_meta_store
        .create(builder.clone(), TEST_SPACE_ID)
        .await
        .unwrap();
    let requests_sent = Arc::new(Mutex::new(Vec::new()));
    let mock_transport = make_mock_transport(requests_sent.clone());

    let fetch = CoreFetch::new(
        config.clone(),
        TEST_SPACE_ID,
        op_store.clone(),
        peer_meta_store.clone(),
        mock_transport.clone(),
    );

    TestCase {
        fetch,
        requests_sent,
        op_store,
        peer_meta_store,
        _transport: mock_transport,
    }
}

fn make_mock_transport(
    requests_sent: Arc<Mutex<RequestsSent>>,
) -> Arc<MockTransport> {
    let mut mock_transport = MockTransport::new();
    mock_transport.expect_send_module().returning({
        let requests_sent = requests_sent.clone();
        move |peer, space, module, data| {
            assert_eq!(space, TEST_SPACE_ID);
            assert_eq!(module, crate::factories::core_fetch::MOD_NAME);
            Box::pin({
                let requests_sent = requests_sent.clone();
                async move {
                    let fetch_message = K2FetchMessage::decode(data).unwrap();
                    let op_ids: Vec<OpId> =
                        match fetch_message.fetch_message_type() {
                            FetchMessageType::Request => {
                                let request =
                                    FetchRequest::decode(fetch_message.data)
                                        .unwrap();
                                request.into()
                            }
                            _ => panic!("unexpected fetch message type"),
                        };
                    let mut lock = requests_sent.lock().unwrap();
                    op_ids.into_iter().for_each(|op_id| {
                        lock.push((op_id, peer.clone()));
                    });

                    Ok(())
                }
            })
        }
    });
    mock_transport
        .expect_register_module_handler()
        .returning(|_, _, _| ());
    Arc::new(mock_transport)
}

#[tokio::test(flavor = "multi_thread")]
async fn outgoing_request_queue() {
    let config = CoreFetchConfig {
        re_insert_outgoing_request_delay_ms: 50,
        ..Default::default()
    };
    let TestCase {
        fetch,
        requests_sent,
        _transport,
        ..
    } = setup_test(&config).await;

    let op_id = random_op_id();
    let op_list = vec![op_id.clone()];
    let peer_url = random_peer_url();

    assert!(requests_sent.lock().unwrap().is_empty());

    // Add 1 op.
    fetch.request_ops(op_list, peer_url.clone()).await.unwrap();

    // Let the fetch request be sent multiple times. As only 1 op was added to the queue,
    // this proves that it is being re-added to the queue after sending a request for it.
    iter_check!(500, {
        if requests_sent.lock().unwrap().len() >= 2 {
            break;
        }
    });

    // Clear set of ops to fetch to stop sending requests.
    fetch.state.lock().unwrap().requests.clear();

    let num_requests_sent = requests_sent.lock().unwrap().len();

    // Check that all requests have been made for the 1 op to the agent.
    assert!(requests_sent
        .lock()
        .unwrap()
        .iter()
        .all(|request| request == &(op_id.clone(), peer_url.clone())));

    // Give time for more requests to be sent, which shouldn't happen now that the set of
    // ops to fetch is cleared.
    tokio::time::sleep(Duration::from_millis(
        (config.re_insert_outgoing_request_delay_ms * 2) as u64,
    ))
    .await;

    // No more requests should have been sent.
    // Ideally it were possible to check that no more fetch request have been passed back into
    // the internal channel, but that would require a custom wrapper around the channel.
    let requests_sent = requests_sent.lock().unwrap().clone();
    assert_eq!(requests_sent.len(), num_requests_sent);
}

#[tokio::test(flavor = "multi_thread")]
async fn happy_op_fetch_from_multiple_agents() {
    let config = CoreFetchConfig {
        parallel_request_count: 5,
        ..Default::default()
    };
    let TestCase {
        fetch,
        requests_sent,
        _transport,
        ..
    } = setup_test(&config).await;

    let op_list_1 = create_op_id_list(10);
    let op_list_2 = create_op_id_list(20);
    let op_list_3 = create_op_id_list(30);
    let total_ops = op_list_1.len() + op_list_2.len() + op_list_3.len();

    let peer_url_1 = random_peer_url();
    let peer_url_2 = random_peer_url();
    let peer_url_3 = random_peer_url();

    let mut expected_requests = Vec::new();
    op_list_1
        .clone()
        .into_iter()
        .for_each(|op_id| expected_requests.push((op_id, peer_url_1.clone())));
    op_list_2
        .clone()
        .into_iter()
        .for_each(|op_id| expected_requests.push((op_id, peer_url_2.clone())));
    op_list_3
        .clone()
        .into_iter()
        .for_each(|op_id| expected_requests.push((op_id, peer_url_3.clone())));
    let mut expected_ops = Vec::new();
    op_list_1
        .clone()
        .into_iter()
        .for_each(|op_id| expected_ops.push((op_id, peer_url_1.clone())));
    op_list_2
        .clone()
        .into_iter()
        .for_each(|op_id| expected_ops.push((op_id, peer_url_2.clone())));
    op_list_3
        .clone()
        .into_iter()
        .for_each(|op_id| expected_ops.push((op_id, peer_url_3.clone())));

    futures::future::join_all([
        fetch.request_ops(op_list_1.clone(), peer_url_1.clone()),
        fetch.request_ops(op_list_2.clone(), peer_url_2.clone()),
        fetch.request_ops(op_list_3.clone(), peer_url_3.clone()),
    ])
    .await;

    // Check that at least one request was sent for each op.
    iter_check!({
        let requests_sent = requests_sent.lock().unwrap().clone();
        if requests_sent.len() >= total_ops
            && expected_requests
                .iter()
                .all(|expected_op| requests_sent.contains(expected_op))
        {
            break;
        }
    });

    // Check that op ids are still part of ops to fetch.
    let lock = fetch.state.lock().unwrap();
    assert!(expected_ops.iter().all(|v| lock.requests.contains(v)));
}

#[tokio::test(flavor = "multi_thread")]
async fn filter_requests_for_held_ops() {
    let TestCase {
        fetch,
        requests_sent,
        op_store,
        _transport,
        ..
    } = setup_test(&CoreFetchConfig::default()).await;

    let held_op_1 = make_op(vec![1; 64]);
    let held_op_id_1 = held_op_1.compute_op_id();
    let held_op_2 = make_op(vec![2; 64]);
    let held_op_id_2 = held_op_2.compute_op_id();
    let peer_url = random_peer_url();

    op_store
        .process_incoming_ops(vec![held_op_1.into(), held_op_2.into()])
        .await
        .unwrap();

    let new_op = make_op(vec![3; 64]);
    let new_op_id = new_op.compute_op_id();
    let op_list = vec![
        held_op_id_1.clone(),
        held_op_id_2.clone(),
        new_op_id.clone(),
    ];

    fetch.request_ops(op_list, peer_url.clone()).await.unwrap();
    iter_check!(1000, 100, {
        let lock = requests_sent.lock().unwrap();
        if lock.contains(&(new_op_id.clone(), peer_url.clone())) {
            assert!(!lock.contains(&(held_op_id_1.clone(), peer_url.clone())));
            assert!(!lock.contains(&(held_op_id_2.clone(), peer_url.clone())));
            break;
        }
    });
}

#[tokio::test(flavor = "multi_thread")]
async fn unresponsive_urls_are_filtered() {
    let responsive_url = random_peer_url();
    let unresponsive_url = random_peer_url();

    let builder =
        Arc::new(default_test_builder().with_default_config().unwrap());
    let op_store = builder
        .op_store
        .create(builder.clone(), TEST_SPACE_ID)
        .await
        .unwrap();
    // Create a mock transport to track where fetch requests are sent.
    // Set up a channel to know when the intended fetch request has been sent.
    let (expected_fetch_request_sent_tx, mut expected_fetch_request_sent_rx) =
        tokio::sync::mpsc::channel(1);
    let mut transport = MockTransport::new();
    let responsive_url_clone = responsive_url.clone();
    let unresponsive_url_clone = unresponsive_url.clone();
    transport.expect_send_module().returning({
        move |url, _, _, _| {
            if url == unresponsive_url_clone {
                panic!("fetch request sent to unresponsive URL");
            }
            if url == responsive_url_clone {
                expected_fetch_request_sent_tx.try_send(()).unwrap();
            }
            Box::pin(async move { Ok(()) })
        }
    });
    transport
        .expect_register_module_handler()
        .returning(|_, _, _| ());
    let transport = Arc::new(transport);
    let peer_meta_store = builder
        .peer_meta_store
        .create(builder.clone(), TEST_SPACE_ID)
        .await
        .unwrap();
    let fetch = CoreFetch::new(
        CoreFetchConfig::default(),
        TEST_SPACE_ID,
        op_store.clone(),
        peer_meta_store.clone(),
        transport.clone(),
    );

    let op_list_1 = create_op_id_list(1);
    let op_list_2 = create_op_id_list(1);
    // Set unresponsive URL as unresponsive in peer meta store.
    peer_meta_store
        .set_unresponsive(
            unresponsive_url.clone(),
            Timestamp::now(),
            Timestamp::now(),
        )
        .await
        .unwrap();

    // Request ops from unresponsive URL, which should be omitted.
    fetch
        .request_ops(op_list_1, responsive_url.clone())
        .await
        .unwrap();
    // Request ops from another URL which is not unresponsive, so that this request
    // can be awaited to have happened, which implies that the previous
    // publish to the unresponsive URL has been omitted.
    fetch
        .request_ops(op_list_2, unresponsive_url.clone())
        .await
        .unwrap();

    // Timeout after waiting 100 ms for fetch to request from the responsive URL.
    tokio::select! {
        _ = expected_fetch_request_sent_rx.recv() => {},
        _ = tokio::time::sleep(Duration::from_millis(100)) => {
            panic!("publish not sent to responsive URL")
        },
    };
}

#[tokio::test(flavor = "multi_thread")]
async fn requests_are_dropped_when_peer_url_unresponsive() {
    let config = CoreFetchConfig {
        re_insert_outgoing_request_delay_ms: 10,
        ..Default::default()
    };
    let TestCase {
        fetch,
        peer_meta_store,
        _transport,
        ..
    } = setup_test(&config).await;

    let op_list_1 = create_op_id_list(2);
    let unresponsive_url = random_peer_url();

    // Create a second agent to later check that their ops have not been removed.
    let op_list_2 = create_op_id_list(2);
    let responsive_url = random_peer_url();

    peer_meta_store
        .set_unresponsive(
            unresponsive_url.clone(),
            Timestamp::now(),
            Timestamp::now(),
        )
        .await
        .unwrap();

    // Check that when the requests are dropped, the queue is treated as drained.
    let (tx, rx) = futures::channel::oneshot::channel();
    fetch.notify_on_drained(tx);

    fetch
        .request_ops(op_list_1.clone(), unresponsive_url.clone())
        .await
        .unwrap();

    // All ops in the request set are to be sent to the unresponsive URL.
    assert!(fetch
        .state
        .lock()
        .unwrap()
        .requests
        .iter()
        .all(|(_, peer_url)| *peer_url == unresponsive_url));

    // Check that the drop has happened.
    tokio::time::timeout(Duration::from_secs(1), rx)
        .await
        .expect("Timed out waiting for drained notification")
        .unwrap();

    // Add control agent's ops to set.
    fetch
        .request_ops(op_list_2, responsive_url.clone())
        .await
        .unwrap();

    // Wait for a request attempt to the unresponsive URL, which should remove all of its requests
    // from the set.
    iter_check!({
        if fetch
            .state
            .lock()
            .unwrap()
            .requests
            .iter()
            .all(|(_, peer_url)| *peer_url != unresponsive_url)
        {
            break;
        }
    });
}

#[tokio::test(flavor = "multi_thread")]
async fn empty_fetch_queue_notifies_drained() {
    let config = CoreFetchConfig {
        re_insert_outgoing_request_delay_ms: 10,
        ..Default::default()
    };
    let TestCase {
        fetch, _transport, ..
    } = setup_test(&config).await;

    let (tx, mut rx) = futures::channel::oneshot::channel();
    fetch.notify_on_drained(tx);

    // The fetch queue should be immediately empty, so the notification should be sent
    // immediately.
    assert!(rx.try_recv().unwrap().is_some());
}

#[tokio::test(flavor = "multi_thread")]
async fn fetch_queue_notify_on_last_op_fetched() {
    let config = CoreFetchConfig {
        re_insert_outgoing_request_delay_ms: 10,
        ..Default::default()
    };
    let TestCase {
        fetch, _transport, ..
    } = setup_test(&config).await;

    let op_list = create_op_id_list(10);
    let peer_url = random_peer_url();

    let mut expected_requests = Vec::new();
    op_list
        .clone()
        .into_iter()
        .for_each(|op_id| expected_requests.push((op_id, peer_url.clone())));
    let mut expected_ops = Vec::new();
    op_list
        .clone()
        .into_iter()
        .for_each(|op_id| expected_ops.push((op_id, peer_url.clone())));

    let (tx, rx) = futures::channel::oneshot::channel();
    fetch.notify_on_drained(tx);

    fetch
        .request_ops(op_list.clone(), peer_url.clone())
        .await
        .unwrap();

    tokio::time::timeout(Duration::from_secs(1), rx)
        .await
        .expect("Timed out")
        .unwrap();
}
