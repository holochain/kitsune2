//! bootstrap http server types.

use std::sync::Arc;

use crate::*;
use tiny_http::*;

/// Don't allow created_at to be greater than this far away from now.
/// 3 minutes.
const CREATED_AT_CLOCK_SKEW_ALLOWED_MICROS: i64 =
    std::time::Duration::from_secs(60 * 3).as_micros() as i64;

/// Don't allow expires_at - created_at to be greater than this duration.
/// 30 minutes.
const EXPIRES_AT_DURATION_MAX_ALLOWED_MICROS: i64 =
    std::time::Duration::from_secs(60 * 30).as_micros() as i64;

/// Print out a message if this thread dies.
struct ThreadGuard(&'static str);

impl Drop for ThreadGuard {
    fn drop(&mut self) {
        eprintln!("{}", self.0);
    }
}

/// An actual kitsune2_bootstrap_srv server instance.
///
/// This server is built to be direct, light-weight, and responsive.
/// On the server-side, as one aspect toward accomplishing this,
/// we are eschewing async code in favor of os thread workers.
pub struct BootstrapSrv {
    cont: Arc<std::sync::atomic::AtomicBool>,
    workers: Vec<std::thread::JoinHandle<std::io::Result<()>>>,
    addr: std::net::SocketAddr,
}

impl Drop for BootstrapSrv {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

impl BootstrapSrv {
    /// Construct a new BootstrapSrv instance.
    pub fn new(config: Config) -> std::io::Result<Self> {
        let config = Arc::new(config);

        // atomic flag for telling worker threads to shutdown
        let cont = Arc::new(std::sync::atomic::AtomicBool::new(true));

        // synchronization type for managing infos in spaces
        let space_map = crate::SpaceMap::default();

        // tiny_http configuration
        let sconf = ServerConfig {
            addr: ConfigListenAddr::IP(vec![config.listen_address]),
            // TODO make the server able to accept TLS certificates
            ssl: None,
        };

        // virtual-memory-like file system storage for infos
        let store = Arc::new(crate::Store::default());

        // start the actual http server
        let server =
            Arc::new(Server::new(sconf).map_err(std::io::Error::other)?);

        // get the address that was assigned
        let addr = server.server_addr().to_ip().expect("BadAddress");
        println!("Listening at {:?}", addr);

        // spawn our worker threads
        let mut workers = Vec::with_capacity(config.worker_thread_count + 1);
        for _ in 0..config.worker_thread_count {
            let config = config.clone();
            let cont = cont.clone();
            let store = store.clone();
            let server = server.clone();
            let space_map = space_map.clone();
            workers.push(std::thread::spawn(move || {
                worker(config, cont, store, server, space_map)
            }));
        }

        // also set up a worker for pruning expired infos
        let prune_cont = cont.clone();
        let prune_space_map = space_map.clone();
        workers.push(std::thread::spawn(move || {
            prune_worker(config, prune_cont, prune_space_map)
        }));

        Ok(Self {
            cont,
            workers,
            addr,
        })
    }

    /// Shutdown the server, returning an error result if any
    /// of the worker threads had panicked.
    pub fn shutdown(&mut self) -> std::io::Result<()> {
        let mut is_err = false;
        self.cont.store(false, std::sync::atomic::Ordering::SeqCst);
        for worker in self.workers.drain(..) {
            if worker.join().is_err() {
                is_err = true;
            }
        }
        if is_err {
            Err(std::io::Error::other("Failure shutting down worker thread"))
        } else {
            Ok(())
        }
    }

    /// Get the bound listening address of this server.
    pub fn listen_addr(&self) -> std::net::SocketAddr {
        self.addr
    }
}

fn prune_worker(
    config: Arc<Config>,
    cont: Arc<std::sync::atomic::AtomicBool>,
    space_map: crate::SpaceMap,
) -> std::io::Result<()> {
    let _g = ThreadGuard("WARN: prune_worker thread has ended");

    let mut last_check = std::time::Instant::now();

    while cont.load(std::sync::atomic::Ordering::SeqCst) {
        std::thread::sleep(config.request_listen_duration);

        if last_check.elapsed() >= config.prune_interval {
            last_check = std::time::Instant::now();

            space_map.update_all(config.max_entries_per_space);
        }
    }

    Ok(())
}

fn worker(
    config: Arc<Config>,
    cont: Arc<std::sync::atomic::AtomicBool>,
    store: Arc<crate::Store>,
    server: Arc<Server>,
    space_map: crate::SpaceMap,
) -> std::io::Result<()> {
    let _g = ThreadGuard("WARN: worker thread has ended");

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

        let handler = Handler {
            config: &config,
            store: &store,
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
    config: &'lt Config,
    store: &'lt crate::Store,
    space_map: &'lt crate::SpaceMap,
    method: String,
    path: Vec<String>,
    req: tiny_http::Request,
}

impl<'lt> Handler<'lt> {
    /// Wrap the handle call so we can respond to the client with errors.
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

    /// Dispatch to the correct handlers.
    fn handle_inner(&mut self) -> std::io::Result<(u16, Vec<u8>)> {
        if let Some(cmd) = self.path.pop() {
            match (self.method.as_str(), cmd.as_str()) {
                ("GET", "health") => {
                    return Ok((200, b"{}".to_vec()));
                }
                ("GET", "bootstrap") => {
                    return self.handle_boot_get();
                }
                ("PUT", "bootstrap") => {
                    return self.handle_boot_put();
                }
                _ => (),
            }
        }
        Ok((400, b"{\"error\":\"bad request\"}".to_vec()))
    }

    /// Respond to a request for the agent infos within a space.
    fn handle_boot_get(&mut self) -> std::io::Result<(u16, Vec<u8>)> {
        let space = self.path_to_bytes()?;

        let res = self.space_map.read(&space)?;

        Ok((200, res))
    }

    /// Validate an incoming agent info and put it in the store if appropriate.
    fn handle_boot_put(&mut self) -> std::io::Result<(u16, Vec<u8>)> {
        use ed25519_dalek::*;

        let now = crate::now();

        let space = self.path_to_bytes()?;
        let agent = self.path_to_bytes()?;

        let info_raw = self.read_body()?;
        let info = crate::ParsedEntry::try_from_slice(&info_raw)?;

        // validate agent matches url path
        if *agent != *info.agent.as_bytes() {
            return Err(std::io::Error::other("InvalidAgent"));
        }

        // validate space matches url path
        if space != info.space {
            return Err(std::io::Error::other("InvalidSpace"));
        }

        // validate created at is not older than 3 min ago
        if info.created_at < now - CREATED_AT_CLOCK_SKEW_ALLOWED_MICROS {
            return Err(std::io::Error::other("InvalidCreatedAt"));
        }

        // validate created at is less than 3 min in the future
        if info.created_at > now + CREATED_AT_CLOCK_SKEW_ALLOWED_MICROS {
            return Err(std::io::Error::other("InvalidCreatedAt"));
        }

        // validate not expired
        if info.expires_at < now {
            return Err(std::io::Error::other("InvalidExpiresAt"));
        }

        // validate expires_at is not before (or equal to) created_at
        if info.expires_at <= info.created_at {
            return Err(std::io::Error::other("InvalidExpiresAt"));
        }

        // validate expires_at is not more than 30 min after created_at
        if info.expires_at - info.created_at
            > EXPIRES_AT_DURATION_MAX_ALLOWED_MICROS
        {
            return Err(std::io::Error::other("InvalidExpiresAt"));
        }

        // validate signature (do this at the end because it's more expensive)
        info.agent
            .verify(info.encoded.as_bytes(), &info.signature)
            .map_err(|err| {
                std::io::Error::other(format!("InvalidSignature: {err:?}"))
            })?;

        let r = if info.is_tombstone {
            None
        } else {
            Some(self.store.write(&info_raw)?)
        };

        self.space_map.update(
            self.config.max_entries_per_space,
            space,
            Some((info, r)),
        );

        Ok((200, b"{}".to_vec()))
    }

    /// Helper to get the next path segment as Bytes.
    fn path_to_bytes(&mut self) -> std::io::Result<bytes::Bytes> {
        use base64::prelude::*;

        let p = match self.path.pop() {
            Some(p) => p,
            None => return Err(std::io::Error::other("InvalidPathSegment")),
        };

        Ok(bytes::Bytes::copy_from_slice(
            &BASE64_URL_SAFE_NO_PAD
                .decode(p)
                .map_err(std::io::Error::other)?,
        ))
    }

    /// Read the body while respecting our max message size.
    fn read_body(&mut self) -> std::io::Result<Vec<u8>> {
        // these are the same right now, but *could* be different
        const MAX_INFO_SIZE: usize = 1024;
        const READ_BUF_SIZE: usize = 1024;

        let mut buf = [0; READ_BUF_SIZE];
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
            if out.len() > MAX_INFO_SIZE {
                return Err(std::io::Error::other("InfoTooLarge"));
            }
        }
    }

    /// Process the response.
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
