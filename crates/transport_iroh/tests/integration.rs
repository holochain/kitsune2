use crate::harness::{IrohTransportTestHarness, MockTxHandler};
use bytes::Bytes;
use kitsune2_api::Url;
use kitsune2_test_utils::{iter_check, space::TEST_SPACE_ID};
use std::{sync::Arc, time::Duration};

mod harness;

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
    iter_check!(10_000, {
        let ep_1_url = handler_1.current_url.lock().unwrap().clone();
        let ep_2_url = handler_2.current_url.lock().unwrap().clone();
        if ep_1_url != dummy_url && ep_2_url != dummy_url {
            break;
        }
    });

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
    iter_check!(10_000, {
        let ep_1_url = handler_1.current_url.lock().unwrap().clone();
        let ep_2_url = handler_2.current_url.lock().unwrap().clone();
        if ep_1_url != dummy_url && ep_2_url != dummy_url {
            break;
        }
    });

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
