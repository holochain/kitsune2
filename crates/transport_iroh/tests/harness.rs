//! iroh transport module test utilities

use iroh_relay::server::Server;
use kitsune2_api::{
    BoxFut, Builder, DynTransport, DynTxHandler, K2Result, SpaceId, Timestamp,
    TxBaseHandler, TxHandler, TxModuleHandler, TxSpaceHandler, Url,
};
use kitsune2_test_utils::iroh_relay::spawn_iroh_relay_server;
use kitsune2_transport_iroh::{
    IrohTransportConfig, IrohTransportFactory, IrohTransportModConfig,
};
use std::sync::{Arc, Mutex};

/// Test harness for the transport_tx5 module
pub struct IrohTransportTestHarness {
    /// iroh relay server
    pub _relay_server: Server,
    /// kitsune2 builder
    pub builder: Arc<Builder>,
}

impl IrohTransportTestHarness {
    /// Create a new test harness.
    pub async fn new() -> Self {
        let builder = Builder {
            transport: IrohTransportFactory::create(),
            ..kitsune2_core::default_test_builder()
        }
        .with_default_config()
        .unwrap();
        let relay_server = spawn_iroh_relay_server().await;
        let relay_url = format!("http://{}", relay_server.http_addr().unwrap());
        builder
            .config
            .set_module_config(&IrohTransportModConfig {
                iroh_transport: IrohTransportConfig {
                    relay_url: Some(relay_url),
                    relay_allow_plain_text: true,
                },
            })
            .unwrap();
        let builder = Arc::new(builder);
        Self {
            builder,
            _relay_server: relay_server,
        }
    }

    /// Build a transport instance.
    pub async fn build_transport(&self, handler: DynTxHandler) -> DynTransport {
        self.builder
            .transport
            .create(self.builder.clone(), handler)
            .await
            .unwrap()
    }
}

pub fn dummy_url() -> Url {
    Url::from_str("http://url.not.set:0/0").unwrap()
}

/// A mock handler that implements the various TxHandler traits
pub struct MockTxHandler {
    /// Mock function to implement [`TxBaseHandler::new_listening_address()`]
    pub new_listening_address: Arc<dyn Fn(Url) + 'static + Send + Sync>,
    /// Mock function to implement [`TxBaseHandler::peer_connect()`]
    pub peer_connect: Arc<dyn Fn(Url) -> K2Result<()> + 'static + Send + Sync>,
    /// Mock function to implement [`TxBaseHandler::peer_disconnect()`]
    pub peer_disconnect:
        Arc<dyn Fn(Url, Option<String>) + 'static + Send + Sync>,
    /// Mock function to implement [`TxHandler::preflight_gather_outgoing()`]
    pub preflight_gather_outgoing:
        Arc<dyn Fn(Url) -> K2Result<bytes::Bytes> + 'static + Send + Sync>,
    /// Mock function to implement [`TxHandler::preflight_validate_incoming()`]
    pub preflight_validate_incoming:
        Arc<dyn Fn(Url, bytes::Bytes) -> K2Result<()> + 'static + Send + Sync>,
    /// Mock function to implement [`TxSpaceHandler::recv_space_notify()`]
    pub recv_space_notify: Arc<
        dyn Fn(Url, SpaceId, bytes::Bytes) -> K2Result<()>
            + 'static
            + Send
            + Sync,
    >,
    /// Mock function to implement [`TxModuleHandler::recv_module_msg()`]
    pub recv_module_msg: Arc<
        dyn Fn(Url, SpaceId, String, bytes::Bytes) -> K2Result<()>
            + 'static
            + Send
            + Sync,
    >,
    /// Mock function to implement [`TxSpaceHandler::set_unresponsive()`]
    pub set_unresponsive:
        Arc<dyn Fn(Url, Timestamp) -> K2Result<()> + 'static + Send + Sync>,
    /// Mock function to implement [`TxSpaceHandler::are_all_agents_at_url_blocked()`]
    #[allow(clippy::type_complexity)]
    pub are_all_agents_at_url_blocked:
        Arc<dyn Fn(&Url) -> K2Result<bool> + 'static + Send + Sync>,
    /// The current URL of this peer.
    pub current_url: Arc<Mutex<Url>>,
}

impl std::fmt::Debug for MockTxHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MockTxHandler").finish()
    }
}

impl Default for MockTxHandler {
    fn default() -> Self {
        Self {
            new_listening_address: Arc::new(|_| {}),
            peer_connect: Arc::new(|_| Ok(())),
            peer_disconnect: Arc::new(|_, _| {}),
            preflight_gather_outgoing: Arc::new(|_| Ok(bytes::Bytes::new())),
            preflight_validate_incoming: Arc::new(|_, _| Ok(())),
            recv_space_notify: Arc::new(|_, _, _| Ok(())),
            recv_module_msg: Arc::new(|_, _, _, _| Ok(())),
            set_unresponsive: Arc::new(|_, _| Ok(())),
            are_all_agents_at_url_blocked: Arc::new(|_| Ok(false)),
            current_url: Arc::new(Mutex::new(dummy_url())),
        }
    }
}

impl TxBaseHandler for MockTxHandler {
    fn new_listening_address(&self, url: Url) -> BoxFut<'static, ()> {
        (self.new_listening_address)(url.clone());
        *self.current_url.lock().unwrap() = url;
        Box::pin(async {})
    }

    fn peer_connect(&self, peer: Url) -> K2Result<()> {
        (self.peer_connect)(peer)
    }

    fn peer_disconnect(&self, peer: Url, reason: Option<String>) {
        (self.peer_disconnect)(peer, reason)
    }
}

impl TxHandler for MockTxHandler {
    fn preflight_gather_outgoing(
        &self,
        peer_url: Url,
    ) -> BoxFut<'_, K2Result<bytes::Bytes>> {
        Box::pin(async { (self.preflight_gather_outgoing)(peer_url) })
    }

    fn preflight_validate_incoming(
        &self,
        peer_url: Url,
        data: bytes::Bytes,
    ) -> BoxFut<'_, K2Result<()>> {
        Box::pin(async { (self.preflight_validate_incoming)(peer_url, data) })
    }
}

impl TxSpaceHandler for MockTxHandler {
    fn recv_space_notify(
        &self,
        peer: Url,
        space_id: SpaceId,
        data: bytes::Bytes,
    ) -> K2Result<()> {
        (self.recv_space_notify)(peer, space_id, data)
    }

    fn set_unresponsive(
        &self,
        peer: Url,
        when: Timestamp,
    ) -> BoxFut<'_, K2Result<()>> {
        Box::pin(async move { (self.set_unresponsive)(peer, when) })
    }

    fn are_all_agents_at_url_blocked(&self, peer_url: &Url) -> K2Result<bool> {
        (self.are_all_agents_at_url_blocked)(peer_url)
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
        (self.recv_module_msg)(peer, space_id, module, data)
    }
}
