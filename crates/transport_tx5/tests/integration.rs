use std::sync::{Arc, Mutex};

use kitsune2_api::{DynInnerError, K2Error, Url};
use kitsune2_test_utils::space::TEST_SPACE_ID;
use kitsune2_transport_tx5::harness::{MockTxHandler, Tx5TransportTestHarness};

/// Tests that a peer that goes offline gets marked unresponsive by the
/// transport_tx5 module when a message is unsuccessfully attempted to be sent
/// to it.
///
/// This test has been moved to integration tests because it's slow (~10 seconds).
#[tokio::test(flavor = "multi_thread")]
async fn offline_peer_marked_unresponsive() {
    // set the tx5 timeout to 5 seconds to keep the test reasonably short
    let test = Tx5TransportTestHarness::new(None, Some(5)).await;

    let (unresp_send, mut unresp_recv) = tokio::sync::mpsc::unbounded_channel();

    let url1 =
        Arc::new(Mutex::new(Url::from_str("ws://bla.bla:38/1").unwrap()));
    let url1_2 = url1.clone();
    let tx_handler1 = Arc::new(MockTxHandler {
        new_addr: Arc::new(move |url| {
            *url1_2.lock().unwrap() = url;
        }),
        set_unresp: Arc::new({
            move |url, when| {
                unresp_send.send((url, when)).map_err(|_| K2Error::Other {
                    ctx: "Failed to send url to oneshot channel".into(),
                    src: DynInnerError::default(),
                })
            }
        }),
        ..Default::default()
    });
    let transport1 = test.build_transport(tx_handler1.clone()).await;
    transport1.register_space_handler(TEST_SPACE_ID, tx_handler1.clone());
    transport1.register_module_handler(
        TEST_SPACE_ID,
        "mod".into(),
        tx_handler1.clone(),
    );

    let (s_send, mut s_recv) = tokio::sync::mpsc::unbounded_channel();
    let url2 =
        Arc::new(Mutex::new(Url::from_str("ws://bla.bla:38/1").unwrap()));
    let url2_2 = url2.clone();
    let tx_hanlder2 = Arc::new(MockTxHandler {
        new_addr: Arc::new(move |url| {
            *url2_2.lock().unwrap() = url;
        }),
        recv_space_not: Arc::new(move |url, space_id, data| {
            let _ = s_send.send((url, space_id, data));
            Ok(())
        }),
        ..Default::default()
    });
    let transport2 = test.build_transport(tx_hanlder2.clone()).await;
    transport2.register_space_handler(TEST_SPACE_ID, tx_hanlder2.clone());
    transport2.register_module_handler(
        TEST_SPACE_ID,
        "mod".into(),
        tx_hanlder2.clone(),
    );

    let url2: Url = url2.lock().unwrap().clone();
    println!("got url2: {}", url2);

    // check that send works initially while peer 2 is still online
    transport1
        .send_space_notify(
            url2.clone(),
            TEST_SPACE_ID,
            bytes::Bytes::from_static(b"hello"),
        )
        .await
        .unwrap();

    // checks that recv works
    let (_, _, bytes) =
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            s_recv.recv().await
        })
        .await
        .unwrap()
        .unwrap();

    assert_eq!(b"hello", bytes.as_ref());

    // Check that the peer has not been marked as unresponsive
    let r = unresp_recv.try_recv();
    assert!(r.is_err());

    // Now peer 2 goes offline and we check that it gets marked as unresponsive
    drop(transport2);

    // We need to wait for a while in order for the webrtc connection to get
    // disconnected. If this test becomes flaky, this waiting period may need
    // to be increased a bit.
    tokio::time::sleep(std::time::Duration::from_secs(9)).await;

    let res = transport1
        .send_space_notify(
            url2.clone(),
            TEST_SPACE_ID,
            bytes::Bytes::from_static(b"anyone here?"),
        )
        .await;

    // We expect it to have timed out because peer 2 is offline now
    assert!(res.is_err());

    let r = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        unresp_recv.recv().await
    })
    .await
    .unwrap();

    let url = r.unwrap().0;

    assert!(url2 == url);
}
