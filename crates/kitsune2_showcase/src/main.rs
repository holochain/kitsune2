use tokio::sync::mpsc;

mod app;
mod readline;

/// Kitsune2 Showcase chat and file sharing app.
#[derive(clap::Parser)]
struct Args {
    /// The signal server to use.
    #[arg(long, default_value = "wss://dev-test-bootstrap2.holochain.org")]
    signal_url: String,

    /// The relay server to use.
    #[arg(
        long,
        default_value = "https://use1-1.relay.n0.iroh-canary.iroh.link./"
    )]
    relay_url: String,

    /// The bootstrap server to use.
    #[arg(long, default_value = "https://dev-test-bootstrap2.holochain.org")]
    bootstrap_url: String,

    /// Override the default network seed.
    #[arg(long)]
    network_seed: Option<String>,

    /// The nickname you'd like to use.
    nick: String,
}

#[tokio::main]
async fn main() {
    let pid = std::process::id();
    let file_appender = tracing_appender::rolling::never(
        "./logs",
        format!("{}-{}.log", env!("CARGO_PKG_NAME"), pid),
    );
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);
    tracing_subscriber::fmt().with_writer(non_blocking).init();

    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

    let args = <Args as clap::Parser>::parse();
    let nick = args.nick.clone();

    let (printer_tx, printer_rx) = mpsc::channel(10);
    let app = app::App::new(printer_tx, args)
        .await
        .expect("Failed to create the App");

    readline::readline(nick, printer_rx, app).await;
}
