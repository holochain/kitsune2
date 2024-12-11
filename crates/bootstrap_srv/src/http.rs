use axum::*;
use std::io::Result;

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
        match self.0.recv_blocking() {
            Ok(r) => Some(r),
            Err(_) => None,
        }
    }
}

pub struct ServerConfig {
    pub addr: std::net::SocketAddr,
}

pub struct Server {
    t_join: Option<std::thread::JoinHandle<()>>,
    addr: std::net::SocketAddr,
    receiver: HttpReceiver,
    h_send: HSend,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
}

impl Drop for Server {
    fn drop(&mut self) {
        self.h_send.close();
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        if let Some(t_join) = self.t_join.take() {
            let _ = t_join.join();
        }
    }
}

impl Server {
    pub fn new(config: ServerConfig) -> Result<Self> {
        let (s_ready, r_ready) = tokio::sync::oneshot::channel();
        let t_join = std::thread::spawn(move || tokio_thread(config, s_ready));
        match r_ready.blocking_recv() {
            Ok(Ok(Ready {
                h_send,
                addr,
                receiver,
                shutdown,
            })) => Ok(Self {
                t_join: Some(t_join),
                addr,
                receiver,
                h_send,
                shutdown: Some(shutdown),
            }),
            Ok(Err(err)) => Err(err),
            Err(_) => Err(std::io::Error::other("failed to bind server")),
        }
    }

    pub fn server_addr(&self) -> std::net::SocketAddr {
        self.addr
    }

    pub fn receiver(&self) -> &HttpReceiver {
        &self.receiver
    }
}

struct Ready {
    h_send: HSend,
    addr: std::net::SocketAddr,
    receiver: HttpReceiver,
    shutdown: tokio::sync::oneshot::Sender<()>,
}

fn tokio_thread(
    config: ServerConfig,
    ready: tokio::sync::oneshot::Sender<std::io::Result<Ready>>,
) {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async move {
            // actually use the worker thread count??
            let (h_send, h_recv) = async_channel::bounded(1024);

            let app: Router = Router::new()
                .route("/health", routing::get(handle_health_get))
                .route("/bootstrap/:space", routing::get(handle_boot_get))
                .route(
                    "/bootstrap/:space/:agent",
                    routing::put(handle_boot_put),
                )
                .layer(extract::DefaultBodyLimit::max(1024))
                .with_state(h_send.clone());

            let receiver = HttpReceiver(h_recv);

            let listener =
                match tokio::net::TcpListener::bind(config.addr).await {
                    Ok(listener) => listener,
                    Err(err) => {
                        let _ = ready.send(Err(err));
                        return;
                    }
                };

            let addr = match listener.local_addr() {
                Ok(addr) => addr,
                Err(err) => {
                    let _ = ready.send(Err(err));
                    return;
                }
            };

            let (s_shutdown, r_shutdown) = tokio::sync::oneshot::channel();

            if ready
                .send(Ok(Ready {
                    h_send,
                    addr,
                    receiver,
                    shutdown: s_shutdown,
                }))
                .is_err()
            {
                return;
            }

            let _ = serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = r_shutdown.await;
                })
                .await;
        });
}

async fn handle_dispatch(
    h_send: &HSend,
    req: HttpRequest,
) -> response::Response {
    let (s, r) = tokio::sync::oneshot::channel();
    let s = Box::new(move |res| {
        let _ = s.send(res);
    });
    match tokio::time::timeout(std::time::Duration::from_secs(10), async move {
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
    {
        Ok(r) => r,
        Err(_) => HttpResponse {
            status: 500,
            body: b"{\"error\":\"internal timeout\"}".to_vec(),
        }
        .respond(),
    }
}

async fn handle_health_get(
    extract::State(h_send): extract::State<HSend>,
) -> response::Response {
    handle_dispatch(&h_send, HttpRequest::HealthGet).await
}

async fn handle_boot_get(
    extract::Path(space): extract::Path<String>,
    extract::State(h_send): extract::State<HSend>,
) -> response::Response {
    use base64::prelude::*;

    // TODO FIX UNWRAP
    let space = bytes::Bytes::copy_from_slice(
        &BASE64_URL_SAFE_NO_PAD.decode(&space).unwrap(),
    );
    handle_dispatch(&h_send, HttpRequest::BootstrapGet { space }).await
}

async fn handle_boot_put(
    extract::Path((space, agent)): extract::Path<(String, String)>,
    extract::State(h_send): extract::State<HSend>,
    body: bytes::Bytes,
) -> response::Response<body::Body> {
    use base64::prelude::*;

    // TODO FIX UNWRAP
    let space = bytes::Bytes::copy_from_slice(
        &BASE64_URL_SAFE_NO_PAD.decode(&space).unwrap(),
    );
    let agent = bytes::Bytes::copy_from_slice(
        &BASE64_URL_SAFE_NO_PAD.decode(&agent).unwrap(),
    );
    handle_dispatch(&h_send, HttpRequest::BootstrapPut { space, agent, body })
        .await
}
