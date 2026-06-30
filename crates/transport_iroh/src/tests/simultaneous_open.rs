//! Unit tests for the connection-reader exit cleanup, which decides whether a
//! reader loop ending means "the peer is gone" (mark unresponsive + fire
//! `peer_disconnect`) or "this connection was deliberately superseded" (close
//! quietly).
//!
//! Regression context: simultaneous-open resolution produces two connections
//! for a pair and tears the losing one down. The teardown of the loser is
//! driven by the *remote* peer, so on the node whose losing connection is
//! still its active map entry — because the winning connection has not yet
//! finished its preflight and registered — that remote close must not be
//! mistaken for a genuine peer disconnect. Doing so spuriously marks the peer
//! unresponsive (which lasts until the agent info expires) and stalls gossip.

use crate::connection::{Connection, DynConnection};
use crate::connection_context::{
    ConnectionContext, ConnectionContextParams, SUPERSEDED_CLOSE_REASON,
};
use crate::stream::{DynIrohRecvStream, DynIrohSendStream};
use crate::test_utils::MockTxHandler;
use crate::url::endpoint_from_url;
use bytes::Bytes;
use iroh::EndpointId;
use kitsune2_api::{
    BoxFut, DefaultTransport, K2Error, K2Result, TransportStats, TxImp,
    TxImpHnd, Url,
};
use kitsune2_test_utils::space::TEST_SPACE_ID;
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

/// A fake [`Connection`] whose first `accept_uni` blocks until released and
/// then fails, simulating the connection being closed. `remote_close_reason`
/// returns whatever the test configured, letting us drive both the
/// "superseded by remote" and the "genuine death" cleanup paths.
struct FakeConnection {
    /// Released by the test once the context is registered as active, so the
    /// reader's exit cleanup observes the populated `connections` map.
    accept_gate: Arc<tokio::sync::Notify>,
    remote_close_reason: Option<Bytes>,
    remote_id: EndpointId,
    close_calls: Arc<AtomicUsize>,
}

impl Connection for FakeConnection {
    fn open_uni(&self) -> BoxFut<'_, K2Result<DynIrohSendStream>> {
        Box::pin(async {
            Err(K2Error::other("open_uni not used in this test"))
        })
    }

    fn accept_uni(&self) -> BoxFut<'_, K2Result<DynIrohRecvStream>> {
        let gate = self.accept_gate.clone();
        Box::pin(async move {
            gate.notified().await;
            Err(K2Error::other("remote closed connection"))
        })
    }

    fn remote_id(&self) -> EndpointId {
        self.remote_id
    }

    fn close(&self, _code: u8, _reason: &[u8]) {
        self.close_calls.fetch_add(1, Ordering::SeqCst);
    }

    fn is_direct(&self) -> bool {
        false
    }

    fn remote_close_reason(&self) -> Option<Bytes> {
        self.remote_close_reason.clone()
    }
}

/// Minimal `TxImp` stub, used only so we can build a `DefaultTransport` and
/// register a space handler against the shared `TxImpHnd` (which is what makes
/// `set_unresponsive` reach our recording handler).
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

struct Recorder {
    handler: Arc<TxImpHnd>,
    unresponsive_calls: Arc<AtomicUsize>,
    disconnect_calls: Arc<AtomicUsize>,
}

/// Build a `TxImpHnd` whose `set_unresponsive` and `peer_disconnect` calls are
/// counted, with a space handler registered so `set_unresponsive` propagates.
fn build_recording_handler() -> Recorder {
    let unresponsive_calls = Arc::new(AtomicUsize::new(0));
    let disconnect_calls = Arc::new(AtomicUsize::new(0));
    let mock = {
        let unresponsive = unresponsive_calls.clone();
        let disconnect = disconnect_calls.clone();
        Arc::new(MockTxHandler {
            set_unresponsive: Arc::new(move |_peer, _ts| {
                unresponsive.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }),
            peer_disconnect: Arc::new(move |_peer, _reason| {
                disconnect.fetch_add(1, Ordering::SeqCst);
            }),
            ..MockTxHandler::default()
        })
    };
    let handler = TxImpHnd::new(mock.clone());
    let transport = DefaultTransport::create(&handler, Arc::new(StubTxImp));
    transport.register_space_handler(TEST_SPACE_ID, mock);
    Recorder {
        handler,
        unresponsive_calls,
        disconnect_calls,
    }
}

fn remote_url() -> Url {
    // 64 hex characters → valid iroh EndpointId encoding.
    Url::from_str(
        "https://relay.example.com:443/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    )
    .unwrap()
}

/// Build a connection context whose reader is parked in `accept_uni`, register
/// it as the active connection for `remote_url`, then release the reader so its
/// exit cleanup runs against a populated `connections` map.
fn spawn_active_reader(
    handler: Arc<TxImpHnd>,
    remote_close_reason: Option<Bytes>,
) -> (Arc<tokio::sync::Notify>, Arc<AtomicUsize>) {
    let url = remote_url();
    let accept_gate = Arc::new(tokio::sync::Notify::new());
    let close_calls = Arc::new(AtomicUsize::new(0));
    let connection: DynConnection = Arc::new(FakeConnection {
        accept_gate: accept_gate.clone(),
        remote_close_reason,
        remote_id: endpoint_from_url(&url).unwrap().id,
        close_calls: close_calls.clone(),
    });

    let connections = Arc::new(RwLock::new(HashMap::new()));

    let ctx = ConnectionContext::new(ConnectionContextParams {
        handler,
        connection,
        local_id: [0u8; 32],
        dialed_by_us: true,
        remote_url: Some(url.clone()),
        preflight_sent: true,
        opened_at_s: 0,
        connections: connections.clone(),
        local_url: Arc::new(RwLock::new(Some(url.clone()))),
        space_relays: Arc::new(RwLock::new(HashMap::new())),
        max_frame_bytes: 64 * 1024,
    });

    // Register as the active connection *before* releasing the reader, so the
    // exit cleanup sees this context as the live map entry (`was_active`).
    connections.write().unwrap().insert(url, ctx);

    // Release the parked `accept_uni`, which fails and drives the reader into
    // its exit cleanup.
    accept_gate.notify_one();

    (accept_gate, close_calls)
}

/// Poll `condition` every 10ms until it returns true or `timeout` elapses.
async fn wait_for<F: FnMut() -> bool>(mut condition: F, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    loop {
        if condition() {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "condition not met within timeout"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// When the active connection is closed by the remote with the superseded
/// reason — i.e. the remote preferred a different connection during
/// simultaneous-open resolution — the reader exit must NOT mark the peer
/// unresponsive or fire `peer_disconnect`.
#[tokio::test(flavor = "multi_thread")]
async fn superseded_remote_close_does_not_mark_unresponsive() {
    let recorder = build_recording_handler();

    let (_gate, close_calls) = spawn_active_reader(
        recorder.handler.clone(),
        Some(Bytes::from_static(SUPERSEDED_CLOSE_REASON)),
    );

    // The exit cleanup finishes by closing the connection.
    wait_for(
        || close_calls.load(Ordering::SeqCst) >= 1,
        Duration::from_secs(5),
    )
    .await;

    assert_eq!(
        recorder.unresponsive_calls.load(Ordering::SeqCst),
        0,
        "a connection superseded by the remote must not mark the peer unresponsive"
    );
    assert_eq!(
        recorder.disconnect_calls.load(Ordering::SeqCst),
        0,
        "a connection superseded by the remote must not fire peer_disconnect"
    );
}

/// When the active connection dies without the superseded reason (a genuine
/// peer disconnect), the reader exit must still mark the peer unresponsive and
/// fire `peer_disconnect`. Guards against the fix over-suppressing real
/// disconnects.
#[tokio::test(flavor = "multi_thread")]
async fn genuine_remote_close_marks_unresponsive() {
    let recorder = build_recording_handler();

    let (_gate, _close_calls) =
        spawn_active_reader(recorder.handler.clone(), None);

    wait_for(
        || recorder.disconnect_calls.load(Ordering::SeqCst) >= 1,
        Duration::from_secs(5),
    )
    .await;

    assert_eq!(
        recorder.unresponsive_calls.load(Ordering::SeqCst),
        1,
        "a genuine peer disconnect must mark the peer unresponsive"
    );
}
