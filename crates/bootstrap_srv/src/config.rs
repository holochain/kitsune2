//! config types.

/// Configuration for running a BootstrapSrv.
#[derive(Debug)]
pub struct Config {
    /// Worker thread count.
    ///
    /// This server is currently built using blocking io and filesystem
    /// storage. It is therefore beneficial to have more worker threads
    /// than system cpus, since the workers will be bound on io, not
    /// on cpu. On the other hand, increasing this will also increase
    /// memory overhead and tempfile handle count, so we don't want to
    /// set it too high.
    ///
    /// Defaults:
    /// - `testing = 2`
    /// - `production = 4 * cpu_count`
    pub worker_thread_count: usize,

    /// The maximum agent info entry count per space.
    ///
    /// All entries will be returned in a get space request, so
    /// this count should be low enough to reasonably send this response
    /// over http without needing pagination.
    ///
    /// Defaults:
    /// - `testing = 32`
    /// - `production = 32`
    pub max_entries_per_space: usize,

    /// The duration worker threads will block waiting for incoming connections
    /// before checking to see if the server is shutting down.
    ///
    /// Setting this very high will cause ctrl-c / server shutdown to be slow.
    /// Setting this very low will increase cpu overhead (and in extreme
    /// conditions, could cause a lack of responsiveness in the server).
    ///
    /// Defaults:
    /// - `testing = 10ms`
    /// - `production = 2s`
    pub request_listen_duration: std::time::Duration,

    /// The address(es) at which to listen.
    ///
    /// Defaults:
    /// - `testing = "[127.0.0.1:0]"`
    /// - `production = "[0.0.0.0:443, [::]:443]"`
    pub listen_address_list: Vec<std::net::SocketAddr>,

    /// The interval at which expired agents are purged from the cache.
    ///
    /// This is a fairly expensive operation that requires iterating
    /// through every registered space and loading all the infos off the disk,
    /// so it should not be undertaken too frequently.
    ///
    /// Defaults:
    /// - `testing = 10s`
    /// - `production = 60s`
    pub prune_interval: std::time::Duration,

    /// The path to a TLS certificate file.
    ///
    /// Must be provided when `tls_cert` is provided.
    ///
    /// Default:
    /// - `None`
    pub tls_cert: Option<std::path::PathBuf>,

    /// The path to a TLS key file.
    ///
    /// Must be provided when `tls_cert` is provided.
    ///
    /// Default:
    /// - `None`
    pub tls_key: Option<std::path::PathBuf>,

    /// The socket address on which the QUIC Address Discovery (QAD) server
    /// should bind. When set (along with TLS cert/key), a QUIC endpoint is
    /// started that lets iroh clients discover their public address.
    ///
    /// Default: `None` (disabled)
    #[cfg(feature = "iroh-relay")]
    pub quic_bind_addr: Option<std::net::SocketAddr>,

    /// Sustained inbound byte rate per relay client connection, in bytes
    /// per second.
    ///
    /// `None` disables the limiter entirely. Setting this without
    /// [`Self::relay_client_rx_burst_bytes`] derives the burst as
    /// `(bytes_per_second / 10).max(1)` to match iroh's own default.
    ///
    /// Default: `None` in both `testing()` and `production()`.
    #[cfg(feature = "iroh-relay")]
    pub relay_client_rx_bytes_per_second: Option<std::num::NonZeroU32>,

    /// Maximum allowed inbound burst per relay client connection, in bytes.
    ///
    /// Has effect only when [`Self::relay_client_rx_bytes_per_second`] is
    /// also `Some`. Setting this alone is rejected at `BootstrapSrv::new`
    /// time.
    ///
    /// Default: `None` in both `testing()` and `production()`.
    #[cfg(feature = "iroh-relay")]
    pub relay_client_rx_burst_bytes: Option<std::num::NonZeroU32>,

    /// Disable the relay server.
    pub no_relay_server: bool,

    /// The allowed origins for CORS requests.
    ///
    /// If `None`, defaults to allowing any origin.
    pub allowed_origins: Option<Vec<String>>,

    /// Authentication configuration.
    ///
    /// This is independent of the relay implementation (SBD or Iroh).
    pub auth: crate::auth::AuthConfig,

    /// The SBD server configuration.
    #[cfg(feature = "sbd")]
    pub sbd: sbd_server::Config,
}

impl Config {
    /// Get a boot_srv config suitable for testing.
    pub fn testing() -> Self {
        Self {
            worker_thread_count: 2,
            max_entries_per_space: 32,
            request_listen_duration: std::time::Duration::from_millis(10),
            listen_address_list: vec![
                (std::net::Ipv4Addr::LOCALHOST, 0).into(),
            ],
            prune_interval: std::time::Duration::from_secs(10),
            tls_cert: None,
            tls_key: None,
            #[cfg(feature = "iroh-relay")]
            quic_bind_addr: Some((std::net::Ipv4Addr::LOCALHOST, 0).into()),
            #[cfg(feature = "iroh-relay")]
            relay_client_rx_bytes_per_second: None,
            #[cfg(feature = "iroh-relay")]
            relay_client_rx_burst_bytes: None,
            no_relay_server: false,
            allowed_origins: None,
            auth: crate::auth::AuthConfig::default(),
            #[cfg(feature = "sbd")]
            sbd: sbd_server::Config::default(),
        }
    }

    /// Get a boot_srv config suitable for production.
    pub fn production() -> Self {
        Self {
            worker_thread_count: num_cpus::get() * 4,
            max_entries_per_space: 32,
            request_listen_duration: std::time::Duration::from_secs(2),
            listen_address_list: vec![
                (std::net::Ipv4Addr::UNSPECIFIED, 443).into(),
                (std::net::Ipv6Addr::UNSPECIFIED, 443).into(),
            ],
            prune_interval: std::time::Duration::from_secs(60),
            tls_cert: None,
            tls_key: None,
            #[cfg(feature = "iroh-relay")]
            quic_bind_addr: Some(
                (std::net::Ipv6Addr::UNSPECIFIED, 7842).into(),
            ),
            #[cfg(feature = "iroh-relay")]
            relay_client_rx_bytes_per_second: None,
            #[cfg(feature = "iroh-relay")]
            relay_client_rx_burst_bytes: None,
            no_relay_server: false,
            allowed_origins: None,
            auth: crate::auth::AuthConfig::default(),
            #[cfg(feature = "sbd")]
            sbd: sbd_server::Config::default(),
        }
    }
}

#[cfg(feature = "iroh-relay")]
impl Config {
    /// Resolve the per-connection inbound byte rate limit from the two
    /// `relay_client_rx_*` fields.
    ///
    /// Returns:
    /// - `Ok(None)` when no limit is configured.
    /// - `Ok(Some(_))` with the resolved [`crate::iroh_relay_axum::RelayClientRxRateLimit`]
    ///   when a sustained rate is set; burst defaults to `(bps / 10).max(1)` if
    ///   not set explicitly.
    /// - `Err(_)` when only the burst is set but not the sustained rate.
    pub fn resolve_relay_rate_limit(
        &self,
    ) -> Result<
        Option<crate::iroh_relay_axum::RelayClientRxRateLimit>,
        std::io::Error,
    > {
        match (
            self.relay_client_rx_bytes_per_second,
            self.relay_client_rx_burst_bytes,
        ) {
            (None, None) => Ok(None),
            (None, Some(_)) => Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "relay_client_rx_burst_bytes is set but \
                 relay_client_rx_bytes_per_second is not; both required \
                 (burst alone has no meaning)",
            )),
            (Some(bps), Some(burst)) => {
                Ok(Some(crate::iroh_relay_axum::RelayClientRxRateLimit {
                    bytes_per_second: bps,
                    burst_bytes: burst,
                }))
            }
            (Some(bps), None) => {
                let derived = (bps.get() / 10).max(1);
                let burst = std::num::NonZeroU32::new(derived)
                    .expect("derived burst is always >= 1 by construction");
                Ok(Some(crate::iroh_relay_axum::RelayClientRxRateLimit {
                    bytes_per_second: bps,
                    burst_bytes: burst,
                }))
            }
        }
    }
}

#[cfg(all(test, feature = "iroh-relay"))]
mod resolve_rate_limit_tests {
    use super::*;
    use std::num::NonZeroU32;

    fn cfg() -> Config {
        Config::testing()
    }

    #[test]
    fn none_when_unset() {
        let c = cfg();
        assert!(c.resolve_relay_rate_limit().unwrap().is_none());
    }

    #[test]
    fn err_when_only_burst_set() {
        let mut c = cfg();
        c.relay_client_rx_burst_bytes = NonZeroU32::new(1024);
        assert!(c.resolve_relay_rate_limit().is_err());
    }

    #[test]
    fn explicit_burst_used_as_is() {
        let mut c = cfg();
        c.relay_client_rx_bytes_per_second = NonZeroU32::new(1000);
        c.relay_client_rx_burst_bytes = NonZeroU32::new(2048);
        let r = c.resolve_relay_rate_limit().unwrap().unwrap();
        assert_eq!(r.bytes_per_second.get(), 1000);
        assert_eq!(r.burst_bytes.get(), 2048);
    }

    #[test]
    fn burst_defaults_to_one_tenth_when_unset() {
        let mut c = cfg();
        c.relay_client_rx_bytes_per_second = NonZeroU32::new(1000);
        let r = c.resolve_relay_rate_limit().unwrap().unwrap();
        assert_eq!(r.bytes_per_second.get(), 1000);
        assert_eq!(r.burst_bytes.get(), 100);
    }

    #[test]
    fn burst_floor_of_one_for_tiny_rate() {
        let mut c = cfg();
        c.relay_client_rx_bytes_per_second = NonZeroU32::new(1);
        let r = c.resolve_relay_rate_limit().unwrap().unwrap();
        assert_eq!(r.burst_bytes.get(), 1);
    }
}
