//! tx5 transport module tests

use crate::harness::{MockTxHandler, Tx5TransportTestHarness};

use super::*;
use kitsune2_test_utils::space::TEST_SPACE_ID;
use std::collections::HashSet;
use std::sync::Mutex;

// We don't need or want to test all of tx5 in here... that should be done
// in the tx5 repo. Here we should only test the kitsune2 integration of tx5.
//
// Specifically:
//
// - That new_listening_address is called if the sbd server is restarted
// - That peer connect / disconnect are invoked appropriately.
// - That messages can be sent / received.
// - That preflight generation and checking work, which are a little weird
//   because in kitsune2 the check logic is handled in the same recv_data
//   callback, where tx5 handles it as a special callback.

#[test]
fn validate_bad_server_url() {
    let builder = Builder {
        transport: Tx5TransportFactory::create(),
        ..kitsune2_core::default_test_builder()
    };

    builder
        .config
        .set_module_config(&Tx5TransportModConfig {
            tx5_transport: Tx5TransportConfig {
                signal_allow_plain_text: true,
                server_url: "<bad-url>".into(),
                ..Default::default()
            },
        })
        .unwrap();

    assert!(builder.validate_config().is_err());
}

#[test]
fn validate_plain_server_url() {
    let builder = Builder {
        transport: Tx5TransportFactory::create(),
        ..kitsune2_core::default_test_builder()
    };

    builder
        .config
        .set_module_config(&Tx5TransportModConfig {
            tx5_transport: Tx5TransportConfig {
                signal_allow_plain_text: false,
                server_url: "ws://test.url".into(),
                ..Default::default()
            },
        })
        .unwrap();

    let result = format!("{:?}", builder.validate_config());
    assert!(result.contains("disallowed plaintext signal url"));
}

#[tokio::test(flavor = "multi_thread")]
async fn restart_addr() {
    let mut test = Tx5TransportTestHarness::new(None, None).await;

    let addr = Arc::new(Mutex::new(Vec::new()));
    let addr2 = addr.clone();

    let h = Arc::new(MockTxHandler {
        new_addr: Arc::new(move |url| {
            addr2.lock().unwrap().push(url);
        }),
        ..Default::default()
    });
    let _t = test.build_transport(h).await;

    let init_len = addr.lock().unwrap().len();
    assert!(init_len > 0);

    test.restart().await;

    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            if addr.lock().unwrap().len() > init_len {
                // End the test, we're happy!
                return;
            }

            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "disconnects currently broken in tx5"]
async fn peer_connect_disconnect() {
    let test = Tx5TransportTestHarness::new(None, None).await;

    let u1 = Arc::new(Mutex::new(Url::from_str("ws://bla.bla:38/1").unwrap()));
    let u1_2 = u1.clone();
    let h1 = Arc::new(MockTxHandler {
        new_addr: Arc::new(move |url| {
            *u1_2.lock().unwrap() = url;
        }),
        ..Default::default()
    });
    let t1 = test.build_transport(h1.clone()).await;

    let (s_send, mut s_recv) = tokio::sync::mpsc::unbounded_channel();
    let u2 = Arc::new(Mutex::new(Url::from_str("ws://bla.bla:38/1").unwrap()));
    let u2_2 = u2.clone();
    let s_send_2 = s_send.clone();
    let h2 = Arc::new(MockTxHandler {
        new_addr: Arc::new(move |url| {
            *u2_2.lock().unwrap() = url;
        }),
        peer_con: Arc::new(move |_| {
            let _ = s_send.send("con");
            Ok(())
        }),
        peer_dis: Arc::new(move |_, _| {
            let _ = s_send_2.send("dis");
        }),
        ..Default::default()
    });
    let t2 = test.build_transport(h2.clone()).await;

    let u1: Url = u1.lock().unwrap().clone();
    println!("got u1: {}", u1);
    let u2: Url = u2.lock().unwrap().clone();
    println!("got u2: {}", u2);

    // trigger a connection establish
    t1.send_space_notify(
        u2,
        TEST_SPACE_ID,
        bytes::Bytes::from_static(b"hello"),
    )
    .await
    .unwrap();

    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        let con = s_recv.recv().await.unwrap();
        assert_eq!("con", con);
    })
    .await
    .unwrap();

    std::mem::drop(t1);

    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            // trigger a connection establish
            t2.send_space_notify(
                u1.clone(),
                TEST_SPACE_ID,
                bytes::Bytes::from_static(b"world"),
            )
            .await
            .unwrap();

            if let Ok(dis) = s_recv.try_recv() {
                assert_eq!("dis", dis);
                // test pass
                return;
            }

            // haven't received yet, wait a bit and try again
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn message_send_recv() {
    let test = Tx5TransportTestHarness::new(None, None).await;

    let h1 = Arc::new(MockTxHandler {
        ..Default::default()
    });
    let t1 = test.build_transport(h1.clone()).await;
    t1.register_space_handler(TEST_SPACE_ID, h1.clone());
    t1.register_module_handler(TEST_SPACE_ID, "mod".into(), h1.clone());

    let (s_send, mut s_recv) = tokio::sync::mpsc::unbounded_channel();
    let u2 = Arc::new(Mutex::new(Url::from_str("ws://bla.bla:38/1").unwrap()));
    let u2_2 = u2.clone();
    let h2 = Arc::new(MockTxHandler {
        new_addr: Arc::new(move |url| {
            *u2_2.lock().unwrap() = url;
        }),
        recv_space_not: Arc::new(move |url, space, data| {
            let _ = s_send.send((url, space, data));
            Ok(())
        }),
        ..Default::default()
    });
    let t2 = test.build_transport(h2.clone()).await;
    t2.register_space_handler(TEST_SPACE_ID, h2.clone());
    t2.register_module_handler(TEST_SPACE_ID, "mod".into(), h2.clone());

    let u2: Url = u2.lock().unwrap().clone();
    println!("got u2: {}", u2);

    // checks that send works
    t1.send_space_notify(
        u2,
        TEST_SPACE_ID,
        bytes::Bytes::from_static(b"hello"),
    )
    .await
    .unwrap();

    // checks that recv works
    let r = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        s_recv.recv().await
    })
    .await
    .unwrap()
    .unwrap();

    println!("{r:?}");
    assert_eq!(b"hello", r.2.as_ref());
}

#[tokio::test(flavor = "multi_thread")]
async fn message_send_recv_auth() {
    let test = Tx5TransportTestHarness::new(Some(b"hello".into()), None).await;

    let h1 = Arc::new(MockTxHandler {
        ..Default::default()
    });
    let t1 = test.build_transport(h1.clone()).await;
    t1.register_space_handler(TEST_SPACE_ID, h1.clone());
    t1.register_module_handler(TEST_SPACE_ID, "mod".into(), h1.clone());

    let (s_send, mut s_recv) = tokio::sync::mpsc::unbounded_channel();
    let u2 = Arc::new(Mutex::new(Url::from_str("ws://bla.bla:38/1").unwrap()));
    let u2_2 = u2.clone();
    let h2 = Arc::new(MockTxHandler {
        new_addr: Arc::new(move |url| {
            *u2_2.lock().unwrap() = url;
        }),
        recv_space_not: Arc::new(move |url, space, data| {
            let _ = s_send.send((url, space, data));
            Ok(())
        }),
        ..Default::default()
    });
    let t2 = test.build_transport(h2.clone()).await;
    t2.register_space_handler(TEST_SPACE_ID, h2.clone());
    t2.register_module_handler(TEST_SPACE_ID, "mod".into(), h2.clone());

    let u2: Url = u2.lock().unwrap().clone();
    println!("got u2: {}", u2);

    // checks that send works
    t1.send_space_notify(
        u2,
        TEST_SPACE_ID,
        bytes::Bytes::from_static(b"hello"),
    )
    .await
    .unwrap();

    // checks that recv works
    let r = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        s_recv.recv().await
    })
    .await
    .unwrap()
    .unwrap();

    println!("{r:?}");
    assert_eq!(b"hello", r.2.as_ref());
}

#[tokio::test(flavor = "multi_thread")]
async fn preflight_send_recv() {
    use std::sync::atomic::*;
    let test = Tx5TransportTestHarness::new(None, None).await;

    let r1 = Arc::new(AtomicBool::new(false));
    let r1_2 = r1.clone();

    let h1 = Arc::new(MockTxHandler {
        pre_out: Arc::new(|_| Ok(bytes::Bytes::from_static(b"hello"))),
        pre_in: Arc::new(move |_, data| {
            assert_eq!(b"world", data.as_ref());
            r1_2.store(true, Ordering::SeqCst);
            Ok(())
        }),
        ..Default::default()
    });
    let t1 = test.build_transport(h1.clone()).await;

    let r2 = Arc::new(AtomicBool::new(false));
    let r2_2 = r2.clone();

    let u2 = Arc::new(Mutex::new(Url::from_str("ws://bla.bla:38/1").unwrap()));
    let u2_2 = u2.clone();
    let h2 = Arc::new(MockTxHandler {
        pre_out: Arc::new(|_| Ok(bytes::Bytes::from_static(b"world"))),
        pre_in: Arc::new(move |_, data| {
            assert_eq!(b"hello", data.as_ref());
            r2_2.store(true, Ordering::SeqCst);
            Ok(())
        }),
        new_addr: Arc::new(move |url| {
            *u2_2.lock().unwrap() = url;
        }),
        ..Default::default()
    });
    let _t2 = test.build_transport(h2.clone()).await;

    let u2: Url = u2.lock().unwrap().clone();
    println!("got u2: {}", u2);

    // establish a connection
    t1.send_space_notify(
        u2,
        TEST_SPACE_ID,
        bytes::Bytes::from_static(b"hello"),
    )
    .await
    .unwrap();

    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if r1.load(Ordering::SeqCst) && r2.load(Ordering::SeqCst) {
                // test pass
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn nonexistent_peer_marked_unresponsive() {
    // set the tx5 timeout to 5 seconds to keep the test reasonably short
    let test = Tx5TransportTestHarness::new(None, Some(3)).await;

    let (unresp_send, mut unresp_recv) = tokio::sync::mpsc::unbounded_channel();

    let tx_handler1 = Arc::new(MockTxHandler {
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

    let faulty_url = Url::from_str(
        "ws://127.0.0.1:40813/VtK2IOCncQM6LbWkvhB_CYwajQzw6Dii-Oc-0IRtHmc",
    )
    .unwrap();

    let res = transport1
        .send_space_notify(
            faulty_url.clone(),
            TEST_SPACE_ID,
            bytes::Bytes::from_static(b"anyone here?"),
        )
        .await;

    // We expect it to have timed out because no peer should be reachable at above URL
    assert!(res.is_err());

    // We expect the set_unresponsive() method on the TxSpaceHandler trait to have
    // been invoked
    let maybe_url_and_when =
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            unresp_recv.recv().await
        })
        .await
        .unwrap();

    let url = maybe_url_and_when.unwrap().0;

    assert!(faulty_url == url);
}

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
        recv_space_not: Arc::new(move |url, space, data| {
            let _ = s_send.send((url, space, data));
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

#[tokio::test(flavor = "multi_thread")]
async fn dump_network_stats() {
    let test = Tx5TransportTestHarness::new(None, None).await;

    let h1 = Arc::new(MockTxHandler {
        ..Default::default()
    });
    let t1 = test.build_transport(h1.clone()).await;

    let u2 = Arc::new(Mutex::new(Url::from_str("ws://bla.bla:38/1").unwrap()));
    let h2 = Arc::new(MockTxHandler {
        new_addr: Arc::new({
            let u2 = u2.clone();
            move |url| {
                *u2.lock().unwrap() = url;
            }
        }),
        ..Default::default()
    });
    let t2 = test.build_transport(h2.clone()).await;

    // establish a connection
    let url2 = u2.lock().unwrap().clone();
    t1.send_space_notify(
        url2,
        TEST_SPACE_ID,
        bytes::Bytes::from_static(b"hello"),
    )
    .await
    .unwrap();

    // Check that we can get network stats
    let stats_1 = t1.dump_network_stats().await.unwrap();
    let stats_2 = t2.dump_network_stats().await.unwrap();

    assert_eq!(stats_1.backend, "BackendLibDataChannel");

    let peer_url_1 = stats_1.peer_urls.first().unwrap();
    let peer_id_1 = peer_url_1.peer_id().unwrap();

    let peer_url_2 = stats_2.peer_urls.first().unwrap();
    let peer_id_2 = peer_url_2.peer_id().unwrap();

    let connection_list_1 = stats_1
        .connections
        .iter()
        .map(|c| c.pub_key.clone())
        .collect::<HashSet<_>>();
    let connection_list_2 = stats_2
        .connections
        .iter()
        .map(|c| c.pub_key.clone())
        .collect::<HashSet<_>>();

    assert!(connection_list_1.contains(peer_id_2));
    assert!(connection_list_2.contains(peer_id_1));
}
