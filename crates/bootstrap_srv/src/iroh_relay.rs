use iroh_relay::server::{AccessConfig, Handlers, Metrics, RelayService};
use iroh_relay::KeyCache;
use std::num::NonZeroU32;
use std::sync::Arc;

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

/// Creates a new embedded iroh relay service that can be integrated into an existing HTTP server
pub(super) fn create_relay_service(limits: &Limits) -> RelayService {
    let handlers = Handlers::default();
    let headers = http::HeaderMap::new();
    let rate_limit = limits.client_rx;
    let key_cache = KeyCache::new(1024); // Default cache capacity
    let access = AccessConfig::Everyone;
    let metrics = Arc::new(Metrics::default());

    RelayService::new(handlers, headers, rate_limit, key_cache, access, metrics)
}
