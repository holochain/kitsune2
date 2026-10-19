use kitsune2_api::*;
use kitsune2_test_utils::enable_tracing;
use kitsune2_test_utils::space::TEST_SPACE_ID;
use std::sync::{Arc, Mutex};

#[derive(Debug)]
enum Track {
    ThisUrl(Url),
    Connect(Url),
    Disconnect(Url, Option<String>),
    SpaceRecv(Url, SpaceId, bytes::Bytes),
    ModRecv(Url, SpaceId, String, bytes::Bytes),
    PreflightSend,
    PreflightRecv,
    AreBlocked(Url),
}

type G = Box<dyn Fn(Url) -> K2Result<bytes::Bytes> + 'static + Send + Sync>;
type V = Box<dyn Fn(Url, bytes::Bytes) -> K2Result<()> + 'static + Send + Sync>;

struct TrackHnd {
    track: Mutex<Vec<Track>>,
    preflight_gather_outgoing: G,
    preflight_validate_incoming: V,
}

impl std::fmt::Debug for TrackHnd {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        (*self.track.lock().unwrap()).fmt(f)
    }
}

impl TxBaseHandler for TrackHnd {
    fn new_listening_address(&self, this_url: Url) -> BoxFut<'static, ()> {
        self.track.lock().unwrap().push(Track::ThisUrl(this_url));
        Box::pin(async move {})
    }

    fn peer_connect(&self, peer: Url) -> K2Result<()> {
        self.track.lock().unwrap().push(Track::Connect(peer));
        Ok(())
    }

    fn peer_disconnect(&self, peer: Url, reason: Option<String>) {
        self.track
            .lock()
            .unwrap()
            .push(Track::Disconnect(peer, reason));
    }
}

impl TxHandler for TrackHnd {
    fn preflight_gather_outgoing(
        &self,
        peer_url: Url,
    ) -> BoxFut<'_, K2Result<bytes::Bytes>> {
        self.track.lock().unwrap().push(Track::PreflightSend);
        Box::pin(async { (self.preflight_gather_outgoing)(peer_url) })
    }

    fn preflight_validate_incoming(
        &self,
        peer_url: Url,
        data: bytes::Bytes,
    ) -> BoxFut<'_, K2Result<()>> {
        self.track.lock().unwrap().push(Track::PreflightRecv);
        Box::pin(async { (self.preflight_validate_incoming)(peer_url, data) })
    }
}

impl TxSpaceHandler for TrackHnd {
    fn recv_space_notify(
        &self,
        peer: Url,
        space_id: SpaceId,
        data: bytes::Bytes,
    ) -> K2Result<()> {
        self.track
            .lock()
            .unwrap()
            .push(Track::SpaceRecv(peer, space_id, data));
        Ok(())
    }

    fn is_any_agent_at_url_blocked(&self, peer_url: &Url) -> K2Result<bool> {
        self.track
            .lock()
            .unwrap()
            .push(Track::AreBlocked(peer_url.clone()));

        Ok(false)
    }
}

impl TxModuleHandler for TrackHnd {
    fn recv_module_msg(
        &self,
        peer: Url,
        space_id: SpaceId,
        module: String,
        data: bytes::Bytes,
    ) -> K2Result<()> {
        self.track
            .lock()
            .unwrap()
            .push(Track::ModRecv(peer, space_id, module, data));
        Ok(())
    }
}

impl TrackHnd {
    pub fn new() -> Arc<Self> {
        Self::new_preflight(
            Box::new(|_| Ok(bytes::Bytes::new())),
            Box::new(|_, _| Ok(())),
        )
    }

    pub fn new_preflight(g: G, v: V) -> Arc<Self> {
        Arc::new(Self {
            track: Mutex::new(Vec::new()),
            preflight_gather_outgoing: g,
            preflight_validate_incoming: v,
        })
    }

    pub fn url(&self) -> Url {
        for t in self.track.lock().unwrap().iter() {
            if let Track::ThisUrl(url) = t {
                return url.clone();
            }
        }
        panic!("no url found");
    }

    pub fn check_connect(&self, peer: &Url) -> K2Result<()> {
        for t in self.track.lock().unwrap().iter() {
            if let Track::Connect(u) = t
                && u == peer
            {
                return Ok(());
            }
        }
        Err(K2Error::other(format!(
            "matching connect entry not found {peer}, out of {:#?}",
            self.track.lock().unwrap(),
        )))
    }

    pub fn check_disconnect(
        &self,
        peer: &Url,
        reason: Option<&str>,
    ) -> K2Result<()> {
        for t in self.track.lock().unwrap().iter() {
            if let Track::Disconnect(u, r) = t {
                if u != peer {
                    continue;
                }
                if let Some(reason) = reason
                    && let Some(r) = r
                    && r.contains(reason)
                {
                    return Ok(());
                }
            }
        }
        Err(K2Error::other(format!(
            "matching disconnect entry not found {peer} {reason:?}, out of {:#?}",
            self.track.lock().unwrap(),
        )))
    }

    pub fn check_notify(
        &self,
        peer: &Url,
        space_id: &SpaceId,
        msg: &[u8],
    ) -> K2Result<()> {
        for t in self.track.lock().unwrap().iter() {
            if let Track::SpaceRecv(u, s, d) = t
                && u == peer
                && space_id == s
                && &d[..] == msg
            {
                return Ok(());
            }
        }
        Err(K2Error::other(format!(
            "matching notify not found {peer} {space_id} {}, out of {:#?}",
            String::from_utf8_lossy(msg),
            self.track.lock().unwrap(),
        )))
    }

    pub fn check_mod(
        &self,
        peer: &Url,
        space_id: &SpaceId,
        module: &str,
        msg: &[u8],
    ) -> K2Result<()> {
        for t in self.track.lock().unwrap().iter() {
            if let Track::ModRecv(u, s, m, d) = t
                && u == peer
                && space_id == s
                && module == m
                && &d[..] == msg
            {
                return Ok(());
            }
        }
        Err(K2Error::other(format!(
            "matching mod not found {peer} {space_id} {module} {}, out of {:#?}",
            String::from_utf8_lossy(msg),
            self.track.lock().unwrap(),
        )))
    }

    pub fn check_preflight_before_other_messages(&self) {
        let lock = self.track.lock().unwrap();
        let preflight_index = std::cmp::max(
            lock.iter()
                .position(|t| matches!(t, Track::PreflightSend))
                .expect("no preflight send"),
            lock.iter()
                .position(|t| matches!(t, Track::PreflightRecv))
                .expect("no preflight recv"),
        );
        let message_index = std::cmp::min(
            lock.iter()
                .position(|t| matches!(t, Track::SpaceRecv(_, _, _))),
            lock.iter()
                .position(|t| matches!(t, Track::ModRecv(_, _, _, _))),
        )
        .expect("No messages found");

        if preflight_index > message_index {
            panic!(
                "Preflight messages were not sent/received before other messages"
            );
        }
    }

    /// Check that is_any_agent_at_url_blocked() has been called on the
    /// TxSpaceHandler
    pub fn check_checked_for_blocks(&self, peer_url: &Url) -> K2Result<()> {
        let track = self.track.lock().unwrap();
        track
            .iter()
            .find(|t| matches!(t, Track::AreBlocked(url) if peer_url == url))
            .ok_or(K2Error::other(format!(
                "matching AreBlocked not found {peer_url}, out of {track:#?}"
            )))?;
        Ok(())
    }
}

async fn gen_tx(hnd: DynTxHandler) -> DynTransport {
    let builder = Arc::new(crate::default_test_builder());
    builder
        .transport
        .create(builder.clone(), hnd)
        .await
        .unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn transport_notify() {
    let h1 = TrackHnd::new();
    let t1 = gen_tx(h1.clone()).await;
    t1.register_space_handler(TEST_SPACE_ID, h1.clone());
    let u1 = h1.url();

    let h2 = TrackHnd::new();
    let t2 = gen_tx(h2.clone()).await;
    t2.register_space_handler(TEST_SPACE_ID, h2.clone());
    let u2 = h2.url();

    t1.send_space_notify(
        u2.clone(),
        TEST_SPACE_ID,
        bytes::Bytes::from_static(b"hello"),
    )
    .await
    .unwrap();

    t2.send_space_notify(
        u1.clone(),
        TEST_SPACE_ID,
        bytes::Bytes::from_static(b"world"),
    )
    .await
    .unwrap();

    h1.check_connect(&u2).unwrap();
    h2.check_connect(&u1).unwrap();
    h1.check_notify(&u2, &TEST_SPACE_ID, b"world").unwrap();
    h2.check_notify(&u1, &TEST_SPACE_ID, b"hello").unwrap();
    h1.check_checked_for_blocks(&u2).unwrap();
    h2.check_checked_for_blocks(&u1).unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn transport_module() {
    enable_tracing();

    let h1 = TrackHnd::new();
    let t1 = gen_tx(h1.clone()).await;
    t1.register_module_handler(TEST_SPACE_ID, "test".into(), h1.clone());
    t1.register_space_handler(TEST_SPACE_ID, h1.clone());
    let u1 = h1.url();

    let h2 = TrackHnd::new();
    let t2 = gen_tx(h2.clone()).await;
    t2.register_module_handler(TEST_SPACE_ID, "test".into(), h2.clone());
    t2.register_space_handler(TEST_SPACE_ID, h2.clone());
    let u2 = h2.url();

    t1.send_module(
        u2.clone(),
        TEST_SPACE_ID,
        "test".into(),
        bytes::Bytes::from_static(b"hello"),
    )
    .await
    .unwrap();

    t2.send_module(
        u1.clone(),
        TEST_SPACE_ID,
        "test".into(),
        bytes::Bytes::from_static(b"world"),
    )
    .await
    .unwrap();

    h1.check_connect(&u2).unwrap();
    h2.check_connect(&u1).unwrap();
    h1.check_mod(&u2, &TEST_SPACE_ID, "test", b"world").unwrap();
    h2.check_mod(&u1, &TEST_SPACE_ID, "test", b"hello").unwrap();
    h1.check_checked_for_blocks(&u2).unwrap();
    h2.check_checked_for_blocks(&u1).unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn transport_disconnect() {
    let h1 = TrackHnd::new();
    let t1 = gen_tx(h1.clone()).await;
    t1.register_space_handler(TEST_SPACE_ID, h1.clone());
    let u1 = h1.url();

    let h2 = TrackHnd::new();
    let t2 = gen_tx(h2.clone()).await;
    t2.register_space_handler(TEST_SPACE_ID, h2.clone());
    let u2 = h2.url();

    t1.send_space_notify(
        u2.clone(),
        TEST_SPACE_ID,
        bytes::Bytes::from_static(b"hello"),
    )
    .await
    .unwrap();

    h2.check_connect(&u1).unwrap();

    let mut s1 = t1.dump_network_stats().await.unwrap();
    println!("before disconnect: {s1:#?}");

    assert_eq!("kitsune2-core-mem", s1.transport_stats.backend);
    assert!(!s1.transport_stats.connections.is_empty());
    let c1 = s1.transport_stats.connections.remove(0);
    assert!(c1.send_message_count > 0);
    assert!(c1.send_bytes > 0);

    t1.disconnect(u2.clone(), Some("test-reason".into())).await;

    h1.check_disconnect(&u2, Some("test-reason")).unwrap();
    h2.check_disconnect(&u1, Some("test-reason")).unwrap();

    let s1 = t1.dump_network_stats().await.unwrap();
    println!("after disconnect: {s1:#?}");

    assert!(s1.transport_stats.connections.is_empty());
}

/// A [`MemTransport`](super::MemTransport) with a space handler, connected
/// to a second transport by a sent space notification.
struct ConnectedPair {
    h1: Arc<TrackHnd>,
    imp1: Arc<super::MemTransport>,
    t1: DynTransport,
    s1: Arc<TrackHnd>,
    /// Kept alive so the connection to it stays open.
    _t2: DynTransport,
    u2: Url,
}

/// Build a [`ConnectedPair`] whose connection is already established.
async fn connected_pair() -> ConnectedPair {
    let h1 = TrackHnd::new();
    let hnd1 = TxImpHnd::new(h1.clone());
    let imp1 = super::MemTransport::create(hnd1.clone()).await;
    let t1 = DefaultTransport::create(&hnd1, imp1.clone());
    let s1 = TrackHnd::new();
    t1.register_space_handler(TEST_SPACE_ID, s1.clone());

    let h2 = TrackHnd::new();
    let t2 = gen_tx(h2.clone()).await;
    t2.register_space_handler(TEST_SPACE_ID, h2.clone());
    let u1 = h1.url();
    let u2 = h2.url();

    t1.send_space_notify(
        u2.clone(),
        TEST_SPACE_ID,
        bytes::Bytes::from_static(b"hello"),
    )
    .await
    .unwrap();
    h2.check_connect(&u1).unwrap();

    ConnectedPair {
        h1,
        imp1,
        t1,
        s1,
        _t2: t2,
        u2,
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn transport_close_waits_for_connections_to_drop() {
    let ConnectedPair {
        h1,
        imp1,
        t1,
        s1,
        _t2,
        u2,
    } = connected_pair().await;

    // Neither of two concurrent closes may return before the connection has
    // been dropped and the local space handler has heard about it.
    let close_and_check = || async {
        imp1.close().await;
        assert!(
            has_disconnect(&s1, &u2),
            "space handler must see the disconnect before close returns: {s1:?}"
        );
    };
    tokio::join!(close_and_check(), close_and_check());

    // A close after a completed close is a no-op that returns promptly.
    tokio::time::timeout(std::time::Duration::from_secs(1), imp1.close())
        .await
        .expect("repeated close must return promptly");

    // Once the transport is closed too, nothing references our handlers,
    // even though `t1` itself is still alive.
    t1.close().await;
    kitsune2_test_utils::retry_fn_until_timeout(
        || async { Arc::strong_count(&h1) == 1 && Arc::strong_count(&s1) == 1 },
        Some(5000),
        Some(50),
    )
    .await
    .expect("handlers must be released after close");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn transport_close_after_cancelled_close_finishes_the_close() {
    use std::sync::atomic::{AtomicBool, Ordering};

    let ConnectedPair {
        imp1, s1, _t2, u2, ..
    } = connected_pair().await;

    // A task that blocks its thread cannot be aborted until it yields, so
    // the first close is still shutting it down when it gets cancelled. The
    // task blocks until the test releases it after that cancelled close.
    let (started_send, started_recv) = tokio::sync::oneshot::channel();
    let (release_send, release_recv) = std::sync::mpsc::channel::<()>();
    let finished = Arc::new(AtomicBool::new(false));
    imp1.task_list.lock().expect("poison").spawn({
        let finished = finished.clone();
        async move {
            started_send.send(()).unwrap();
            release_recv.recv().expect("release sender dropped");
            finished.store(true, Ordering::SeqCst);
            std::future::pending::<()>().await;
        }
    });
    started_recv.await.unwrap();

    tokio::time::timeout(std::time::Duration::from_millis(50), imp1.close())
        .await
        .expect_err("the first close must be cancelled mid shutdown");

    release_send.send(()).unwrap();
    imp1.close().await;
    assert!(
        finished.load(Ordering::SeqCst),
        "close must wait for the task the cancelled close was shutting down"
    );
    assert!(
        has_disconnect(&s1, &u2),
        "space handler must see the disconnect once close returns: {s1:?}"
    );
    assert!(
        imp1.task_list.lock().expect("poison").is_empty(),
        "no task may survive the close"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn transport_close_notifies_space_handlers_of_disconnects() {
    let ConnectedPair {
        t1, s1, _t2, u2, ..
    } = connected_pair().await;

    // Closing the transport, not just the imp, must still tell local space
    // handlers about the connections it drops.
    t1.close().await;
    assert!(
        has_disconnect(&s1, &u2),
        "space handler must see the disconnect before close returns: {s1:?}"
    );
}

/// Whether `handler` recorded a disconnect from `peer`, for any reason.
fn has_disconnect(handler: &TrackHnd, peer: &Url) -> bool {
    handler
        .track
        .lock()
        .expect("poison")
        .iter()
        .any(|t| matches!(t, Track::Disconnect(u, _) if u == peer))
}

#[tokio::test(flavor = "multi_thread")]
async fn transport_preflight_happy() {
    use std::sync::atomic::*;
    let count = Arc::new(AtomicUsize::new(0));
    let count2 = count.clone();
    let h = TrackHnd::new_preflight(
        Box::new(|_| Ok(bytes::Bytes::from_static(b"preflight"))),
        Box::new(move |_, v| {
            if &v[..] == b"preflight" {
                count2.fetch_add(1, Ordering::SeqCst);
            }
            Ok(())
        }),
    );

    let t1 = gen_tx(h.clone()).await;
    t1.register_space_handler(TEST_SPACE_ID, h.clone());
    let u = h.url();

    let t2 = gen_tx(h.clone()).await;
    t2.register_space_handler(TEST_SPACE_ID, h.clone());

    t2.send_space_notify(u, TEST_SPACE_ID, bytes::Bytes::from_static(b"hello"))
        .await
        .unwrap();

    // ... this is lame, but whatever
    for _ in 0..5 {
        if count.load(Ordering::SeqCst) == 2 {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await
    }

    panic!("Didn't get preflights in both directions");
}

#[tokio::test(flavor = "multi_thread")]
async fn transport_preflight_reject() {
    let h1 = TrackHnd::new_preflight(
        Box::new(|_| Ok(bytes::Bytes::from_static(b"preflight"))),
        Box::new(move |_, _| Err(K2Error::other("test-error"))),
    );
    let h2 = TrackHnd::new_preflight(
        Box::new(|_| Ok(bytes::Bytes::from_static(b"preflight"))),
        Box::new(move |_, _| Err(K2Error::other("test-error"))),
    );

    let t1 = gen_tx(h1.clone()).await;
    t1.register_space_handler(TEST_SPACE_ID, h1.clone());
    let u1 = h1.url();

    let t2 = gen_tx(h2.clone()).await;
    t2.register_space_handler(TEST_SPACE_ID, h2.clone());
    let u2 = h2.url();

    t1.send_space_notify(
        u2,
        TEST_SPACE_ID,
        bytes::Bytes::from_static(b"hello"),
    )
    .await
    .unwrap();

    // ... this is lame, but whatever
    for _ in 0..5 {
        if h2.check_disconnect(&u1, Some("test-error")).is_ok() {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await
    }

    panic!("expected disconnect in reasonable time");
}

#[tokio::test(flavor = "multi_thread")]
async fn preflight_before_other() {
    let h1 = TrackHnd::new();
    let t1 = gen_tx(h1.clone()).await;
    t1.register_space_handler(TEST_SPACE_ID, h1.clone());
    t1.register_module_handler(TEST_SPACE_ID, "test".into(), h1.clone());
    let u1 = h1.url();

    let h2 = TrackHnd::new();
    let t2 = gen_tx(h2.clone()).await;
    t2.register_space_handler(TEST_SPACE_ID, h2.clone());
    t2.register_module_handler(TEST_SPACE_ID, "test".into(), h2.clone());
    let u2 = h2.url();

    t1.send_space_notify(
        u2.clone(),
        TEST_SPACE_ID,
        bytes::Bytes::from_static(b"hello"),
    )
    .await
    .unwrap();

    t2.send_space_notify(
        u1.clone(),
        TEST_SPACE_ID,
        bytes::Bytes::from_static(b"world"),
    )
    .await
    .unwrap();

    t1.send_module(
        u2.clone(),
        TEST_SPACE_ID,
        "test".into(),
        bytes::Bytes::from_static(b"hello"),
    )
    .await
    .unwrap();

    t2.send_module(
        u1.clone(),
        TEST_SPACE_ID,
        "test".into(),
        bytes::Bytes::from_static(b"world"),
    )
    .await
    .unwrap();

    h1.check_preflight_before_other_messages();
    h2.check_preflight_before_other_messages();
}
