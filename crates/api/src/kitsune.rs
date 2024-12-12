//! Kitsune2 top-level module related types.

use crate::*;
use std::sync::Arc;

/// Handler for events coming out of Kitsune2.
pub trait KitsuneHandler: 'static + Send + Sync + std::fmt::Debug {
    /// Gather preflight data to send to a new opening connection.
    /// Returning an Err result will close this connection.
    ///
    /// The default implementation sends an empty preflight message.
    fn preflight_gather_outgoing(
        &self,
        peer_url: Url,
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
        peer_url: Url,
        data: bytes::Bytes,
    ) -> K2Result<()> {
        drop(peer_url);
        drop(data);
        Ok(())
    }

    /// Kitsune would like to construct a space. Provide a handler.
    fn create_space(
        &self,
        space: SpaceId,
    ) -> BoxFut<'_, K2Result<space::DynSpaceHandler>>;
}

/// Trait-object [KitsuneHandler].
pub type DynKitsuneHandler = Arc<dyn KitsuneHandler>;

/// The top-level Kitsune2 api trait.
pub trait Kitsune: 'static + Send + Sync + std::fmt::Debug {
    /// Get an existing space with the provided [SpaceId] or create
    /// a new one.
    fn space(&self, space: SpaceId) -> BoxFut<'_, K2Result<space::DynSpace>>;
}

/// Trait-object [Kitsune].
pub type DynKitsune = Arc<dyn Kitsune>;

/// A factory for constructing Kitsune instances.
pub trait KitsuneFactory: 'static + Send + Sync + std::fmt::Debug {
    /// Help the builder construct a default config from the chosen
    /// module factories.
    fn default_config(&self, config: &mut config::Config) -> K2Result<()>;

    /// Construct a space instance.
    fn create(
        &self,
        builder: Arc<builder::Builder>,
        handler: DynKitsuneHandler,
    ) -> BoxFut<'static, K2Result<DynKitsune>>;
}

/// Trait-object [KitsuneFactory].
pub type DynKitsuneFactory = Arc<dyn KitsuneFactory>;
