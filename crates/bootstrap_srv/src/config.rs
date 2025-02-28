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
            listen_address_list: vec![(std::net::Ipv4Addr::LOCALHOST, 0).into()],
            prune_interval: std::time::Duration::from_secs(10),
            tls_cert: None,
            tls_key: None,
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
            #[cfg(feature = "sbd")]
            sbd: sbd_server::Config::default(),
        }
    }
}
