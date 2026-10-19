//! Unit tests for closing an [`IrohTransport`].
//!
//! These tests use a fake [`Endpoint`] whose `close` waits on a gate, so they
//! can deterministically cancel a close or race two closes while the endpoint
//! is still closing. Its relay insertions never finish, so the tests can
//! check that close aborts them and that a closed transport starts none.

use super::support::{
    PendingWatcher, build_recording_handler, build_transport,
};
use crate::connection::DynConnection;
use crate::endpoint::{DynIrohEndpoint, Endpoint, EndpointAddrWatcher};
use crate::{
    IrohTransport, IrohTransportConfig, IrohTransportModConfig,
    RelayAuthParams, SpaceRelays,
};
use iroh::{EndpointAddr, RelayConfig, RelayUrl};
use kitsune2_api::{BoxFut, Config, K2Error, K2Result, TxImp, Url};
use kitsune2_test_utils::retry_fn_until_timeout;
use kitsune2_test_utils::space::TEST_SPACE_ID;
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;
use tokio::sync::watch;
use tokio::task::JoinHandle;

/// A fake `Endpoint` that counts `close` and `insert_relay` calls and only
/// lets a close finish once its gate is open. An `insert_relay` never
/// finishes and holds a clone of `insert_relay_guard` until it is dropped.
#[derive(Debug)]
struct GatedCloseEndpoint {
    close_calls: AtomicUsize,
    insert_relay_calls: AtomicUsize,
    insert_relay_guard: Arc<()>,
    gate: watch::Receiver<bool>,
}

impl Endpoint for GatedCloseEndpoint {
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
        Box::pin(async { Err(K2Error::other("connect is not used")) })
    }

    fn close(&self) -> BoxFut<'_, ()> {
        self.close_calls.fetch_add(1, Ordering::SeqCst);
        let mut gate = self.gate.clone();
        Box::pin(async move {
            gate.wait_for(|open| *open)
                .await
                .expect("gate sender dropped");
        })
    }

    fn insert_relay(
        &self,
        _url: RelayUrl,
        _config: Arc<RelayConfig>,
    ) -> BoxFut<'_, ()> {
        self.insert_relay_calls.fetch_add(1, Ordering::SeqCst);
        let guard = self.insert_relay_guard.clone();
        Box::pin(async move {
            let _guard = guard;
            std::future::pending::<()>().await;
        })
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

/// Builds a transport over a [`GatedCloseEndpoint`] whose gate starts as
/// `gate_open`, returning the transport, the endpoint and the gate sender.
fn gated_transport(
    gate_open: bool,
) -> (IrohTransport, Arc<GatedCloseEndpoint>, watch::Sender<bool>) {
    let (gate_send, gate) = watch::channel(gate_open);
    let endpoint = Arc::new(GatedCloseEndpoint {
        close_calls: AtomicUsize::new(0),
        insert_relay_calls: AtomicUsize::new(0),
        insert_relay_guard: Arc::new(()),
        gate,
    });
    let dyn_endpoint: DynIrohEndpoint = endpoint.clone();
    let transport = build_transport(
        dyn_endpoint,
        build_recording_handler().handler,
        crate::Connections::new(),
        Arc::new(RwLock::new(None)),
        IrohTransportConfig::default(),
    );
    (transport, endpoint, gate_send)
}

/// A per-space config that names a relay without auth material.
fn relay_space_config() -> Config {
    let config = Config::default();
    config
        .set_module_config(&IrohTransportModConfig {
            iroh_transport: IrohTransportConfig {
                relay_url: Some("http://relay.example.com".to_string()),
                relay_allow_plain_text: true,
                ..Default::default()
            },
        })
        .unwrap();
    config
}

/// The background tasks of a transport, as join handles that report how
/// each task ended.
struct BackgroundTasks {
    watch_addr: JoinHandle<()>,
    accept: JoinHandle<()>,
    relay_keepalive: JoinHandle<()>,
    space_relay_keepalive: JoinHandle<()>,
}

impl BackgroundTasks {
    /// Replaces the transport's background tasks with tasks that never
    /// finish on their own.
    fn install(transport: &mut IrohTransport) -> Self {
        let pending = || tokio::spawn(std::future::pending::<()>());
        let tasks = Self {
            watch_addr: pending(),
            accept: pending(),
            relay_keepalive: pending(),
            space_relay_keepalive: pending(),
        };
        transport.watch_addr_task = tasks.watch_addr.abort_handle();
        transport.accept_task = tasks.accept.abort_handle();
        transport.relay_keepalive_task =
            Some(tasks.relay_keepalive.abort_handle());
        transport
            .space_relay_keepalives
            .lock()
            .expect("poison")
            .insert(
                RelayUrl::from_str("https://relay.example.com/").unwrap(),
                tasks.space_relay_keepalive.abort_handle(),
            );
        tasks
    }

    /// Asserts that every task was aborted.
    async fn assert_aborted(self) {
        for (name, task) in [
            ("watch_addr", self.watch_addr),
            ("accept", self.accept),
            ("relay_keepalive", self.relay_keepalive),
            ("space_relay_keepalive", self.space_relay_keepalive),
        ] {
            let err = tokio::time::timeout(Duration::from_secs(1), task)
                .await
                .unwrap_or_else(|_| panic!("{name} task must end"))
                .expect_err("task never finishes on its own");
            assert!(err.is_cancelled(), "{name} task must be aborted");
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn close_aborts_background_tasks() {
    let (mut transport, _endpoint, _gate) = gated_transport(true);
    let tasks = BackgroundTasks::install(&mut transport);

    transport.close().await;

    tasks.assert_aborted().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn drop_aborts_background_tasks() {
    let (mut transport, _endpoint, _gate) = gated_transport(true);
    let tasks = BackgroundTasks::install(&mut transport);

    drop(transport);

    tasks.assert_aborted().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn close_after_cancelled_close_closes_endpoint() {
    let (transport, endpoint, gate) = gated_transport(false);

    tokio::time::timeout(Duration::from_millis(50), transport.close())
        .await
        .expect_err("close must wait for the endpoint to close");

    gate.send(true).unwrap();
    tokio::time::timeout(Duration::from_secs(1), transport.close())
        .await
        .expect("close after a cancelled close must complete");

    assert_eq!(endpoint.close_calls.load(Ordering::SeqCst), 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn concurrent_close_waits_for_running_close() {
    let (transport, endpoint, gate) = gated_transport(false);
    let transport = Arc::new(transport);

    let first = tokio::spawn({
        let transport = transport.clone();
        async move { transport.close().await }
    });
    retry_fn_until_timeout(
        || async { endpoint.close_calls.load(Ordering::SeqCst) == 1 },
        Some(1000),
        Some(10),
    )
    .await
    .expect("first close must reach the endpoint");

    tokio::time::timeout(Duration::from_millis(100), transport.close())
        .await
        .expect_err("a concurrent close must wait for the running close");

    gate.send(true).unwrap();
    tokio::time::timeout(Duration::from_secs(1), first)
        .await
        .expect("first close must complete once the endpoint closes")
        .unwrap();
    transport.close().await;

    assert_eq!(endpoint.close_calls.load(Ordering::SeqCst), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn drop_after_cancelled_close_closes_endpoint() {
    let (transport, endpoint, gate) = gated_transport(false);

    tokio::time::timeout(Duration::from_millis(50), transport.close())
        .await
        .expect_err("close must wait for the endpoint to close");
    gate.send(true).unwrap();
    drop(transport);

    retry_fn_until_timeout(
        || async { endpoint.close_calls.load(Ordering::SeqCst) == 2 },
        Some(1000),
        Some(10),
    )
    .await
    .expect("dropping after a cancelled close must close the endpoint");
}

#[tokio::test(flavor = "multi_thread")]
async fn configure_for_space_after_close_starts_no_relay_insertion() {
    let (transport, endpoint, _gate) = gated_transport(true);
    transport.close().await;

    transport
        .configure_for_space(TEST_SPACE_ID, &relay_space_config())
        .await
        .unwrap();

    assert!(
        transport
            .relay_insert_tasks
            .lock()
            .expect("poison")
            .is_empty(),
        "a closed transport must not start a relay insertion"
    );
    assert_eq!(endpoint.insert_relay_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn close_aborts_a_running_relay_insertion() {
    let (transport, endpoint, _gate) = gated_transport(true);
    transport
        .configure_for_space(TEST_SPACE_ID, &relay_space_config())
        .await
        .unwrap();
    retry_fn_until_timeout(
        || async { endpoint.insert_relay_calls.load(Ordering::SeqCst) == 1 },
        Some(1000),
        Some(10),
    )
    .await
    .expect("relay insertion must start");

    transport.close().await;

    // The insertion task held the guard while it awaited `insert_relay`, so
    // the guard is released once the task has been aborted.
    retry_fn_until_timeout(
        || async { Arc::strong_count(&endpoint.insert_relay_guard) == 1 },
        Some(1000),
        Some(10),
    )
    .await
    .expect("close must abort the running relay insertion");
    assert!(
        transport
            .relay_insert_tasks
            .lock()
            .expect("poison")
            .is_empty()
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn record_space_relay_skips_a_closed_transport() {
    let relay_url = RelayUrl::from_str("https://relay.example.com/").unwrap();
    // 64 hex characters, a valid iroh endpoint id.
    let local_url = Url::from_str(
        "https://relay.example.com:443/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    )
    .unwrap();
    let auth_params = || {
        Some(Arc::new(RelayAuthParams {
            server_url: ::url::Url::parse("http://127.0.0.1:1/").unwrap(),
            auth_material: kitsune2_bootstrap_client::AuthMaterial::new(
                Vec::new(),
            ),
            relay_url: relay_url.clone(),
            key_bytes: [0u8; 32],
        }))
    };
    let space_relays: SpaceRelays = Arc::new(RwLock::new(HashMap::new()));
    let keepalives = Mutex::new(HashMap::new());

    let recorded = IrohTransport::record_space_relay(
        &AtomicBool::new(true),
        &space_relays,
        &keepalives,
        TEST_SPACE_ID,
        (relay_url.clone(), local_url.clone(), auth_params()),
        Duration::from_secs(60),
    );
    assert!(!recorded);
    assert!(space_relays.read().expect("poison").is_empty());
    assert!(keepalives.lock().expect("poison").is_empty());

    // The same insertion on an open transport is recorded with a keepalive.
    let recorded = IrohTransport::record_space_relay(
        &AtomicBool::new(false),
        &space_relays,
        &keepalives,
        TEST_SPACE_ID,
        (relay_url.clone(), local_url, auth_params()),
        Duration::from_secs(60),
    );
    assert!(recorded);
    assert!(
        space_relays
            .read()
            .expect("poison")
            .contains_key(&TEST_SPACE_ID)
    );
    let keepalive = keepalives
        .lock()
        .expect("poison")
        .remove(&relay_url)
        .expect("keepalive must be started");
    keepalive.abort();
}

#[tokio::test(flavor = "multi_thread")]
async fn track_relay_insert_task_aborts_on_a_closed_transport() {
    let relay_insert_tasks = Mutex::new(Vec::new());
    let task = tokio::spawn(std::future::pending::<()>());

    IrohTransport::track_relay_insert_task(
        &AtomicBool::new(true),
        &relay_insert_tasks,
        task.abort_handle(),
    );

    assert!(
        task.await.expect_err("task must be aborted").is_cancelled(),
        "a task tracked after close must be aborted"
    );

    // The same task on an open transport is tracked and keeps running.
    let task = tokio::spawn(std::future::pending::<()>());
    IrohTransport::track_relay_insert_task(
        &AtomicBool::new(false),
        &relay_insert_tasks,
        task.abort_handle(),
    );
    assert!(!task.is_finished());
    let tracked = relay_insert_tasks.lock().expect("poison");
    assert_eq!(1, tracked.len());
    assert_eq!(task.id(), tracked[0].id());
    tracked[0].abort();
}
