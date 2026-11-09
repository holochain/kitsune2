//! Abstractions for endpoint operations, enabling unit testing.

use crate::connection::{DynConnection, IrohConnection};
use iroh::{EndpointAddr, RelayConfig, RelayUrl};
use kitsune2_api::{BoxFut, K2Error, K2Result};
use n0_watcher::{Disconnected, Watcher};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

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

    /// Connects to the given endpoint address.
    fn connect(
        &self,
        endpoint_addr: EndpointAddr,
        alpn: &[u8],
    ) -> BoxFut<'_, K2Result<DynConnection>>;

    /// Closes the endpoint.
    fn close(&self) -> BoxFut<'_, ()>;

    /// Dynamically add a relay server to this endpoint.
    fn insert_relay(
        &self,
        url: RelayUrl,
        config: Arc<RelayConfig>,
    ) -> BoxFut<'_, ()>;

    /// Remove a relay server from this endpoint.
    fn remove_relay(
        &self,
        url: &RelayUrl,
    ) -> BoxFut<'_, Option<Arc<RelayConfig>>>;

    /// Returns the public key bytes of this endpoint.
    fn id_bytes(&self) -> [u8; 32];

    /// Returns `true` if the home relay has failed and has not been
    /// connected again since.
    ///
    /// Returns `false` until a failure is observed, so the initial
    /// `Connecting` phase never counts as down. Once a failure has been seen
    /// the answer stays `true` through the following redial attempts, until
    /// the relay is `Connected` again. iroh reports `Disconnected` only for
    /// the short backoff between attempts and drops the recorded error as
    /// soon as it redials, so the instantaneous state is not enough: when a
    /// dial takes long to fail, as it does on Windows, it is almost always
    /// `Connecting`.
    fn is_home_relay_known_down(&self) -> bool;
}

/// Sticky "home relay is down" flag derived from observed relay statuses.
#[derive(Debug, Default)]
struct RelayDownLatch(AtomicBool);

impl RelayDownLatch {
    /// The next value of the flag after observing a relay state.
    ///
    /// A connected relay clears the flag and a failed one sets it. Any other
    /// state (still dialling, or not selected yet) keeps the previous value.
    fn next(down: bool, connected: bool, has_error: bool) -> bool {
        match (connected, has_error) {
            (true, _) => false,
            (false, true) => true,
            (false, false) => down,
        }
    }

    fn observe(&self, statuses: &[iroh::endpoint::RelayStatus]) {
        let connected = statuses.iter().any(|s| s.is_connected());
        let has_error = statuses.iter().any(|s| s.last_error().is_some());
        let down = self.0.load(Ordering::SeqCst);
        self.0
            .store(Self::next(down, connected, has_error), Ordering::SeqCst);
    }

    fn is_down(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

#[derive(Debug)]
pub(crate) struct IrohEndpoint {
    inner: iroh::Endpoint,
    relay_down: Arc<RelayDownLatch>,
}

impl IrohEndpoint {
    /// Wraps an iroh endpoint.
    ///
    /// Spawns a task on the current tokio runtime that tracks the home relay
    /// status until the endpoint closes.
    pub(crate) fn new(inner: iroh::Endpoint) -> Self {
        let relay_down = Arc::new(RelayDownLatch::default());

        let mut status = inner.home_relay_status();
        relay_down.observe(&status.get());
        let closed = inner.closed();
        tokio::spawn({
            let relay_down = relay_down.clone();
            async move {
                tokio::pin!(closed);
                loop {
                    tokio::select! {
                        _ = &mut closed => break,
                        update = status.updated() => match update {
                            Ok(statuses) => relay_down.observe(&statuses),
                            Err(_) => break,
                        },
                    }
                }
            }
        });

        Self { inner, relay_down }
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

    fn close(&self) -> BoxFut<'_, ()> {
        Box::pin(async { self.inner.close().await })
    }

    fn insert_relay(
        &self,
        url: RelayUrl,
        config: Arc<RelayConfig>,
    ) -> BoxFut<'_, ()> {
        Box::pin(async move {
            self.inner.insert_relay(url, config).await;
        })
    }

    fn remove_relay(
        &self,
        url: &RelayUrl,
    ) -> BoxFut<'_, Option<Arc<RelayConfig>>> {
        let url = url.clone();
        Box::pin(async move { self.inner.remove_relay(&url).await })
    }

    fn id_bytes(&self) -> [u8; 32] {
        *self.inner.id().as_bytes()
    }

    fn is_home_relay_known_down(&self) -> bool {
        self.relay_down.is_down()
    }
}

pub(crate) type DynIrohEndpoint = Arc<dyn Endpoint>;

#[cfg(test)]
mod tests {
    use super::RelayDownLatch;

    #[test]
    fn startup_dial_is_not_down() {
        assert!(!RelayDownLatch::next(false, false, false));
    }

    #[test]
    fn failure_marks_down() {
        assert!(RelayDownLatch::next(false, false, true));
    }

    #[test]
    fn redial_after_failure_stays_down() {
        // `Connecting` carries no error, yet must not clear the flag.
        assert!(RelayDownLatch::next(true, false, false));
    }

    #[test]
    fn connected_clears_down() {
        assert!(!RelayDownLatch::next(true, true, false));
    }
}
