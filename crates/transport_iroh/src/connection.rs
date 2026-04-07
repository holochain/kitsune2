//! Abstractions for connection operations, enabling unit testing.

use crate::stream::{
    DynIrohRecvStream, DynIrohSendStream, IrohRecvStream, IrohSendStream,
};
use iroh::EndpointId;
use iroh::endpoint::ConnectionType;
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
}

/// Production implementation wrapping iroh's Connection.
pub(crate) struct IrohConnection {
    inner: Arc<iroh::endpoint::Connection>,
    endpoint: Arc<iroh::Endpoint>,
}

impl IrohConnection {
    /// Create a new wrapper around an iroh Connection, with access to
    /// the endpoint for connection type queries (iroh 0.95.1 API).
    pub fn new(
        connection: Arc<iroh::endpoint::Connection>,
        endpoint: Arc<iroh::Endpoint>,
    ) -> Self {
        Self {
            inner: connection,
            endpoint,
        }
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
        use n0_watcher::Watcher;
        self.endpoint
            .conn_type(self.inner.remote_id())
            .map(|mut watcher| {
                matches!(
                    watcher.get(),
                    ConnectionType::Direct(_) | ConnectionType::Mixed(_, _)
                )
            })
            .unwrap_or(false)
    }
}

pub(crate) type DynConnection = Arc<dyn Connection>;
