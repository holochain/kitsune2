//! tx5 transport module test utilities

use std::sync::Arc;

use kitsune2_api::{
    BoxFut, Builder, DynTransport, DynTxHandler, K2Result, SpaceId, Timestamp,
    TxBaseHandler, TxHandler, TxModuleHandler, TxSpaceHandler, Url,
};

use crate::{Tx5TransportFactory, config};

/// Test harness for the transport_tx5 module
pub struct Tx5TransportTestHarness {
    /// sbd server
    pub srv: Option<sbd_server::SbdServer>,
    /// server port
    pub port: u16,
    /// kitsune2 builder
    pub builder: Arc<Builder>,
}

impl Tx5TransportTestHarness {
    /// Create a new test harness
    pub async fn new(
        auth_material: Option<Vec<u8>>,
        tx5_timeout_s: Option<u32>,
    ) -> Self {
        let mut this = Self {
            srv: None,
            port: 0,
            builder: Arc::new(kitsune2_core::default_test_builder()),
        };

        // Note the `port: 0` above, so we get a free port the first time.
        // This restart function will set the port to the actual value.
        this.restart().await;

        let builder = Builder {
            auth_material,
            transport: Tx5TransportFactory::create(),
            ..kitsune2_core::default_test_builder()
        }
        .with_default_config()
        .unwrap();

        builder
            .config
            .set_module_config(&config::Tx5TransportModConfig {
                tx5_transport: config::Tx5TransportConfig {
                    signal_allow_plain_text: true,
                    server_url: format!("ws://127.0.0.1:{}", this.port),
                    timeout_s: tx5_timeout_s.unwrap_or(60),
                    ..Default::default()
                },
            })
            .unwrap();

        this.builder = Arc::new(builder);

        this
    }

    /// Restart the sbd server, but re-use the port we first got in our
    /// constructor so that already configured transports being tested
    /// will be able to find the server automatically again.
    ///
    /// There is a small chance something else could grab the port
    /// in the mean time, and this will error/flake.
    pub async fn restart(&mut self) {
        std::mem::drop(self.srv.take());

        let mut srv = None;

        let mut wait_ms = 250;
        for _ in 0..5 {
            srv = sbd_server::SbdServer::new(Arc::new(sbd_server::Config {
                bind: vec![format!("127.0.0.1:{}", self.port)],
                limit_clients: 100,
                disable_rate_limiting: true,
                ..Default::default()
            }))
            .await
            .ok();

            if srv.is_some() {
                break;
            }

            // allow time for the original port to be cleaned up
            tokio::time::sleep(std::time::Duration::from_millis(wait_ms)).await;
            wait_ms *= 2;
        }

        if srv.is_none() {
            panic!("could not start sbd server on port {}", self.port);
        }

        self.port = srv.as_ref().unwrap().bind_addrs().first().unwrap().port();
        self.srv = srv;
    }

    /// Build a Transport trait implementation
    pub async fn build_transport(&self, handler: DynTxHandler) -> DynTransport {
        self.builder
            .transport
            .create(self.builder.clone(), handler)
            .await
            .unwrap()
    }
}

/// A mock handler that implements the various TxHandler traits
pub struct MockTxHandler {
    /// Mock function to implement the new_listening_address() method of the
    /// TxBaseHandler trait
    pub new_addr: Arc<dyn Fn(Url) + 'static + Send + Sync>,
    /// Mock function to implement the peer_connection() method of the
    /// TxBaseHandler trait
    pub peer_con: Arc<dyn Fn(Url) -> K2Result<()> + 'static + Send + Sync>,
    /// Mock function to implement the peer_disconnection() method of the
    /// TxBaseHandler trait
    pub peer_dis: Arc<dyn Fn(Url, Option<String>) + 'static + Send + Sync>,
    /// Mock function to implement the preflight_gather_outgoing() method of
    /// the TxHandler trait
    pub pre_out:
        Arc<dyn Fn(Url) -> K2Result<bytes::Bytes> + 'static + Send + Sync>,
    /// Mock function to implement the preflight_validate_incoming() method of
    /// the TxHandler trait
    pub pre_in:
        Arc<dyn Fn(Url, bytes::Bytes) -> K2Result<()> + 'static + Send + Sync>,
    /// Mock function to implement the recv_space_notify() method of the
    /// TxSpaceHandler trait
    pub recv_space_not: Arc<
        dyn Fn(Url, SpaceId, bytes::Bytes) -> K2Result<()>
            + 'static
            + Send
            + Sync,
    >,
    /// Mock function to implement the set_unresponsive() method of the
    /// TxSpaceHandler trait
    pub set_unresp:
        Arc<dyn Fn(Url, Timestamp) -> K2Result<()> + 'static + Send + Sync>,
    /// Mock function to implement the recv_module_msg() method of the
    /// TxModuleHandler trait
    pub recv_mod_msg: Arc<
        dyn Fn(Url, SpaceId, String, bytes::Bytes) -> K2Result<()>
            + 'static
            + Send
            + Sync,
    >,
    /// Mock function to implement the [`TxSpaceHandler::is_any_agent_at_url_blocked()`] method.
    #[allow(clippy::type_complexity)]
    pub url_blocked:
        Arc<dyn Fn(&Url) -> K2Result<bool> + 'static + Send + Sync>,
}

impl std::fmt::Debug for MockTxHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MockTxHandler").finish()
    }
}

impl Default for MockTxHandler {
    fn default() -> Self {
        Self {
            new_addr: Arc::new(|_| {}),
            peer_con: Arc::new(|_| Ok(())),
            peer_dis: Arc::new(|_, _| {}),
            pre_out: Arc::new(|_| Ok(bytes::Bytes::new())),
            pre_in: Arc::new(|_, _| Ok(())),
            recv_space_not: Arc::new(|_, _, _| Ok(())),
            set_unresp: Arc::new(|_, _| Ok(())),
            recv_mod_msg: Arc::new(|_, _, _, _| Ok(())),
            url_blocked: Arc::new(|_| Ok(false)),
        }
    }
}

impl TxBaseHandler for MockTxHandler {
    fn new_listening_address(&self, this_url: Url) -> BoxFut<'static, ()> {
        (self.new_addr)(this_url);
        Box::pin(async {})
    }

    fn peer_connect(&self, peer: Url) -> K2Result<()> {
        (self.peer_con)(peer)
    }

    fn peer_disconnect(&self, peer: Url, reason: Option<String>) {
        (self.peer_dis)(peer, reason)
    }
}

impl TxHandler for MockTxHandler {
    fn preflight_gather_outgoing(
        &self,
        peer_url: Url,
    ) -> BoxFut<'_, K2Result<bytes::Bytes>> {
        Box::pin(async { (self.pre_out)(peer_url) })
    }

    fn preflight_validate_incoming(
        &self,
        peer_url: Url,
        data: bytes::Bytes,
    ) -> BoxFut<'_, K2Result<()>> {
        Box::pin(async { (self.pre_in)(peer_url, data) })
    }
}

impl TxSpaceHandler for MockTxHandler {
    fn recv_space_notify(
        &self,
        peer: Url,
        space_id: SpaceId,
        data: bytes::Bytes,
    ) -> K2Result<()> {
        (self.recv_space_not)(peer, space_id, data)
    }

    fn set_unresponsive(
        &self,
        peer: Url,
        when: Timestamp,
    ) -> BoxFut<'_, K2Result<()>> {
        Box::pin(async move { (self.set_unresp)(peer, when) })
    }

    fn is_any_agent_at_url_blocked(&self, peer_url: &Url) -> K2Result<bool> {
        (self.url_blocked)(peer_url)
    }
}

impl TxModuleHandler for MockTxHandler {
    fn recv_module_msg(
        &self,
        peer: Url,
        space_id: SpaceId,
        module: String,
        data: bytes::Bytes,
    ) -> K2Result<()> {
        (self.recv_mod_msg)(peer, space_id, module, data)
    }
}
