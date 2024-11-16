//! boot_srv server types.

use std::sync::Arc;

use tiny_http::*;

/// Configuration for running a BootSrv.
pub struct Config {
    /// Worker thread count. [Default: cpu count].
    pub worker_thread_count: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            worker_thread_count: num_cpus::get(),
        }
    }
}

/// An actual boot_srv server instance.
///
/// This server is built to be direct, light-weight, and responsive.
/// On the server-side, as one aspect toward accomplishing this,
/// we are eschewing async code in favor of os thread workers.
pub struct BootSrv {
    cont: Arc<std::sync::atomic::AtomicBool>,
    workers: Vec<std::thread::JoinHandle<std::io::Result<()>>>,
}

impl Drop for BootSrv {
    fn drop(&mut self) {
        self.cont.store(false, std::sync::atomic::Ordering::SeqCst);
        for worker in self.workers.drain(..) {
            let _ = worker.join().expect("Failure shutting down worker thread");
        }
    }
}

impl BootSrv {
    /// Construct a new BootSrv instance.
    pub fn new(config: Config) -> std::io::Result<Self> {
        let cont = Arc::new(std::sync::atomic::AtomicBool::new(true));

        let server = Arc::new(
            Server::http("0.0.0.0:8080").map_err(std::io::Error::other)?,
        );

        let mut workers = Vec::with_capacity(config.worker_thread_count);
        for _ in 0..config.worker_thread_count {
            let cont = cont.clone();
            let server = server.clone();
            workers.push(std::thread::spawn(move || worker(cont, server)));
        }
        Ok(Self { cont, workers })
    }
}

trait RespondExt {
    fn respond_easy(self, status: u16, bytes: Vec<u8>) -> std::io::Result<()>;
}

impl RespondExt for Request {
    fn respond_easy(self, status: u16, bytes: Vec<u8>) -> std::io::Result<()> {
        let len = bytes.len();
        self.respond(Response::new(
            StatusCode(status),
            vec![Header {
                field: HeaderField::from_bytes(b"Content-Type").unwrap(),
                value: std::str::FromStr::from_str("application/json").unwrap(),
            }],
            std::io::Cursor::new(bytes),
            Some(len),
            None,
        ))
    }
}

fn worker(
    cont: Arc<std::sync::atomic::AtomicBool>,
    server: Arc<Server>,
) -> std::io::Result<()> {
    while cont.load(std::sync::atomic::Ordering::SeqCst) {
        let req =
            match server.recv_timeout(std::time::Duration::from_secs(1))? {
                Some(req) => req,
                None => continue,
            };

        println!("req: {} {}", req.method(), req.url());

        match (req.method().as_str(), req.url()) {
            ("GET", "/") => {
                req.respond_easy(200, b"{}".to_vec())?;
            }
            _ => {
                req.respond_easy(400, b"{\"error\":\"bad request\"}".to_vec())?;
            }
        }
    }
    Ok(())
}
