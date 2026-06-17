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
    halt: Arc<std::sync::atomic::AtomicBool>,
    addr: String,
    #[cfg(feature = "relay")]
    _relay_state: Option<kitsune2_bootstrap_srv::iroh_relay_axum::RelayState>,
}

impl Drop for TestBootstrapSrv {
    fn drop(&mut self) {
        // Sending the kill signal causes the server task to call
        // clients.shutdown(), which closes all relay WebSocket connections.
        // This lets connected iroh endpoints detect relay loss immediately
        // rather than waiting for the QUIC idle timeout.
        if let Some(kill) = self.kill.take() {
            let _ = kill.send(());
        }
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
                kitsune2_bootstrap_srv::iroh_relay_axum::create_relay_state(
                    None,
                );

            let relay_router = Router::new()
                .route(
                    "/relay",
                    routing::get(
                        kitsune2_bootstrap_srv::iroh_relay_axum::relay_handler,
                    ),
                )
                .route(
                    "/ping",
                    routing::get(
                        kitsune2_bootstrap_srv::iroh_relay_axum::relay_probe_handler,
                    ),
                )
                .with_state(relay_state.clone());

            app = app.merge(relay_router);
            Some(relay_state)
        };

        // Clone the relay clients handle so the task can shut down connections
        // on drop.  Closing the connections lets connected iroh endpoints
        // detect relay loss promptly (rather than waiting for QUIC idle
        // timeout), which is essential for tests that simulate relay outages.
        #[cfg(feature = "relay")]
        let relay_clients = relay_state.as_ref().map(|s| s.clients.clone());

        tokio::task::spawn(async move {
            // Stop accepting when the kill signal fires.
            tokio::select! {
                _ = kill_r => {},
                result = serve(l, app) => { let _ = result; }
            }

            // Gracefully close all relay WebSocket connections so iroh
            // endpoints see the relay as disconnected immediately.
            #[cfg(feature = "relay")]
            if let Some(clients) = relay_clients {
                clients.shutdown().await;
            }
        });

        Self {
            kill,
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
