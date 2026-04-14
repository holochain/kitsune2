//! QUIC Address Discovery (QAD) server integration.
//!
//! Spawns an iroh-relay QUIC server that allows iroh clients to discover
//! their public IP address via QUIC's observed address reporting.

use std::net::SocketAddr;
use std::path::Path;

/// Spawn a QAD server from PEM certificate and key files.
///
/// The server binds a UDP socket at `bind_addr` and uses the provided TLS
/// certificate and key for the QUIC handshake.
pub async fn spawn_qad_server(
    bind_addr: SocketAddr,
    cert_path: &Path,
    key_path: &Path,
) -> Result<iroh_relay::server::Server, Box<dyn std::error::Error + Send + Sync>>
{
    let server_config = build_rustls_server_config(cert_path, key_path)?;
    spawn_with_config(bind_addr, server_config).await
}

/// Spawn a QAD server with a self-signed certificate.
///
/// Generates a self-signed TLS certificate at runtime. This is intended for
/// testing and local development where no real TLS certificate is available.
pub async fn spawn_qad_server_self_signed(
    bind_addr: SocketAddr,
) -> Result<iroh_relay::server::Server, Box<dyn std::error::Error + Send + Sync>>
{
    let cert =
        rcgen::generate_simple_self_signed(vec!["localhost".to_string()])?;

    let cert_der = cert.cert.der().clone();
    let key_der = rustls::pki_types::PrivateKeyDer::from(
        rustls::pki_types::PrivatePkcs8KeyDer::from(
            cert.key_pair.serialize_der(),
        ),
    );

    let server_config = rustls::ServerConfig::builder_with_provider(
        std::sync::Arc::new(rustls::crypto::ring::default_provider()),
    )
    .with_safe_default_protocol_versions()?
    .with_no_client_auth()
    .with_single_cert(vec![cert_der], key_der)?;

    spawn_with_config(bind_addr, server_config).await
}

/// Spawn the QAD server with a pre-built `rustls::ServerConfig`.
async fn spawn_with_config(
    bind_addr: SocketAddr,
    server_config: rustls::ServerConfig,
) -> Result<iroh_relay::server::Server, Box<dyn std::error::Error + Send + Sync>>
{
    let quic_config = iroh_relay::server::QuicConfig {
        bind_addr,
        server_config,
    };

    let config = iroh_relay::server::ServerConfig::<(), ()> {
        relay: None,
        quic: Some(quic_config),
        metrics_addr: None,
    };

    let server = iroh_relay::server::Server::spawn(config).await?;

    Ok(server)
}

/// Build a `rustls::ServerConfig` from PEM-encoded certificate and key files.
fn build_rustls_server_config(
    cert_path: &Path,
    key_path: &Path,
) -> Result<rustls::ServerConfig, Box<dyn std::error::Error + Send + Sync>> {
    let cert_pem = std::fs::read(cert_path)?;
    let key_pem = std::fs::read(key_path)?;

    let certs: Vec<rustls::pki_types::CertificateDer<'static>> =
        rustls_pemfile::certs(&mut &cert_pem[..]).collect::<Result<_, _>>()?;

    let key: rustls::pki_types::PrivateKeyDer<'static> =
        rustls_pemfile::private_key(&mut &key_pem[..])?
            .ok_or("no private key found in key file")?;

    let config = rustls::ServerConfig::builder_with_provider(
        std::sync::Arc::new(rustls::crypto::ring::default_provider()),
    )
    .with_safe_default_protocol_versions()?
    .with_no_client_auth()
    .with_single_cert(certs, key)?;

    Ok(config)
}
