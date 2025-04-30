//! bootstrap http server types.

use std::sync::Arc;

use crate::*;

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
        tracing::debug!("{}", self.0);
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
    addrs: Vec<std::net::SocketAddr>,
    server: Option<Server>,
}

impl Drop for BootstrapSrv {
    fn drop(&mut self) {
        let _g = ThreadGuard("Server Shutdown Complete!");

        tracing::debug!("begin server shutdown...");
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
            addrs: config.listen_address_list.clone(),
            worker_thread_count: config.worker_thread_count,
            tls_config: if let (Some(cert), Some(key)) =
                (&config.tls_cert, &config.tls_key)
            {
                Some(TlsConfig::new(cert.clone(), key.clone()))
            } else {
                None
            },
        };

        // virtual-memory-like file system storage for infos
        let store = Arc::new(crate::Store::default());

        // start the actual http server
        let server = Server::new(config.clone(), sconf)
            .map_err(std::io::Error::other)?;

        // get the address that was assigned
        let addrs = server.server_addrs().to_vec();
        tracing::info!(?addrs, "Listening");

        // spawn our worker threads
        let mut workers = Vec::with_capacity(config.worker_thread_count + 1);
        for _ in 0..config.worker_thread_count {
            let config = config.clone();
            let cont = cont.clone();
            let store = store.clone();
            let recv = server.receiver().clone();
            let space_map = space_map.clone();
            workers.push(std::thread::spawn(move || {
                worker(config, cont, store, recv, space_map)
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
            addrs,
            server: Some(server),
        })
    }

    /// Shutdown the server, returning an error result if any
    /// of the worker threads had panicked.
    pub fn shutdown(&mut self) -> std::io::Result<()> {
        let mut is_err = false;
        self.cont.store(false, std::sync::atomic::Ordering::SeqCst);
        drop(self.server.take());
        while !self.workers.is_empty() {
            tracing::debug!(
                "waiting on {} threads to close...",
                self.workers.len()
            );
            if self.workers.pop().unwrap().join().is_err() {
                is_err = true;
            }
        }
        tracing::debug!("all threads closed.");
        if is_err {
            Err(std::io::Error::other("Failure shutting down worker thread"))
        } else {
            Ok(())
        }
    }

    /// Get the bound listening addresses of this server.
    pub fn listen_addrs(&self) -> &[std::net::SocketAddr] {
        self.addrs.as_slice()
    }

    /// Print the address server started on
    pub fn print_addrs(&self) {
        println!("#kitsune2_bootstrap_srv#running#");
        for addr in self.addrs.iter() {
            // print these incase someone wants to parse for them
            println!("#kitsune2_bootstrap_srv#listening#{addr:?}#");
        }
    }
}

fn prune_worker(
    config: Arc<Config>,
    cont: Arc<std::sync::atomic::AtomicBool>,
    space_map: crate::SpaceMap,
) -> std::io::Result<()> {
    let _g = ThreadGuard("prune_worker thread has ended");

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
    recv: HttpReceiver,
    space_map: crate::SpaceMap,
) -> std::io::Result<()> {
    let _g = ThreadGuard("worker thread has ended");

    while cont.load(std::sync::atomic::Ordering::SeqCst) {
        let (req, res) = match recv.recv() {
            None => break,
            Some(r) => r,
        };

        let handler = Handler {
            config: &config,
            store: &store,
            space_map: &space_map,
            res,
        };

        handler.handle(req)?;
    }

    Ok(())
}

struct Handler<'lt> {
    config: &'lt Config,
    store: &'lt crate::Store,
    space_map: &'lt crate::SpaceMap,
    res: HttpRespondCb,
}

impl Handler<'_> {
    /// Wrap the handle call so we can respond to the client with errors.
    pub fn handle(mut self, req: HttpRequest) -> std::io::Result<()> {
        match self.handle_inner(req) {
            Ok((status, body)) => self.respond(status, body),
            Err(err) => self.respond(
                500,
                serde_json::to_string(&serde_json::json!({
                    "error": format!("{err:?}"),
                }))?
                .into_bytes(),
            ),
        }

        Ok(())
    }

    /// Dispatch to the correct handlers.
    fn handle_inner(
        &mut self,
        req: HttpRequest,
    ) -> std::io::Result<(u16, Vec<u8>)> {
        match req {
            HttpRequest::HealthGet => Ok((200, b"{}".to_vec())),
            HttpRequest::BootstrapGet { space } => self.handle_boot_get(space),
            HttpRequest::BootstrapPut { space, agent, body } => {
                self.handle_boot_put(space, agent, body)
            }
        }
    }

    /// Respond to a request for the agent infos within a space.
    fn handle_boot_get(
        &mut self,
        space: bytes::Bytes,
    ) -> std::io::Result<(u16, Vec<u8>)> {
        let res = self.space_map.read(&space)?;

        Ok((200, res))
    }

    /// Validate an incoming agent info and put it in the store if appropriate.
    fn handle_boot_put(
        &mut self,
        space: bytes::Bytes,
        agent: bytes::Bytes,
        body: bytes::Bytes,
    ) -> std::io::Result<(u16, Vec<u8>)> {
        use ed25519_dalek::*;

        let now = crate::now();

        let info = crate::ParsedEntry::try_from_slice(&body)?;

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

        tracing::info!("Basic checks passed");

        // validate signature (do this at the end because it's more expensive)
        info.agent
            .verify(info.encoded.as_bytes(), &info.signature)
            .map_err(|err| {
                std::io::Error::other(format!("InvalidSignature: {err:?}"))
            })?;

        let r = if info.is_tombstone {
            None
        } else {
            Some(self.store.write(&body)?)
        };

        self.space_map.update(
            self.config.max_entries_per_space,
            space,
            Some((info, r)),
        );

        Ok((200, b"{}".to_vec()))
    }

    /// Process the response.
    fn respond(self, status: u16, body: Vec<u8>) {
        let Self { res, .. } = self;
        res(HttpResponse { status, body });
    }
}
