use crate::Config;
use crate::tls::TlsConfig;
use axum::*;
use axum_server::tls_rustls::RustlsAcceptor;
use http::{HeaderName, HeaderValue, Method};
use std::net::SocketAddr;
use std::str::FromStr;
use std::sync::Arc;
#[cfg(feature = "iroh-relay")]
use {tracing::info};

pub struct HttpResponse {
    pub status: u16,
    pub body: Vec<u8>,
}

impl HttpResponse {
    fn respond(self) -> response::Response {
        response::Response::builder()
            .status(self.status)
            .header("Content-Type", "application/json")
            .body(body::Body::from(self.body))
            .expect("failed to encode response")
    }
}

pub type HttpRespondCb = Box<dyn FnOnce(HttpResponse) + 'static + Send>;

pub enum HttpRequest {
    HealthGet,
    BootstrapGet {
        space: bytes::Bytes,
    },
    BootstrapPut {
        space: bytes::Bytes,
        agent: bytes::Bytes,
        body: bytes::Bytes,
    },
}

type HSend = async_channel::Sender<(HttpRequest, HttpRespondCb)>;
type HRecv = async_channel::Receiver<(HttpRequest, HttpRespondCb)>;

#[derive(Clone)]
pub struct HttpReceiver(HRecv);

impl HttpReceiver {
    pub fn recv(&self) -> Option<(HttpRequest, HttpRespondCb)> {
        self.0.recv_blocking().ok()
    }
}

pub struct ServerConfig {
    pub addrs: Vec<std::net::SocketAddr>,
    pub worker_thread_count: usize,
    pub tls_config: Option<TlsConfig>,
}

pub struct Server {
    t_join: Option<std::thread::JoinHandle<()>>,
    addrs: Vec<std::net::SocketAddr>,
    receiver: HttpReceiver,
    h_send: HSend,
    tls_reload_handle: Option<tokio::task::AbortHandle>,
    shutdown: Option<axum_server::Handle>,
    auth_tracker: crate::auth::AuthTokenTracker,
}

impl Drop for Server {
    fn drop(&mut self) {
        self.h_send.close();
        if let Some(shutdown) = self.shutdown.take() {
            shutdown.shutdown();
        }
        if let Some(t_join) = self.t_join.take() {
            let _ = t_join.join();
        }
        if let Some(tls_reload_handle) = self.tls_reload_handle.take() {
            tls_reload_handle.abort();
        }
    }
}

impl Server {
    pub fn new(
        config: Arc<Config>,
        server_config: ServerConfig,
    ) -> std::io::Result<Self> {
        let (s_ready, r_ready) = tokio::sync::oneshot::channel();
        let t_join = std::thread::spawn(move || {
            tokio_thread(config, server_config, s_ready)
        });
        match r_ready.blocking_recv() {
            Ok(Ok(Ready {
                h_send,
                addrs,
                receiver,
                tls_reload_handle,
                shutdown,
                auth_tracker,
            })) => Ok(Self {
                t_join: Some(t_join),
                addrs,
                receiver,
                h_send,
                tls_reload_handle,
                shutdown: Some(shutdown),
                auth_tracker,
            }),
            Ok(Err(err)) => Err(err),
            Err(_) => Err(std::io::Error::other("failed to bind server")),
        }
    }

    pub fn server_addrs(&self) -> &[std::net::SocketAddr] {
        self.addrs.as_slice()
    }

    pub fn receiver(&self) -> &HttpReceiver {
        &self.receiver
    }

    pub fn auth_tracker(&self) -> &crate::auth::AuthTokenTracker {
        &self.auth_tracker
    }
}

struct Ready {
    h_send: HSend,
    addrs: Vec<std::net::SocketAddr>,
    receiver: HttpReceiver,
    tls_reload_handle: Option<tokio::task::AbortHandle>,
    shutdown: axum_server::Handle,
    auth_tracker: crate::auth::AuthTokenTracker,
}

#[derive(Clone)]
pub struct AppState {
    pub h_send: HSend,

    // Feature-independent authentication
    pub auth_tracker: crate::auth::AuthTokenTracker,
    pub auth_config: Arc<crate::auth::AuthConfig>,
    pub auth_failures: opentelemetry::metrics::Counter<u64>,

    // SBD-specific (keep for SBD websockets)
    #[cfg(feature = "sbd")]
    pub sbd_state: Option<crate::sbd::SbdState>,

    #[cfg(feature = "iroh-relay")]
    pub iroh_relay_service: iroh_relay::server::RelayService,
}

type BoxFut<'a, T> =
    std::pin::Pin<Box<dyn std::future::Future<Output = T> + Send + 'a>>;

fn tokio_thread(
    config: Arc<Config>,
    server_config: ServerConfig,
    ready: tokio::sync::oneshot::Sender<std::io::Result<Ready>>,
) {
    tracing::trace!(?config, "Starting tokio thread");

    let allowed_headers =
        match ["Authorization", "Content-Type", "Content-Length", "Accept"]
            .iter()
            .map(|h| HeaderName::from_str(h))
            .collect::<Result<Vec<_>, _>>()
        {
            Ok(values) => tower_http::cors::AllowHeaders::list(values),
            Err(err) => {
                if ready.send(Err(std::io::Error::other(err))).is_err() {
                    tracing::error!("Failed to send ready error");
                }
                return;
            }
        };

    let origin = match &config.allowed_origins {
        Some(origins) if !origins.is_empty() => {
            match origins
                .iter()
                .map(|o| HeaderValue::from_str(o.as_str()))
                .collect::<Result<Vec<_>, _>>()
            {
                Ok(values) => tower_http::cors::AllowOrigin::list(values),
                Err(err) => {
                    if ready.send(Err(std::io::Error::other(err))).is_err() {
                        tracing::error!("Failed to send ready error");
                    }
                    return;
                }
            }
        }
        _ => tower_http::cors::AllowOrigin::any(),
    };

    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async move {
            let (h_send, h_recv) =
                async_channel::bounded(server_config.worker_thread_count);

            #[cfg(feature = "sbd")]
            let sbd_config = Arc::new(config.sbd.clone());

            #[cfg(feature = "sbd")]
            let ip_rate = Arc::new(sbd_server::IpRate::new(sbd_config.clone()));

            #[cfg(feature = "sbd")]
            let sbd_server_meter = opentelemetry::global::meter("sbd-server");

            #[cfg(feature = "sbd")]
            let c_slot = if config.no_relay_server {
                None
            } else {
                Some(sbd_server::CSlot::new(sbd_config.clone(), ip_rate.clone(), sbd_server_meter.clone()))
            };

            let (rustls_config, tls_reload_handle) = if let Some(tls_config) = server_config.tls_config {
                let rustls_config = tls_config
                    .create_tls_config()
                    .await
                    .expect("Failed to create TLS config");

                let tls_reload_handle = tokio::task::spawn(tls_config.reload_task(rustls_config.clone())).abort_handle();

                (Some(rustls_config), Some(tls_reload_handle))
            } else {
                (None, None)
            };

            // Initialize feature-independent authentication
            let meter = opentelemetry::global::meter("bootstrap-auth");
            let auth_failures = meter
                .u64_counter("bootstrap.auth_failures")
                .with_description("Number of failed authentication attempts")
                .with_unit("count")
                .build();
            let auth_tracker = crate::auth::AuthTokenTracker::default();
            let auth_config = Arc::new(config.auth.clone());

            // CORS credentials based on auth config
            let allow_credentials = config.auth.authentication_hook_server.is_some();

            let mut app = Router::<AppState>::new()
                .route("/authenticate", routing::put(handle_auth))
                .route("/health", routing::get(handle_health_get))
                .route("/bootstrap/{space}", routing::get(handle_boot_get))
                .route(
                    "/bootstrap/{space}/{agent}",
                    routing::put(handle_boot_put),
                )
                .layer(tower_http::cors::CorsLayer::new()
                    .allow_methods(tower_http::cors::AllowMethods::list([Method::GET, Method::PUT, Method::OPTIONS]))
                    .allow_headers(allowed_headers)
                    .allow_origin(origin)
                    .allow_credentials(allow_credentials)
                );

            #[cfg(feature = "iroh-relay")]
            let iroh_relay_service = crate::iroh_relay::create_relay_service(&config.iroh_relay.limits);

            if !config.no_relay_server {
                #[cfg(feature = "sbd")]
                {
                    app = app.route("/{pub_key}", routing::get(crate::sbd::handle_sbd));
                }
                #[cfg(feature = "iroh-relay")]
                {
                    info!("Embedded iroh relay service created");
                    // The relay service handles the /relay endpoint and will be used as a fallback
                    app = app.fallback(handle_iroh_relay);
                }
            }

            // Clone auth_tracker before moving it into AppState so we can return it
            let auth_tracker_for_ready = auth_tracker.clone();

            let app: Router = app
                .layer(extract::DefaultBodyLimit::max(1024))
                .with_state(AppState {
                    h_send: h_send.clone(),
                    auth_tracker,
                    auth_config,
                    auth_failures,
                    #[cfg(feature = "sbd")]
                    sbd_state: if config.no_relay_server {
                        None
                    } else {
                        {
                            Some(crate::sbd::SbdState {
                                config: sbd_config.clone(),
                                ip_rate: ip_rate.clone(),
                                c_slot: c_slot.as_ref().expect("Missing c_slot with SBD enabled").weak(),
                            })
                        }
                    },
                    #[cfg(feature = "iroh-relay")]
                    iroh_relay_service,
                });

            let receiver = HttpReceiver(h_recv);

            let mut addrs = Vec::with_capacity(server_config.addrs.len());
            let mut servers: Vec<BoxFut<'static, std::io::Result<()>>> =
                Vec::with_capacity(server_config.addrs.len());

            let shutdown_handle = axum_server::Handle::new();

            for addr in server_config.addrs {
                tracing::info!("Binding to: {}", addr);

                let listener = match tokio::task::spawn_blocking(move || {
                    std::net::TcpListener::bind(addr)
                })
                .await
                .expect("Failed to run bind task")
                {
                    Ok(listener) => listener,
                    Err(err) => {
                        let _ = ready.send(Err(err));
                        return;
                    }
                };

                match listener.local_addr() {
                    Ok(addr) => {
                        tracing::info!("Bound with local address: {}", addr);
                        addrs.push(addr)
                    },
                    Err(err) => {
                        let _ = ready.send(Err(err));
                        return;
                    }
                }

                let app = app.clone();
                let shutdown_handle = shutdown_handle.clone();
                if let Some(tls_config) = &rustls_config {
                    let acceptor = RustlsAcceptor::new(tls_config.clone());

                    #[cfg(feature = "sbd")]
                    let acceptor = {
                        acceptor.acceptor(crate::sbd::SbdAcceptor::new(
                                sbd_config.clone(),
                                ip_rate.clone(),
                            ))
                    };

                    let s = axum_server::Server::from_tcp(listener)
                        .acceptor(acceptor)
                        .handle(shutdown_handle)
                        .serve(app.into_make_service_with_connect_info::<SocketAddr>());

                    servers.push(Box::pin(s));
                } else {
                    let server = axum_server::Server::from_tcp(listener);

                    #[cfg(feature = "sbd")]
                    let server = {
                        server.acceptor(crate::sbd::SbdAcceptor::new(
                            sbd_config.clone(),
                            ip_rate.clone(),
                        ))
                    };

                    let s = std::future::IntoFuture::into_future(
                        server
                            .handle(shutdown_handle)
                            .serve(app.into_make_service_with_connect_info::<SocketAddr>()),
                    );
                    servers.push(Box::pin(s));
                };
            }

            tracing::info!("Sending ready signal");

            if ready
                .send(Ok(Ready {
                    h_send,
                    addrs,
                    receiver,
                    tls_reload_handle,
                    shutdown: shutdown_handle,
                    auth_tracker: auth_tracker_for_ready,
                }))
                .is_err()
            {
                return;
            }

            let _ = futures::future::join_all(servers).await;
        });
}

async fn handle_auth(
    extract::State(state): extract::State<AppState>,
    body: bytes::Bytes,
) -> axum::response::Response {
    match crate::auth::process_authenticate(
        &state.auth_config,
        &state.auth_tracker,
        state.auth_failures,
        body,
    )
    .await
    {
        Ok(token) => axum::response::IntoResponse::into_response(axum::Json(
            serde_json::json!({
                "authToken": *token,
            }),
        )),
        Err(crate::auth::AuthenticateError::Unauthorized) => {
            tracing::debug!("/authenticate: UNAUTHORIZED");
            axum::response::IntoResponse::into_response((
                axum::http::StatusCode::UNAUTHORIZED,
                "Unauthorized",
            ))
        }
        Err(crate::auth::AuthenticateError::HookServerError(err)) => {
            tracing::debug!(?err, "/authenticate: BAD_GATEWAY");
            axum::response::IntoResponse::into_response((
                axum::http::StatusCode::BAD_GATEWAY,
                format!("BAD_GATEWAY: {err:?}"),
            ))
        }
        Err(crate::auth::AuthenticateError::OtherError(err)) => {
            tracing::warn!(?err, "/authenticate: INTERNAL_SERVER_ERROR");
            axum::response::IntoResponse::into_response((
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                format!("INTERNAL_SERVER_ERROR: {err:?}"),
            ))
        }
    }
}

async fn handle_dispatch(
    h_send: &HSend,
    req: HttpRequest,
) -> response::Response {
    let (s, r) = tokio::sync::oneshot::channel();
    let s = Box::new(move |res| {
        let _ = s.send(res);
    });
    tokio::time::timeout(std::time::Duration::from_secs(10), async move {
        let _ = h_send.send((req, s)).await;
        match r.await {
            Ok(r) => r.respond(),
            Err(_) => HttpResponse {
                status: 500,
                body: b"{\"error\":\"request dropped\"}".to_vec(),
            }
            .respond(),
        }
    })
    .await
    .unwrap_or_else(|_| {
        HttpResponse {
            status: 500,
            body: b"{\"error\":\"internal timeout\"}".to_vec(),
        }
        .respond()
    })
}

async fn handle_health_get(
    extract::State(state): extract::State<AppState>,
) -> response::Response {
    // NOTE - This health call is currently not protected by auth token.
    //        Perhaps in the future, this should be configurable so
    //        infrastructure maintainers can weigh the trade-offs.

    handle_dispatch(&state.h_send, HttpRequest::HealthGet).await
}

async fn handle_boot_get(
    extract::Path(space): extract::Path<String>,
    headers: axum::http::HeaderMap,
    extract::State(state): extract::State<AppState>,
) -> response::Response {
    // Check authentication (feature-independent)
    let token: Option<Arc<str>> = headers
        .get("Authorization")
        .and_then(|t| t.to_str().ok())
        .and_then(|t| t.strip_prefix("Bearer "))
        .map(<Arc<str>>::from);

    if !state.auth_tracker.is_valid(&token, &state.auth_config) {
        return axum::response::IntoResponse::into_response((
            axum::http::StatusCode::UNAUTHORIZED,
            "Unauthorized",
        ));
    }

    let space = match b64_to_bytes(&space) {
        Ok(space) => space,
        Err(err) => return *err,
    };
    handle_dispatch(&state.h_send, HttpRequest::BootstrapGet { space }).await
}

async fn handle_boot_put(
    extract::Path((space, agent)): extract::Path<(String, String)>,
    headers: axum::http::HeaderMap,
    extract::State(state): extract::State<AppState>,
    body: bytes::Bytes,
) -> response::Response<body::Body> {
    // Check authentication (feature-independent)
    let token: Option<Arc<str>> = headers
        .get("Authorization")
        .and_then(|t| t.to_str().ok())
        .and_then(|t| t.strip_prefix("Bearer "))
        .map(<Arc<str>>::from);

    if !state.auth_tracker.is_valid(&token, &state.auth_config) {
        return axum::response::IntoResponse::into_response((
            axum::http::StatusCode::UNAUTHORIZED,
            "Unauthorized",
        ));
    }

    let space = match b64_to_bytes(&space) {
        Ok(space) => space,
        Err(err) => return *err,
    };
    let agent = match b64_to_bytes(&agent) {
        Ok(agent) => agent,
        Err(err) => return *err,
    };
    handle_dispatch(
        &state.h_send,
        HttpRequest::BootstrapPut { space, agent, body },
    )
    .await
}

fn b64_to_bytes(
    s: &str,
) -> Result<bytes::Bytes, Box<response::Response<body::Body>>> {
    use base64::prelude::*;
    Ok(bytes::Bytes::copy_from_slice(
        &match BASE64_URL_SAFE_NO_PAD.decode(s) {
            Ok(b) => b,
            Err(err) => {
                return Err(Box::new(
                    HttpResponse {
                        status: 400,
                        body: err.to_string().into_bytes(),
                    }
                    .respond(),
                ));
            }
        },
    ))
}

#[cfg(feature = "iroh-relay")]
async fn handle_iroh_relay(
    extract::State(state): extract::State<AppState>,
    req: http::Request<body::Body>,
) -> response::Response {
    use http_body_util::BodyExt;
    // Import the Service trait from hyper
    use hyper::service::Service;

    // Convert the axum request to a hyper request
    // We need to read the axum body and create a hyper-compatible body
    let (parts, axum_body) = req.into_parts();

    // Read the entire body into bytes
    let body_bytes = match axum::body::to_bytes(axum_body, usize::MAX).await {
        Ok(bytes) => bytes,
        Err(err) => {
            tracing::error!("Error reading request body: {:?}", err);
            return response::Response::builder()
                .status(500)
                .body(body::Body::from("Failed to read request body"))
                .unwrap();
        }
    };

    // Create a hyper Incoming body from the bytes
    // Note: We use Full<Bytes> which can be used where Incoming is expected
    let hyper_body = http_body_util::Full::new(body_bytes);
    let hyper_req = http::Request::from_parts(parts, hyper_body);

    // Call the relay service using the Service trait
    let mut service = state.iroh_relay_service.clone();
    match service.call(hyper_req).await {
        Ok(response) => {
            // Convert the response body back to axum body
            let (parts, hyper_body) = response.into_parts();
            let bytes = match hyper_body.collect().await {
                Ok(collected) => collected.to_bytes(),
                Err(err) => {
                    tracing::error!("Error collecting response body: {:?}", err);
                    return response::Response::builder()
                        .status(500)
                        .body(body::Body::from("Failed to collect response body"))
                        .unwrap();
                }
            };
            http::Response::from_parts(parts, body::Body::from(bytes))
        }
        Err(err) => {
            tracing::error!("Error handling relay request: {:?}", err);
            response::Response::builder()
                .status(500)
                .body(body::Body::from("Internal Server Error"))
                .unwrap()
        }
    }
}
