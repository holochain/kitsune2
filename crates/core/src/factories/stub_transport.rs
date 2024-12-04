//! The core space implementation provided by Kitsune2.

use kitsune2_api::{config::*, transport::*, *};
use std::sync::Arc;

const MOD_NAME: &str = "StubTransport";

/// Configuration parameters for [StubTransportFactory].
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StubTransportConfig {}

impl ModConfig for StubTransportConfig {}

/// The core space implementation provided by Kitsune2.
/// You probably will have no reason to use something other than this.
/// This abstraction is mainly here for testing purposes.
#[derive(Debug)]
pub struct StubTransportFactory {}

impl StubTransportFactory {
    /// Construct a new StubTransportFactory.
    pub fn create() -> DynTransportFactory {
        let out: DynTransportFactory = Arc::new(StubTransportFactory {});
        out
    }
}

impl TransportFactory for StubTransportFactory {
    fn default_config(&self, config: &mut Config) -> K2Result<()> {
        config.add_default_module_config::<StubTransportConfig>(
            MOD_NAME.into(),
        )?;
        Ok(())
    }

    fn create(
        &self,
        builder: Arc<builder::Builder>,
        _handler: DynTxHandler,
    ) -> BoxFut<'static, K2Result<DynTransport>> {
        Box::pin(async move {
            let config = builder
                .config
                .get_module_config::<StubTransportConfig>(MOD_NAME)?;
            let out: DynTransport = Arc::new(StubTransport::new(config));
            Ok(out)
        })
    }
}

#[derive(Debug)]
struct StubTransport {}

impl StubTransport {
    pub fn new(_config: StubTransportConfig) -> Self {
        Self {}
    }
}

impl Transport for StubTransport {
    fn register_space_handler(
        &self,
        _space: SpaceId,
        _handler: DynTxSpaceHandler,
    ) {
    }

    fn register_module_handler(
        &self,
        _space: SpaceId,
        _module: String,
        _handler: DynTxModuleHandler,
    ) {
    }

    fn disconnect(&self, _reason: String) -> BoxFut<'_, ()> {
        Box::pin(async move { todo!() })
    }

    fn send_space_notify(
        &self,
        _peer: Url,
        _space: SpaceId,
        _data: bytes::Bytes,
    ) -> BoxFut<'_, K2Result<()>> {
        Box::pin(async move { todo!() })
    }

    fn send_module(
        &self,
        _peer: Url,
        _space: SpaceId,
        _module: String,
        _data: bytes::Bytes,
    ) -> BoxFut<'_, K2Result<()>> {
        Box::pin(async move { todo!() })
    }
}
