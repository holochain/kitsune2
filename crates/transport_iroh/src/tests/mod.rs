//! iroh transport module tests

use super::*;
use kitsune2_test_utils::{
    iroh_relay::spawn_iroh_relay_server, iter_check, space::TEST_SPACE_ID,
};
use std::{fmt, time::Duration};

mod url;

#[test]
fn validate_bad_relay_url() {
    let builder = Builder {
        transport: IrohTransportFactory::create(),
        ..kitsune2_core::default_test_builder()
    };

    builder
        .config
        .set_module_config(&IrohTransportModConfig {
            iroh_transport: IrohTransportConfig {
                relay_url: Some("<bad-url>".into()),
                ..Default::default()
            },
        })
        .unwrap();

    assert!(builder.validate_config().is_err());
}

#[test]
fn validate_plain_relay_url() {
    let builder = Builder {
        transport: IrohTransportFactory::create(),
        ..kitsune2_core::default_test_builder()
    };

    builder
        .config
        .set_module_config(&IrohTransportModConfig {
            iroh_transport: IrohTransportConfig {
                relay_url: Some("http://test.url".into()),
                ..Default::default()
            },
        })
        .unwrap();

    let result = builder.validate_config();
    assert!(result.is_err());
    assert!(format!("{result:?}").contains("disallowed plaintext relay url"));
}

struct MockTxHandler {
    /// Mock function to implement the new_listening_address() method of the
    /// TxBaseHandler trait
    pub current_url: Arc<Mutex<Url>>,
    /// Mock function to implement the recv_space_notify() method of the
    /// TxSpaceHandler trait
    pub recv_space_notify: Arc<
        dyn Fn(Url, SpaceId, bytes::Bytes) -> K2Result<()>
            + 'static
            + Send
            + Sync,
    >,
}

impl std::fmt::Debug for MockTxHandler {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MockTxHandler").finish()
    }
}

impl TxBaseHandler for MockTxHandler {
    fn new_listening_address(&self, this_url: Url) -> BoxFut<'static, ()> {
        *self.current_url.lock().unwrap() = this_url;
        Box::pin(async {})
    }

    fn peer_connect(&self, _peer: Url) -> K2Result<()> {
        Ok(())
    }

    fn peer_disconnect(&self, _peer: Url, _reason: Option<String>) {}
}

impl TxHandler for MockTxHandler {
    fn preflight_gather_outgoing(
        &self,
        _peer_url: Url,
    ) -> BoxFut<'_, K2Result<bytes::Bytes>> {
        Box::pin(async { Ok(Bytes::new()) })
    }

    fn preflight_validate_incoming(
        &self,
        _peer_url: Url,
        _data: bytes::Bytes,
    ) -> BoxFut<'_, K2Result<()>> {
        Box::pin(async { Ok(()) })
    }
}

impl TxSpaceHandler for MockTxHandler {
    fn recv_space_notify(
        &self,
        peer: Url,
        space_id: SpaceId,
        data: bytes::Bytes,
    ) -> K2Result<()> {
        (self.recv_space_notify)(peer, space_id, data)
    }

    fn set_unresponsive(
        &self,
        _peer: Url,
        _when: Timestamp,
    ) -> BoxFut<'_, K2Result<()>> {
        Box::pin(async { Ok(()) })
    }

    fn are_all_agents_at_url_blocked(&self, _peer_url: &Url) -> K2Result<bool> {
        Ok(false)
    }
}

#[tokio::test]
async fn connect_two_endpoints() {
    let relay_server = spawn_iroh_relay_server().await;
    let relay_url = relay_server.http_addr().unwrap();

    let builder = Builder {
        transport: IrohTransportFactory::create(),
        ..kitsune2_core::default_test_builder()
    }
    .with_default_config()
    .unwrap();
    builder
        .config
        .set_module_config(&IrohTransportModConfig {
            iroh_transport: IrohTransportConfig {
                relay_url: Some(format!("http://{relay_url}")),
                relay_allow_plain_text: true,
            },
        })
        .unwrap();
    let builder = Arc::new(builder);

    const DUMMY_URL: &str = "http://url.not.set:0/0";

    let recv_space_notify = Arc::new(|_peer, _space_id, _data| Ok(()));
    let h1 = Arc::new(MockTxHandler {
        current_url: Arc::new(Mutex::new(Url::from_str(DUMMY_URL).unwrap())),
        recv_space_notify,
    });
    let ep1 = builder
        .transport
        .create(builder.clone(), h1.clone())
        .await
        .unwrap();
    ep1.register_space_handler(TEST_SPACE_ID, h1.clone());

    assert!(
        ep1.get_connected_peers().await.unwrap().is_empty(),
        "peers connected to ep1 should be empty"
    );

    let builder = Builder {
        transport: IrohTransportFactory::create(),
        ..kitsune2_core::default_test_builder()
    }
    .with_default_config()
    .unwrap();
    builder
        .config
        .set_module_config(&IrohTransportModConfig {
            iroh_transport: IrohTransportConfig {
                relay_url: Some(format!("http://{relay_url}")),
                relay_allow_plain_text: true,
            },
        })
        .unwrap();
    let builder = Arc::new(builder);

    let (space_notify_sender, mut space_notify_receiver) =
        tokio::sync::mpsc::unbounded_channel();
    let recv_space_notify = Arc::new(move |_peer, _space_id, data| {
        space_notify_sender.send(data).unwrap();
        Ok(())
    });
    let h2 = Arc::new(MockTxHandler {
        current_url: Arc::new(Mutex::new(Url::from_str(DUMMY_URL).unwrap())),
        recv_space_notify,
    });
    let ep2 = builder
        .transport
        .create(builder.clone(), h2.clone())
        .await
        .unwrap();
    ep2.register_space_handler(TEST_SPACE_ID, h2.clone());

    assert!(
        ep2.get_connected_peers().await.unwrap().is_empty(),
        "peers connected to ep2 should be empty"
    );

    // Wait for URLs to be updated
    iter_check!(10_000, 100, {
        let ep1_url = h1.current_url.lock().unwrap().clone();
        let ep2_url = h2.current_url.lock().unwrap().clone();
        if ep1_url != Url::from_str(DUMMY_URL).unwrap()
            && ep2_url != Url::from_str(DUMMY_URL).unwrap()
        {
            break;
        }
    });

    let message = b"hello";
    let ep2_url = h2.current_url.lock().unwrap().clone();
    ep1.send_space_notify(ep2_url, TEST_SPACE_ID, Bytes::from_static(message))
        .await
        .unwrap();

    tokio::time::timeout(Duration::from_secs(1), async {
        let received_space_notification =
            space_notify_receiver.recv().await.unwrap();
        assert_eq!(*received_space_notification, *message);
    })
    .await
    .expect("message was not received by ep2");

    assert_eq!(ep1.get_connected_peers().await.unwrap().len(), 1);
    assert_eq!(ep2.get_connected_peers().await.unwrap().len(), 1);
}

#[tokio::test]
async fn preflight_only_called_once_per_peer() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    let relay_server = spawn_iroh_relay_server().await;
    let relay_url = relay_server.http_addr().unwrap();

    let builder = Builder {
        transport: IrohTransportFactory::create(),
        ..kitsune2_core::default_test_builder()
    }
    .with_default_config()
    .unwrap();
    builder
        .config
        .set_module_config(&IrohTransportModConfig {
            iroh_transport: IrohTransportConfig {
                relay_url: Some(format!("http://{relay_url}")),
                relay_allow_plain_text: true,
            },
        })
        .unwrap();
    let builder = Arc::new(builder);

    const DUMMY_URL: &str = "http://url.not.set:0/0";

    let preflight_count = Arc::new(AtomicUsize::new(0));

    let recv_space_notify = Arc::new(|_peer, _space_id, _data| Ok(()));
    let h1 = Arc::new(MockTxHandler {
        current_url: Arc::new(Mutex::new(Url::from_str(DUMMY_URL).unwrap())),
        recv_space_notify,
    });

    struct CountingTxHandler {
        inner: Arc<MockTxHandler>,
        preflight_count: Arc<AtomicUsize>,
    }

    impl std::fmt::Debug for CountingTxHandler {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("CountingTxHandler").finish()
        }
    }

    impl TxBaseHandler for CountingTxHandler {
        fn new_listening_address(&self, this_url: Url) -> BoxFut<'static, ()> {
            self.inner.new_listening_address(this_url)
        }

        fn peer_connect(&self, peer: Url) -> K2Result<()> {
            self.inner.peer_connect(peer)
        }

        fn peer_disconnect(&self, peer: Url, reason: Option<String>) {
            self.inner.peer_disconnect(peer, reason)
        }
    }

    impl TxHandler for CountingTxHandler {
        fn preflight_gather_outgoing(
            &self,
            peer_url: Url,
        ) -> BoxFut<'_, K2Result<bytes::Bytes>> {
            self.preflight_count.fetch_add(1, Ordering::SeqCst);
            self.inner.preflight_gather_outgoing(peer_url)
        }

        fn preflight_validate_incoming(
            &self,
            peer_url: Url,
            data: bytes::Bytes,
        ) -> BoxFut<'_, K2Result<()>> {
            self.inner.preflight_validate_incoming(peer_url, data)
        }
    }

    impl TxSpaceHandler for CountingTxHandler {
        fn recv_space_notify(
            &self,
            peer: Url,
            space_id: SpaceId,
            data: bytes::Bytes,
        ) -> K2Result<()> {
            self.inner.recv_space_notify(peer, space_id, data)
        }

        fn set_unresponsive(
            &self,
            peer: Url,
            when: Timestamp,
        ) -> BoxFut<'_, K2Result<()>> {
            self.inner.set_unresponsive(peer, when)
        }

        fn are_all_agents_at_url_blocked(
            &self,
            peer_url: &Url,
        ) -> K2Result<bool> {
            self.inner.are_all_agents_at_url_blocked(peer_url)
        }
    }

    let counting_handler = Arc::new(CountingTxHandler {
        inner: h1.clone(),
        preflight_count: preflight_count.clone(),
    });

    let ep1 = builder
        .transport
        .create(builder.clone(), counting_handler.clone())
        .await
        .unwrap();
    ep1.register_space_handler(TEST_SPACE_ID, counting_handler.clone());

    let builder = Builder {
        transport: IrohTransportFactory::create(),
        ..kitsune2_core::default_test_builder()
    }
    .with_default_config()
    .unwrap();
    builder
        .config
        .set_module_config(&IrohTransportModConfig {
            iroh_transport: IrohTransportConfig {
                relay_url: Some(format!("http://{relay_url}")),
                relay_allow_plain_text: true,
            },
        })
        .unwrap();
    let builder = Arc::new(builder);

    let (space_notify_sender, mut space_notify_receiver) =
        tokio::sync::mpsc::unbounded_channel();
    let recv_space_notify = Arc::new(move |_peer, _space_id, data| {
        space_notify_sender.send(data).unwrap();
        Ok(())
    });
    let h2 = Arc::new(MockTxHandler {
        current_url: Arc::new(Mutex::new(Url::from_str(DUMMY_URL).unwrap())),
        recv_space_notify,
    });
    let ep2 = builder
        .transport
        .create(builder.clone(), h2.clone())
        .await
        .unwrap();
    ep2.register_space_handler(TEST_SPACE_ID, h2.clone());

    // Wait for URLs to be updated
    iter_check!(10_000, 100, {
        let ep1_url =
            counting_handler.inner.current_url.lock().unwrap().clone();
        let ep2_url = h2.current_url.lock().unwrap().clone();
        if ep1_url != Url::from_str(DUMMY_URL).unwrap()
            && ep2_url != Url::from_str(DUMMY_URL).unwrap()
        {
            break;
        }
    });

    let ep2_url = h2.current_url.lock().unwrap().clone();

    // Spawn two concurrent sends
    let message1 = Bytes::from_static(b"msg1");
    let send1 = tokio::spawn({
        let ep1 = ep1.clone();
        let ep2_url = ep2_url.clone();
        let message1 = message1.clone();
        async move {
            ep1.send_space_notify(ep2_url, TEST_SPACE_ID, message1)
                .await
        }
    });

    let message2 = Bytes::from_static(b"msg2");
    let send2 = tokio::spawn({
        let ep1 = ep1.clone();
        let ep2_url = ep2_url.clone();
        let message2 = message2.clone();
        async move {
            ep1.send_space_notify(ep2_url, TEST_SPACE_ID, message2)
                .await
        }
    });

    let (result1, result2) = tokio::join!(send1, send2);
    result1.unwrap().unwrap();
    result2.unwrap().unwrap();

    let response1 = space_notify_receiver.recv().await.unwrap();
    let response2 = space_notify_receiver.recv().await.unwrap();
    let mut expected = vec![message1, message2];
    expected.sort();
    let mut responses = vec![response1, response2];
    responses.sort();
    assert_eq!(responses, expected);

    // Preflight should have been called exactly once.
    assert_eq!(preflight_count.load(Ordering::SeqCst), 1);
}
