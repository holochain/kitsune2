use crate::http::AppState;
use axum::extract;
use axum::extract::ws::{Message, WebSocket};
use axum::extract::{ConnectInfo, State, WebSocketUpgrade};
use axum::http::HeaderMap;
use axum::response::IntoResponse;
use axum_server::accept::Accept;
use base64::Engine;
use futures::future::BoxFuture;
use futures::stream::{SplitSink, SplitStream};
use futures::{SinkExt, StreamExt};
use sbd_server::ws::{Payload, SbdWebsocket};
use sbd_server::{
    handle_upgraded, preflight_ip_check, spawn_prune_task, to_canonical_ip,
    Config, IpRate, PubKey, WeakCSlot,
};
use std::io;
use std::io::Error;
use std::net::{IpAddr, Ipv6Addr, SocketAddr};
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio::task::AbortHandle;

struct DropAbortHandle(AbortHandle);

impl Drop for DropAbortHandle {
    fn drop(&mut self) {
        self.0.abort();
    }
}

#[derive(Clone)]
pub struct SbdAcceptor {
    config: Arc<Config>,
    ip_rate: Arc<IpRate>,
    _ip_rate_prune: Arc<DropAbortHandle>,
}

impl SbdAcceptor {
    pub fn new(config: Arc<Config>, ip_rate: Arc<IpRate>) -> Self {
        let ip_rate_prune = spawn_prune_task(ip_rate.clone()).abort_handle();

        Self {
            config,
            ip_rate,
            _ip_rate_prune: Arc::new(DropAbortHandle(ip_rate_prune)),
        }
    }
}

impl<I> Accept<TcpStream, I> for SbdAcceptor
where
    I: Send + Sync + 'static,
{
    type Stream = TcpStream;
    type Service = I;
    type Future = BoxFuture<'static, io::Result<(Self::Stream, Self::Service)>>;

    fn accept(&self, stream: TcpStream, service: I) -> Self::Future {
        let this = self.clone();
        Box::pin(async move {
            if preflight_ip_check(
                &this.config,
                &this.ip_rate,
                stream.peer_addr()?,
            )
            .await
            .is_none()
            {
                return Err(Error::other("ip not allowed"));
            }

            Ok((stream, service))
        })
    }
}

#[derive(Clone)]
pub struct WebsocketForSbd {
    write: Arc<tokio::sync::Mutex<SplitSink<WebSocket, Message>>>,
    read: Arc<tokio::sync::Mutex<SplitStream<WebSocket>>>,
}

impl SbdWebsocket for WebsocketForSbd {
    fn recv(&self) -> BoxFuture<'static, io::Result<Payload>> {
        let this = self.clone();
        Box::pin(async move {
            let mut read = this.read.lock().await;
            use futures::stream::StreamExt;
            loop {
                match read.next().await {
                    None => return Err(Error::other("closed")),
                    Some(r) => {
                        let msg = r.map_err(Error::other)?;
                        match msg {
                            Message::Text(s) => {
                                return Ok(Payload::Vec(s.as_bytes().to_vec()))
                            }
                            Message::Binary(v) => {
                                return Ok(Payload::Vec(v.to_vec()))
                            }
                            Message::Ping(_) | Message::Pong(_) => (),
                            Message::Close(_) => {
                                return Err(Error::other("closed"))
                            }
                        }
                    }
                }
            }
        })
    }

    fn send(&self, payload: Payload) -> BoxFuture<'static, io::Result<()>> {
        let this = self.clone();
        Box::pin(async move {
            let mut write = this.write.lock().await;
            let b = match payload {
                Payload::Vec(v) => bytes::Bytes::from(v),
                Payload::BytesMut(b) => b.freeze(),
            };
            write.send(Message::Binary(b)).await.map_err(Error::other)?;
            write.flush().await.map_err(Error::other)?;
            Ok(())
        })
    }

    fn close(&self) -> BoxFuture<'static, ()> {
        let this = self.clone();
        Box::pin(async move {
            let _ = this.write.lock().await.close().await;
        })
    }
}

impl WebsocketForSbd {
    fn new(ws: WebSocket) -> Self {
        let (tx, rx) = ws.split();
        Self {
            write: Arc::new(tokio::sync::Mutex::new(tx)),
            read: Arc::new(tokio::sync::Mutex::new(rx)),
        }
    }
}

#[derive(Clone)]
pub struct SbdState {
    pub config: Arc<Config>,
    pub ip_rate: Arc<IpRate>,
    pub c_slot: WeakCSlot,
}

pub async fn handle_sbd(
    extract::Path(pub_key): extract::Path<String>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    let token: Option<Arc<str>> = headers
        .get("Authorization")
        .and_then(|t| t.to_str().ok().map(<Arc<str>>::from));

    let maybe_auth = Some((token.clone(), state.token_tracker.clone()));

    if !state
        .token_tracker
        .check_is_token_valid(&state.sbd_config, token)
    {
        return axum::response::IntoResponse::into_response((
            axum::http::StatusCode::UNAUTHORIZED,
            "Unauthorized",
        ));
    }

    let pk = match base64::prelude::BASE64_URL_SAFE_NO_PAD.decode(pub_key) {
        Ok(pk) if pk.len() == 32 => {
            let mut sized_pk = [0; 32];
            sized_pk.copy_from_slice(&pk);
            PubKey(Arc::new(sized_pk))
        }
        _ => return axum::http::StatusCode::BAD_REQUEST.into_response(),
    };

    let mut calc_ip = to_canonical_ip(addr.ip());

    if let Some(trusted_ip_header) = &state
        .sbd_state
        .as_ref()
        .expect("Missing SBD state")
        .config
        .trusted_ip_header
    {
        if let Some(header) =
            headers.get(trusted_ip_header).and_then(|h| h.to_str().ok())
        {
            if let Ok(ip) = header.parse::<IpAddr>() {
                calc_ip = to_canonical_ip(ip);
            }
        }
    }

    // finalize the upgrade process by returning upgrade callback.
    // we can customize the callback by sending additional info such as address.
    ws.on_upgrade(move |socket| {
        handle_socket(state, socket, pk, calc_ip, maybe_auth)
    })
}

async fn handle_socket(
    state: AppState,
    socket: WebSocket,
    pk: PubKey,
    calc_ip: Arc<Ipv6Addr>,
    maybe_auth: Option<(Option<Arc<str>>, sbd_server::AuthTokenTracker)>,
) {
    let sbd_state = state.sbd_state.as_ref().expect("Missing sbd state");

    handle_upgraded(
        sbd_state.config.clone(),
        sbd_state.ip_rate.clone(),
        sbd_state.c_slot.clone(),
        Arc::new(WebsocketForSbd::new(socket)),
        pk,
        calc_ip,
        maybe_auth,
    )
    .await;
}
