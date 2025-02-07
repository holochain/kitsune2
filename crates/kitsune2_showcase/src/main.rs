use bytes::Bytes;

mod app;
mod readline;

/// Kitsune2 Showcase chat and file sharing app.
#[derive(clap::Parser)]
struct Args {
    /// The signal server to use.
    #[arg(long, default_value = "wss://sbd.holo.host")]
    signal_url: String,

    /// The bootstrap server to use. TODO - default to a real server!!
    #[arg(long, default_value = "http://localhost:12345")]
    bootstrap_url: String,

    /// The nickname you'd like to use.
    nick: String,
}

const COMMAND_LIST: &[(&str, &str)] = &[
    ("/share", "[filename] share a file if under 1K"),
    ("/list", "list files shared"),
    ("/fetch", "[filename] fetch a shared file"),
];

fn main() {
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
    readline::readline(nick, COMMAND_LIST, print, line_send);
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
        if line.starts_with("/") {
            print.print_line("NOT IMPLEMENTED".into()).await;
        } else {
            app.chat(Bytes::copy_from_slice(line.as_bytes()))
                .await
                .unwrap();
        }
    }
}
