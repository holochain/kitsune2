//! Abstractions for connection operations, enabling unit testing.

use crate::stream::{
    DynIrohRecvStream, DynIrohSendStream, IrohRecvStream, IrohSendStream,
};
use bytes::Bytes;
use iroh::EndpointId;
use kitsune2_api::{BoxFut, K2Error, K2Result};
use std::sync::Arc;

/// Abstraction for connection operations.
///
/// This trait allows injecting mock implementations for testing
/// without requiring async trait syntax.
#[cfg_attr(test, mockall::automock)]
pub(crate) trait Connection: 'static + Send + Sync {
    /// Open a unidirectional send stream.
    fn open_uni(&self) -> BoxFut<'_, K2Result<DynIrohSendStream>>;

    /// Accept a unidirectional receive stream.
    fn accept_uni(&self) -> BoxFut<'_, K2Result<DynIrohRecvStream>>;

    /// Get the remote node ID.
    fn remote_id(&self) -> EndpointId;

    /// Close the connection with a code and reason.
    fn close(&self, code: u8, reason: &[u8]);

    /// Check if the connection has a direct (non-relay) path.
    fn is_direct(&self) -> bool;

    /// If the connection has been closed *by the remote* with an application
    /// close, return the reason bytes the remote sent.
    ///
    /// Returns `None` while the connection is still open, when it was closed
    /// locally, or when it was torn down by a transport-level error (timeout,
    /// reset, ...) rather than a graceful application close. This lets the
    /// reader distinguish a deliberate, signalled teardown (such as a
    /// connection superseded during simultaneous-open resolution) from a
    /// genuine peer disconnect.
    fn remote_close_reason(&self) -> Option<Bytes>;
}

/// Production implementation wrapping iroh's Connection.
pub(crate) struct IrohConnection {
    inner: Arc<iroh::endpoint::Connection>,
}

impl IrohConnection {
    /// Create a new wrapper around an iroh Connection.
    pub fn new(connection: Arc<iroh::endpoint::Connection>) -> Self {
        Self { inner: connection }
    }
}

impl Connection for IrohConnection {
    fn open_uni(&self) -> BoxFut<'_, K2Result<DynIrohSendStream>> {
        Box::pin(async move {
            let stream = self.inner.open_uni().await.map_err(|err| {
                K2Error::other_src("Failed to open uni-directional stream", err)
            })?;
            Ok(Arc::new(IrohSendStream::new(stream)) as DynIrohSendStream)
        })
    }

    fn accept_uni(&self) -> BoxFut<'_, K2Result<DynIrohRecvStream>> {
        Box::pin(async move {
            let stream = self.inner.accept_uni().await.map_err(|err| {
                K2Error::other_src(
                    "Failed to accept uni-directional stream",
                    err,
                )
            })?;
            Ok(Arc::new(IrohRecvStream::new(stream)) as DynIrohRecvStream)
        })
    }

    fn remote_id(&self) -> EndpointId {
        self.inner.remote_id()
    }

    fn close(&self, code: u8, reason: &[u8]) {
        self.inner.close(code.into(), reason);
    }

    fn is_direct(&self) -> bool {
        self.inner
            .paths()
            .iter()
            .any(|p| p.is_ip() && p.is_selected())
    }

    fn remote_close_reason(&self) -> Option<Bytes> {
        match self.inner.close_reason() {
            Some(iroh::endpoint::ConnectionError::ApplicationClosed(
                application_close,
            )) => Some(application_close.reason),
            _ => None,
        }
    }
}

pub(crate) type DynConnection = Arc<dyn Connection>;
