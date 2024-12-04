//! Kitsune2 space related types.

use crate::*;
use std::sync::Arc;

/*
/// Handler for .
pub trait TransportHandler: 'static + Send + Sync + std::fmt::Debug {
}

/// Trait-object [TransportHandler].
pub type DynTransportHandler = Arc<dyn TransportHandler>;
*/

/// Represents a unique dht space within which to communicate with peers.
pub trait Transport: 'static + Send + Sync + std::fmt::Debug {
    /// Notify a remote peer within a space. This is a fire-and-forget
    /// type message. The future this call returns will indicate any errors
    /// that occur up to the point where the message is handed off to
    /// the transport backend. After that, the future will return `Ok(())`
    /// but the remote peer may or may not actually receive the message.
    fn send_space_notify(
        &self,
        space: SpaceId,
        data: bytes::Bytes,
    ) -> BoxFut<'_, K2Result<()>>;
}

/// Trait-object [Transport].
pub type DynTransport = Arc<dyn Transport>;

/// A factory for constructing Transport instances.
pub trait TransportFactory: 'static + Send + Sync + std::fmt::Debug {
    /// Help the builder construct a default config from the chosen
    /// module factories.
    fn default_config(&self, config: &mut config::Config) -> K2Result<()>;

    /// Construct a transport instance.
    fn create(
        &self,
        builder: Arc<builder::Builder>,
    ) -> BoxFut<'static, K2Result<DynTransport>>;
}

/// Trait-object [TransportFactory].
pub type DynTransportFactory = Arc<dyn TransportFactory>;
