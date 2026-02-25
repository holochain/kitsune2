//! Test utilities associated with bootstrapping.

use axum::*;
use std::sync::atomic::*;
use std::sync::{Arc, Mutex};

/// A test bootstrap server with optional relay support.
///
/// When the "relay" feature is enabled, this server also provides
/// iroh relay endpoints at `/relay` and `/ping`.
pub struct TestBootstrapSrv {
    kill: Option<tokio::sync::oneshot::Sender<()>>,
    task: tokio::task::JoinHandle<std::io::Result<()>>,
    halt: Arc<std::sync::atomic::AtomicBool>,
    addr: String,
    #[cfg(feature = "relay")]
    _relay_state: Option<kitsune2_bootstrap_srv::iroh_relay::RelayState>,
}

impl Drop for TestBootstrapSrv {
    fn drop(&mut self) {
        if let Some(kill) = self.kill.take() {
            let _ = kill.send(());
        }
        self.task.abort();
    }
}

impl TestBootstrapSrv {
    /// Construct a test bootstrap server.
    ///
    /// When compiled with the "relay" feature, this server also provides
    /// iroh relay functionality at `/relay` and `/ping` endpoints.
    pub async fn new(initial_halt: bool) -> Self {
        let (kill, kill_r) = tokio::sync::oneshot::channel();
        let kill = Some(kill);
        let kill_r = async move {
            let _ = kill_r.await;
        };

        let l = tokio::net::TcpListener::bind(std::net::SocketAddr::from((
            [127, 0, 0, 1],
            0,
        )))
        .await
        .unwrap();
        let addr = format!("http://{:?}", l.local_addr().unwrap());

        let halt = Arc::new(std::sync::atomic::AtomicBool::new(initial_halt));

        #[derive(Clone)]
        struct State {
            halt: Arc<AtomicBool>,
            data: Arc<Mutex<Vec<String>>>,
        }

        let get_state = State {
            halt: halt.clone(),
            data: Arc::new(Mutex::new(Vec::new())),
        };
        let put_state = get_state.clone();

        #[cfg_attr(not(feature = "relay"), allow(unused_mut))]
        let mut app: Router = Router::new()
            .route(
                "/bootstrap/{space}",
                routing::get(move || async move {
                    if get_state.halt.load(Ordering::SeqCst) {
                        return Err(
                            http::status::StatusCode::INTERNAL_SERVER_ERROR,
                        );
                    }
                    let mut out = "[".to_string();
                    let mut is_first = true;
                    for d in get_state.data.lock().unwrap().iter() {
                        if is_first {
                            is_first = false;
                        } else {
                            out.push(',');
                        }
                        out.push_str(d);
                    }
                    out.push(']');
                    Ok(out)
                }),
            )
            .route(
                "/bootstrap/{space}/{agent}",
                routing::put(move |body: String| async move {
                    if put_state.halt.load(Ordering::SeqCst) {
                        return Err(
                            http::status::StatusCode::INTERNAL_SERVER_ERROR,
                        );
                    }
                    put_state.data.lock().unwrap().push(body);
                    Ok("{}".to_string())
                }),
            );

        // Add relay routes when the feature is enabled
        #[cfg(feature = "relay")]
        let relay_state = {
            let relay_state =
                kitsune2_bootstrap_srv::iroh_relay::create_relay_state();

            let relay_router = Router::new()
                .route(
                    "/relay",
                    routing::get(
                        kitsune2_bootstrap_srv::iroh_relay::relay_handler,
                    ),
                )
                .route(
                    "/ping",
                    routing::get(
                        kitsune2_bootstrap_srv::iroh_relay::relay_probe_handler,
                    ),
                )
                .with_state(relay_state.clone());

            app = app.merge(relay_router);
            Some(relay_state)
        };

        let task = tokio::task::spawn(std::future::IntoFuture::into_future(
            serve(l, app).with_graceful_shutdown(kill_r),
        ));

        Self {
            kill,
            task,
            halt,
            addr,
            #[cfg(feature = "relay")]
            _relay_state: relay_state,
        }
    }

    /// Set whether server should return an error.
    pub fn set_halt(&self, halt: bool) {
        self.halt.store(halt, std::sync::atomic::Ordering::SeqCst);
    }

    /// Return server address.
    pub fn addr(&self) -> &str {
        &self.addr
    }
}
