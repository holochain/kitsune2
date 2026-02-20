//! Abstractions for endpoint operations, enabling unit testing.

use crate::connection::{DynConnection, IrohConnection};
use iroh::EndpointAddr;
use kitsune2_api::{BoxFut, K2Error, K2Result};
use n0_watcher::Disconnected;
use std::sync::Arc;

pub(crate) trait EndpointAddrWatcher: Send + Sync {
    fn updated(&mut self) -> BoxFut<'_, Result<EndpointAddr, Disconnected>>;
}

// Wrapper around iroh's watcher that implements our trait
struct IrohWatcher<W>
where
    W: n0_watcher::Watcher<Value = EndpointAddr> + Send + Sync + Unpin,
{
    inner: W,
}

impl<W> EndpointAddrWatcher for IrohWatcher<W>
where
    W: n0_watcher::Watcher<Value = EndpointAddr> + Send + Sync + Unpin,
{
    fn updated(&mut self) -> BoxFut<'_, Result<EndpointAddr, Disconnected>> {
        Box::pin(async {
            n0_watcher::Watcher::updated(&mut self.inner).await
        })
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

    /// Connects to the given endpoint address.
    fn connect(
        &self,
        endpoint_addr: EndpointAddr,
        alpn: &[u8],
    ) -> BoxFut<'_, K2Result<DynConnection>>;
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
}

pub(crate) type DynIrohEndpoint = Arc<dyn Endpoint>;
