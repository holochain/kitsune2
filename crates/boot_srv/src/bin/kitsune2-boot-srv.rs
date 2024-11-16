use kitsune2_boot_srv::*;

fn main() {
    let (send, recv) = std::sync::mpsc::channel();

    ctrlc::set_handler(move || {
        send.send(()).unwrap();
    })
    .unwrap();

    let srv = BootSrv::new(Config::default());

    let _ = recv.recv();

    println!("Terminating...");
    drop(srv);
    println!("Done.");
    std::process::exit(0);
}
