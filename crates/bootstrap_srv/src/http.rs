use crate::Config;
use crate::tls::TlsConfig;
use axum::*;
use axum_server::tls_rustls::RustlsAcceptor;
use http::{HeaderName, HeaderValue, Method};
use std::net::SocketAddr;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

/// Maximum time a client may take to get from an accepted TCP connection to a
/// fully-read HTTP request head (including the TLS handshake on TLS listeners).
///
/// This bounds the connection-establishment phase so a stalled or malicious
/// client cannot hold a half-open connection open indefinitely without ever
/// completing the WebSocket upgrade on the `/relay` route. It mirrors
/// `iroh-relay`'s own 30s relay establish timeout, which Kitsune2 bypasses by
/// serving the relay through its own axum route instead of iroh's
/// `RelayService`. Once the request head is read the timeout no longer applies,
/// so it does not interfere with long-lived relay connections; those are bound
/// by the relay protocol handshake timeout and per-client write timeout.
const ESTABLISH_TIMEOUT: Duration = Duration::from_secs(30);

/// An [`axum_server`] acceptor that bounds the connection-establishment phase
/// with a single wall-clock deadline, mirroring `iroh-relay`'s own relay
/// establish timeout.
///
/// One absolute deadline (`now + timeout`, computed when the connection is
/// accepted) covers both phases of establishment:
///
/// 1. The wrapped acceptor's handshake — e.g. the TLS handshake performed by
///    [`RustlsAcceptor`]. A peer cannot stall mid-handshake past the deadline.
/// 2. Reading the request head, via [`EstablishTimeoutStream`], which carries
///    the *same* deadline forward. This bounds both a silent peer that never
///    sends a first byte (hyper's auto HTTP-version detection blocks on that
///    byte) and a peer that trickles the head without ever finishing it.
///
/// Once the server writes its first response byte — which only happens after
/// the request head has been read and routed — the deadline is disarmed and
/// the wrapper is transparent for the rest of the connection's life, so
/// long-lived relay connections are unaffected.
#[derive(Clone)]
struct EstablishTimeoutAcceptor<A> {
    inner: A,
    timeout: Duration,
}

impl<A> EstablishTimeoutAcceptor<A> {
    fn new(inner: A, timeout: Duration) -> Self {
        Self { inner, timeout }
    }
}

impl<A, I, S> axum_server::accept::Accept<I, S> for EstablishTimeoutAcceptor<A>
where
    A: axum_server::accept::Accept<I, S>,
    A::Future: Send + 'static,
    A::Stream: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send,
    A::Service: Send,
{
    type Stream = EstablishTimeoutStream<A::Stream>;
    type Service = A::Service;
    type Future = std::pin::Pin<
        Box<
            dyn std::future::Future<
                    Output = std::io::Result<(Self::Stream, Self::Service)>,
                > + Send,
        >,
    >;

    fn accept(&self, stream: I, service: S) -> Self::Future {
        let fut = self.inner.accept(stream, service);
        // One absolute deadline for the whole establishment phase, carried
        // through the handshake and then into the request-head read.
        let deadline = tokio::time::Instant::now() + self.timeout;
        Box::pin(async move {
            match tokio::time::timeout_at(deadline, fut).await {
                Ok(Ok((stream, service))) => {
                    Ok((EstablishTimeoutStream::new(stream, deadline), service))
                }
                Ok(Err(e)) => Err(e),
                Err(_) => Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "connection establish (handshake) timed out",
                )),
            }
        })
    }
}

/// Wraps an accepted byte stream so reading the request head is bounded by an
/// absolute deadline. The deadline stays armed across every read until the
/// server writes its first response byte — which only happens once the request
/// head has been read and routed — after which the wrapper is fully transparent
/// for the rest of the connection's life. See [`EstablishTimeoutAcceptor`].
struct EstablishTimeoutStream<S> {
    inner: S,
    /// `Some` until the connection is established (first server write); `None`
    /// afterwards.
    deadline: Option<std::pin::Pin<Box<tokio::time::Sleep>>>,
}

impl<S> EstablishTimeoutStream<S> {
    fn new(inner: S, deadline: tokio::time::Instant) -> Self {
        Self {
            inner,
            deadline: Some(Box::pin(tokio::time::sleep_until(deadline))),
        }
    }

    /// Disarm the establish deadline: the connection is established.
    fn established(&mut self) {
        self.deadline = None;
    }
}

impl<S: tokio::io::AsyncRead + Unpin> tokio::io::AsyncRead
    for EstablishTimeoutStream<S>
{
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        use std::future::Future;
        use std::task::Poll;

        if let Some(deadline) = self.deadline.as_mut()
            && deadline.as_mut().poll(cx).is_ready()
        {
            return Poll::Ready(Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "connection establish timed out before request head was read",
            )));
        }

        std::pin::Pin::new(&mut self.inner).poll_read(cx, buf)
    }
}

impl<S: tokio::io::AsyncWrite + Unpin> tokio::io::AsyncWrite
    for EstablishTimeoutStream<S>
{
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        // The server only writes after it has read and routed the request
        // head, so the first write marks the connection as established.
        self.established();
        std::pin::Pin::new(&mut self.inner).poll_write(cx, buf)
    }

    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.inner).poll_shutdown(cx)
    }

    fn poll_write_vectored(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        bufs: &[std::io::IoSlice<'_>],
    ) -> std::task::Poll<std::io::Result<usize>> {
        // See `poll_write`: the first write establishes the connection.
        self.established();
        std::pin::Pin::new(&mut self.inner).poll_write_vectored(cx, bufs)
    }

    fn is_write_vectored(&self) -> bool {
        self.inner.is_write_vectored()
    }
}

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
    #[cfg(feature = "iroh-relay")]
    pub quic_bind_addr: Option<std::net::SocketAddr>,
}

pub struct Server {
    t_join: Option<std::thread::JoinHandle<()>>,
    addrs: Vec<std::net::SocketAddr>,
    receiver: HttpReceiver,
    h_send: HSend,
    tls_reload_handle: Option<tokio::task::AbortHandle>,
    shutdown: Option<axum_server::Handle>,
    auth_tracker: crate::auth::AuthTokenTracker,
    #[cfg(feature = "iroh-relay")]
    relay_allowlist: Option<crate::RelayAllowlist>,
    /// Held to keep the QAD server task alive; dropped on shutdown.
    #[cfg(feature = "iroh-relay")]
    _qad_server: Option<iroh_relay::server::Server>,
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
                #[cfg(feature = "iroh-relay")]
                relay_allowlist,
                #[cfg(feature = "iroh-relay")]
                qad_server,
            })) => Ok(Self {
                t_join: Some(t_join),
                addrs,
                receiver,
                h_send,
                tls_reload_handle,
                shutdown: Some(shutdown),
                auth_tracker,
                #[cfg(feature = "iroh-relay")]
                relay_allowlist,
                #[cfg(feature = "iroh-relay")]
                _qad_server: qad_server,
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

    #[cfg(feature = "iroh-relay")]
    pub fn relay_allowlist(&self) -> Option<&crate::RelayAllowlist> {
        self.relay_allowlist.as_ref()
    }
}

struct Ready {
    h_send: HSend,
    addrs: Vec<std::net::SocketAddr>,
    receiver: HttpReceiver,
    tls_reload_handle: Option<tokio::task::AbortHandle>,
    shutdown: axum_server::Handle,
    auth_tracker: crate::auth::AuthTokenTracker,
    #[cfg(feature = "iroh-relay")]
    relay_allowlist: Option<crate::RelayAllowlist>,
    #[cfg(feature = "iroh-relay")]
    qad_server: Option<iroh_relay::server::Server>,
}

#[derive(Clone)]
pub struct AppState {
    pub h_send: HSend,

    // Feature-independent authentication
    pub auth_tracker: crate::auth::AuthTokenTracker,
    pub auth_config: Arc<crate::auth::AuthConfig>,
    pub auth_failures: opentelemetry::metrics::Counter<u64>,

    // Iroh relay allowlist: Some when authentication hook server is configured
    #[cfg(feature = "iroh-relay")]
    pub relay_allowlist: Option<crate::RelayAllowlist>,
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

    let auth_enabled = config.auth.authentication_hook_server.is_some();

    // When specific origins are listed, use them and allow credentials (if auth is on).
    // When no origins are listed and auth is on, mirror the request origin back — this
    // satisfies the CORS spec (credentials cannot be paired with `*`) while still
    // accepting requests from any origin, including browsers.
    // When auth is off, allow any origin without credentials.
    let (origin, allow_credentials) = match &config.allowed_origins {
        Some(origins) if !origins.is_empty() => {
            match origins
                .iter()
                .map(|o| HeaderValue::from_str(o.as_str()))
                .collect::<Result<Vec<_>, _>>()
            {
                Ok(values) => {
                    (tower_http::cors::AllowOrigin::list(values), auth_enabled)
                }
                Err(err) => {
                    if ready.send(Err(std::io::Error::other(err))).is_err() {
                        tracing::error!("Failed to send ready error");
                    }
                    return;
                }
            }
        }
        _ if auth_enabled => {
            (tower_http::cors::AllowOrigin::mirror_request(), true)
        }
        _ => (tower_http::cors::AllowOrigin::any(), false),
    };

    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async move {
            let (h_send, h_recv) =
                async_channel::bounded(server_config.worker_thread_count);

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

            let app = Router::<AppState>::new()
                .route("/authenticate", routing::put(handle_auth))
                .route("/health", routing::get(handle_health_get))
                .route("/bootstrap/{space}", routing::get(handle_boot_get))
                .route(
                    "/bootstrap/{space}/{agent}",
                    routing::put(handle_boot_put),
                );

            let mut app = app
                .layer(tower_http::cors::CorsLayer::new()
                    .allow_methods(tower_http::cors::AllowMethods::list([Method::GET, Method::PUT, Method::OPTIONS]))
                    .allow_headers(allowed_headers)
                    .allow_origin(origin)
                    .allow_credentials(allow_credentials)
                );

            #[cfg(feature = "iroh-relay")]
            let relay_allowlist: Option<crate::RelayAllowlist> =
                if config.auth.authentication_hook_server.is_some() {
                    Some(crate::RelayAllowlist::default())
                } else {
                    None
                };

            // Declare outside the `if` block so the guard lives until the
            // server futures complete.  Dropping it earlier would
            // unregister the OTEL observable-counter callbacks.
            #[cfg(feature = "iroh-relay")]
            let mut _relay_otel_metrics = None;

            if !config.no_relay_server {
                #[cfg(feature = "iroh-relay")]
                {
                    app = app.route(
                        "/relay/register",
                        routing::put(handle_relay_register),
                    );

                    let relay_rate_limit = config
                        .resolve_relay_rate_limit()
                        .expect(
                            "Config rate-limit invariants validated by \
                             BootstrapSrv::new",
                        );
                    let relay_state =
                        if let Some(allowlist) = relay_allowlist.clone() {
                            crate::iroh_relay_axum::create_relay_state_with_allowlist(
                                allowlist,
                                relay_rate_limit,
                            )
                        } else {
                            crate::iroh_relay_axum::create_relay_state(
                                relay_rate_limit,
                            )
                        };

                    _relay_otel_metrics = Some({
                        crate::iroh_relay_metrics::register(
                            relay_state.metrics.clone(),
                        )
                    });

                    let relay_router = Router::new()
                        .route("/relay", routing::get(crate::iroh_relay_axum::relay_handler))
                        .route("/ping", routing::get(crate::iroh_relay_axum::relay_probe_handler))
                        .route("/generate_204", routing::get(crate::iroh_relay_axum::captive_portal_handler))
                        .with_state(relay_state);

                    app = app.merge(relay_router);
                }
            }

            // Clone auth_tracker before moving it into AppState so we can return it
            let auth_tracker_for_ready = auth_tracker.clone();
            #[cfg(feature = "iroh-relay")]
            let relay_allowlist_for_ready = relay_allowlist.clone();

            // Only advertise TLS-related security headers when TLS is
            // actually terminated here; they would be misleading on a plain
            // HTTP listener. Values are kept in sync with the headers
            // upstream `iroh-relay` sets on its own HTTP(S) server. This is
            // applied last, after every route (including the relay routes
            // merged in above) has been added, since `Router::layer` only
            // wraps routes that already exist at the time it's called.
            if rustls_config.is_some() {
                app = app
                    .layer(tower_http::set_header::SetResponseHeaderLayer::overriding(
                        http::header::STRICT_TRANSPORT_SECURITY,
                        HeaderValue::from_static(
                            "max-age=63072000; includeSubDomains",
                        ),
                    ))
                    .layer(tower_http::set_header::SetResponseHeaderLayer::overriding(
                        http::header::CONTENT_SECURITY_POLICY,
                        HeaderValue::from_static(
                            "default-src 'none'; frame-ancestors 'none'; form-action 'none'; base-uri 'self'; block-all-mixed-content; plugin-types 'none'",
                        ),
                    ));
            }

            let app: Router = app
                .layer(extract::DefaultBodyLimit::max(1024))
                .with_state(AppState {
                    h_send: h_send.clone(),
                    auth_tracker,
                    auth_config,
                    auth_failures,
                    #[cfg(feature = "iroh-relay")]
                    relay_allowlist,
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
                    // Wrap the TLS acceptor so a stalled client cannot hold a
                    // half-open TCP/TLS connection without ever completing the
                    // handshake. See `ESTABLISH_TIMEOUT`.
                    let acceptor = EstablishTimeoutAcceptor::new(
                        RustlsAcceptor::new(tls_config.clone()),
                        ESTABLISH_TIMEOUT,
                    );

                    let server = axum_server::Server::from_tcp(listener)
                        .acceptor(acceptor)
                        .handle(shutdown_handle);

                    let s = server.serve(
                        app.into_make_service_with_connect_info::<SocketAddr>(),
                    );

                    servers.push(Box::pin(s));
                } else {
                    // No TLS handshake to bound, but still wrap the acceptor so
                    // a connected client that never finishes sending its
                    // request head is dropped once the establish deadline
                    // elapses. See `ESTABLISH_TIMEOUT`.
                    let acceptor = EstablishTimeoutAcceptor::new(
                        axum_server::accept::DefaultAcceptor::new(),
                        ESTABLISH_TIMEOUT,
                    );

                    let server = axum_server::Server::from_tcp(listener)
                        .acceptor(acceptor)
                        .handle(shutdown_handle);

                    let s = std::future::IntoFuture::into_future(server.serve(
                        app.into_make_service_with_connect_info::<SocketAddr>(),
                    ));
                    servers.push(Box::pin(s));
                };
            }

            // Spawn the QUIC Address Discovery (QAD) server if configured
            #[cfg(feature = "iroh-relay")]
            let qad_server = if let Some(quic_bind_addr) = server_config.quic_bind_addr {
                let result = if let (Some(cert_path), Some(key_path)) = (&config.tls_cert, &config.tls_key) {
                    crate::qad::spawn_qad_server(quic_bind_addr, cert_path, key_path).await
                } else {
                    tracing::info!("No TLS cert/key configured, using self-signed certificate for QAD");
                    crate::qad::spawn_qad_server_self_signed(quic_bind_addr).await
                };
                match result {
                    Ok(server) => {
                        tracing::info!(
                            addr = ?server.quic_addr(),
                            "QAD (QUIC Address Discovery) server started"
                        );
                        Some(server)
                    }
                    Err(err) => {
                        let _ = ready.send(Err(std::io::Error::other(format!(
                            "failed to start QAD server: {err}"
                        ))));
                        return;
                    }
                }
            } else {
                None
            };

            tracing::info!("Sending ready signal");

            if ready
                .send(Ok(Ready {
                    h_send,
                    addrs,
                    receiver,
                    tls_reload_handle,
                    shutdown: shutdown_handle,
                    auth_tracker: auth_tracker_for_ready,
                    #[cfg(feature = "iroh-relay")]
                    relay_allowlist: relay_allowlist_for_ready,
                    #[cfg(feature = "iroh-relay")]
                    qad_server,
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
        Err(crate::auth::AuthenticateError::Pending) => {
            tracing::debug!("/authenticate: PENDING");
            axum::response::IntoResponse::into_response(
                axum::http::StatusCode::ACCEPTED,
            )
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
    // Check authentication
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
    // Check authentication
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

#[cfg(feature = "iroh-relay")]
async fn handle_relay_register(
    extract::State(state): extract::State<AppState>,
    headers: axum::http::HeaderMap,
    body: bytes::Bytes,
) -> response::Response {
    tracing::debug!(body_len = body.len(), "Relay register request received");

    // Validate bearer token
    let token: Option<Arc<str>> = headers
        .get("Authorization")
        .and_then(|t| t.to_str().ok())
        .and_then(|t| t.strip_prefix("Bearer "))
        .map(<Arc<str>>::from);

    if !state.auth_tracker.is_valid(&token, &state.auth_config) {
        tracing::warn!("Relay register: bearer token invalid or missing");
        return axum::response::IntoResponse::into_response((
            axum::http::StatusCode::UNAUTHORIZED,
            "Unauthorized",
        ));
    }

    // Expect exactly 32 bytes (iroh public key)
    let key_bytes: &[u8; 32] = match body.as_ref().try_into() {
        Ok(b) => b,
        Err(_) => {
            tracing::warn!(
                body_len = body.len(),
                "Relay register: expected 32 bytes"
            );
            return axum::response::IntoResponse::into_response((
                axum::http::StatusCode::BAD_REQUEST,
                "Expected exactly 32 bytes (iroh public key)",
            ));
        }
    };

    let key = match iroh_base::PublicKey::from_bytes(key_bytes) {
        Ok(k) => k,
        Err(_) => {
            tracing::warn!("Relay register: invalid public key bytes");
            return axum::response::IntoResponse::into_response((
                axum::http::StatusCode::BAD_REQUEST,
                "Invalid public key bytes",
            ));
        }
    };

    // Register the key in the allowlist (if auth is enabled).
    // token must be Some here: is_valid returned true, and the allowlist is
    // only present when an auth hook server is configured, which requires a
    // bearer token for is_valid to succeed. Guard defensively anyway.
    if let Some(allowlist) = &state.relay_allowlist {
        let Some(token) = token else {
            return axum::response::IntoResponse::into_response((
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "Internal server error",
            ));
        };
        allowlist.register(key, token);
        tracing::info!(
            key = %key.fmt_short(),
            "Relay register: key added to allowlist"
        );
    } else {
        tracing::warn!(
            "Relay register: no allowlist configured, registration has no effect"
        );
    }

    axum::response::IntoResponse::into_response(axum::Json(serde_json::json!(
        {}
    )))
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

#[cfg(test)]
mod establish_timeout_tests {
    use super::*;
    use axum_server::accept::Accept;
    use std::io;
    use std::net::Ipv4Addr;
    use std::pin::Pin;
    use std::time::Duration;
    use tokio::io::AsyncReadExt;

    /// A stand-in acceptor whose handshake takes `delay` before succeeding.
    #[derive(Clone)]
    struct SlowAcceptor {
        delay: Duration,
    }

    impl<I, S> Accept<I, S> for SlowAcceptor
    where
        I: Send + 'static,
        S: Send + 'static,
    {
        type Stream = I;
        type Service = S;
        type Future = Pin<
            Box<dyn std::future::Future<Output = io::Result<(I, S)>> + Send>,
        >;

        fn accept(&self, stream: I, service: S) -> Self::Future {
            let delay = self.delay;
            Box::pin(async move {
                tokio::time::sleep(delay).await;
                Ok((stream, service))
            })
        }
    }

    #[tokio::test]
    async fn establish_acceptor_times_out_slow_handshake() {
        let acceptor = EstablishTimeoutAcceptor::new(
            SlowAcceptor {
                delay: Duration::from_secs(30),
            },
            Duration::from_millis(50),
        );

        let (stream, _peer) = tokio::io::duplex(64);
        let err = match Accept::accept(&acceptor, stream, ()).await {
            Ok(_) => panic!("slow handshake should time out"),
            Err(e) => e,
        };
        assert_eq!(err.kind(), io::ErrorKind::TimedOut);
    }

    #[tokio::test]
    async fn establish_acceptor_passes_fast_handshake() {
        let acceptor = EstablishTimeoutAcceptor::new(
            SlowAcceptor {
                delay: Duration::from_millis(0),
            },
            Duration::from_secs(30),
        );

        let (stream, _peer) = tokio::io::duplex(64);
        let res = Accept::accept(&acceptor, stream, ()).await;

        assert!(res.is_ok(), "fast handshake should pass through");
    }

    /// A connection that completes TCP but never sends a request head must be
    /// dropped once the establish deadline elapses.
    #[tokio::test]
    async fn establish_deadline_closes_idle_connection() {
        let listener =
            std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let addr = listener.local_addr().unwrap();

        let app = Router::new()
            .route("/", routing::get(|| async { "ok" }))
            .into_make_service();

        let acceptor = EstablishTimeoutAcceptor::new(
            axum_server::accept::DefaultAcceptor::new(),
            Duration::from_millis(200),
        );
        let server = axum_server::Server::from_tcp(listener).acceptor(acceptor);
        let server_handle = tokio::spawn(async move {
            let _ = server.serve(app).await;
        });

        let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();

        // Send nothing. The server should close the connection (read => EOF)
        // shortly after the header-read timeout elapses.
        let mut buf = [0u8; 1];
        let n =
            tokio::time::timeout(Duration::from_secs(5), stream.read(&mut buf))
                .await
                .expect("server did not close the idle connection in time")
                .expect("read failed");

        assert_eq!(n, 0, "expected EOF once header-read timeout elapsed");

        server_handle.abort();
    }

    /// A client that sends part of a request head and then stalls must be
    /// dropped once the establish deadline elapses: the deadline stays armed
    /// across reads until the head is fully read and the server responds.
    #[tokio::test]
    async fn establish_deadline_closes_partial_head() {
        use tokio::io::AsyncWriteExt;

        let listener =
            std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let addr = listener.local_addr().unwrap();

        let app = Router::new()
            .route("/", routing::get(|| async { "ok" }))
            .into_make_service();

        let acceptor = EstablishTimeoutAcceptor::new(
            axum_server::accept::DefaultAcceptor::new(),
            Duration::from_millis(200),
        );
        let server = axum_server::Server::from_tcp(listener).acceptor(acceptor);
        let server_handle = tokio::spawn(async move {
            let _ = server.serve(app).await;
        });

        let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
        // Start a request head but never terminate it.
        stream.write_all(b"GET / HTTP/1.1\r\n").await.unwrap();
        stream.flush().await.unwrap();

        // Server should close (EOF) or return a timeout response and close.
        let mut buf = Vec::new();
        let read_to_close = tokio::time::timeout(
            Duration::from_secs(5),
            stream.read_to_end(&mut buf),
        )
        .await
        .expect("server did not drop the partial-head connection in time");
        read_to_close.expect("read failed");

        server_handle.abort();
    }

    /// Once a full request head arrives the wrapper must be transparent: the
    /// request is served normally and the establish timeouts do not fire.
    #[tokio::test]
    async fn established_request_served_normally() {
        let listener =
            std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let addr = listener.local_addr().unwrap();

        let app = Router::new()
            .route("/", routing::get(|| async { "ok" }))
            .into_make_service();

        let acceptor = EstablishTimeoutAcceptor::new(
            axum_server::accept::DefaultAcceptor::new(),
            Duration::from_millis(200),
        );
        let server = axum_server::Server::from_tcp(listener).acceptor(acceptor);
        let server_handle = tokio::spawn(async move {
            let _ = server.serve(app).await;
        });

        // Give the listener a moment, then issue a normal request and read the
        // body. Sleeping past the (short) establish timeout before connecting
        // proves the timeout is per-connection, not a global clock.
        tokio::time::sleep(Duration::from_millis(300)).await;

        let url = format!("http://{addr}/");
        let body = tokio::task::spawn_blocking(move || {
            ureq::get(&url)
                .call()
                .unwrap()
                .body_mut()
                .read_to_string()
                .unwrap()
        })
        .await
        .unwrap();
        assert_eq!(body, "ok");

        server_handle.abort();
    }
}
