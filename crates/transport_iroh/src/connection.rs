//! I/O abstractions for connection operations, enabling unit testing.

use crate::stream::{
    DynIrohRecvStream, DynIrohSendStream, IrohRecvStream, IrohSendStream,
};
use kitsune2_api::{BoxFut, K2Error, K2Result};
use std::sync::Arc;

/// Node identifier type from iroh.
pub(crate) type NodeId = iroh::PublicKey;

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
    fn remote_id(&self) -> NodeId;

    /// Close the connection with a code and reason.
    fn close(&self, code: u8, reason: &[u8]);
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
                K2Error::other_src("failed to open uni-directional stream", err)
            })?;
            Ok(Arc::new(IrohSendStream::new(stream)) as DynIrohSendStream)
        })
    }

    fn accept_uni(&self) -> BoxFut<'_, K2Result<DynIrohRecvStream>> {
        Box::pin(async move {
            let stream = self.inner.accept_uni().await.map_err(|err| {
                K2Error::other_src(
                    "failed to accept uni-directional stream",
                    err,
                )
            })?;
            Ok(Arc::new(IrohRecvStream::new(stream)) as DynIrohRecvStream)
        })
    }

    fn remote_id(&self) -> NodeId {
        self.inner.remote_id()
    }

    fn close(&self, code: u8, reason: &[u8]) {
        self.inner.close(code.into(), reason);
    }
}

pub(crate) type DynConnection = Arc<dyn Connection>;
