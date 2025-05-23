//! The core stub transport implementation provided by Kitsune2.

use kitsune2_api::*;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

/// The core stub transport implementation provided by Kitsune2.
/// This is NOT a production module. It is for testing only.
/// It will only establish "connections" within the same process.
#[derive(Debug)]
pub struct MemTransportFactory {}

impl MemTransportFactory {
    /// Construct a new MemTransportFactory.
    pub fn create() -> DynTransportFactory {
        let out: DynTransportFactory = Arc::new(MemTransportFactory {});
        out
    }
}

impl TransportFactory for MemTransportFactory {
    fn default_config(&self, _config: &mut Config) -> K2Result<()> {
        Ok(())
    }

    fn validate_config(&self, _config: &Config) -> K2Result<()> {
        Ok(())
    }

    fn create(
        &self,
        _builder: Arc<Builder>,
        handler: DynTxHandler,
    ) -> BoxFut<'static, K2Result<DynTransport>> {
        Box::pin(async move {
            let handler = TxImpHnd::new(handler);
            let imp = MemTransport::create(handler.clone()).await;
            Ok(DefaultTransport::create(&handler, imp))
        })
    }
}

#[derive(Debug)]
struct MemTransport {
    this_url: Url,
    task_list: Arc<Mutex<tokio::task::JoinSet<()>>>,
    cmd_send: CmdSend,
    net_stats: Arc<Mutex<TransportStats>>,
}

impl Drop for MemTransport {
    fn drop(&mut self) {
        tracing::trace!("Dropping mem transport");

        self.task_list.lock().unwrap().abort_all();
    }
}

impl MemTransport {
    pub async fn create(handler: Arc<TxImpHnd>) -> DynTxImp {
        let mut listener = get_stat().listen();
        let this_url = listener.url();
        handler.new_listening_address(this_url.clone()).await;

        let task_list = Arc::new(Mutex::new(tokio::task::JoinSet::new()));

        let (cmd_send, cmd_recv) =
            tokio::sync::mpsc::unbounded_channel::<Cmd>();

        let net_stats = Arc::new(Mutex::new(TransportStats {
            backend: "kitsune2-core-mem".into(),
            peer_urls: vec![this_url.clone()],
            connections: vec![],
        }));

        // listen for incoming connections
        let cmd_send2 = cmd_send.clone();
        task_list.lock().unwrap().spawn(async move {
            while let Some((u, s, r)) = listener.recv.recv().await {
                if cmd_send2.send(Cmd::RegCon(u, s, r)).is_err() {
                    break;
                }
            }
        });

        // our core command runner task
        task_list.lock().unwrap().spawn(cmd_task(
            task_list.clone(),
            handler,
            this_url.clone(),
            cmd_send.clone(),
            cmd_recv,
            net_stats.clone(),
        ));

        let out: DynTxImp = Arc::new(Self {
            this_url,
            task_list,
            cmd_send,
            net_stats,
        });

        out
    }
}

impl TxImp for MemTransport {
    fn url(&self) -> Option<Url> {
        Some(self.this_url.clone())
    }

    fn disconnect(
        &self,
        peer: Url,
        payload: Option<(String, bytes::Bytes)>,
    ) -> BoxFut<'_, ()> {
        Box::pin(async move {
            let (s, r) = tokio::sync::oneshot::channel();
            if self
                .cmd_send
                .send(Cmd::Disconnect(peer, payload, s))
                .is_ok()
            {
                let _ = r.await;
            }
        })
    }

    fn send(&self, peer: Url, data: bytes::Bytes) -> BoxFut<'_, K2Result<()>> {
        Box::pin(async move {
            let (s, r) = tokio::sync::oneshot::channel();
            match self.cmd_send.send(Cmd::Send(peer, data, s)) {
                Err(_) => Err(K2Error::other("Connection Closed")),
                Ok(_) => match r.await {
                    Ok(r) => r,
                    Err(_) => Err(K2Error::other("Connection Closed")),
                },
            }
        })
    }

    fn get_connected_peers(&self) -> BoxFut<'_, K2Result<Vec<Url>>> {
        // The memory transport is always connected to everyone but doesn't
        // expose who is connected here.
        Box::pin(async move {
            Err(K2Error::other(
                "get_connected_peers is not implemented for the mem transport",
            ))
        })
    }

    fn dump_network_stats(&self) -> BoxFut<'_, K2Result<TransportStats>> {
        Box::pin(async move { Ok(self.net_stats.lock().unwrap().clone()) })
    }
}

type Res = tokio::sync::oneshot::Sender<K2Result<()>>;
type CmdSend = tokio::sync::mpsc::UnboundedSender<Cmd>;
type CmdRecv = tokio::sync::mpsc::UnboundedReceiver<Cmd>;
type DataSend = tokio::sync::mpsc::UnboundedSender<(bytes::Bytes, Res)>;
type DataRecv = tokio::sync::mpsc::UnboundedReceiver<(bytes::Bytes, Res)>;
type ConSend = tokio::sync::mpsc::UnboundedSender<(Url, DataSend, DataRecv)>;
type ConRecv = tokio::sync::mpsc::UnboundedReceiver<(Url, DataSend, DataRecv)>;

struct DropSend {
    send: DataSend,
    handler: Arc<TxImpHnd>,
    peer: Url,
    reason: Option<String>,
    net_stats: Arc<Mutex<TransportStats>>,
}

impl Drop for DropSend {
    fn drop(&mut self) {
        let peer_str = self.peer.to_string();
        self.net_stats
            .lock()
            .unwrap()
            .connections
            .retain(|c| c.pub_key != peer_str);
        self.handler
            .peer_disconnect(self.peer.clone(), self.reason.take());
    }
}

impl DropSend {
    fn new(
        send: DataSend,
        handler: Arc<TxImpHnd>,
        peer: Url,
        net_stats: Arc<Mutex<TransportStats>>,
    ) -> Self {
        Self {
            send,
            handler,
            peer,
            reason: None,
            net_stats,
        }
    }
}

enum Cmd {
    RegCon(Url, DataSend, DataRecv),
    InData(Url, bytes::Bytes, Res),
    Disconnect(Url, Option<(String, bytes::Bytes)>, Res),
    Send(Url, bytes::Bytes, Res),
}

async fn cmd_task(
    task_list: Arc<Mutex<tokio::task::JoinSet<()>>>,
    handler: Arc<TxImpHnd>,
    this_url: Url,
    cmd_send: CmdSend,
    mut cmd_recv: CmdRecv,
    net_stats: Arc<Mutex<TransportStats>>,
) {
    let mut con_pool = HashMap::new();

    fn ns_ref<Cb: FnOnce(&mut TransportConnectionStats)>(
        net_stats: &Mutex<TransportStats>,
        url: &Url,
        cb: Cb,
    ) {
        let url_str = url.to_string();

        let mut lock = net_stats.lock().unwrap();

        for r in lock.connections.iter_mut() {
            if r.pub_key == url_str {
                return cb(r);
            }
        }

        lock.connections.push(TransportConnectionStats {
            pub_key: url_str,
            send_message_count: 0,
            send_bytes: 0,
            recv_message_count: 0,
            recv_bytes: 0,
            opened_at_s: std::time::SystemTime::UNIX_EPOCH
                .elapsed()
                .unwrap()
                .as_secs(),
            is_webrtc: false,
        });

        cb(lock.connections.last_mut().unwrap())
    }

    while let Some(cmd) = cmd_recv.recv().await {
        match cmd {
            Cmd::RegCon(url, data_send, mut data_recv) => {
                match handler.peer_connect(url.clone()) {
                    Err(_) => continue,
                    Ok(preflight) => {
                        let (s, _) = tokio::sync::oneshot::channel();
                        let _ = data_send.send((preflight, s));
                    }
                }

                let cmd_send2 = cmd_send.clone();
                let url2 = url.clone();
                task_list.lock().unwrap().spawn(async move {
                    while let Some((data, res)) = data_recv.recv().await {
                        if cmd_send2
                            .send(Cmd::InData(url2.clone(), data, res))
                            .is_err()
                        {
                            break;
                        }
                    }
                });

                con_pool.insert(
                    url.clone(),
                    DropSend::new(
                        data_send,
                        handler.clone(),
                        url,
                        net_stats.clone(),
                    ),
                );
            }
            Cmd::InData(url, data, res) => {
                ns_ref(&net_stats, &url, |r| {
                    r.recv_message_count += 1;
                    r.recv_bytes += data.len() as u64;
                });
                if let Err(err) = handler.recv_data(url.clone(), data) {
                    if let Some(mut data_send) = con_pool.remove(&url) {
                        data_send.reason = Some(format!("{err:?}"));
                    }
                    let _ = res.send(Err(err));
                } else {
                    let _ = res.send(Ok(()));
                }
            }
            Cmd::Disconnect(url, payload, res) => {
                if let Some(mut data_send) = con_pool.remove(&url) {
                    if let Some((reason, payload)) = payload {
                        data_send.reason = Some(reason);
                        let _ = data_send.send.send((payload, res));
                    }
                }
            }
            Cmd::Send(url, data, res) => {
                if let Some(send) = get_stat().connect(
                    &cmd_send,
                    &mut con_pool,
                    &url,
                    &this_url,
                ) {
                    ns_ref(&net_stats, &url, |r| {
                        r.send_message_count += 1;
                        r.send_bytes += data.len() as u64;
                    });
                    let _ = send.send((data, res));
                }
            }
        }
    }
}

/// A Listener instance is the receiver side of a pseudo connection.
/// If this is dropped by test code, it will remove the sender side
/// from our static global.
struct Listener {
    id: u64,
    url: Url,
    recv: ConRecv,
}

impl std::fmt::Debug for Listener {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Listener").field("url", &self.url).finish()
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        get_stat().remove(self.id);
    }
}

impl Listener {
    pub fn url(&self) -> Url {
        self.url.clone()
    }
}

/// This struct will be instantiated as a static global called STAT.
/// The purpose is to hold the sender side of channels that let us
/// open "connections" to endpoints. These senders will remain in memory
/// until the [Listener] instance is dropped.
struct Stat {
    con_map: Mutex<HashMap<u64, ConSend>>,
}

impl Stat {
    fn new() -> Self {
        Self {
            con_map: Mutex::new(HashMap::new()),
        }
    }

    /// "Bind" a new [Listener].
    fn listen(&self) -> Listener {
        use std::sync::atomic::*;
        static ID: AtomicU64 = AtomicU64::new(1);
        let id = ID.fetch_add(1, Ordering::Relaxed);
        let url = Url::from_str(format!("ws://stub.tx:42/{id}")).unwrap();
        let (send, recv) = tokio::sync::mpsc::unbounded_channel();
        self.con_map.lock().unwrap().insert(id, send);
        Listener { id, url, recv }
    }

    /// Remove a sender. Called by [Listener::drop].
    fn remove(&self, id: u64) {
        self.con_map.lock().unwrap().remove(&id);
    }

    /// If the destination peer is still in memory, this will
    /// establish an in-memory "connection" to them.
    fn connect(
        &self,
        cmd_send: &CmdSend,
        map: &mut HashMap<Url, DropSend>,
        to_peer: &Url,
        from_peer: &Url,
    ) -> Option<DataSend> {
        if let Some(send) = map.get(to_peer) {
            return Some(send.send.clone());
        }

        let id: u64 = match to_peer.peer_id() {
            None => return None,
            Some(id) => match id.parse() {
                Err(_) => return None,
                Ok(id) => id,
            },
        };

        let send = match self.con_map.lock().unwrap().get(&id) {
            None => return None,
            Some(send) => send.clone(),
        };

        let (ds1, dr1) = tokio::sync::mpsc::unbounded_channel();
        let (ds2, dr2) = tokio::sync::mpsc::unbounded_channel();

        if send.send((from_peer.clone(), ds1, dr2)).is_err() {
            return None;
        }

        let _ = cmd_send.send(Cmd::RegCon(to_peer.clone(), ds2.clone(), dr1));

        Some(ds2)
    }
}

/// This is our static global instance of the [Stat] struct.
static STAT: OnceLock<Stat> = OnceLock::new();
fn get_stat() -> &'static Stat {
    STAT.get_or_init(Stat::new)
}

#[cfg(test)]
mod test;
