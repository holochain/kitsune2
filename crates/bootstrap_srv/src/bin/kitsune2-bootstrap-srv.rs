//! The binary kitsune2-bootstrap-srv.

use kitsune2_bootstrap_srv::*;

#[derive(clap::Parser, Debug)]
#[command(version)]
pub struct Args {
    /// By default, kitsune2-boot-srv runs in "testing" configuration
    /// with much lighter resource usage settings. This testing mode
    /// should be more than enough for most developer application testing
    /// and continuous integration or automated tests.
    ///
    /// To set up the server to be ready to use most of the resources available
    /// on a single given machine, you can set this "production" mode.
    #[arg(long)]
    pub production: bool,

    /// Output tracing in json format.
    #[arg(long)]
    pub json: bool,

    /// The address(es) at which to listen.
    ///
    /// Defaults: testing = "[127.0.0.1:0]", production = "[0.0.0.0:443, [::]:443]"
    #[arg(long)]
    pub listen: Vec<std::net::SocketAddr>,

    /// The path to a TLS certificate file.
    ///
    /// The certificate must be PEM encoded.
    #[arg(long, requires = "tls_key")]
    pub tls_cert: Option<std::path::PathBuf>,

    /// The path to a TLS key file.
    ///
    /// The key must be PEM encoded.
    #[arg(long, requires = "tls_cert")]
    pub tls_key: Option<std::path::PathBuf>,

    /// The number of worker threads to use.
    ///
    /// Defaults: testing = 2, production = 4 * cpu_count
    #[arg(long)]
    pub worker_thread_count: Option<usize>,

    /// The maximum agent info entry count per space.
    ///
    /// Defaults: testing = 32, production = 32
    #[arg(long)]
    pub max_entries_per_space: Option<usize>,

    /// The number of milliseconds that worker threads will block waiting for incoming connections
    /// before checking to see if the server is shutting down.
    ///
    /// Defaults: testing = 10ms, production = 2s
    #[arg(long)]
    pub request_listen_duration_ms: Option<u32>,

    /// The interval at which expired agents are purged from the cache.
    ///
    /// Defaults: testing = 10s, production = 60s
    #[arg(long)]
    pub prune_interval_ms: Option<u32>,

    /// If specified, this server will only handle bootstrap requests,
    /// dropping websocket upgrade requests from sbd clients.
    #[arg(long)]
    pub no_sbd: bool,

    /// Use this http header to determine IP address instead of the raw
    /// TCP connection details.
    #[arg(long)]
    pub sbd_trusted_ip_header: Option<String>,

    /// Limit client connections.
    #[arg(long)]
    pub sbd_limit_clients: Option<i32>,

    /// If set, rate-limiting will be disabled on the server,
    /// and clients will be informed they have an 8gbps rate limit.
    ///
    /// Note that this is an SBD option, but when SBD is enabled, this applies to all connections.
    #[arg(long)]
    pub sbd_disable_rate_limiting: bool,
}

fn main() {
    let args = <Args as clap::Parser>::parse();

    let t = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::builder()
                .with_default_directive(tracing::Level::DEBUG.into())
                .from_env_lossy(),
        )
        .with_file(true)
        .with_line_number(true);

    if args.json {
        t.json().try_init()
    } else {
        t.try_init()
    }
    .expect("failed to init tracing");

    let mut config = if args.production {
        Config::production()
    } else {
        Config::testing()
    };

    if args.tls_cert.is_some() || args.tls_key.is_some() {
        rustls::crypto::ring::default_provider()
            .install_default()
            .expect("Failed to configure default TLS provider");
    }

    // Apply bootstrap command line arguments
    config.tls_cert = args.tls_cert;
    config.tls_key = args.tls_key;
    if !args.listen.is_empty() {
        config.listen_address_list = args.listen;
    }
    if let Some(count) = args.worker_thread_count {
        config.worker_thread_count = count;
    }
    if let Some(count) = args.max_entries_per_space {
        config.max_entries_per_space = count;
    }
    if let Some(ms) = args.request_listen_duration_ms {
        config.request_listen_duration =
            std::time::Duration::from_millis(ms as u64);
    }
    if let Some(ms) = args.prune_interval_ms {
        config.prune_interval = std::time::Duration::from_millis(ms as u64);
    }

    // Apply SBD command line arguments
    if let Some(header) = args.sbd_trusted_ip_header {
        config.sbd.trusted_ip_header = Some(header);
    }
    if let Some(limit) = args.sbd_limit_clients {
        config.sbd.limit_clients = limit;
    }
    if args.sbd_disable_rate_limiting {
        config.sbd.disable_rate_limiting = true;
    }

    tracing::info!(?config);

    let (send, recv) = std::sync::mpsc::channel();

    ctrlc::set_handler(move || {
        send.send(()).unwrap();
    })
    .unwrap();

    let srv = BootstrapSrv::new(config);

    let _ = recv.recv();

    tracing::info!("Terminating...");
    drop(srv);
    tracing::info!("Exit Process.");
    std::process::exit(0);
}
