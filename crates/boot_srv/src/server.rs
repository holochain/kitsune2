//! boot_srv server types.

use std::sync::Arc;

use tiny_http::*;

/// Configuration for running a BootSrv.
#[derive(Debug)]
pub struct Config {
    /// Worker thread count.
    ///
    /// This server is currently built using blocking io and filesystem
    /// storage. It is therefore beneficial to have more worker threads
    /// than system cpus, since the workers will be bound on io, not
    /// on cpu. On the other hand, increasing this will also increase
    /// memory overhead and tempfile handle count, so we don't want to
    /// set it too high.
    ///
    /// Defaults:
    /// - `testing = 2`
    /// - `production = 4 * cpu_count`
    pub worker_thread_count: usize,

    /// The maximum agent info entry count per space.
    ///
    /// All entries will be returned in a get space request, so
    /// this count should be low enough to reasonably send this response
    /// over http without needing pagination.
    ///
    /// Defaults:
    /// - `testing = 32`
    /// - `production = 32`
    pub max_entries_per_space: usize,

    /// The duration worker threads will block waiting for incoming connections
    /// before checking to see if the server is shutting down.
    ///
    /// Setting this very high will cause ctrl-c / server shutdown to be slow.
    /// Setting this very low will increase cpu overhead (and in extreme
    /// conditions, could cause a lack of responsiveness in the server).
    ///
    /// Defaults:
    /// - `testing = 10ms`
    /// - `production = 2s`
    pub request_listen_duration: std::time::Duration,
}

impl Config {
    /// Get a boot_srv config suitable for testing.
    pub fn testing() -> Self {
        Self {
            worker_thread_count: 2,
            max_entries_per_space: 32,
            request_listen_duration: std::time::Duration::from_millis(10),
        }
    }

    /// Get a boot_srv config suitable for production.
    pub fn production() -> Self {
        Self {
            worker_thread_count: num_cpus::get() * 4,
            max_entries_per_space: 32,
            request_listen_duration: std::time::Duration::from_secs(2),
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
        let config = Arc::new(config);
        let cont = Arc::new(std::sync::atomic::AtomicBool::new(true));

        let space_map = crate::SpaceMap::default();

        let server = Arc::new(
            Server::http("0.0.0.0:8080").map_err(std::io::Error::other)?,
        );

        let mut workers = Vec::with_capacity(config.worker_thread_count);
        for _ in 0..config.worker_thread_count {
            let config = config.clone();
            let cont = cont.clone();
            let server = server.clone();
            let space_map = space_map.clone();
            workers.push(std::thread::spawn(move || {
                worker(config, cont, server, space_map)
            }));
        }
        Ok(Self { cont, workers })
    }
}

fn worker(
    config: Arc<Config>,
    cont: Arc<std::sync::atomic::AtomicBool>,
    server: Arc<Server>,
    space_map: crate::SpaceMap,
) -> std::io::Result<()> {
    while cont.load(std::sync::atomic::Ordering::SeqCst) {
        let req = match server.recv_timeout(config.request_listen_duration)? {
            Some(req) => req,
            None => continue,
        };

        let path = req
            .url()
            .split('/')
            .rev()
            .filter_map(|p| {
                if p.is_empty() {
                    None
                } else {
                    Some(p.to_string())
                }
            })
            .collect::<Vec<_>>();

        println!("req: {} {path:?}", req.method());

        let handler = Handler {
            space_map: &space_map,
            method: req.method().as_str().to_string(),
            path,
            req,
        };

        handler.handle()?;
    }
    Ok(())
}

struct Handler<'lt> {
    space_map: &'lt crate::SpaceMap,
    method: String,
    path: Vec<String>,
    req: tiny_http::Request,
}

impl<'lt> Handler<'lt> {
    pub fn handle(mut self) -> std::io::Result<()> {
        match self.handle_inner() {
            Ok((status, body)) => self.respond(status, body),
            Err(err) => self.respond(
                500,
                serde_json::to_string(&serde_json::json!({
                    "error": format!("{err:?}"),
                }))?
                .into_bytes(),
            ),
        }
    }

    pub fn handle_inner(&mut self) -> std::io::Result<(u16, Vec<u8>)> {
        if let Some(cmd) = self.path.pop() {
            if &self.method == "GET" && &cmd == "boot" {
                return self.handle_boot();
            }
        }
        Ok((400, b"{\"error\":\"bad request\"}".to_vec()))
    }

    pub fn handle_boot(&mut self) -> std::io::Result<(u16, Vec<u8>)> {
        use base64::prelude::*;

        let space = match self.path.pop() {
            Some(space) => space,
            None => return Err(std::io::Error::other("InvalidSpace")),
        };

        let space = bytes::Bytes::copy_from_slice(
            &BASE64_URL_SAFE_NO_PAD
                .decode(space)
                .map_err(std::io::Error::other)?,
        );

        let res = self.space_map.read(&space)?;

        Ok((200, res))
    }

    pub fn read_body(&mut self) -> std::io::Result<Vec<u8>> {
        let mut buf = [0; 1024];
        let mut out = Vec::new();
        loop {
            let read = match self.req.as_reader().read(&mut buf[..]) {
                Ok(read) => read,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {
                    continue;
                }
                Err(e) => return Err(e),
            };
            if read == 0 {
                return Ok(out);
            }
            out.extend_from_slice(&buf[..read]);
            if out.len() > 1024 {
                return Err(std::io::Error::other("InfoTooLarge"));
            }
        }
    }

    fn respond(self, status: u16, bytes: Vec<u8>) -> std::io::Result<()> {
        let len = bytes.len();
        self.req.respond(Response::new(
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
