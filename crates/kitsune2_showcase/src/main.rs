use bytes::Bytes;

mod app;
mod readline;

/// Kitsune2 Showcase chat and file sharing app.
#[derive(clap::Parser)]
struct Args {
    /// The signal server to use.
    #[arg(long, default_value = "wss://dev-test-bootstrap2.holochain.org")]
    signal_url: String,

    /// The bootstrap server to use.
    #[arg(long, default_value = "https://dev-test-bootstrap2.holochain.org")]
    bootstrap_url: String,

    /// Override the default network seed.
    #[arg(long)]
    network_seed: Option<String>,

    /// The nickname you'd like to use.
    nick: String,
}

fn main() {
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

    let print = readline::Print::default();
    let (line_send, line_recv) = tokio::sync::mpsc::channel(2);

    // spawn a new thread for tokio runtime, all kitsune stuff
    // will be in tokio task threads
    let print2 = print.clone();
    std::thread::spawn(move || {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async_main(print2, args, line_recv));
    });

    // readline on the main thread
    readline::readline(nick, print, line_send);
}

async fn async_main(
    print: readline::Print,
    args: Args,
    mut line_recv: tokio::sync::mpsc::Receiver<String>,
) {
    // create the kitsune connection
    let app = app::App::new(print.clone(), args).await.unwrap();

    // loop over cli input lines either executing commands or sending chats
    while let Some(line) = line_recv.recv().await {
        if line.starts_with("/stats") {
            app.stats().await.unwrap();
        } else if line.starts_with("/") {
            print.print_line("NOT IMPLEMENTED".into());
        } else {
            app.chat(Bytes::copy_from_slice(line.as_bytes()))
                .await
                .unwrap();
        }
    }
}
