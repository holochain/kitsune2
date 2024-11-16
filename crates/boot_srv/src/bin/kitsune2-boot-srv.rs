use kitsune2_boot_srv::*;

fn main() {
    let _srv = BootSrv::new(Config::default());
    loop {
        std::thread::park();
    }
}
