use super::*;
use std::sync::Mutex;

// We don't need or want to test all of tx5 in here... that should be done
// in the tx5 repo. Here we should only test the kitsune2 integration of tx5.
//
// Specifically:
//
// - That new_listening_address is called if the sbd server is restarted
// - That peer connect / disconnect are invoked appropriately.
// - That messages can be send / received.
// - That preflight generation and checking work, which are a little weird
//   because in kitsune2 the check logic is handled in the same recv_data
//   callback, where tx5 handles it as a special callback.

struct Test {
    #[allow(dead_code)]
    pub srv: sbd_server::SbdServer,
    #[allow(dead_code)]
    pub port: u16,
    pub builder: Arc<kitsune2_api::builder::Builder>,
}

impl Test {
    pub async fn new() -> Self {
        let srv = sbd_server::SbdServer::new(Arc::new(sbd_server::Config {
            bind: vec!["127.0.0.1:0".into()],
            limit_clients: 100,
            disable_rate_limiting: true,
            ..Default::default()
        }))
        .await
        .unwrap();

        let port = srv.bind_addrs().first().unwrap().port();

        let builder = kitsune2_api::builder::Builder {
            transport: Tx5TransportFactory::create(),
            ..kitsune2_core::default_test_builder()
        };

        builder
            .config
            .set_module_config(&config::Tx5TransportModConfig {
                tx5_transport: config::Tx5TransportConfig {
                    server_url: format!("ws://127.0.0.1:{port}"),
                },
            })
            .unwrap();

        Self {
            srv,
            port,
            builder: Arc::new(builder),
        }
    }

    pub async fn build_transport(&self, handler: DynTxHandler) -> DynTransport {
        self.builder
            .transport
            .create(self.builder.clone(), handler)
            .await
            .unwrap()
    }
}

struct CbHandler {
    new_addr: Arc<dyn Fn(Url) + 'static + Send + Sync>,
    peer_con: Arc<dyn Fn(Url) -> K2Result<()> + 'static + Send + Sync>,
    peer_dis: Arc<dyn Fn(Url, Option<String>) + 'static + Send + Sync>,
    pre_out: Arc<dyn Fn(Url) -> K2Result<bytes::Bytes> + 'static + Send + Sync>,
    pre_in:
        Arc<dyn Fn(Url, bytes::Bytes) -> K2Result<()> + 'static + Send + Sync>,
    space: Arc<
        dyn Fn(Url, SpaceId, bytes::Bytes) -> K2Result<()>
            + 'static
            + Send
            + Sync,
    >,
    module: Arc<
        dyn Fn(Url, SpaceId, String, bytes::Bytes) -> K2Result<()>
            + 'static
            + Send
            + Sync,
    >,
}

impl std::fmt::Debug for CbHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CbHandler").finish()
    }
}

impl Default for CbHandler {
    fn default() -> Self {
        Self {
            new_addr: Arc::new(|_| {}),
            peer_con: Arc::new(|_| Ok(())),
            peer_dis: Arc::new(|_, _| {}),
            pre_out: Arc::new(|_| Ok(bytes::Bytes::new())),
            pre_in: Arc::new(|_, _| Ok(())),
            space: Arc::new(|_, _, _| Ok(())),
            module: Arc::new(|_, _, _, _| Ok(())),
        }
    }
}

impl TxBaseHandler for CbHandler {
    fn new_listening_address(&self, this_url: Url) {
        (self.new_addr)(this_url);
    }

    fn peer_connect(&self, peer: Url) -> K2Result<()> {
        (self.peer_con)(peer)
    }

    fn peer_disconnect(&self, peer: Url, reason: Option<String>) {
        (self.peer_dis)(peer, reason)
    }
}

impl TxHandler for CbHandler {
    fn preflight_gather_outgoing(
        &self,
        peer_url: Url,
    ) -> K2Result<bytes::Bytes> {
        (self.pre_out)(peer_url)
    }

    fn preflight_validate_incoming(
        &self,
        peer_url: Url,
        data: bytes::Bytes,
    ) -> K2Result<()> {
        (self.pre_in)(peer_url, data)
    }
}

impl TxSpaceHandler for CbHandler {
    fn recv_space_notify(
        &self,
        peer: Url,
        space: SpaceId,
        data: bytes::Bytes,
    ) -> K2Result<()> {
        (self.space)(peer, space, data)
    }
}

impl TxModuleHandler for CbHandler {
    fn recv_module_msg(
        &self,
        peer: Url,
        space: SpaceId,
        module: String,
        data: bytes::Bytes,
    ) -> K2Result<()> {
        (self.module)(peer, space, module, data)
    }
}

const S1: SpaceId = SpaceId(id::Id(bytes::Bytes::from_static(b"space1")));

#[tokio::test(flavor = "multi_thread")]
async fn message_send_recv() {
    let test = Test::new().await;

    let h1 = Arc::new(CbHandler {
        ..Default::default()
    });
    let t1 = test.build_transport(h1.clone()).await;
    t1.register_space_handler(S1, h1.clone());
    t1.register_module_handler(S1, "mod".into(), h1.clone());

    let (s_send, mut s_recv) = tokio::sync::mpsc::unbounded_channel();
    let u2 = Arc::new(Mutex::new(Url::from_str("ws://bla.bla:38/1").unwrap()));
    let u2_2 = u2.clone();
    let h2 = Arc::new(CbHandler {
        new_addr: Arc::new(move |url| {
            *u2_2.lock().unwrap() = url;
        }),
        space: Arc::new(move |url, space, data| {
            let _ = s_send.send((url, space, data));
            Ok(())
        }),
        ..Default::default()
    });
    let t2 = test.build_transport(h2.clone()).await;
    t2.register_space_handler(S1, h2.clone());
    t2.register_module_handler(S1, "mod".into(), h2.clone());

    let u2: Url = u2.lock().unwrap().clone();
    println!("got u2: {}", u2);

    t1.send_space_notify(u2, S1, bytes::Bytes::from_static(b"hello"))
        .await
        .unwrap();

    let r = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        s_recv.recv().await
    })
    .await
    .unwrap()
    .unwrap();

    println!("{r:?}");
    assert_eq!(b"hello", r.2.as_ref())
}
