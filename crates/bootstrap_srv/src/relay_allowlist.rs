use iroh_base::PublicKey;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Allowlist mapping iroh endpoint public keys to the bearer token
/// that was used to register them.
///
/// When the bootstrap server is configured with an authentication hook
/// server, the relay validates the bearer token presented on the WebSocket
/// upgrade, and falls back to this allowlist for recovery: iroh cannot
/// refresh the token on a live relay connection actor, so an actor whose
/// token went stale is re-admitted by its handshake-proven public key.
/// Clients keep their entry alive by periodically calling
/// `PUT /relay/keepalive` with a valid bearer token.
#[derive(Clone, Default)]
pub struct RelayAllowlist {
    entries: Arc<Mutex<HashMap<PublicKey, Arc<str>>>>,
}

impl RelayAllowlist {
    /// Register an iroh endpoint key with the given bearer token.
    ///
    /// If the key is already registered, the token is updated.
    pub fn register(&self, key: PublicKey, token: Arc<str>) {
        self.entries.lock().unwrap().insert(key, token);
    }

    /// Returns true if the given key is currently in the allowlist.
    pub fn is_allowed(&self, key: &PublicKey) -> bool {
        self.entries.lock().unwrap().contains_key(key)
    }

    /// Returns the number of entries in the allowlist.
    pub fn len(&self) -> usize {
        self.entries.lock().unwrap().len()
    }

    /// Returns true if the allowlist is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.lock().unwrap().is_empty()
    }

    /// Remove entries whose associated bearer token has been pruned from
    /// the auth tracker (i.e. the token has expired).
    pub fn prune_for_expired_tokens(&self, expired_tokens: &[Arc<str>]) {
        if expired_tokens.is_empty() {
            return;
        }
        let mut entries = self.entries.lock().unwrap();
        entries.retain(|_, token| !expired_tokens.contains(token));
    }
}
