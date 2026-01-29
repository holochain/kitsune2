use bytes::Bytes;
use kitsune2_api::{K2Proto, K2WireType, Timestamp, Url};
use kitsune2_test_utils::{
    enable_tracing, retry_fn_until_timeout, space::TEST_SPACE_ID,
};
use kitsune2_transport_iroh::test_utils::{
    IrohTransportTestHarness, MockTxHandler, dummy_url,
};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::{
    sync::Arc,
    time::{Duration, SystemTime},
};

#[tokio::test]
async fn get_connected_peers() {
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
async fn send_and_receive_space_notify() {
    enable_tracing();
    let harness = IrohTransportTestHarness::new().await;
    let dummy_url = Url::from_str("http://url.not.set:0/0").unwrap();

    let handler_1 = Arc::new(MockTxHandler::default());
    let ep_1 = harness.build_transport(handler_1.clone()).await;
    ep_1.register_space_handler(TEST_SPACE_ID, handler_1.clone());

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
}

#[tokio::test]
async fn send_and_receive_module_message() {
    enable_tracing();
    let harness = IrohTransportTestHarness::new().await;
    let dummy_url = Url::from_str("http://url.not.set:0/0").unwrap();
    let module_id = "test-mod".to_string();

    let handler_1 = Arc::new(MockTxHandler::default());
    let ep_1 = harness.build_transport(handler_1.clone()).await;
    ep_1.register_space_handler(TEST_SPACE_ID, handler_1.clone());
    ep_1.register_module_handler(
        TEST_SPACE_ID,
        module_id.clone(),
        handler_1.clone(),
    );

    let (module_message_sender, mut module_message_receiver) =
        tokio::sync::mpsc::unbounded_channel();
    let recv_module_msg =
        Arc::new(move |_peer, _space_id, _module_id, data| {
            module_message_sender.send(data).unwrap();
            Ok(())
        });
    let handler_2 = Arc::new(MockTxHandler {
        recv_module_msg,
        ..Default::default()
    });
    let ep_2 = harness.build_transport(handler_2.clone()).await;
    ep_2.register_space_handler(TEST_SPACE_ID, handler_2.clone());
    ep_2.register_module_handler(
        TEST_SPACE_ID,
        module_id.clone(),
        handler_2.clone(),
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
    ep_1.send_module(
        ep2_url,
        TEST_SPACE_ID,
        module_id,
        Bytes::from_static(message),
    )
    .await
    .unwrap();

    tokio::time::timeout(Duration::from_secs(1), async {
        let received_module_message =
            module_message_receiver.recv().await.unwrap();
        assert_eq!(*received_module_message, *message);
    })
    .await
    .expect("message was not received by ep_2");
}

#[tokio::test]
async fn peer_is_set_unresponsive_after_connection_error() {
    enable_tracing();
    let harness = IrohTransportTestHarness::new().await;
    let dummy_url = dummy_url();

    let (space_notify_sender, mut space_notify_receiver) =
        tokio::sync::mpsc::unbounded_channel();
    let (set_unresponsive_sender, mut set_unresponsive_receiver) =
        tokio::sync::mpsc::unbounded_channel();
    let handler_1 = Arc::new(MockTxHandler {
        set_unresponsive: Arc::new(move |peer, timestamp| {
            set_unresponsive_sender.send((peer, timestamp)).unwrap();
            Ok(())
        }),
        ..Default::default()
    });
    let ep_1 = harness.build_transport(handler_1.clone()).await;
    ep_1.register_space_handler(TEST_SPACE_ID, handler_1.clone());

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
            handler_1.current_url.lock().unwrap().clone() != dummy_url
                && handler_2.current_url.lock().unwrap().clone() != dummy_url
        },
        Some(6000),
        Some(500),
    )
    .await
    .unwrap();

    let ep_2_url = handler_2.current_url.lock().unwrap().clone();

    // Send first message to ep_2 which should work.
    let message = Bytes::from_static(b"message");
    ep_1.send_space_notify(ep_2_url.clone(), TEST_SPACE_ID, message.clone())
        .await
        .unwrap();

    tokio::time::timeout(
        Duration::from_millis(100),
        space_notify_receiver.recv(),
    )
    .await
    .expect("message should have been sent");

    // ep 2 goes offline
    drop(ep_2);

    let expected_timestamp = Timestamp::now();

    // ep 1 should notice that ep 2 went offline and mark it unresponsive
    tokio::time::timeout(Duration::from_secs(2), async {
        let (unresponsive_url, timestamp) =
            set_unresponsive_receiver.recv().await.unwrap();
        assert_eq!(unresponsive_url, ep_2_url);
        // Timestamp should be accurate to ~1 second.
        assert!(
            timestamp
                .as_micros()
                .abs_diff(expected_timestamp.as_micros())
                <= 1_000_000
        );
    })
    .await
    .expect("peer not set unresponsive");
}

#[tokio::test]
async fn reconnect_succeeds_after_connection_lost() {
    enable_tracing();
    let harness = IrohTransportTestHarness::new().await;
    let dummy_url = dummy_url();

    let (space_notify_sender, mut space_notify_receiver) =
        tokio::sync::mpsc::unbounded_channel();
    let handler_1 = Arc::new(MockTxHandler {
        recv_space_notify: Arc::new(move |_peer, _space_id, data| {
            space_notify_sender.send(data).unwrap();
            Ok(())
        }),
        ..Default::default()
    });
    let ep_1 = harness.build_transport(handler_1.clone()).await;
    ep_1.register_space_handler(TEST_SPACE_ID, handler_1.clone());

    let handler_2 = Arc::new(MockTxHandler::default());
    let ep_2 = harness.build_transport(handler_2.clone()).await;
    ep_2.register_space_handler(TEST_SPACE_ID, handler_2.clone());

    // Wait for URLs to be updated
    retry_fn_until_timeout(
        || async {
            handler_1.current_url.lock().unwrap().clone() != dummy_url
                && handler_2.current_url.lock().unwrap().clone() != dummy_url
        },
        Some(6000),
        Some(500),
    )
    .await
    .unwrap();

    let ep_1_url = handler_1.current_url.lock().unwrap().clone();

    // Establish connection by sending first message
    let first_message = Bytes::from_static(b"first");
    ep_2.send_space_notify(
        ep_1_url.clone(),
        TEST_SPACE_ID,
        first_message.clone(),
    )
    .await
    .unwrap();

    // Verify first message received
    tokio::time::timeout(Duration::from_secs(2), async {
        let received = space_notify_receiver.recv().await.unwrap();
        assert_eq!(received, first_message);
    })
    .await
    .expect("first message should be received");

    // Verify connection exists
    assert_eq!(
        ep_2.get_connected_peers().await.unwrap().len(),
        1,
        "connection should exist"
    );

    // Force disconnect the connection
    ep_2.disconnect(ep_1_url.clone(), None).await;

    // Verify connection is removed
    retry_fn_until_timeout(
        || async { ep_2.get_connected_peers().await.unwrap().is_empty() },
        None,
        None,
    )
    .await
    .expect("connection should be removed after disconnect");

    // Send second message - this should trigger automatic reconnection
    let second_message = Bytes::from_static(b"second");
    ep_2.send_space_notify(
        ep_1_url.clone(),
        TEST_SPACE_ID,
        second_message.clone(),
    )
    .await
    .unwrap();

    // Verify second message received (proves reconnection worked)
    tokio::time::timeout(Duration::from_secs(2), async {
        let received = space_notify_receiver.recv().await.unwrap();
        assert_eq!(received, second_message);
    })
    .await
    .expect("second message should be received after reconnection");

    // Verify connection was reestablished
    assert_eq!(
        ep_2.get_connected_peers().await.unwrap().len(),
        1,
        "connection should be reestablished"
    );
}

#[tokio::test]
async fn reconnect_succeeds_after_stream_error() {
    enable_tracing();
    let harness = IrohTransportTestHarness::new().await;
    let dummy_url = dummy_url();

    // Use atomic to fail only the first message
    let should_fail = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let (space_notify_sender, mut space_notify_receiver) =
        tokio::sync::mpsc::unbounded_channel();
    let handler_1 = Arc::new(MockTxHandler {
        recv_space_notify: Arc::new({
            let should_fail = should_fail.clone();
            move |_peer, _space_id, data| {
                if should_fail.swap(false, std::sync::atomic::Ordering::SeqCst)
                {
                    Err(kitsune2_api::K2Error::other("stream processing error"))
                } else {
                    space_notify_sender.send(data).unwrap();
                    Ok(())
                }
            }
        }),
        ..Default::default()
    });
    let ep_1 = harness.build_transport(handler_1.clone()).await;
    ep_1.register_space_handler(TEST_SPACE_ID, handler_1.clone());

    let handler_2 = Arc::new(MockTxHandler::default());
    let ep_2 = harness.build_transport(handler_2.clone()).await;
    ep_2.register_space_handler(TEST_SPACE_ID, handler_2.clone());

    // Wait for URLs to be updated
    retry_fn_until_timeout(
        || async {
            handler_1.current_url.lock().unwrap().clone() != dummy_url
                && handler_2.current_url.lock().unwrap().clone() != dummy_url
        },
        Some(6000),
        Some(500),
    )
    .await
    .unwrap();

    let ep_1_url = handler_1.current_url.lock().unwrap().clone();

    // First message - will fail with stream error
    let first_message = Bytes::from_static(b"first");
    ep_2.send_space_notify(ep_1_url.clone(), TEST_SPACE_ID, first_message)
        .await
        .unwrap();

    // Wait for error to be processed
    retry_fn_until_timeout(
        || async {
            !should_fail.load(std::sync::atomic::Ordering::SeqCst)
                && ep_2
                    .dump_network_stats()
                    .await
                    .unwrap()
                    .transport_stats
                    .connections[0]
                    .send_message_count
                    == 1
        },
        Some(1000),
        Some(100),
    )
    .await
    .expect("first message should have been sent and failed to be received");

    // Connection should still exist after stream error
    assert_eq!(
        ep_2.get_connected_peers().await.unwrap().len(),
        1,
        "connection remains after stream error"
    );

    // Second message - should trigger reconnection and succeed
    let second_message = Bytes::from_static(b"second");
    ep_2.send_space_notify(
        ep_1_url.clone(),
        TEST_SPACE_ID,
        second_message.clone(),
    )
    .await
    .unwrap();

    // Verify second message received (proves reconnection worked)
    tokio::time::timeout(Duration::from_secs(2), async {
        let received = space_notify_receiver.recv().await.unwrap();
        assert_eq!(received, second_message);
    })
    .await
    .expect("second message should be received after reconnection");

    // Verify connection was reestablished
    assert_eq!(
        ep_2.get_connected_peers().await.unwrap().len(),
        1,
        "connection should be reestablished"
    );
}

#[tokio::test]
async fn reconnect_succeeds_after_preflight_validation_error() {
    enable_tracing();
    let harness = IrohTransportTestHarness::new().await;
    let dummy_url = dummy_url();

    // Use atomic to fail only the first preflight validation
    let should_fail = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let (space_notify_sender, mut space_notify_receiver) =
        tokio::sync::mpsc::unbounded_channel();
    let handler_1 = Arc::new(MockTxHandler {
        preflight_validate_incoming: Arc::new({
            let should_fail = should_fail.clone();
            move |_, _| {
                if should_fail.swap(false, std::sync::atomic::Ordering::SeqCst)
                {
                    Err(kitsune2_api::K2Error::other(
                        "preflight validation failed",
                    ))
                } else {
                    Ok(())
                }
            }
        }),
        recv_space_notify: Arc::new(move |_peer, _space_id, data| {
            space_notify_sender.send(data).unwrap();
            Ok(())
        }),
        ..Default::default()
    });
    let ep_1 = harness.build_transport(handler_1.clone()).await;
    ep_1.register_space_handler(TEST_SPACE_ID, handler_1.clone());

    let handler_2 = Arc::new(MockTxHandler::default());
    let ep_2 = harness.build_transport(handler_2.clone()).await;
    ep_2.register_space_handler(TEST_SPACE_ID, handler_2.clone());

    // Wait for URLs to be updated
    retry_fn_until_timeout(
        || async {
            handler_1.current_url.lock().unwrap().clone() != dummy_url
                && handler_2.current_url.lock().unwrap().clone() != dummy_url
        },
        Some(6000),
        Some(500),
    )
    .await
    .unwrap();

    let ep_1_url = handler_1.current_url.lock().unwrap().clone();

    // First message - will fail during preflight validation
    let first_message = Bytes::from_static(b"first");
    ep_2.send_space_notify(
        ep_1_url.clone(),
        TEST_SPACE_ID,
        first_message.clone(),
    )
    .await
    .unwrap();

    // ep 1 will close the stream, which will lead to closing the connection.
    // Wait until the closed connection shows in ep 2's stats.
    retry_fn_until_timeout(
        || async {
            ep_2.dump_network_stats()
                .await
                .unwrap()
                .transport_stats
                .connections
                .is_empty()
        },
        Some(100),
        Some(10),
    )
    .await
    .unwrap();

    // Second message - should trigger reconnection with successful preflight
    let second_message = Bytes::from_static(b"second");
    ep_2.send_space_notify(
        ep_1_url.clone(),
        TEST_SPACE_ID,
        second_message.clone(),
    )
    .await
    .unwrap();

    // Verify second message received (proves reconnection worked)
    tokio::time::timeout(Duration::from_secs(2), async {
        let received = space_notify_receiver.recv().await.unwrap();
        assert_eq!(received, second_message);
    })
    .await
    .expect("second message should be received after reconnection");

    // Verify connection was reestablished
    assert_eq!(
        ep_2.get_connected_peers().await.unwrap().len(),
        1,
        "connection should be reestablished"
    );
}

#[tokio::test]
async fn connect_succeeds_after_preflight_gather_error() {
    let harness = IrohTransportTestHarness::new().await;
    let dummy_url = dummy_url();

    let (space_notify_sender, mut space_notify_receiver) =
        tokio::sync::mpsc::unbounded_channel();
    let handler_1 = Arc::new(MockTxHandler {
        recv_space_notify: Arc::new(move |_peer, _space_id, data| {
            space_notify_sender.send(data).unwrap();
            Ok(())
        }),
        ..Default::default()
    });
    let ep_1 = harness.build_transport(handler_1.clone()).await;
    ep_1.register_space_handler(TEST_SPACE_ID, handler_1.clone());

    // Use atomic to fail only the first preflight gather
    let should_fail = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let handler_2 = Arc::new(MockTxHandler {
        preflight_gather_outgoing: Arc::new({
            let should_fail = should_fail.clone();
            move |_| {
                if should_fail.swap(false, std::sync::atomic::Ordering::SeqCst)
                {
                    Err(kitsune2_api::K2Error::other("preflight gather failed"))
                } else {
                    Ok(Bytes::new())
                }
            }
        }),
        ..Default::default()
    });
    let ep_2 = harness.build_transport(handler_2.clone()).await;
    ep_2.register_space_handler(TEST_SPACE_ID, handler_2.clone());

    // Wait for URLs to be updated
    retry_fn_until_timeout(
        || async {
            handler_1.current_url.lock().unwrap().clone() != dummy_url
                && handler_2.current_url.lock().unwrap().clone() != dummy_url
        },
        Some(6000),
        Some(500),
    )
    .await
    .unwrap();

    let ep_1_url = handler_1.current_url.lock().unwrap().clone();

    // First message - will fail during preflight gather
    let first_message = Bytes::from_static(b"first");
    let result = ep_2
        .send_space_notify(ep_1_url.clone(), TEST_SPACE_ID, first_message)
        .await;

    // Preflight gather error should propagate as send error
    assert!(
        result.is_err(),
        "send should fail when preflight gather fails"
    );

    // Connection should not exist since preflight failed before connection was added to map
    assert!(
        ep_2.get_connected_peers().await.unwrap().is_empty(),
        "connection should not exist after preflight gather failure"
    );

    // Second message - should succeed with working preflight gather
    let second_message = Bytes::from_static(b"second");
    ep_2.send_space_notify(
        ep_1_url.clone(),
        TEST_SPACE_ID,
        second_message.clone(),
    )
    .await
    .unwrap();

    // Verify second message received (proves connection worked)
    tokio::time::timeout(Duration::from_secs(2), async {
        let received = space_notify_receiver.recv().await.unwrap();
        assert_eq!(received, second_message);
    })
    .await
    .expect("second message should be received");

    // Verify connection was established
    assert_eq!(
        ep_2.get_connected_peers().await.unwrap().len(),
        1,
        "connection should be established"
    );
}

#[tokio::test]
async fn preflight_sent_and_received_by_both_endpoints() {
    // enable_tracing();
    let harness = IrohTransportTestHarness::new().await;
    let dummy_url = Url::from_str("http://url.not.set:0/0").unwrap();

    let preflight_count_sent_1 = Arc::new(AtomicUsize::new(0));
    let preflight_count_received_1 = Arc::new(AtomicUsize::new(0));
    let preflight_count_sent_2 = Arc::new(AtomicUsize::new(0));
    let preflight_count_received_2 = Arc::new(AtomicUsize::new(0));

    let handler_1 = Arc::new(MockTxHandler {
        preflight_gather_outgoing: Arc::new({
            let preflight_count_sent_1 = preflight_count_sent_1.clone();
            move |_| {
                preflight_count_sent_1.fetch_add(1, Ordering::SeqCst);
                Ok(Bytes::new())
            }
        }),
        preflight_validate_incoming: Arc::new({
            let preflight_count_received_1 = preflight_count_received_1.clone();
            move |_, _| {
                preflight_count_received_1.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
        }),
        ..Default::default()
    });
    let ep_1 = harness.build_transport(handler_1.clone()).await;
    ep_1.register_space_handler(TEST_SPACE_ID, handler_1.clone());

    let (space_notify_sender, mut space_notify_receiver) =
        tokio::sync::mpsc::unbounded_channel();
    let handler_2 = Arc::new(MockTxHandler {
        preflight_gather_outgoing: Arc::new({
            let preflight_count_sent_2 = preflight_count_sent_2.clone();
            move |_| {
                preflight_count_sent_2.fetch_add(1, Ordering::SeqCst);
                Ok(Bytes::new())
            }
        }),
        preflight_validate_incoming: Arc::new({
            let preflight_count_received_2 = preflight_count_received_2.clone();
            move |_, _| {
                preflight_count_received_2.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
        }),
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

    // Send a message to establish the connection.
    let message = Bytes::from_static(b"msg1");
    ep_1.send_space_notify(ep_2_url, TEST_SPACE_ID, message.clone())
        .await
        .unwrap();

    let message_received = space_notify_receiver.recv().await.unwrap();
    assert_eq!(message_received, message);

    // Preflights should have been sent and received exactly once per endpoint.
    assert_eq!(preflight_count_sent_1.load(Ordering::SeqCst), 1);
    assert_eq!(preflight_count_received_2.load(Ordering::SeqCst), 1);
    retry_fn_until_timeout(
        || async {
            preflight_count_sent_2.load(Ordering::SeqCst) == 1
                && preflight_count_received_1.load(Ordering::SeqCst) == 1
        },
        Some(100),
        Some(10),
    )
    .await
    .expect("preflight should have been sent by peer 2 and received by peer 1");
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
    // difference is within 1 seconds.
    assert!(
        opened_at_s.abs_diff(stats_ep_2.opened_at_s) <= 1,
        "opened at s is expected to be within one second difference, but is {}",
        opened_at_s.abs_diff(stats_ep_2.opened_at_s)
    );
    // It's not guaranteed that by now the connection has been upgraded
    // to a direct connection, so this isn't asserted.
}

/// Test that demonstrates Jost's scenario: when a peer fails to return a preflight
/// due to having no local agents, the connecting peer should NOT be marked as unresponsive.
///
/// Scenario:
/// 1. ep_1 connects to ep_2 and sends its preflight
/// 2. ep_2 receives the preflight but fails to return its own because
///    ep_2 has no local agents (has_local_agents returns false)
/// 3. The connection fails, but ep_1 should NOT be marked as unresponsive
///    because this is a temporary state (no local agents yet), not a network failure
///
/// THIS TEST IS EXPECTED TO FAIL without the fix that adds a specific error type
/// for "no local agents" and handles it specially in spawn_connection_reader.
///
/// The fix requires:
/// 1. A specific K2Error variant (e.g., NoLocalAgents) instead of K2Error::other
/// 2. Checking for this error type in spawn_connection_reader and skipping set_unresponsive
#[tokio::test]
async fn no_local_agents_should_not_mark_peer_unresponsive() {
    enable_tracing();
    let harness = IrohTransportTestHarness::new().await;
    let dummy_url = dummy_url();

    // ep_1 is the initiating peer - it will connect to ep_2
    let handler_1 = Arc::new(MockTxHandler::default());
    let ep_1 = harness.build_transport(handler_1.clone()).await;
    ep_1.register_space_handler(TEST_SPACE_ID, handler_1.clone());

    // Track whether set_unresponsive was called on ep_2's handler
    let set_unresponsive_called = Arc::new(AtomicBool::new(false));
    let set_unresponsive_called_clone = set_unresponsive_called.clone();

    // ep_2 has no local agents - this will cause peer_connect to fail
    // when trying to generate the return preflight.
    // The error is created by the real code in TxImpHnd::peer_connect.
    let handler_2 = Arc::new(MockTxHandler {
        has_local_agents: Arc::new(|| Ok(false)), // No local agents!
        set_unresponsive: Arc::new(move |_peer, _timestamp| {
            // Track that set_unresponsive was called
            set_unresponsive_called_clone.store(true, Ordering::SeqCst);
            Ok(())
        }),
        ..Default::default()
    });
    let ep_2 = harness.build_transport(handler_2.clone()).await;
    ep_2.register_space_handler(TEST_SPACE_ID, handler_2.clone());

    // Wait for URLs to be updated
    retry_fn_until_timeout(
        || async {
            handler_1.current_url.lock().unwrap().clone() != dummy_url
                && handler_2.current_url.lock().unwrap().clone() != dummy_url
        },
        Some(6000),
        Some(500),
    )
    .await
    .unwrap();

    let ep_2_url = handler_2.current_url.lock().unwrap().clone();

    // ep_1 tries to send a message to ep_2
    // This will:
    // 1. ep_1 connects to ep_2 and sends its preflight
    // 2. ep_2 receives ep_1's preflight
    // 3. ep_2 tries to return its preflight but fails because has_local_agents=false
    // 4. The connection fails with "Cannot generate preflight before local agent has joined space"
    let message = Bytes::from_static(b"hello");
    let _ = ep_1
        .send_space_notify(ep_2_url.clone(), TEST_SPACE_ID, message)
        .await;

    // Wait for the error handling to complete
    tokio::time::sleep(Duration::from_millis(500)).await;

    // ASSERTION: set_unresponsive should NOT be called for "no local agents" errors
    //
    // This assertion will FAIL without the fix because currently ALL errors
    // from handle_incoming_stream cause set_unresponsive to be called.
    //
    // After the fix (using K2Error::NoLocalAgents and handling it specially),
    // this assertion will PASS.
    assert!(
        !set_unresponsive_called.load(Ordering::SeqCst),
        "BUG: set_unresponsive was called when connection failed due to 'no local agents' error. \
         This is incorrect - 'no local agents' is a temporary state, not a network failure, \
         and should not cause the peer to be marked as unresponsive."
    );
}
