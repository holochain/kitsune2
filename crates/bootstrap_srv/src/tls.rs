use axum_server::tls_rustls::RustlsConfig;

/// Configuration for TLS.
///
/// This struct holds the paths to the certificate and key files to use for serving over TLS.
pub struct TlsConfig {
    cert_path: std::path::PathBuf,
    key_path: std::path::PathBuf,
}

impl TlsConfig {
    /// Create a new [TlsConfig] with the given certificate and key paths.
    pub fn new(
        cert_path: std::path::PathBuf,
        key_path: std::path::PathBuf,
    ) -> Self {
        Self {
            cert_path,
            key_path,
        }
    }

    /// Create a new [RustlsConfig] using the configured certificate and key paths.
    pub async fn create_tls_config(&self) -> std::io::Result<RustlsConfig> {
        RustlsConfig::from_pem_file(
            self.cert_path.clone(),
            self.key_path.clone(),
        )
        .await
    }
}
