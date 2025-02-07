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
            .block_on(async_main(print2, line_recv));
    });

    // readline on the main thread
    readline::readline(nick, COMMAND_LIST, print, line_send);
}

async fn async_main(
    print: readline::Print,
    mut line_recv: tokio::sync::mpsc::Receiver<String>,
) {
    // this just shows that we can print things from other threads while
    // reading lines
    let print2 = print.clone();
    tokio::task::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;

            print2.print_line("tick".into());
        }
    });

    // this is the real loop where we'd send messages to peers
    // or deal with the files on the dht
    while let Some(line) = line_recv.recv().await {
        print.print_line(format!("got: {line} - NOT IMPLEMENTED"));
    }
}
