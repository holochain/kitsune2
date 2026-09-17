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
use iroh::{EndpointAddr, EndpointId, RelayConfig, RelayUrl, TransportAddr};
use kitsune2_api::{
    BoxFut, DefaultTransport, K2Error, K2Result, TransportStats, TxImp,
    TxImpHnd, Url,
};
use kitsune2_test_utils::space::TEST_SPACE_ID;
use n0_watcher::Disconnected;
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

/// A fake `Endpoint` whose `connect()` returns a configurable error.
///
/// `watch_addr`, `accept` and `close` are stubs that should not be exercised
/// by the tests in this module.
struct FakeEndpoint {
    connect_error_factory: Arc<dyn Fn() -> K2Error + 'static + Send + Sync>,
    connect_calls: Arc<AtomicUsize>,
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
        self.connect_calls.fetch_add(1, Ordering::Relaxed);
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
    fn get(&mut self) -> EndpointAddr {
        endpoint_addr(None)
    }

    fn updated(&mut self) -> BoxFut<'_, Result<EndpointAddr, Disconnected>> {
        Box::pin(std::future::pending())
    }
}

/// Provides a controllable endpoint address for transport startup tests.
#[derive(Debug)]
struct ControlledEndpoint {
    initial_addr: EndpointAddr,
    updates: Arc<
        tokio::sync::Mutex<tokio::sync::mpsc::UnboundedReceiver<EndpointAddr>>,
    >,
    watcher_started: Arc<Mutex<Option<tokio::sync::oneshot::Sender<()>>>>,
}

/// Returns the configured initial address and waits for test-driven updates.
struct ControlledWatcher {
    initial_addr: EndpointAddr,
    updates: Arc<
        tokio::sync::Mutex<tokio::sync::mpsc::UnboundedReceiver<EndpointAddr>>,
    >,
    watcher_started: Arc<Mutex<Option<tokio::sync::oneshot::Sender<()>>>>,
}

impl EndpointAddrWatcher for ControlledWatcher {
    fn get(&mut self) -> EndpointAddr {
        self.initial_addr.clone()
    }

    fn updated(&mut self) -> BoxFut<'_, Result<EndpointAddr, Disconnected>> {
        if let Some(watcher_started) =
            self.watcher_started.lock().expect("poisoned").take()
        {
            let _ = watcher_started.send(());
        }
        Box::pin(async move {
            self.updates.lock().await.recv().await.ok_or(Disconnected)
        })
    }
}

impl Endpoint for ControlledEndpoint {
    fn watch_addr(&self) -> Box<dyn EndpointAddrWatcher> {
        Box::new(ControlledWatcher {
            initial_addr: self.initial_addr.clone(),
            updates: self.updates.clone(),
            watcher_started: self.watcher_started.clone(),
        })
    }

    fn accept(&self) -> BoxFut<'_, Option<K2Result<DynConnection>>> {
        Box::pin(std::future::pending())
    }

    fn connect(
        &self,
        _endpoint_addr: EndpointAddr,
        _alpn: &[u8],
    ) -> BoxFut<'_, K2Result<DynConnection>> {
        Box::pin(async { unreachable!("startup test must not connect") })
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
        [0; 32]
    }

    fn is_home_relay_known_down(&self) -> bool {
        false
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

fn endpoint_addr(relay_url: Option<RelayUrl>) -> EndpointAddr {
    let endpoint_id = EndpointId::from_str(
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    )
    .unwrap();
    let addrs = relay_url
        .map(|url| vec![TransportAddr::Relay(url)])
        .unwrap_or_default();
    EndpointAddr::from_parts(endpoint_id, addrs)
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

#[tokio::test]
async fn transport_creation_waits_for_first_listening_url() {
    // Construct a test endpoint where we can manually set the listening URL
    let (update_tx, update_rx) = tokio::sync::mpsc::unbounded_channel();
    let (watcher_started_tx, watcher_started_rx) =
        tokio::sync::oneshot::channel();
    let endpoint: DynIrohEndpoint = Arc::new(ControlledEndpoint {
        initial_addr: endpoint_addr(None),
        updates: Arc::new(tokio::sync::Mutex::new(update_rx)),
        watcher_started: Arc::new(Mutex::new(Some(watcher_started_tx))),
    });
    let handler = TxImpHnd::new(Arc::new(MockTxHandler::default()));

    // Try to create a transport without a listening URL. It should not be created.
    let creation = tokio::spawn(IrohTransport::create_with_endpoint(
        endpoint,
        handler,
        config(),
        None,
    ));
    watcher_started_rx
        .await
        .expect("address watcher should signal when it starts waiting");
    assert!(
        !creation.is_finished(),
        "transport must not be available before its listening URL"
    );

    // Set the listening url. Transport should be created.
    let endpoint_id = EndpointId::from_str(
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    )
    .unwrap();
    let relay_url =
        RelayUrl::from_str("https://relay.example.com:443/").unwrap();
    update_tx
        .send(EndpointAddr::from_parts(
            endpoint_id,
            vec![TransportAddr::Relay(relay_url)],
        ))
        .unwrap();

    let transport = tokio::time::timeout(Duration::from_secs(1), creation)
        .await
        .expect("transport creation should finish after receiving the URL")
        .expect("creation task should not panic")
        .expect("transport creation should succeed");
    assert!(
        transport.local_url.read().expect("poisoned").is_some(),
        "transport must publish the listening URL before creation completes"
    );
}

#[tokio::test]
async fn transport_creation_uses_current_listening_url() {
    // Create an endpoint whose watcher already holds a listening URL
    let relay_url =
        RelayUrl::from_str("https://relay.example.com:443/").unwrap();
    let (_update_tx, update_rx) = tokio::sync::mpsc::unbounded_channel();
    let (watcher_started_tx, _watcher_started_rx) =
        tokio::sync::oneshot::channel();
    let endpoint: DynIrohEndpoint = Arc::new(ControlledEndpoint {
        initial_addr: endpoint_addr(Some(relay_url)),
        updates: Arc::new(tokio::sync::Mutex::new(update_rx)),
        watcher_started: Arc::new(Mutex::new(Some(watcher_started_tx))),
    });
    let handler = TxImpHnd::new(Arc::new(MockTxHandler::default()));

    // Create the transport without publishing another address update
    let transport = tokio::time::timeout(
        Duration::from_secs(1),
        IrohTransport::create_with_endpoint(endpoint, handler, config(), None),
    )
    .await
    .expect("transport creation should use the watcher's current URL")
    .expect("transport creation should succeed");

    // Creation should publish the watcher's current URL
    assert_eq!(
        *transport.local_url.read().expect("poisoned"),
        Some(fake_remote_url())
    );
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
        connect_calls: Arc::new(AtomicUsize::new(0)),
    });

    let remote_url = fake_remote_url();
    let target = endpoint_from_url(&remote_url).unwrap();

    let connections = Arc::new(RwLock::new(HashMap::new()));
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
    assert!(connections.read().unwrap().is_empty());
}

#[tokio::test]
async fn missing_local_url_prevents_opening_connection() {
    // Build a transport with no local URL and a fake endpoint that records
    // every connection attempt.
    let handler = build_handler_with_space(Arc::new(Mutex::new(Vec::new())));
    let connect_calls = Arc::new(AtomicUsize::new(0));
    let fake_endpoint: DynIrohEndpoint = Arc::new(FakeEndpoint {
        connect_error_factory: Arc::new(|| K2Error::other("connected")),
        connect_calls: connect_calls.clone(),
    });
    let remote_url = fake_remote_url();
    let target = endpoint_from_url(&remote_url).unwrap();
    let transport = build_transport(
        fake_endpoint,
        handler,
        Arc::new(RwLock::new(HashMap::new())),
        Arc::new(RwLock::new(None)),
        config(),
    );

    // Attempt to create a connection before the transport has a listening url
    let error = transport
        .create_connection_and_context(target, remote_url)
        .await
        .expect_err("connection should require a local URL");

    assert!(
        error
            .to_string()
            .contains("Connection attempted before home relay URL is known")
    );
    assert_eq!(
        connect_calls.load(Ordering::Relaxed),
        0,
        "endpoint must not be dialled without a local URL"
    );
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

    let connections = Arc::new(RwLock::new(HashMap::new()));
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
