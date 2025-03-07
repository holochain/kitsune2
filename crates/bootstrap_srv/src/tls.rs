use axum_server::tls_rustls::RustlsConfig;
use futures::future::BoxFuture;

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

    /// Create a future that reloads the TLS configuration every hour.
    pub fn reload_task(self, config: RustlsConfig) -> BoxFuture<'static, ()> {
        const ONE_HOUR: std::time::Duration =
            std::time::Duration::from_secs(60 * 60);

        Box::pin(async move {
            loop {
                tokio::time::sleep(ONE_HOUR).await;

                // Reload rustls configuration.
                if let Err(e) = config
                    .reload_from_pem_file(
                        self.cert_path.clone(),
                        self.key_path.clone(),
                    )
                    .await
                {
                    tracing::error!(
                        "failed to reload rustls configuration: {}",
                        e
                    );
                } else {
                    tracing::info!("rustls configuration reloaded");
                }
            }
        })
    }
}
