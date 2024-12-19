use kitsune2_api::{transport::*, *};
use std::sync::{Arc, Mutex};

const S1: SpaceId = SpaceId(id::Id(bytes::Bytes::from_static(b"space-1")));

#[derive(Debug)]
enum Track {
    ThisUrl(Url),
    Connect(Url),
    Disconnect(Url, Option<String>),
    SpaceRecv(Url, SpaceId, bytes::Bytes),
    ModRecv(Url, SpaceId, String, bytes::Bytes),
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
    fn new_listening_address(&self, this_url: Url) {
        self.track.lock().unwrap().push(Track::ThisUrl(this_url));
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
    ) -> K2Result<bytes::Bytes> {
        (self.preflight_gather_outgoing)(peer_url)
    }

    fn preflight_validate_incoming(
        &self,
        peer_url: Url,
        data: bytes::Bytes,
    ) -> K2Result<()> {
        (self.preflight_validate_incoming)(peer_url, data)
    }
}

impl TxSpaceHandler for TrackHnd {
    fn recv_space_notify(
        &self,
        peer: Url,
        space: SpaceId,
        data: bytes::Bytes,
    ) -> K2Result<()> {
        self.track
            .lock()
            .unwrap()
            .push(Track::SpaceRecv(peer, space, data));
        Ok(())
    }
}

impl TxModuleHandler for TrackHnd {
    fn recv_module_msg(
        &self,
        peer: Url,
        space: SpaceId,
        module: String,
        data: bytes::Bytes,
    ) -> K2Result<()> {
        self.track
            .lock()
            .unwrap()
            .push(Track::ModRecv(peer, space, module, data));
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
            if let Track::Connect(u) = t {
                if u == peer {
                    return Ok(());
                }
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
                if let Some(reason) = reason {
                    if let Some(r) = r {
                        if r.contains(reason) {
                            return Ok(());
                        }
                    }
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
        space: &SpaceId,
        msg: &[u8],
    ) -> K2Result<()> {
        for t in self.track.lock().unwrap().iter() {
            if let Track::SpaceRecv(u, s, d) = t {
                if u == peer && space == s && &d[..] == msg {
                    return Ok(());
                }
            }
        }
        Err(K2Error::other(format!(
            "matching notify not found {peer} {space} {}, out of {:#?}",
            String::from_utf8_lossy(msg),
            self.track.lock().unwrap(),
        )))
    }

    pub fn check_mod(
        &self,
        peer: &Url,
        space: &SpaceId,
        module: &str,
        msg: &[u8],
    ) -> K2Result<()> {
        for t in self.track.lock().unwrap().iter() {
            if let Track::ModRecv(u, s, m, d) = t {
                if u == peer && space == s && module == m && &d[..] == msg {
                    return Ok(());
                }
            }
        }
        Err(K2Error::other(format!(
            "matching mod not found {peer} {space} {module} {}, out of {:#?}",
            String::from_utf8_lossy(msg),
            self.track.lock().unwrap(),
        )))
    }
}

async fn gen_tx(hnd: DynTxHandler) -> DynTransport {
    let builder = Arc::new(crate::default_builder());
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
    t1.register_space_handler(S1.clone(), h1.clone());
    let u1 = h1.url();

    let h2 = TrackHnd::new();
    let t2 = gen_tx(h2.clone()).await;
    t2.register_space_handler(S1.clone(), h2.clone());
    let u2 = h2.url();

    t1.send_space_notify(
        u2.clone(),
        S1.clone(),
        bytes::Bytes::from_static(b"hello"),
    )
    .await
    .unwrap();

    t2.send_space_notify(
        u1.clone(),
        S1.clone(),
        bytes::Bytes::from_static(b"world"),
    )
    .await
    .unwrap();

    h1.check_connect(&u2).unwrap();
    h2.check_connect(&u1).unwrap();
    h1.check_notify(&u2, &S1, b"world").unwrap();
    h2.check_notify(&u1, &S1, b"hello").unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn transport_module() {
    let h1 = TrackHnd::new();
    let t1 = gen_tx(h1.clone()).await;
    t1.register_module_handler(S1.clone(), "test".into(), h1.clone());
    let u1 = h1.url();

    let h2 = TrackHnd::new();
    let t2 = gen_tx(h2.clone()).await;
    t2.register_module_handler(S1.clone(), "test".into(), h2.clone());
    let u2 = h2.url();

    t1.send_module(
        u2.clone(),
        S1.clone(),
        "test".into(),
        bytes::Bytes::from_static(b"hello"),
    )
    .await
    .unwrap();

    t2.send_module(
        u1.clone(),
        S1.clone(),
        "test".into(),
        bytes::Bytes::from_static(b"world"),
    )
    .await
    .unwrap();

    h1.check_connect(&u2).unwrap();
    h2.check_connect(&u1).unwrap();
    h1.check_mod(&u2, &S1, "test", b"world").unwrap();
    h2.check_mod(&u1, &S1, "test", b"hello").unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn transport_disconnect() {
    let h1 = TrackHnd::new();
    let t1 = gen_tx(h1.clone()).await;
    t1.register_space_handler(S1.clone(), h1.clone());
    let u1 = h1.url();

    let h2 = TrackHnd::new();
    let t2 = gen_tx(h2.clone()).await;
    t2.register_space_handler(S1.clone(), h2.clone());
    let u2 = h2.url();

    t1.send_space_notify(
        u2.clone(),
        S1.clone(),
        bytes::Bytes::from_static(b"hello"),
    )
    .await
    .unwrap();

    h2.check_connect(&u1).unwrap();

    t1.disconnect(u2.clone(), Some("test-reason".into())).await;

    h1.check_disconnect(&u2, Some("test-reason")).unwrap();
    h2.check_disconnect(&u1, Some("test-reason")).unwrap();
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

    let _t1 = gen_tx(h.clone()).await;
    let u = h.url();

    let t2 = gen_tx(h.clone()).await;

    t2.send_space_notify(u, S1.clone(), bytes::Bytes::from_static(b"hello"))
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
    let u1 = h1.url();

    let _t2 = gen_tx(h2.clone()).await;
    let u2 = h2.url();

    t1.send_space_notify(u2, S1.clone(), bytes::Bytes::from_static(b"hello"))
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
