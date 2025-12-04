use iroh::endpoint::{Connection, ConnectionType};
use kitsune2_api::{TxImpHnd, Url};
use n0_watcher::Watcher;
use std::{
    fmt,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex, RwLock,
    },
};

pub(super) struct ConnectionContext {
    handler: Arc<TxImpHnd>,
    connection: Arc<Connection>,
    remote_url: RwLock<Option<Url>>,
    send_message_count: AtomicU64,
    send_bytes: AtomicU64,
    recv_message_count: AtomicU64,
    recv_bytes: AtomicU64,
    opened_at_s: u64,
    connection_type_watcher: Mutex<Option<n0_watcher::Direct<ConnectionType>>>,
}

impl fmt::Debug for ConnectionContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ConnectionContext").finish()
    }
}

impl ConnectionContext {
    pub fn new(
        handler: Arc<TxImpHnd>,
        connection: Arc<Connection>,
        remote_url: Option<Url>,
        opened_at_s: u64,
        connection_type_watcher: Option<n0_watcher::Direct<ConnectionType>>,
    ) -> Self {
        Self {
            handler,
            connection,
            remote_url: RwLock::new(remote_url),
            send_message_count: AtomicU64::new(0),
            send_bytes: AtomicU64::new(0),
            recv_message_count: AtomicU64::new(0),
            recv_bytes: AtomicU64::new(0),
            opened_at_s,
            connection_type_watcher: Mutex::new(connection_type_watcher),
        }
    }

    pub fn set_remote_url(&self, peer: Url) {
        *self.remote_url.write().unwrap() = Some(peer);
    }

    pub fn remote(&self) -> Option<Url> {
        self.remote_url.read().unwrap().clone()
    }

    pub fn handler(&self) -> Arc<TxImpHnd> {
        self.handler.clone()
    }

    pub fn connection(&self) -> Arc<Connection> {
        self.connection.clone()
    }

    pub fn notify_disconnect(&self) {
        if let Some(peer) = self.remote() {
            self.handler
                .peer_disconnect(peer, Some("disconnected".to_string()));
        }
    }

    pub fn get_send_message_count(&self) -> u64 {
        self.send_message_count.load(Ordering::SeqCst)
    }

    pub fn increment_send_message_count(&self) {
        self.send_message_count.fetch_add(1, Ordering::SeqCst);
    }

    pub fn get_send_bytes(&self) -> u64 {
        self.send_bytes.load(Ordering::SeqCst)
    }

    pub fn increment_send_bytes(&self, len: u64) {
        self.send_bytes.fetch_add(len, Ordering::SeqCst);
    }

    pub fn get_recv_message_count(&self) -> u64 {
        self.recv_message_count.load(Ordering::SeqCst)
    }

    pub fn increment_recv_message_count(&self) {
        self.recv_message_count.fetch_add(1, Ordering::SeqCst);
    }

    pub fn get_recv_bytes(&self) -> u64 {
        self.recv_bytes.load(Ordering::SeqCst)
    }

    pub fn increment_recv_bytes(&self, len: u64) {
        self.recv_bytes.fetch_add(len, Ordering::SeqCst);
    }

    pub fn get_opened_at_s(&self) -> u64 {
        self.opened_at_s
    }

    pub fn get_connection_type(&self) -> ConnectionType {
        let mut lock = self.connection_type_watcher.lock().expect("poisoned");
        match lock.take() {
            Some(mut watcher) => {
                let connection_type = watcher.get();
                *lock = Some(watcher);
                connection_type
            }
            None => ConnectionType::None,
        }
    }
}
