//! Kitsune2 space related types.

use crate::*;
use std::sync::Arc;

/// Handler for whole transport-level events.
pub trait TxHandler: 'static + Send + Sync + std::fmt::Debug {
    /// Gather preflight data to send to a new opening connection.
    /// Returning an Err result will close this connection.
    ///
    /// The default implementation sends an empty preflight message.
    fn preflight_gather_outgoing(
        &self,
        peer_url: String,
    ) -> K2Result<bytes::Bytes> {
        drop(peer_url);
        Ok(bytes::Bytes::new())
    }

    /// Validate preflight data sent by a remote peer on a new connection.
    /// Returning an Err result will close this connection.
    ///
    /// The default implementation ignores the preflight data,
    /// and considers it valid.
    fn preflight_validate_incoming(
        &self,
        peer_url: String,
        data: bytes::Bytes,
    ) -> K2Result<()> {
        drop(peer_url);
        drop(data);
        Ok(())
    }
}

/// Trait-object [TxHandler].
pub type DynTxHandler = Arc<dyn TxHandler>;

/// Handler for space-related events.
pub trait TxSpaceHandler: 'static + Send + Sync + std::fmt::Debug {
    /// The sync handler for receiving notifications sent by a remote
    /// peer in reference to a particular space. Responding with an
    /// error to this call will result in the handler being unregistered
    /// and never again invoked.
    fn recv_space_notify(
        &self,
        peer: Url,
        space: SpaceId,
        data: bytes::Bytes,
    ) -> K2Result<()>;
}

/// Trait-object [TxSpaceHandler].
pub type DynTxSpaceHandler = Arc<dyn TxSpaceHandler>;

/// Handler for module-related events.
pub trait TxModuleHandler: 'static + Send + Sync + std::fmt::Debug {
    /// The sync handler for receiving module messages sent by a remote
    /// peer in reference to a particular space. Responding with an
    /// error to this call will result in the handler being unregistered
    /// and never again invoked.
    fn recv_module(
        &self,
        peer: Url,
        space: SpaceId,
        module: String,
        data: bytes::Bytes,
    ) -> K2Result<()>;
}

/// Trait-object [TxModuleHandler].
pub type DynTxModuleHandler = Arc<dyn TxModuleHandler>;

/// Represents a unique dht space within which to communicate with peers.
pub trait Transport: 'static + Send + Sync + std::fmt::Debug {
    /// Register a space handler for receiving incoming notifications.
    fn register_space_handler(
        &self,
        space: SpaceId,
        handler: DynTxSpaceHandler,
    );

    /// Register a module handler for receiving incoming module messages.
    fn register_module_handler(
        &self,
        space: SpaceId,
        module: String,
        handler: DynTxModuleHandler,
    );

    /// Make a best effort to notify a peer that we are disconnecting and why.
    /// After a short time out, the connection will be closed even if the
    /// disconnect reason message is still pending.
    fn disconnect(&self, reason: String) -> BoxFut<'_, ()>;

    /// Notify a remote peer within a space. This is a fire-and-forget
    /// type message. The future this call returns will indicate any errors
    /// that occur up to the point where the message is handed off to
    /// the transport backend. After that, the future will return `Ok(())`
    /// but the remote peer may or may not actually receive the message.
    fn send_space_notify(
        &self,
        peer: Url,
        space: SpaceId,
        data: bytes::Bytes,
    ) -> BoxFut<'_, K2Result<()>>;

    /// Notify a remote peer module within a space. This is a fire-and-forget
    /// type message. The future this call returns will indicate any errors
    /// that occur up to the point where the message is handed off to
    /// the transport backend. After that, the future will return `Ok(())`
    /// but the remote peer may or may not actually receive the message.
    fn send_module(
        &self,
        peer: Url,
        space: SpaceId,
        module: String,
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
        handler: DynTxHandler,
    ) -> BoxFut<'static, K2Result<DynTransport>>;
}

/// Trait-object [TransportFactory].
pub type DynTransportFactory = Arc<dyn TransportFactory>;
