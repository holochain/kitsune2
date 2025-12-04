use bytes::Bytes;
use kitsune2_api::{K2Proto, K2WireType, Url};
use kitsune2_test_utils::{retry_fn_until_timeout, space::TEST_SPACE_ID};
use kitsune2_transport_iroh::test_utils::{
    dummy_url, IrohTransportTestHarness, MockTxHandler,
};
use std::{
    sync::Arc,
    time::{Duration, SystemTime},
};

#[tokio::test]
async fn connect_two_endpoints() {
    let harness = IrohTransportTestHarness::new().await;
    let dummy_url = Url::from_str("http://url.not.set:0/0").unwrap();

    let handler_1 = Arc::new(MockTxHandler::default());
    let ep_1 = harness.build_transport(handler_1.clone()).await;
    ep_1.register_space_handler(TEST_SPACE_ID, handler_1.clone());

    assert!(
        ep_1.get_connected_peers().await.unwrap().is_empty(),
        "peers connected to ep_1 should be empty"
    );

    let (space_notify_sender, mut space_notify_receiver) =
        tokio::sync::mpsc::unbounded_channel();
    let recv_space_notify = Arc::new(move |_peer, _space_id, data| {
        space_notify_sender.send(data).unwrap();
        Ok(())
    });
    let handler_2 = Arc::new(MockTxHandler {
        recv_space_notify,
        ..Default::default()
    });
    let ep_2 = harness.build_transport(handler_2.clone()).await;
    ep_2.register_space_handler(TEST_SPACE_ID, handler_2.clone());

    assert!(
        ep_2.get_connected_peers().await.unwrap().is_empty(),
        "peers connected to ep_2 should be empty"
    );

    // Wait for URLs to be updated
    retry_fn_until_timeout(
        || async {
            let ep_1_url = handler_1.current_url.lock().unwrap().clone();
            let ep_2_url = handler_2.current_url.lock().unwrap().clone();
            ep_1_url != dummy_url && ep_2_url != dummy_url
        },
        Some(5000),
        Some(500),
    )
    .await
    .unwrap();

    let message = b"hello";
    let ep2_url = handler_2.current_url.lock().unwrap().clone();
    ep_1.send_space_notify(ep2_url, TEST_SPACE_ID, Bytes::from_static(message))
        .await
        .unwrap();

    tokio::time::timeout(Duration::from_secs(1), async {
        let received_space_notification =
            space_notify_receiver.recv().await.unwrap();
        assert_eq!(*received_space_notification, *message);
    })
    .await
    .expect("message was not received by ep_2");

    assert_eq!(ep_1.get_connected_peers().await.unwrap().len(), 1);
    assert_eq!(ep_2.get_connected_peers().await.unwrap().len(), 1);
}

#[tokio::test]
async fn preflight_only_called_once_per_peer() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    let harness = IrohTransportTestHarness::new().await;

    let dummy_url = Url::from_str("http://url.not.set:0/0").unwrap();

    let preflight_count = Arc::new(AtomicUsize::new(0));
    let handler_1 = Arc::new(MockTxHandler {
        preflight_gather_outgoing: Arc::new({
            let preflight_count = preflight_count.clone();
            move |_| {
                preflight_count.fetch_add(1, Ordering::SeqCst);
                Ok(Bytes::new())
            }
        }),
        ..Default::default()
    });
    let ep_1 = harness.build_transport(handler_1.clone()).await;
    ep_1.register_space_handler(TEST_SPACE_ID, handler_1.clone());

    let (space_notify_sender, mut space_notify_receiver) =
        tokio::sync::mpsc::unbounded_channel();
    let handler_2 = Arc::new(MockTxHandler {
        recv_space_notify: Arc::new(move |_peer, _space_id, data| {
            space_notify_sender.send(data).unwrap();
            Ok(())
        }),
        ..Default::default()
    });
    let ep_2 = harness.build_transport(handler_2.clone()).await;
    ep_2.register_space_handler(TEST_SPACE_ID, handler_2.clone());

    // Wait for URLs to be updated
    retry_fn_until_timeout(
        || async {
            let ep_1_url = handler_1.current_url.lock().unwrap().clone();
            let ep_2_url = handler_2.current_url.lock().unwrap().clone();
            ep_1_url != dummy_url && ep_2_url != dummy_url
        },
        Some(6000),
        Some(500),
    )
    .await
    .unwrap();

    let ep_2_url = handler_2.current_url.lock().unwrap().clone();

    // Spawn two concurrent sends
    let message_1 = Bytes::from_static(b"msg1");
    let send_1 = tokio::spawn({
        let ep_1 = ep_1.clone();
        let ep_2_url = ep_2_url.clone();
        let message_1 = message_1.clone();
        async move {
            ep_1.send_space_notify(ep_2_url, TEST_SPACE_ID, message_1)
                .await
        }
    });

    let message_2 = Bytes::from_static(b"msg2");
    let send_2 = tokio::spawn({
        let ep_1 = ep_1.clone();
        let ep_2_url = ep_2_url.clone();
        let message_2 = message_2.clone();
        async move {
            ep_1.send_space_notify(ep_2_url, TEST_SPACE_ID, message_2)
                .await
        }
    });

    let (result_1, result_2) = tokio::join!(send_1, send_2);
    result_1.unwrap().unwrap();
    result_2.unwrap().unwrap();

    let response_1 = space_notify_receiver.recv().await.unwrap();
    let response_2 = space_notify_receiver.recv().await.unwrap();
    let mut expected = vec![message_1, message_2];
    expected.sort();
    let mut responses = vec![response_1, response_2];
    responses.sort();
    assert_eq!(responses, expected);

    // Preflight should have been called exactly once.
    assert_eq!(preflight_count.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn network_stats() {
    let harness = IrohTransportTestHarness::new().await;
    let (response_received_sender, mut response_received_receiver) =
        tokio::sync::mpsc::unbounded_channel();
    let handler_1 = Arc::new(MockTxHandler {
        recv_space_notify: Arc::new(move |_, _, _| {
            response_received_sender.send(()).unwrap();
            Ok(())
        }),
        ..Default::default()
    });
    let ep_1 = harness.build_transport(handler_1.clone()).await;
    ep_1.register_space_handler(TEST_SPACE_ID, handler_1.clone());

    let network_stats = ep_1.dump_network_stats().await.unwrap();
    assert_eq!(
        network_stats.blocked_message_counts.len(),
        0,
        "expected 0 blocked messages but got {}",
        network_stats.blocked_message_counts.len()
    );
    assert_eq!(
        network_stats.transport_stats.backend, "iroh",
        "expected iroh backend but got {}",
        network_stats.transport_stats.backend
    );
    assert!(
        network_stats.transport_stats.connections.is_empty(),
        "expected transport connections to be empty but got {}",
        network_stats.transport_stats.connections.len()
    );

    // Create another endpoint to connect to.
    let (request_received_sender, mut request_received_receiver) =
        tokio::sync::mpsc::unbounded_channel();
    let handler_2 = Arc::new(MockTxHandler {
        recv_space_notify: Arc::new(move |_, _, _| {
            request_received_sender.send(()).unwrap();
            Ok(())
        }),
        ..Default::default()
    });
    let ep_2 = harness.build_transport(handler_2.clone()).await;
    ep_2.register_space_handler(TEST_SPACE_ID, handler_2.clone());

    let dummy_url = dummy_url();
    retry_fn_until_timeout(
        || async {
            handler_1.current_url.lock().unwrap().clone() != dummy_url
                && handler_2.current_url.lock().unwrap().clone() != dummy_url
        },
        Some(5000),
        Some(500),
    )
    .await
    .unwrap();
    let ep_1_url = handler_1.current_url.lock().unwrap().clone();
    let ep_2_url = handler_2.current_url.lock().unwrap().clone();

    // ep 1 connects to ep 2 and sends a space notification message.
    let request = Bytes::from_static(b"request");
    let opened_at_s = SystemTime::UNIX_EPOCH.elapsed().unwrap().as_secs();
    ep_1.send_space_notify(ep_2_url.clone(), TEST_SPACE_ID, request.clone())
        .await
        .unwrap();
    tokio::time::timeout(
        Duration::from_secs(1),
        request_received_receiver.recv(),
    )
    .await
    .expect("request not received");

    // ep 2 send a message back to have some stats.
    let response = Bytes::from_static(b"response");
    ep_2.send_space_notify(ep_1_url.clone(), TEST_SPACE_ID, response.clone())
        .await
        .unwrap();
    tokio::time::timeout(
        Duration::from_secs(1),
        response_received_receiver.recv(),
    )
    .await
    .expect("response not received");

    let network_stats = ep_1.dump_network_stats().await.unwrap();
    assert_eq!(
        network_stats.blocked_message_counts.len(),
        0,
        "expected 0 blocked messages but got {}",
        network_stats.blocked_message_counts.len()
    );
    assert_eq!(
        network_stats.transport_stats.backend, "iroh",
        "expected iroh backend but got {}",
        network_stats.transport_stats.backend
    );
    // ep 1's local URL
    assert_eq!(
        network_stats.transport_stats.peer_urls.len(),
        1,
        "expected 1 peer url but got {}",
        network_stats.transport_stats.peer_urls.len()
    );
    assert_eq!(network_stats.transport_stats.peer_urls[0], ep_1_url);
    assert_eq!(
        network_stats.transport_stats.connections.len(),
        1,
        "expected 1 connection but got {}",
        network_stats.transport_stats.connections.len()
    );
    let stats_ep_2 = &network_stats.transport_stats.connections[0];
    assert_eq!(
        stats_ep_2.pub_key,
        ep_2_url.peer_id().unwrap(),
        "unexpected pub key"
    );
    assert_eq!(
        stats_ep_2.send_message_count, 1,
        "expected send message count to be 1 but got {}",
        stats_ep_2.send_message_count
    );
    let expected_send_bytes = (K2Proto {
        ty: K2WireType::Notify as i32,
        data: request,
        space_id: Some(TEST_SPACE_ID.into()),
        module_id: None,
    })
    .encode()
    .unwrap()
    .len() as u64;
    assert_eq!(
        stats_ep_2.send_bytes, expected_send_bytes,
        "unexpected send bytes {}",
        stats_ep_2.send_bytes
    );
    assert_eq!(
        stats_ep_2.recv_message_count, 1,
        "expected recv message count 1 but got {}",
        stats_ep_2.recv_message_count
    );
    let expected_recv_bytes = (K2Proto {
        ty: K2WireType::Notify as i32,
        data: response,
        space_id: Some(TEST_SPACE_ID.into()),
        module_id: None,
    })
    .encode()
    .unwrap()
    .len() as u64;
    assert_eq!(
        stats_ep_2.recv_bytes, expected_recv_bytes,
        "unexpected recv bytes {}",
        stats_ep_2.recv_bytes
    );
    // opened_at_s might differ by some milliseconds, so assert the absolute
    // difference is within less than a second.
    assert!(
        opened_at_s.abs_diff(stats_ep_2.opened_at_s) < 1,
        "opened at s is more than a second off expected moment"
    );
    assert!(
        stats_ep_2.is_direct,
        "expected direct connection but got false"
    );
}
