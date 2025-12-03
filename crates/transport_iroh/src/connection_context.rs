use iroh::endpoint::Connection;
use kitsune2_api::{TxImpHnd, Url};
use std::{
    fmt,
    sync::{Arc, RwLock},
};

pub(super) struct ConnectionContext {
    handler: Arc<TxImpHnd>,
    connection: Arc<Connection>,
    remote_url: RwLock<Option<Url>>,
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
    ) -> Self {
        Self {
            handler,
            connection,
            remote_url: RwLock::new(remote_url),
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
            self.handler.peer_disconnect(peer, None);
        }
    }
}
