use iroh_relay::server::{AccessConfig, Handlers, Metrics, RelayService};
use iroh_relay::KeyCache;
use std::net::SocketAddr;
use std::num::NonZeroU32;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio_util::task::AbortOnDropHandle;
use tracing::{debug, error, info};

pub use iroh_relay::server::ClientRateLimit;

/// Rate limits configuration for the relay server
#[derive(Debug, Default)]
pub struct Limits {
    /// Rate limit for accepting new connection. Unlimited if not set.
    pub accept_conn_limit: Option<f64>,
    /// Burst limit for accepting new connection. Unlimited if not set.
    pub accept_conn_burst: Option<usize>,
    /// Rate limits for incoming traffic from a client connection.
    pub client_rx: Option<ClientRateLimit>,
}

/// Configuration for iroh relay server
#[derive(Debug)]
pub struct Config {
    /// Rate limiting configuration for iroh relay server
    pub limits: Limits,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            limits: Limits {
                accept_conn_limit: Some(250.0),
                accept_conn_burst: Some(100),
                client_rx: Some(ClientRateLimit {
                    bytes_per_second: NonZeroU32::new(2 * 1024 * 1024).unwrap(), // 2 MiB/s
                    max_burst_bytes: Some(NonZeroU32::new(256 * 1024).unwrap()), // 256 KiB burst
                }),
            },
        }
    }
}

/// A handle to the running relay server
pub struct RelayServerHandle {
    pub addr: SocketAddr,
    _task: AbortOnDropHandle<()>,
}

/// Spawns the embedded relay service on the given address using hyper-util
///
/// This runs the relay service at the connection level, which is required for
/// WebSocket upgrades. The service is integrated using hyper-util rather than
/// through an axum handler.
pub(super) async fn spawn_relay_service(
    addr: SocketAddr,
    limits: &Limits,
) -> std::io::Result<RelayServerHandle> {
    let handlers = Handlers::default();
    let headers = http::HeaderMap::new();
    let rate_limit = limits.client_rx;
    let key_cache = KeyCache::new(1024);
    let access = AccessConfig::Everyone;
    let metrics = Arc::new(Metrics::default());

    let relay_service = RelayService::new(handlers, headers, rate_limit, key_cache, access, metrics);

    let listener = TcpListener::bind(addr).await?;
    let bound_addr = listener.local_addr()?;

    info!("Embedded relay service listening on {}", bound_addr);

    let task = tokio::spawn(async move {
        use hyper::service::Service;
        use hyper_util::rt::{TokioExecutor, TokioIo};
        use hyper_util::server::conn::auto;

        loop {
            match listener.accept().await {
                Ok((stream, peer_addr)) => {
                    debug!("Relay connection from {}", peer_addr);

                    let service = relay_service.clone();
                    let io = TokioIo::new(stream);

                    tokio::spawn(async move {
                        if let Err(err) = auto::Builder::new(TokioExecutor::new())
                            .serve_connection(io, service)
                            .await
                        {
                            error!("Error serving relay connection: {:?}", err);
                        }
                    });
                }
                Err(err) => {
                    error!("Error accepting relay connection: {:?}", err);
                }
            }
        }
    });

    Ok(RelayServerHandle {
        addr: bound_addr,
        _task: AbortOnDropHandle::new(task),
    })
}
