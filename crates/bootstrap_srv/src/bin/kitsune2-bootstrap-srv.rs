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

    /// The path to a TLS certificate file.
    #[arg(long, requires = "tls_key")]
    pub tls_cert: Option<std::path::PathBuf>,

    /// The path to a TLS key file.
    #[arg(long, requires = "tls_cert")]
    pub tls_key: Option<std::path::PathBuf>,
    // TODO - Implement the ability to specify the listening address
    // TODO - Implement the ability to override any other relevant
    //        config params that we wish to expose
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

    config.tls_cert = args.tls_cert;
    config.tls_key = args.tls_key;

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
