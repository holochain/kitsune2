//! The core stub transport implementation provided by Kitsune2 that can be
//! used in tests.

use kitsune2_api::*;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use tokio::sync::Semaphore;

/// The core stub transport implementation provided by Kitsune2.
///
/// This is NOT a production module. It is for testing only.
///
/// It will only establish "connections" between different MemTransport
/// instances within the same process, where a "connection" is comprised
/// of two oppositely directed [`tokio::sync::mpsc::unbounded_channel`]s
/// to send data in either direction.
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
        let mut listener = get_transport_instances().listen();
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
            while let Some((url, data_send, data_recv)) =
                listener.connection_recv.recv().await
            {
                if cmd_send2
                    .send(Cmd::RegisterConnection(
                        url,
                        ReadyDataSend::new(data_send),
                        data_recv,
                    ))
                    .is_err()
                {
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
            let (result_sender, result_receiver) =
                tokio::sync::oneshot::channel();
            match self.cmd_send.send(Cmd::Send(peer, data, result_sender)) {
                Err(_) => Err(K2Error::other("Connection Closed")),
                Ok(_) => match result_receiver.await {
                    Ok(result) => result,
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

/// A oneshot sender to report back a K2Result.
type ResultSender = tokio::sync::oneshot::Sender<K2Result<()>>;

/// The sending end of the channel to send commands to a MemTransport's
/// command runner task.
type CmdSend = tokio::sync::mpsc::UnboundedSender<Cmd>;

/// The receiving end of the channel to send commands to a MemTransport's
/// command runner task.
type CmdRecv = tokio::sync::mpsc::UnboundedReceiver<Cmd>;

/// The sending end of a "data" channel between two MemTransport instances.
///
/// Used to send the bytes of a message alongside with a ResultSender to
/// report back the result of handling the message.
type DataSend =
    tokio::sync::mpsc::UnboundedSender<(bytes::Bytes, ResultSender)>;

/// The receiving end of a "data" channel between two MemTransport instances.
///
/// Used to send the bytes of a message alongside with a ResultSender to
/// report back the result of handling the message.
type DataRecv =
    tokio::sync::mpsc::UnboundedReceiver<(bytes::Bytes, ResultSender)>;

/// The sending end of a "connection establishment channel" used to establish
/// connections (in the form of two opposite "data" channels) between different
/// MemTransport instances.
type ConnectionSend =
    tokio::sync::mpsc::UnboundedSender<(Url, DataSend, DataRecv)>;

/// The receiving end of a "connection establishment channel" used to establish
/// connections (in the form of two opposite "data" channels) between different
/// MemTransport instances.
type ConnectionRecv =
    tokio::sync::mpsc::UnboundedReceiver<(Url, DataSend, DataRecv)>;

/// A wrapper around a [`DataSend`] that can be marked as "ready" when the connection preflight
/// has been completed.
#[derive(Clone)]
struct ReadyDataSend {
    /// A semaphore with no available permits that gets closed when the connection is ready.
    ready: Arc<Semaphore>,
    /// The sending end of the "data" channel to send messages to the other peer.
    data_send: DataSend,
}

impl ReadyDataSend {
    fn new(data_send: DataSend) -> Self {
        Self {
            ready: Arc::new(Semaphore::new(0)),
            data_send,
        }
    }

    /// Mark the connection as ready, allowing messages to be sent.
    fn mark_ready(&self) {
        self.ready.close();
    }

    /// Wait until the connection is marked as ready and return the [`DataSend`].
    ///
    /// If the connection is not ready after 5 seconds, return a timeout error.
    async fn wait_ready(&self) -> K2Result<DataSend> {
        let _ = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            self.ready.acquire(),
        )
        .await
        .map_err(|_| K2Error::other("Timeout waiting for ready"))?;

        Ok(self.data_send.clone())
    }
}

/// An open connection to another peer, containing the sending end of the
/// "data" channel connected to their MemTransport instance.
struct DropConnection {
    ready_data_send: ReadyDataSend,
    handler: Arc<TxImpHnd>,
    peer: Url,
    reason: Option<String>,
    net_stats: Arc<Mutex<TransportStats>>,
}

impl Drop for DropConnection {
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

impl DropConnection {
    fn new(
        ready_data_send: ReadyDataSend,
        handler: Arc<TxImpHnd>,
        peer: Url,
        net_stats: Arc<Mutex<TransportStats>>,
    ) -> Self {
        Self {
            ready_data_send,
            handler,
            peer,
            reason: None,
            net_stats,
        }
    }
}

/// A command to be handled by the MemTransport's command runner task.
enum Cmd {
    /// A pseudo-connection to register in the MemTransport's in-memory
    /// connection pool.
    ///
    /// Contains a sender and a receiver for oppositely directed "data"
    /// channels to send messages to and receive messages from the other peer,
    /// respectively.
    RegisterConnection(Url, ReadyDataSend, DataRecv),

    /// Message bytes received from a peer to be handled, alongside with the
    /// ResultSender to be used to report the Result of handling the message.
    RecvData(Url, bytes::Bytes, ResultSender),

    /// Disconnect from a peer.
    Disconnect(Url, Option<(String, bytes::Bytes)>, ResultSender),

    /// Message bytes to send to another peer alongside with the ResultSender
    /// to report back the result of handling the message by the receiving
    /// peer.
    Send(Url, bytes::Bytes, ResultSender),
}

/// The command runner task that gets spawned when creating a MemTransport
/// instance.
async fn cmd_task(
    task_list: Arc<Mutex<tokio::task::JoinSet<()>>>,
    handler: Arc<TxImpHnd>,
    this_url: Url,
    cmd_send: CmdSend,
    mut cmd_recv: CmdRecv,
    net_stats: Arc<Mutex<TransportStats>>,
) {
    // Pool of open in-memory connections.
    let mut con_pool = HashMap::new();

    /// Get a reference to the [`TransportConnectionStats`] associated to the
    /// given peer url and operate with the provided callback on it.
    ///
    /// Creates a new [`TransportConnectionStats`] instance with default
    /// values if none exists yet in the [`TransportStats`] provided via
    /// the nets_stats argument, before applying the callback.
    fn net_stat_ref<Cb: FnOnce(&mut TransportConnectionStats)>(
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
            is_direct: false,
        });

        cb(lock.connections.last_mut().unwrap())
    }

    while let Some(cmd) = cmd_recv.recv().await {
        match cmd {
            Cmd::RegisterConnection(url, ready_data_send, mut data_recv) => {
                match handler.peer_connect(url.clone()).await {
                    Err(_) => continue,
                    Ok(preflight) => {
                        let (result_sender, _) =
                            tokio::sync::oneshot::channel();

                        let _ = ready_data_send
                            .data_send
                            .send((preflight, result_sender));
                    }
                }

                let cmd_send2 = cmd_send.clone();
                let url2 = url.clone();

                // Spawn a task to listen on the receiving end of the "data" channel and
                // send received messages through the command handler channel to the
                // command runner task.
                task_list.lock().unwrap().spawn({
                    let ready_data_send = ready_data_send.clone();
                    async move {
                        let (data, result_sender) = match tokio::time::timeout(std::time::Duration::from_secs(5), data_recv.recv()).await {
                            Ok(Some((data, result_sender))) => (data, result_sender),
                            Ok(None) => {
                                tracing::error!("Failed to receive preflight response - channel closed");
                                return;
                            }
                            Err(_) => {
                                tracing::error!("Failed to receive preflight response - timeout");
                                return;
                            }
                        };

                        match K2Proto::decode(&data) {
                            Ok(d) => {
                                if d.ty() != K2WireType::Preflight {
                                    tracing::error!("Expected preflight message, got {:?}", d.ty());
                                    return;
                                }
                            }
                            Err(e) => {
                                tracing::error!("Failed to decode message: {:?}", e);
                                return;
                            }
                        }

                        if cmd_send2
                            .send(Cmd::RecvData(
                                url2.clone(),
                                data,
                                result_sender,
                            ))
                            .is_err()
                        {
                            return;
                        }

                        ready_data_send.mark_ready();

                        while let Some((data, result_sender)) =
                            data_recv.recv().await
                        {
                            if cmd_send2
                                .send(Cmd::RecvData(
                                    url2.clone(),
                                    data,
                                    result_sender,
                                ))
                                .is_err()
                            {
                                break;
                            }
                        }
                    }
                });

                con_pool.insert(
                    url.clone(),
                    DropConnection::new(
                        ready_data_send,
                        handler.clone(),
                        url,
                        net_stats.clone(),
                    ),
                );
            }
            Cmd::RecvData(url, data, result_sender) => {
                net_stat_ref(&net_stats, &url, |r| {
                    r.recv_message_count += 1;
                    r.recv_bytes += data.len() as u64;
                });
                if let Err(err) = handler.recv_data(url.clone(), data).await {
                    tracing::error!(?url, "Error receiving data: {err:?}");

                    if let Some(mut drop_send) = con_pool.remove(&url) {
                        drop_send.reason = Some(format!("{err:?}"));
                    }
                    let _ = result_sender.send(Err(err));
                } else {
                    let _ = result_sender.send(Ok(()));
                }
            }
            Cmd::Disconnect(url, payload, result_sender) => {
                if let Some(mut drop_send) = con_pool.remove(&url)
                    && let Some((reason, payload)) = payload
                {
                    drop_send.reason = Some(reason);
                    let _ = drop_send
                        .ready_data_send
                        .data_send
                        .send((payload, result_sender));
                }
            }
            Cmd::Send(url, data, result_sender) => {
                if let Some(ready_data_send) = get_transport_instances()
                    .connect(&cmd_send, &mut con_pool, &url, &this_url)
                {
                    net_stat_ref(&net_stats, &url, |r| {
                        r.send_message_count += 1;
                        r.send_bytes += data.len() as u64;
                    });

                    tokio::task::spawn(async move {
                        match ready_data_send.wait_ready().await {
                            Ok(ds) => {
                                let _ = ds.send((data, result_sender));
                            }
                            Err(e) => {
                                let _ = result_sender.send(Err(e));
                            }
                        };
                    });
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
    connection_recv: ConnectionRecv,
}

impl std::fmt::Debug for Listener {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Listener").field("url", &self.url).finish()
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        get_transport_instances().remove(self.id);
    }
}

impl Listener {
    pub fn url(&self) -> Url {
        self.url.clone()
    }
}

/// This struct will be instantiated as a static global called
/// TRANSPORT_INSTANCES. The purpose is to hold the sender side of "connection
/// establishment channels" that let us open (pseudo-)"connections" between
/// different MemTransport instances.
///
/// A "connection" between two MemTransport instances is in essence two
/// unbounded channels ("data" channels) in opposite directions, where each of
/// the MemTransport instances holds a receiving end of one channel and a
/// sending end of the other channel.
///
/// If a new MemTransport instance gets created, it will keep the receiving
/// end of the connection establishment channel and add the sending end to
/// this struct.
///
/// These senders will remain in memory until the [`Listener`] instance is
/// dropped.
struct TransportInstances {
    instances_map: Mutex<HashMap<u64, ConnectionSend>>,
}

impl TransportInstances {
    fn new() -> Self {
        Self {
            instances_map: Mutex::new(HashMap::new()),
        }
    }

    /// "Bind" a new [`Listener`] by adding the sending end of an unbounded
    /// channel to the instances map and return the receiving end of the
    /// channel to the caller.
    ///
    /// Called by [MemTransport::create].
    fn listen(&self) -> Listener {
        use std::sync::atomic::*;
        static ID: AtomicU64 = AtomicU64::new(1);
        let id = ID.fetch_add(1, Ordering::Relaxed);
        let url = Url::from_str(format!("ws://stub.tx:42/{id}")).unwrap();
        let (connection_send, connection_recv) =
            tokio::sync::mpsc::unbounded_channel();
        self.instances_map
            .lock()
            .unwrap()
            .insert(id, connection_send);
        Listener {
            id,
            url,
            connection_recv,
        }
    }

    /// Remove a sender. Called by [Listener::drop].
    fn remove(&self, id: u64) {
        self.instances_map.lock().unwrap().remove(&id);
    }

    /// If the destination peer's MemTransport instance is still in memory,
    /// this will establish an in-memory "connection" to them.
    fn connect(
        &self,
        cmd_send: &CmdSend,
        conn_pool: &mut HashMap<Url, DropConnection>,
        to_peer: &Url,
        from_peer: &Url,
    ) -> Option<ReadyDataSend> {
        if let Some(open_connection) = conn_pool.get(to_peer) {
            return Some(open_connection.ready_data_send.clone());
        }

        let to_peer_id: u64 = match to_peer.peer_id() {
            None => return None,
            Some(id) => match id.parse() {
                Err(_) => return None,
                Ok(id) => id,
            },
        };

        // Get the sender side of the connection establishment channel for the
        // MemTransport associated with the to_peer Url, if an associated
        // MemTransport instance is found.
        let connection_send =
            match self.instances_map.lock().unwrap().get(&to_peer_id) {
                None => return None,
                Some(send) => send.clone(),
            };

        // Create an unbounded "data" channel "B -> A" to send data from
        // peer B's MemTransport instance to peer A's MemTransport instance
        let (data_send_1, data_recv_1) = tokio::sync::mpsc::unbounded_channel();

        // Create an unbounded "data" channel "A -> B" to send data from
        // peer A's MemTransport instance to peer B's MemTransport instance
        let (data_send_2, data_recv_2) = tokio::sync::mpsc::unbounded_channel();

        // Send the sending end of the "data" channel "B > A" and the receiving
        // end of the "data" channel "A > B" to the MemTransport instance
        // of peer B.
        if connection_send
            .send((from_peer.clone(), data_send_1, data_recv_2))
            .is_err()
        {
            return None;
        }

        let ready_send = ReadyDataSend::new(data_send_2.clone());

        // Send the sending end of the "data" channel "A > B" and the receiving
        // end of the "data" channel "B > A" to the MemTransport instance
        // of peer A.
        let _ = cmd_send.send(Cmd::RegisterConnection(
            to_peer.clone(),
            ready_send.clone(),
            data_recv_1,
        ));

        Some(ready_send)
    }
}

/// This is our static global instance of the [`TransportInstances`] struct.
static TRANSPORT_INSTANCES: OnceLock<TransportInstances> = OnceLock::new();
/// Get the global instance of the [`TransportInstances`] struct which holds the sender side of
/// channels that let us open "connections" to endpoints.
fn get_transport_instances() -> &'static TransportInstances {
    TRANSPORT_INSTANCES.get_or_init(TransportInstances::new)
}

#[cfg(test)]
mod test;
