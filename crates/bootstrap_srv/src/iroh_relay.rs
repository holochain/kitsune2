use iroh_relay::server::{AccessConfig, RelayConfig, Server, ServerConfig};
use std::num::NonZeroU32;

pub use iroh_relay::server::{ClientRateLimit, Limits};

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

pub(super) async fn start_iroh_relay_server(limits: &Limits) -> Server {
    Server::spawn(ServerConfig::<(), ()> {
        relay: Some(RelayConfig {
            // Secure because only bound to loopback on localhost
            http_bind_addr: ([127, 0, 0, 1], 0).into(),
            tls: None, // HTTP is fine for localhost
            limits: Limits {
                accept_conn_limit: limits.accept_conn_limit,
                accept_conn_burst: limits.accept_conn_burst,
                client_rx: limits.client_rx,
            },
            key_cache_capacity: None, // uses default
            access: AccessConfig::Everyone,
        }),
        quic: None,         // Disable QUIC for internal proxy
        metrics_addr: None, // Don't expose metrics port either
    })
    .await
    .expect("Failed to start internal iroh relay server")
}
