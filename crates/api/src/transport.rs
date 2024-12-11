//! Kitsune2 transport module.

use std::sync::Arc;

use crate::{BoxFut, K2Result, SpaceId, Url};

/// Trait for implementing a transport module for exchanging messages
/// between agents.
pub trait Transport: 'static + Send + Sync + std::fmt::Debug {
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

/// Trait object [Transport].
pub type DynTransport = Arc<dyn Transport>;
