//! Authentication test fixtures.

use axum::response::IntoResponse;
use axum::{
    Router,
    http::{StatusCode, header},
    routing::put,
};
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

// Rust guideline compliant 2026-07-23

/// Runs a local authentication hook for integration tests.
#[derive(Debug)]
pub struct AuthHookServer {
    addr: SocketAddr,
    request_count: Arc<AtomicUsize>,
    shutdown_tx: Option<tokio::sync::oneshot::Sender<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl AuthHookServer {
    /// Starts an authentication hook on an available local port.
    ///
    /// When `expected_auth_material` is set, requests with other bodies are
    /// rejected. Successful requests receive a unique bearer token.
    ///
    /// # Panics
    ///
    /// Panics if the Tokio runtime or local listener cannot be created, or if
    /// the server thread does not report its address within five seconds.
    pub fn spawn(expected_auth_material: Option<Vec<u8>>) -> Self {
        let request_count = Arc::new(AtomicUsize::new(0));
        let request_count_for_handler = request_count.clone();
        let (started_tx, started_rx) = std::sync::mpsc::sync_channel(1);
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();

        let thread = std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_io()
                .enable_time()
                .build()
                .expect("auth hook Tokio runtime should build");

            runtime.block_on(async move {
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                    .await
                    .expect("auth hook should bind to a local port");
                let addr = listener
                    .local_addr()
                    .expect("auth hook should have a local address");
                let app = Router::new().route(
                    "/authenticate",
                    put(move |body: bytes::Bytes| {
                        let expected_auth_material =
                            expected_auth_material.clone();
                        let request_count = request_count_for_handler.clone();
                        async move {
                            if expected_auth_material.as_deref().is_some_and(
                                |expected| body.as_ref() != expected,
                            ) {
                                return (
                                    StatusCode::UNAUTHORIZED,
                                    "Unauthorized",
                                )
                                    .into_response();
                            }

                            let token_number = request_count
                                .fetch_add(1, Ordering::SeqCst)
                                + 1;
                            let body = serde_json::json!({
                                "authToken": format!(
                                    "test-auth-token-{token_number}"
                                ),
                            })
                            .to_string();
                            ([(header::CONTENT_TYPE, "application/json")], body)
                                .into_response()
                        }
                    }),
                );

                started_tx
                    .send(addr)
                    .expect("auth hook address receiver should be available");

                axum::serve(listener, app)
                    .with_graceful_shutdown(async {
                        let _ = shutdown_rx.await;
                    })
                    .await
                    .expect("auth hook server should run");
            });
        });

        let addr = started_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("auth hook did not start within five seconds");

        Self {
            addr,
            request_count,
            shutdown_tx: Some(shutdown_tx),
            thread: Some(thread),
        }
    }

    /// Returns the hook's listening address.
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// Returns the hook's base URL.
    pub fn url(&self) -> String {
        format!("http://{}", self.addr)
    }

    /// Returns the number of successful authentication requests.
    pub fn request_count(&self) -> usize {
        self.request_count.load(Ordering::SeqCst)
    }
}

impl Drop for AuthHookServer {
    fn drop(&mut self) {
        if let Some(shutdown_tx) = self.shutdown_tx.take() {
            let _ = shutdown_tx.send(());
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
