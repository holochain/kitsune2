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

#[derive(Debug)]
struct TrackHnd(Mutex<Vec<Track>>);

impl TxBaseHandler for TrackHnd {
    fn new_listening_address(&self, this_url: Url) {
        self.0.lock().unwrap().push(Track::ThisUrl(this_url));
    }

    fn peer_connect(&self, peer: Url) -> K2Result<()> {
        self.0.lock().unwrap().push(Track::Connect(peer));
        Ok(())
    }

    fn peer_disconnect(&self, peer: Url, reason: Option<String>) {
        self.0.lock().unwrap().push(Track::Disconnect(peer, reason));
    }
}

impl TxHandler for TrackHnd {}

impl TxSpaceHandler for TrackHnd {
    fn recv_space_notify(&self, peer: Url, space: SpaceId, data: bytes::Bytes) {
        self.0
            .lock()
            .unwrap()
            .push(Track::SpaceRecv(peer, space, data));
    }
}

impl TxModuleHandler for TrackHnd {
    fn recv_module(
        &self,
        peer: Url,
        space: SpaceId,
        module: String,
        data: bytes::Bytes,
    ) {
        self.0
            .lock()
            .unwrap()
            .push(Track::ModRecv(peer, space, module, data));
    }
}

impl TrackHnd {
    pub fn new() -> Arc<Self> {
        Arc::new(Self(Mutex::new(Vec::new())))
    }

    pub fn url(&self) -> Url {
        for t in self.0.lock().unwrap().iter() {
            if let Track::ThisUrl(url) = t {
                return url.clone();
            }
        }
        panic!("no url found");
    }

    pub fn assert_connect(&self, peer: &Url) {
        for t in self.0.lock().unwrap().iter() {
            if let Track::Connect(u) = t {
                if u == peer {
                    return;
                }
            }
        }
        panic!(
            "matching connect entry not found {peer}, {:#?}",
            self.0.lock().unwrap()
        );
    }

    pub fn assert_disconnect(&self, peer: &Url, reason: Option<&str>) {
        for t in self.0.lock().unwrap().iter() {
            if let Track::Disconnect(u, r) = t {
                if u != peer {
                    continue;
                }
                if let Some(reason) = reason {
                    if let Some(r) = r {
                        if r.contains(reason) {
                            return;
                        }
                    }
                }
            }
        }
        panic!("matching disconnect entry not found");
    }

    pub fn assert_notify(&self, peer: &Url, space: &SpaceId, msg: &[u8]) {
        for t in self.0.lock().unwrap().iter() {
            if let Track::SpaceRecv(u, s, d) = t {
                if u == peer && space == s && &d[..] == msg {
                    return;
                }
            }
        }
        panic!("matching notify not found");
    }

    pub fn assert_mod(
        &self,
        peer: &Url,
        space: &SpaceId,
        module: &str,
        msg: &[u8],
    ) {
        for t in self.0.lock().unwrap().iter() {
            if let Track::ModRecv(u, s, m, d) = t {
                if u == peer && space == s && module == m && &d[..] == msg {
                    return;
                }
            }
        }
        panic!("matching mod msg not found");
    }
}

async fn gen_tx(hnd: DynTxHandler) -> Transport {
    let builder = Arc::new(crate::default_builder());
    let hnd = TxImpHnd::new(hnd);
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

    h1.assert_connect(&u2);
    h2.assert_connect(&u1);
    h1.assert_notify(&u2, &S1, b"world");
    h2.assert_notify(&u1, &S1, b"hello");
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

    h1.assert_connect(&u2);
    h2.assert_connect(&u1);
    h1.assert_mod(&u2, &S1, "test", b"world");
    h2.assert_mod(&u1, &S1, "test", b"hello");
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

    h2.assert_connect(&u1);

    t1.disconnect(u2.clone(), Some("test-reason".into())).await;

    h1.assert_disconnect(&u2, Some("test-reason"));
    h2.assert_disconnect(&u1, Some("test-reason"));
}
