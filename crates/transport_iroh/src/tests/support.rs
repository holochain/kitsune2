//! Shared helpers for connection-reader exit-cleanup tests.
//!
//! Used by the `simultaneous_open` and `close` test modules to drive a
//! [`ConnectionContext`] reader into its exit cleanup with a configurable
//! remote close, and to record the resulting handler calls.

use crate::close_code::CloseCode;
use crate::connection::{Connection, DynConnection};
use crate::connection_context::{ConnectionContext, ConnectionContextParams};
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
use std::sync::{Arc, Mutex, RwLock};

/// Recorded [`Connection::close`] calls as `(code, reason)` pairs.
pub(super) type CloseCalls = Arc<Mutex<Vec<(CloseCode, Vec<u8>)>>>;

/// A fake [`Connection`] whose first `accept_uni` blocks until released and
/// then fails, simulating the connection being closed. `remote_close_reason`
/// returns whatever the test configured, letting tests drive each exit
/// cleanup path.
pub(super) struct FakeConnection {
    /// Released by the test once the context is registered as active, so
    /// the reader's exit cleanup observes the populated `connections` map.
    pub accept_gate: Arc<tokio::sync::Notify>,
    pub remote_close: Option<(CloseCode, Bytes)>,
    pub remote_id: EndpointId,
    /// Records every `close` call as `(code, reason)`.
    pub close_calls: CloseCalls,
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

    fn close(&self, code: CloseCode, reason: &[u8]) {
        self.close_calls
            .lock()
            .unwrap()
            .push((code, reason.to_vec()));
    }

    fn is_direct(&self) -> bool {
        false
    }

    fn remote_close_reason(&self) -> Option<(CloseCode, Bytes)> {
        self.remote_close.clone()
    }
}

/// Minimal `TxImp` stub, used only so we can build a `DefaultTransport` and
/// register a space handler against the shared `TxImpHnd` (which is what
/// makes `set_unresponsive` reach our recording handler).
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

pub(super) struct Recorder {
    pub handler: Arc<TxImpHnd>,
    pub unresponsive_calls: Arc<AtomicUsize>,
    /// Reason of every `peer_disconnect` call, in order. NOTE: the handler
    /// fan-out delivers each disconnect to both the space handler and the
    /// base handler, so a single disconnect records two identical entries.
    pub disconnects: Arc<Mutex<Vec<Option<String>>>>,
}

/// Build a `TxImpHnd` whose `set_unresponsive` and `peer_disconnect` calls
/// are recorded, with a space handler registered so `set_unresponsive`
/// propagates.
pub(super) fn build_recording_handler() -> Recorder {
    let unresponsive_calls = Arc::new(AtomicUsize::new(0));
    let disconnects: Arc<Mutex<Vec<Option<String>>>> =
        Arc::new(Mutex::new(Vec::new()));
    let mock = {
        let unresponsive = unresponsive_calls.clone();
        let disconnects = disconnects.clone();
        Arc::new(MockTxHandler {
            set_unresponsive: Arc::new(move |_peer, _ts| {
                unresponsive.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }),
            peer_disconnect: Arc::new(move |_peer, reason| {
                disconnects.lock().unwrap().push(reason);
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
        disconnects,
    }
}

pub(super) fn remote_url() -> Url {
    // 64 hex characters → valid iroh EndpointId encoding.
    Url::from_str(
        "https://relay.example.com:443/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    )
    .unwrap()
}

pub(super) struct ActiveReader {
    pub ctx: Arc<ConnectionContext>,
    /// Release this to let the parked `accept_uni` fail, driving the reader
    /// into its exit cleanup.
    pub accept_gate: Arc<tokio::sync::Notify>,
    pub close_calls: CloseCalls,
}

/// Build a connection context whose reader is parked in `accept_uni` and
/// register it as the active connection for [`remote_url`]. The caller
/// releases the reader via `accept_gate.notify_one()` so its exit cleanup
/// runs against a populated `connections` map.
pub(super) fn spawn_active_reader(
    handler: Arc<TxImpHnd>,
    remote_close: Option<(CloseCode, Bytes)>,
) -> ActiveReader {
    let url = remote_url();
    let accept_gate = Arc::new(tokio::sync::Notify::new());
    let close_calls = Arc::new(Mutex::new(Vec::new()));
    let connection: DynConnection = Arc::new(FakeConnection {
        accept_gate: accept_gate.clone(),
        remote_close,
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

    // Register as the active connection *before* the reader is released, so
    // the exit cleanup sees this context as the live map entry (`was_active`).
    connections.write().unwrap().insert(url, ctx.clone());

    ActiveReader {
        ctx,
        accept_gate,
        close_calls,
    }
}
