//! Test utilities associated with iroh relay servers.

use std::net::Ipv4Addr;

use iroh::RelayUrl;
use iroh_relay_holochain::{RelayMap, RelayConfig, RelayQuicConfig};
use iroh_relay_holochain::server::{
    Server, ServerConfig, RelayConfig as RelayServerConfig,
    AccessConfig, TlsConfig, CertConfig, QuicConfig,
};

pub use iroh_relay_holochain::server::Server as RelayServer;

/// Spawn an iroh relay server at 127.0.0.1.
pub async fn spawn_iroh_relay_server() -> (RelayMap, RelayUrl, Server) {
    let (certs, server_config) = iroh_relay_holochain::server::testing::self_signed_tls_certs_and_config();

    let tls = TlsConfig {
        cert: CertConfig::<(), ()>::Manual { certs },
        https_bind_addr: (Ipv4Addr::LOCALHOST, 0).into(),
        quic_bind_addr: (Ipv4Addr::LOCALHOST, 0).into(),
        server_config,
    };
    let quic = Some(QuicConfig {
        server_config: tls.server_config.clone(),
        bind_addr: tls.quic_bind_addr,
    });
    let config = ServerConfig {
        relay: Some(RelayServerConfig {
            http_bind_addr: (Ipv4Addr::LOCALHOST, 0).into(),
            tls: Some(tls),
            limits: Default::default(),
            key_cache_capacity: Some(1024),
            access: AccessConfig::Everyone,
        }),
        quic,
        ..Default::default()
    };
    let server = Server::spawn(config).await.expect("failed to spawn relay server");
    let url: RelayUrl = format!("https://{}", server.https_addr().expect("configured"))
        .parse()
        .expect("invalid relay url");

    // Configure QUIC endpoint
    let quic = server
        .quic_addr()
        .map(|addr| RelayQuicConfig { port: addr.port() });

    let n: RelayMap = RelayConfig {
        url: url.clone(),
        quic,
    }
    .into();

    (n, url, server)
}
