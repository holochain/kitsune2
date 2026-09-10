//! Unit tests for connection establishment failures.
//!
//! These tests use a fake [`Endpoint`] implementation to deterministically
//! exercise error paths in [`IrohTransport::create_connection_and_context`]
//! that are difficult to trigger reproducibly via the real iroh stack.

use crate::connection::DynConnection;
use crate::endpoint::{DynIrohEndpoint, Endpoint, EndpointAddrWatcher};
use crate::test_utils::MockTxHandler;
use crate::url::endpoint_from_url;
use crate::{IrohTransport, IrohTransportConfig};
use bytes::Bytes;
use iroh::{EndpointAddr, RelayConfig, RelayUrl};
use kitsune2_api::{
    BoxFut, DefaultTransport, K2Error, K2Result, TransportStats, TxImp,
    TxImpHnd, Url,
};
use kitsune2_test_utils::space::TEST_SPACE_ID;
use n0_watcher::Disconnected;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

/// A fake `Endpoint` whose `connect()` returns a configurable error.
///
/// `watch_addr`, `accept` and `close` are stubs that should not be exercised
/// by the tests in this module.
struct FakeEndpoint {
    connect_error_factory: Arc<dyn Fn() -> K2Error + 'static + Send + Sync>,
}

impl std::fmt::Debug for FakeEndpoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FakeEndpoint").finish()
    }
}

impl Endpoint for FakeEndpoint {
    fn watch_addr(&self) -> Box<dyn EndpointAddrWatcher> {
        Box::new(PendingWatcher)
    }

    fn accept(&self) -> BoxFut<'_, Option<K2Result<DynConnection>>> {
        Box::pin(std::future::pending())
    }

    fn connect(
        &self,
        _endpoint_addr: EndpointAddr,
        _alpn: &[u8],
    ) -> BoxFut<'_, K2Result<DynConnection>> {
        let factory = self.connect_error_factory.clone();
        Box::pin(async move { Err(factory()) })
    }

    fn close(&self) -> BoxFut<'_, ()> {
        Box::pin(async {})
    }

    fn insert_relay(
        &self,
        _url: RelayUrl,
        _config: Arc<RelayConfig>,
    ) -> BoxFut<'_, ()> {
        Box::pin(async {})
    }

    fn remove_relay(
        &self,
        _url: &RelayUrl,
    ) -> BoxFut<'_, Option<Arc<RelayConfig>>> {
        Box::pin(async { None })
    }

    fn id_bytes(&self) -> [u8; 32] {
        [0u8; 32]
    }

    fn is_home_relay_known_down(&self) -> bool {
        false
    }
}

/// An `EndpointAddrWatcher` whose `updated()` future never resolves.
struct PendingWatcher;

impl EndpointAddrWatcher for PendingWatcher {
    fn updated(&mut self) -> BoxFut<'_, Result<EndpointAddr, Disconnected>> {
        Box::pin(std::future::pending())
    }
}

/// Minimal `TxImp` stub used only to construct a `DefaultTransport` so that
/// we can register a space handler against the shared `TxImpHnd`. None of
/// these methods are exercised by the tests in this module.
#[derive(Debug)]
struct StubTxImp;

impl TxImp for StubTxImp {
    fn url(&self) -> Option<Url> {
        None
    }

    fn disconnect(
        &self,
        _peer: Url,
        _payload: Option<(String, Bytes)>,
    ) -> BoxFut<'_, ()> {
        Box::pin(async {})
    }

    fn send(&self, _peer: Url, _data: Bytes) -> BoxFut<'_, K2Result<()>> {
        Box::pin(async { unreachable!("StubTxImp::send should not be called") })
    }

    fn get_connected_peers(&self) -> BoxFut<'_, K2Result<Vec<Url>>> {
        Box::pin(async { Ok(Vec::new()) })
    }

    fn dump_network_stats(&self) -> BoxFut<'_, K2Result<TransportStats>> {
        Box::pin(async {
            Ok(TransportStats {
                backend: String::new(),
                peer_urls: Vec::new(),
                connections: Vec::new(),
            })
        })
    }
}

/// Test fixture: builds a `TxImpHnd` wrapped in a `DefaultTransport` with a
/// space handler registered, returning everything the test needs to assert
/// against `set_unresponsive` calls.
fn build_handler_with_space(
    set_unresponsive_calls: Arc<Mutex<Vec<(Url, kitsune2_api::Timestamp)>>>,
) -> Arc<TxImpHnd> {
    let calls = set_unresponsive_calls.clone();
    let mock = Arc::new(MockTxHandler {
        set_unresponsive: Arc::new(move |peer, ts| {
            calls.lock().unwrap().push((peer, ts));
            Ok(())
        }),
        ..Default::default()
    });
    let handler = TxImpHnd::new(mock.clone());
    let transport = DefaultTransport::create(&handler, Arc::new(StubTxImp));
    transport.register_space_handler(TEST_SPACE_ID, mock);
    handler
}

fn fake_remote_url() -> Url {
    // 64 hex characters → valid iroh EndpointId encoding.
    Url::from_str(
        "https://relay.example.com:443/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    )
    .unwrap()
}

/// Build an `IrohTransport` directly from its component pieces, bypassing the
/// async `create()` constructor so unit tests can inject a fake `Endpoint`
/// and observe `connections` / `local_url` after the test runs. The
/// background tasks held by the struct are stubbed out with no-op spawns.
fn build_transport(
    endpoint: DynIrohEndpoint,
    handler: Arc<TxImpHnd>,
    connections: crate::Connections,
    local_url: Arc<RwLock<Option<Url>>>,
    config: IrohTransportConfig,
) -> IrohTransport {
    let noop_handle = || tokio::spawn(async {}).abort_handle();
    IrohTransport {
        endpoint,
        handler,
        local_url,
        connections,
        connection_locks: Arc::new(Mutex::new(HashMap::new())),
        watch_addr_task: noop_handle(),
        accept_task: noop_handle(),
        relay_keepalive_task: None,
        space_relay_keepalives: Arc::new(Mutex::new(HashMap::new())),
        config,
        space_relays: Arc::new(RwLock::new(HashMap::new())),
    }
}

fn config() -> IrohTransportConfig {
    IrohTransportConfig {
        // Make the outer wrapper effectively unreachable so the test
        // observes the *inner* error path, not the outer tokio timeout.
        connect_timeout_s: 60,
        ..Default::default()
    }
}

/// When `iroh::Endpoint::connect` returns an error (the production case-B
/// path: quinn `ConnectionError::TimedOut` after the relay has nothing to
/// say), `create_connection_and_context` must mark the peer unresponsive
/// and surface a clear error.
#[tokio::test]
async fn marks_unresponsive_when_iroh_connect_returns_error() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let handler = build_handler_with_space(calls.clone());

    let fake_endpoint: DynIrohEndpoint = Arc::new(FakeEndpoint {
        connect_error_factory: Arc::new(|| K2Error::other("timed out")),
    });

    let remote_url = fake_remote_url();
    let target = endpoint_from_url(&remote_url).unwrap();

    let connections = crate::Connections::new();
    let local_url = Arc::new(RwLock::new(Some(remote_url.clone())));

    let transport = build_transport(
        fake_endpoint,
        handler,
        connections.clone(),
        local_url,
        config(),
    );

    let result = transport
        .create_connection_and_context(target, remote_url.clone())
        .await;

    let err_str = result.expect_err("connect should fail").to_string();
    assert!(
        err_str.contains("iroh connect error"),
        "expected wrapped 'iroh connect error', got: {err_str}"
    );
    assert!(
        err_str.contains("timed out"),
        "expected inner 'timed out' source to be preserved, got: {err_str}"
    );

    let recorded = calls.lock().unwrap();
    assert_eq!(
        recorded.len(),
        1,
        "set_unresponsive should be called exactly once"
    );
    assert_eq!(recorded[0].0, remote_url);

    // The connections map must not have been mutated for a failed connect.
    assert!(connections.get(&remote_url).is_none());
}

/// When the *outer* `tokio::time::timeout` wrapper fires (i.e. iroh's connect
/// hangs longer than `connect_timeout_s`), the same set_unresponsive
/// guarantee must hold.
#[tokio::test]
async fn marks_unresponsive_when_outer_connect_timeout_fires() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let handler = build_handler_with_space(calls.clone());

    // Hang forever so the outer timeout is what triggers.
    #[derive(Debug)]
    struct HangingEndpoint;
    impl Endpoint for HangingEndpoint {
        fn watch_addr(&self) -> Box<dyn EndpointAddrWatcher> {
            Box::new(PendingWatcher)
        }
        fn accept(&self) -> BoxFut<'_, Option<K2Result<DynConnection>>> {
            Box::pin(std::future::pending())
        }
        fn connect(
            &self,
            _endpoint_addr: EndpointAddr,
            _alpn: &[u8],
        ) -> BoxFut<'_, K2Result<DynConnection>> {
            Box::pin(std::future::pending())
        }
        fn close(&self) -> BoxFut<'_, ()> {
            Box::pin(async {})
        }
        fn insert_relay(
            &self,
            _url: RelayUrl,
            _config: Arc<RelayConfig>,
        ) -> BoxFut<'_, ()> {
            Box::pin(async {})
        }
        fn remove_relay(
            &self,
            _url: &RelayUrl,
        ) -> BoxFut<'_, Option<Arc<RelayConfig>>> {
            Box::pin(async { None })
        }
        fn id_bytes(&self) -> [u8; 32] {
            [0u8; 32]
        }

        fn is_home_relay_known_down(&self) -> bool {
            false
        }
    }

    let endpoint: DynIrohEndpoint = Arc::new(HangingEndpoint);

    let remote_url = fake_remote_url();
    let target = endpoint_from_url(&remote_url).unwrap();

    let connections = crate::Connections::new();
    let local_url = Arc::new(RwLock::new(Some(remote_url.clone())));

    let cfg = IrohTransportConfig {
        // Use the smallest unit (1 second) so the test runs quickly while
        // still going through the real `tokio::time::timeout` codepath.
        connect_timeout_s: 1,
        ..Default::default()
    };

    let transport =
        build_transport(endpoint, handler, connections.clone(), local_url, cfg);

    let start = std::time::Instant::now();
    let result = transport
        .create_connection_and_context(target, remote_url.clone())
        .await;
    let elapsed = start.elapsed();

    let err_str = result.expect_err("connect should time out").to_string();
    assert!(
        err_str.contains("iroh connect timed out"),
        "expected 'iroh connect timed out', got: {err_str}"
    );

    // Sanity check: we did wait for the timeout, but not much longer.
    assert!(
        elapsed >= Duration::from_secs(1),
        "should have waited at least connect_timeout_s, was {elapsed:?}"
    );
    assert!(
        elapsed < Duration::from_secs(5),
        "outer timeout should fire promptly, was {elapsed:?}"
    );

    let recorded = calls.lock().unwrap();
    assert_eq!(
        recorded.len(),
        1,
        "set_unresponsive should be called exactly once"
    );
    assert_eq!(recorded[0].0, remote_url);
}

/// An `Endpoint` whose `connect()` succeeds with a preconfigured connection.
///
/// `watch_addr`, `accept` and `close` are stubs that should not be exercised
/// by the tests in this module.
struct ConnectingEndpoint {
    connection: DynConnection,
}

impl std::fmt::Debug for ConnectingEndpoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConnectingEndpoint").finish()
    }
}

impl Endpoint for ConnectingEndpoint {
    fn watch_addr(&self) -> Box<dyn EndpointAddrWatcher> {
        Box::new(PendingWatcher)
    }

    fn accept(&self) -> BoxFut<'_, Option<K2Result<DynConnection>>> {
        Box::pin(std::future::pending())
    }

    fn connect(
        &self,
        _endpoint_addr: EndpointAddr,
        _alpn: &[u8],
    ) -> BoxFut<'_, K2Result<DynConnection>> {
        let connection = self.connection.clone();
        Box::pin(async move { Ok(connection) })
    }

    fn close(&self) -> BoxFut<'_, ()> {
        Box::pin(async {})
    }

    fn insert_relay(
        &self,
        _url: RelayUrl,
        _config: Arc<RelayConfig>,
    ) -> BoxFut<'_, ()> {
        Box::pin(async {})
    }

    fn remove_relay(
        &self,
        _url: &RelayUrl,
    ) -> BoxFut<'_, Option<Arc<RelayConfig>>> {
        Box::pin(async { None })
    }

    fn id_bytes(&self) -> [u8; 32] {
        [0u8; 32]
    }

    fn is_home_relay_known_down(&self) -> bool {
        false
    }
}

/// A genuine preflight-write failure (the connection's `open_uni` fails, not
/// a simultaneous-open supersede) must close the connection with the real
/// failure reason, not `CloseCode::Superseded`. A remote observing
/// `Superseded` treats it as a signal to retry against the connection that
/// "won", which is wrong here: this connection just failed on its own.
#[tokio::test]
async fn genuine_preflight_write_failure_closes_with_real_reason_not_superseded()
 {
    use super::support::FakeConnection;
    use crate::close_code::CloseCode;

    let calls = Arc::new(Mutex::new(Vec::new()));
    let handler = build_handler_with_space(calls.clone());

    let remote_url = fake_remote_url();
    let target = endpoint_from_url(&remote_url).unwrap();

    let close_calls = Arc::new(Mutex::new(Vec::new()));
    let connection: DynConnection = Arc::new(FakeConnection {
        accept_gate: Arc::new(tokio::sync::Notify::new()),
        remote_close: None,
        remote_id: target.id,
        close_calls: close_calls.clone(),
    });

    let fake_endpoint: DynIrohEndpoint =
        Arc::new(ConnectingEndpoint { connection });

    let connections = crate::Connections::new();
    let local_url = Arc::new(RwLock::new(Some(remote_url.clone())));

    let transport = build_transport(
        fake_endpoint,
        handler,
        connections.clone(),
        local_url,
        config(),
    );

    let result = transport
        .create_connection_and_context(target, remote_url.clone())
        .await;

    assert!(
        result.is_err(),
        "a failed preflight write must surface as an error"
    );

    let recorded_closes = close_calls.lock().unwrap();
    assert_eq!(
        recorded_closes.len(),
        1,
        "the connection must be closed exactly once, got {recorded_closes:?}"
    );
    assert_eq!(
        recorded_closes[0].0,
        CloseCode::Unspecified,
        "a genuine preflight failure must close with the real reason, not \
         Superseded, or the remote will treat it as a signal to retry"
    );
}

mod preflight_timeout {
    use super::*;
    use crate::close_code::CloseCode;
    use crate::connection::MockConnection;
    use crate::stream::mock::{MockRecvStream, MockSendStream};
    use crate::stream::{DynIrohRecvStream, DynIrohSendStream, RecvStream};
    use crate::tests::support::{
        CloseCalls, Recorder, build_recording_handler,
    };
    use std::sync::atomic::Ordering;

    /// Supplies a prefix and then stalls without closing the QUIC stream.
    struct StalledStream(tokio::sync::Mutex<Bytes>);

    impl RecvStream for StalledStream {
        fn read_exact<'a>(
            &'a self,
            buf: &'a mut [u8],
        ) -> BoxFut<'a, K2Result<()>> {
            Box::pin(async move {
                let mut bytes = self.0.lock().await;
                if bytes.len() >= buf.len() {
                    buf.copy_from_slice(&bytes.split_to(buf.len()));
                    return Ok(());
                }
                drop(bytes);
                std::future::pending().await
            })
        }
    }

    struct Test {
        transport: IrohTransport,
        writes: Arc<MockSendStream>,
        recorder: Recorder,
        closes: CloseCalls,
    }

    impl Test {
        fn new(streams: Vec<(Duration, DynIrohRecvStream)>) -> Self {
            let mut streams = streams.into_iter();
            let url = fake_remote_url();
            let remote_id = endpoint_from_url(&url).unwrap().id;
            let recorder = build_recording_handler();
            let writes = Arc::new(MockSendStream::new());
            let closes: CloseCalls = Arc::new(Mutex::new(Vec::new()));
            let mut connection = MockConnection::new();
            let stream = writes.clone();
            connection.expect_open_uni().returning(move || {
                let stream = stream.clone();
                Box::pin(async move { Ok(stream as DynIrohSendStream) })
            });
            connection.expect_accept_uni().returning(move || {
                let next_stream = streams.next();
                Box::pin(async move {
                    if let Some((delay, stream)) = next_stream {
                        tokio::time::sleep(delay).await;
                        Ok(stream)
                    } else {
                        std::future::pending().await
                    }
                })
            });
            connection.expect_remote_id().return_const(remote_id);
            connection.expect_is_direct().return_const(false);
            connection.expect_remote_close_reason().returning(|| None);
            let close_calls = closes.clone();
            connection.expect_close().returning(move |code, reason| {
                close_calls
                    .lock()
                    .expect("poison")
                    .push((code, reason.to_vec()));
            });

            let transport = build_transport(
                Arc::new(ConnectingEndpoint {
                    connection: Arc::new(connection),
                }),
                recorder.handler.clone(),
                crate::Connections::new(),
                Arc::new(RwLock::new(Some(url))),
                config(),
            );
            Self {
                transport,
                writes,
                recorder,
                closes,
            }
        }

        async fn assert_expires(&self) {
            let send = || {
                self.transport
                    .send(fake_remote_url(), Bytes::from_static(b"hello"))
            };
            tokio::time::timeout(Duration::from_secs(11), async {
                let (first, second) = tokio::join!(send(), send());
                // Preflight failures retain the transport's existing result
                // contract; the handler reports the connection failure.
                first.unwrap();
                second.unwrap();
            })
            .await
            .expect(
                "pending sends must finish when the preflight deadline expires",
            );

            assert!(
                self.transport.connections.get(&fake_remote_url()).is_none()
            );
            assert_eq!(
                self.recorder.unresponsive_calls.load(Ordering::SeqCst),
                1
            );
            let closes = self.closes.lock().expect("poison");
            assert_eq!(closes.len(), 1);
            assert_eq!(closes[0].0, CloseCode::Unspecified);
            assert!(
                String::from_utf8_lossy(&closes[0].1)
                    .contains("timed out waiting for preflight")
            );
            assert_eq!(
                self.writes.get_written_data().len(),
                1,
                "only preflight may be written before validation"
            );
        }
    }

    #[tokio::test]
    async fn a_peer_that_never_opens_a_stream_expires() {
        Test::new(Vec::new()).assert_expires().await;
    }

    #[tokio::test]
    async fn a_late_stream_does_not_restart_the_preflight_deadline() {
        Test::new(vec![(
            Duration::from_secs(8),
            Arc::new(StalledStream(tokio::sync::Mutex::new(
                Bytes::from_static(&[0]),
            ))),
        )])
        .assert_expires()
        .await;
    }

    #[tokio::test]
    async fn an_active_connection_survives_the_preflight_deadline() {
        use crate::frame::{Frame, encode_frame};
        use kitsune2_api::{K2Proto, K2WireType};

        let preflight = K2Proto {
            ty: K2WireType::Preflight as i32,
            ..Default::default()
        }
        .encode()
        .unwrap();
        let frame = encode_frame(
            Frame::Preflight((fake_remote_url(), preflight)),
            64 * 1024,
        )
        .unwrap();
        let data = encode_frame(
            Frame::Data(K2Proto::default().encode().unwrap()),
            64 * 1024,
        )
        .unwrap();
        // End the first stream after preflight, then open a data stream only
        // after the original deadline. Established stream acceptance must not
        // reuse the preflight timeout.
        let test = Test::new(vec![
            (
                Duration::ZERO,
                Arc::new(MockRecvStream::new(frame.to_vec())),
            ),
            (
                Duration::from_secs(11),
                Arc::new(MockRecvStream::new(data.to_vec())),
            ),
        ]);
        test.transport
            .send(fake_remote_url(), Bytes::from_static(b"first"))
            .await
            .unwrap();
        let ctx = test.transport.connections.get(&fake_remote_url()).unwrap();
        kitsune2_test_utils::retry_fn_until_timeout(
            || async { ctx.get_recv_message_count() == 1 },
            Some(12_000),
            Some(10),
        )
        .await
        .expect("an established connection must accept data after the preflight deadline");
        test.transport
            .send(fake_remote_url(), Bytes::from_static(b"second"))
            .await
            .unwrap();

        assert_eq!(
            test.transport.get_connected_peers().await.unwrap(),
            vec![fake_remote_url()]
        );
        assert!(test.closes.lock().expect("poison").is_empty());
        assert_eq!(test.recorder.unresponsive_calls.load(Ordering::SeqCst), 0);
        assert_eq!(test.writes.get_written_data().len(), 3);
    }
}
