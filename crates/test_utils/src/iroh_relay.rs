//! Test utilities associated with iroh relay servers.

use iroh_relay::server::{
    AccessConfig, Limits, RelayConfig, Server, ServerConfig,
};
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};

/// Spawn an iroh relay server at 127.0.0.1.
pub async fn spawn_iroh_relay_server() -> Server {
    Server::spawn::<(), ()>(ServerConfig {
        metrics_addr: None,
        quic: None,
        relay: Some(RelayConfig {
            http_bind_addr: SocketAddr::V4(SocketAddrV4::new(
                Ipv4Addr::new(127, 0, 0, 1),
                0,
            )),
            tls: None,
            limits: Limits::default(),
            key_cache_capacity: None,
            access: AccessConfig::Everyone,
        }),
    })
    .await
    .unwrap()
}
