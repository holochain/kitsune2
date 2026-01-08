use iroh_relay::server::axum_integration::RelayState;
use iroh_relay::server::{AccessConfig, Metrics};
use iroh_relay::KeyCache;
use std::num::NonZeroU32;
use std::sync::Arc;

pub use iroh_relay::server::axum_integration::relay_handler;
pub use iroh_relay::server::ClientRateLimit;

/// Handler for the relay probe endpoint (`/ping`).
/// This is used for latency testing and availability checks.
pub async fn relay_probe_handler() -> (
    axum::http::StatusCode,
    axum::response::AppendHeaders<[(axum::http::HeaderName, &'static str); 1]>,
) {
    (
        axum::http::StatusCode::OK,
        axum::response::AppendHeaders([(
            axum::http::header::ACCESS_CONTROL_ALLOW_ORIGIN,
            "*",
        )]),
    )
}

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

/// Creates a RelayState instance for the axum relay handler
///
/// This creates the state needed for the relay handler which can be mounted
/// as a standard axum route.
///
/// Note: The `limits` parameter is currently unused because the axum integration
/// does not support client-side rate limiting. Rate limiting would need to be
/// implemented at the axum middleware level if required.
pub(super) fn create_relay_state(_limits: &Limits) -> RelayState {
    let key_cache = KeyCache::new(1024);
    let access = Arc::new(AccessConfig::Everyone);
    let metrics = Arc::new(Metrics::default());

    RelayState::new(key_cache, access, metrics)
}
