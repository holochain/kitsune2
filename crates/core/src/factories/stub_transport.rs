//! The core stub transport implementation provided by Kitsune2.

use kitsune2_api::{config::*, transport::*, *};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

const MOD_NAME: &str = "StubTransport";

/// Configuration parameters for [StubTransportFactory].
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StubTransportConfig {}

impl ModConfig for StubTransportConfig {}

/// The core stub transport implementation provided by Kitsune2.
/// This is NOT a production module. It is for testing only.
/// It will only establish "connections" within the same process.
#[derive(Debug)]
pub struct StubTransportFactory {}

impl StubTransportFactory {
    /// Construct a new StubTransportFactory.
    pub fn create() -> DynTransportFactory {
        let out: DynTransportFactory = Arc::new(StubTransportFactory {});
        out
    }
}

impl TransportFactory for StubTransportFactory {
    fn default_config(&self, config: &mut Config) -> K2Result<()> {
        config.add_default_module_config::<StubTransportConfig>(
            MOD_NAME.into(),
        )?;
        Ok(())
    }

    fn create(
        &self,
        builder: Arc<builder::Builder>,
        handler: Arc<TxImpHnd>,
    ) -> BoxFut<'static, K2Result<Transport>> {
        Box::pin(async move {
            let config = builder
                .config
                .get_module_config::<StubTransportConfig>(MOD_NAME)?;
            let imp = StubTransport::create(config, handler.clone()).await;
            Ok(handler.gen_transport(imp))
        })
    }
}

#[derive(Debug)]
struct StubTransport {
    task_list: Arc<Mutex<tokio::task::JoinSet<()>>>,
    cmd_send: CmdSend,
}

impl Drop for StubTransport {
    fn drop(&mut self) {
        self.task_list.lock().unwrap().abort_all();
    }
}

impl StubTransport {
    pub async fn create(
        _config: StubTransportConfig,
        handler: Arc<TxImpHnd>,
    ) -> DynTxImp {
        let mut listener = get_stat().listen();
        let this_url = listener.url();
        handler.new_listening_address(this_url.clone());

        let task_list = Arc::new(Mutex::new(tokio::task::JoinSet::new()));

        let (cmd_send, cmd_recv) =
            tokio::sync::mpsc::unbounded_channel::<Cmd>();

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
            this_url,
            cmd_send.clone(),
            cmd_recv,
        ));

        let out: DynTxImp = Arc::new(Self {
            task_list,
            cmd_send,
        });

        out
    }
}

impl TxImp for StubTransport {
    fn disconnect(
        &self,
        peer: Url,
        payload: Option<bytes::Bytes>,
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
}

type Res = tokio::sync::oneshot::Sender<K2Result<()>>;

enum Cmd {
    RegCon(Url, DataSend, DataRecv),
    InData(Url, bytes::Bytes, Res),
    Disconnect(Url, Option<bytes::Bytes>, Res),
    Send(Url, bytes::Bytes, Res),
}

type CmdSend = tokio::sync::mpsc::UnboundedSender<Cmd>;
type CmdRecv = tokio::sync::mpsc::UnboundedReceiver<Cmd>;

async fn cmd_task(
    task_list: Arc<Mutex<tokio::task::JoinSet<()>>>,
    handler: Arc<TxImpHnd>,
    this_url: Url,
    cmd_send: CmdSend,
    mut cmd_recv: CmdRecv,
) {
    let mut con_pool = HashMap::new();

    while let Some(cmd) = cmd_recv.recv().await {
        match cmd {
            Cmd::RegCon(url, data_send, mut data_recv) => {
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

                con_pool.insert(url, data_send);
            }
            Cmd::InData(url, data, res) => {
                if let Err(err) = handler.recv_data(url.clone(), data) {
                    con_pool.remove(&url);
                    let _ = res.send(Err(err));
                } else {
                    let _ = res.send(Ok(()));
                }
            }
            Cmd::Disconnect(url, payload, res) => {
                if let Some(data_send) = con_pool.remove(&url) {
                    if let Some(payload) = payload {
                        let _ = data_send.send((payload, res));
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
                    let _ = send.send((data, res));
                }
            }
        }
    }
}

type DataSend = tokio::sync::mpsc::UnboundedSender<(bytes::Bytes, Res)>;
type DataRecv = tokio::sync::mpsc::UnboundedReceiver<(bytes::Bytes, Res)>;
type ConSend = tokio::sync::mpsc::UnboundedSender<(Url, DataSend, DataRecv)>;
type ConRecv = tokio::sync::mpsc::UnboundedReceiver<(Url, DataSend, DataRecv)>;

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

struct Stat {
    con_map: Mutex<HashMap<u64, ConSend>>,
}

impl Stat {
    fn new() -> Self {
        Self {
            con_map: Mutex::new(HashMap::new()),
        }
    }

    fn listen(&self) -> Listener {
        use std::sync::atomic::*;
        static ID: AtomicU64 = AtomicU64::new(1);
        let id = ID.fetch_add(1, Ordering::Relaxed);
        let url = Url::from_str(format!("ws://stub.tx:42/{id}")).unwrap();
        let (send, recv) = tokio::sync::mpsc::unbounded_channel();
        self.con_map.lock().unwrap().insert(id, send);
        Listener { id, url, recv }
    }

    fn remove(&self, id: u64) {
        self.con_map.lock().unwrap().remove(&id);
    }

    fn connect(
        &self,
        cmd_send: &CmdSend,
        map: &mut HashMap<Url, DataSend>,
        to_peer: &Url,
        from_peer: &Url,
    ) -> Option<DataSend> {
        if let Some(send) = map.get(to_peer) {
            return Some(send.clone());
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

static STAT: OnceLock<Stat> = OnceLock::new();
fn get_stat() -> &'static Stat {
    STAT.get_or_init(Stat::new)
}

#[cfg(test)]
mod test;
