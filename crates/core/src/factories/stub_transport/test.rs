use kitsune2_api::{transport::*, *};
use std::sync::{Arc, Mutex};

const S1: SpaceId = SpaceId(id::Id(bytes::Bytes::from_static(b"space-1")));

#[derive(Debug)]
enum Track {
    ThisUrl(Url),
    SpaceRecv(Url, SpaceId, bytes::Bytes),
}

#[derive(Debug)]
struct TrackHnd(Mutex<Vec<Track>>);

impl TxBaseHandler for TrackHnd {
    fn new_listening_address(&self, this_url: Url) {
        self.0.lock().unwrap().push(Track::ThisUrl(this_url));
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

impl TrackHnd {
    pub fn new() -> Arc<Self> {
        Arc::new(Self(Mutex::new(Vec::new())))
    }

    pub fn next(&self) -> Track {
        self.0.lock().unwrap().remove(0)
    }

    pub fn take_url(&self) -> Url {
        let mut lock = self.0.lock().unwrap();
        loop {
            if let Track::ThisUrl(url) = lock.remove(0) {
                return url;
            }
        }
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
async fn happy_transport_notify() {
    let h1 = TrackHnd::new();
    let t1 = gen_tx(h1.clone()).await;
    t1.register_space_handler(S1.clone(), h1.clone());
    let u1 = h1.take_url();

    let h2 = TrackHnd::new();
    let t2 = gen_tx(h2.clone()).await;
    t2.register_space_handler(S1.clone(), h2.clone());
    let u2 = h2.take_url();

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

    match h1.next() {
        Track::SpaceRecv(peer, space, data)
            if peer == u2 && space == S1 && &data[..] == b"world" => {}
        oth => panic!("unexpected: {:?}", oth),
    };

    match h2.next() {
        Track::SpaceRecv(peer, space, data)
            if peer == u1 && space == S1 && &data[..] == b"hello" => {}
        oth => panic!("unexpected: {:?}", oth),
    };
}
