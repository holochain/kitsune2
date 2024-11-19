use kitsune2_boot_srv::*;

#[derive(clap::Parser, Debug)]
#[command(version)]
pub struct Args {
    /// By default kitsune2-boot-srv runs in "testing" configuration
    /// with much lighter resource usage settings. This testing mode
    /// should be more than enough for most developer application testing
    /// and continuous integration or automated tests.
    ///
    /// To setup the server to be ready to use most of the resources available
    /// on a single given machine, you can set this "production" mode.
    #[arg(long)]
    pub production: bool,
}

fn main() {
    let args = <Args as clap::Parser>::parse();

    let config = if args.production {
        Config::production()
    } else {
        Config::testing()
    };

    println!("{args:?}--{config:?}");

    let (send, recv) = std::sync::mpsc::channel();

    ctrlc::set_handler(move || {
        send.send(()).unwrap();
    })
    .unwrap();

    let srv = BootSrv::new(config);

    let _ = recv.recv();

    println!("Terminating...");
    drop(srv);
    println!("Done.");
    std::process::exit(0);
}
