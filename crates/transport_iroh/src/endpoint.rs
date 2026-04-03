//! Abstractions for endpoint operations, enabling unit testing.

use crate::connection::{DynConnection, IrohConnection};
use iroh::EndpointAddr;
use kitsune2_api::{BoxFut, K2Error, K2Result};
use n0_watcher::{Disconnected, Watcher};
use std::sync::Arc;

pub(crate) trait EndpointAddrWatcher: Send + Sync {
    fn updated(&mut self) -> BoxFut<'_, Result<EndpointAddr, Disconnected>>;
}

struct IrohWatcher<W> {
    inner: W,
}

impl<W> EndpointAddrWatcher for IrohWatcher<W>
where
    W: Watcher<Value = EndpointAddr> + Send + Sync,
{
    fn updated(&mut self) -> BoxFut<'_, Result<EndpointAddr, Disconnected>> {
        Box::pin(self.inner.updated())
    }
}

pub(crate) trait Endpoint:
    'static + Send + Sync + std::fmt::Debug
{
    /// Returns a Watcher for the current EndpointAddr for this endpoint.
    fn watch_addr(&self) -> Box<dyn EndpointAddrWatcher>;

    /// Accepts an incoming connection.
    /// Returns None if the endpoint is closed.
    fn accept(&self) -> BoxFut<'_, Option<K2Result<DynConnection>>>;

    /// Get the current EndpointAddr (addresses + node ID).
    fn addr(&self) -> EndpointAddr;

    /// Connects to the given endpoint address.
    fn connect(
        &self,
        endpoint_addr: EndpointAddr,
        alpn: &[u8],
    ) -> BoxFut<'_, K2Result<DynConnection>>;

    /// Closes the endpoint.
    fn close(&self) -> BoxFut<'_, ()>;
}

#[derive(Debug)]
pub(crate) struct IrohEndpoint {
    inner: iroh::Endpoint,
}

impl IrohEndpoint {
    pub(crate) fn new(inner: iroh::Endpoint) -> Self {
        Self { inner }
    }
}

impl Endpoint for IrohEndpoint {
    fn addr(&self) -> EndpointAddr {
        self.inner.addr()
    }

    fn watch_addr(&self) -> Box<dyn EndpointAddrWatcher> {
        Box::new(IrohWatcher {
            inner: self.inner.watch_addr(),
        })
    }

    fn accept(&self) -> BoxFut<'_, Option<K2Result<DynConnection>>> {
        Box::pin(async move {
            match self.inner.accept().await {
                Some(incoming) => {
                    // Await the incoming connection and wrap it
                    let result = incoming
                        .await
                        .map(|conn| {
                            Arc::new(IrohConnection::new(Arc::new(conn)))
                                as DynConnection
                        })
                        .map_err(|err| {
                            K2Error::other_src(
                                "Accepting incoming connection failed",
                                err,
                            )
                        });
                    Some(result)
                }
                None => None,
            }
        })
    }

    fn connect(
        &self,
        endpoint_addr: EndpointAddr,
        alpn: &[u8],
    ) -> BoxFut<'_, K2Result<DynConnection>> {
        let alpn = alpn.to_vec();
        Box::pin(async move {
            self.inner
                .connect(endpoint_addr, &alpn)
                .await
                .map(|conn| {
                    Arc::new(IrohConnection::new(Arc::new(conn)))
                        as DynConnection
                })
                .map_err(|err| {
                    K2Error::other_src(
                        "Establishing iroh connection failed",
                        err,
                    )
                })
        })
    }

    fn close(&self) -> BoxFut<'_, ()> {
        Box::pin(async { self.inner.close().await })
    }
}

pub(crate) type DynIrohEndpoint = Arc<dyn Endpoint>;
