//! Bearer-token access control for the embedded iroh relay.
//!
//! Replicates upstream iroh's authenticated-relay model: the client presents
//! its auth token as an `Authorization: Bearer` header on the relay WebSocket
//! upgrade (set via `RelayConfig::with_auth_token` on the client side), and
//! the server validates it once per connection in
//! [`AccessControl::on_connect`].
//!
//! Validation goes through [`AuthTokenTracker::is_valid`], which also
//! refreshes the token's last-used timestamp, so every relay (re)connect
//! keeps the token alive. [`RelayConnTracker`] additionally records which
//! token backs each live connection so the prune worker can keep tokens
//! with open relay connections from idle-expiring.

use crate::auth::{AuthConfig, AuthTokenTracker};
use crate::relay_allowlist::RelayAllowlist;
use iroh_base::EndpointId;
use iroh_relay::server::{Access, AccessControl, ClientRequest, ConnectionId};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

/// Tracks which bearer token authorized each live relay connection.
#[derive(Clone, Debug, Default)]
pub struct RelayConnTracker {
    // ConnectionId is process-unique (guaranteed upstream), so it is safe to
    // use as a key here; multiple connections may share one token.
    conns: Arc<Mutex<HashMap<ConnectionId, Arc<str>>>>,
}

impl RelayConnTracker {
    fn insert(&self, id: ConnectionId, token: Arc<str>) {
        self.conns
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(id, token);
    }

    fn remove(&self, id: ConnectionId) {
        self.conns
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&id);
    }

    /// Tokens that back at least one live relay connection, deduplicated.
    pub fn live_tokens(&self) -> Vec<Arc<str>> {
        let conns = self.conns.lock().unwrap_or_else(PoisonError::into_inner);
        let mut tokens: Vec<Arc<str>> = conns.values().cloned().collect();
        tokens.sort();
        tokens.dedup();
        tokens
    }
}

/// Relay access control gating connections on a valid bearer token.
///
/// Falls back to the public-key allowlist populated via
/// `PUT /relay/keepalive` when the presented token is stale. iroh captures
/// the token once per relay connection actor and cannot refresh it while
/// the actor lives, so this recovery path is what re-admits an actor whose
/// token expired. It can be removed once iroh supports refreshing the token
/// on a live connection.
pub struct TokenAccess {
    auth_config: AuthConfig,
    auth_tracker: AuthTokenTracker,
    conns: RelayConnTracker,
    recovery_allowlist: RelayAllowlist,
}

impl TokenAccess {
    /// Create a new token-based access control.
    pub fn new(
        auth_config: AuthConfig,
        auth_tracker: AuthTokenTracker,
        conns: RelayConnTracker,
        recovery_allowlist: RelayAllowlist,
    ) -> Self {
        Self {
            auth_config,
            auth_tracker,
            conns,
            recovery_allowlist,
        }
    }
}

impl std::fmt::Debug for TokenAccess {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TokenAccess").finish()
    }
}

impl AccessControl for TokenAccess {
    async fn on_connect(&self, request: &ClientRequest) -> Access {
        if let Some(token) = request.auth_token() {
            let token: Arc<str> = Arc::from(token);
            if self
                .auth_tracker
                .is_valid(&Some(token.clone()), &self.auth_config)
            {
                self.conns.insert(request.connection_id(), token);
                tracing::debug!(
                    key = %request.endpoint_id().fmt_short(),
                    "Relay access granted via bearer token"
                );
                return Access::Allow;
            }

            // Iroh cannot replace a stale token on the live relay actor, so
            // admit it by its handshake-proven public key when a keepalive
            // has registered the key. Connections without a token are never
            // admitted this way.
            if self.recovery_allowlist.is_allowed(&request.endpoint_id()) {
                tracing::debug!(
                    key = %request.endpoint_id().fmt_short(),
                    "Relay access granted via recovery allowlist"
                );
                return Access::Allow;
            }
        }

        tracing::debug!(
            key = %request.endpoint_id().fmt_short(),
            "Relay access denied"
        );
        Access::Deny {
            reason: Some("not authorized".to_string()),
        }
    }

    fn on_disconnect(
        &self,
        _endpoint_id: EndpointId,
        connection_id: ConnectionId,
    ) {
        self.conns.remove(connection_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::{AuthConfig, AuthTokenTracker};
    use crate::relay_allowlist::RelayAllowlist;
    use iroh_base::SecretKey;
    use iroh_relay::http::ProtocolVersion;
    use iroh_relay::server::{Access, ClientRequest};
    use std::sync::Arc;

    fn auth_config() -> AuthConfig {
        AuthConfig {
            authentication_hook_server: Some("http://example.com".into()),
            auth_token_idle_timeout: std::time::Duration::from_secs(300),
        }
    }

    fn client_request(key: &SecretKey, bearer: Option<&str>) -> ClientRequest {
        let mut builder = http::Request::builder()
            .method(http::Method::GET)
            .uri("https://relay.test/relay");
        if let Some(token) = bearer {
            builder = builder
                .header(http::header::AUTHORIZATION, format!("Bearer {token}"));
        }
        let parts = builder.body(()).unwrap().into_parts().0;
        ClientRequest::new(key.public(), ProtocolVersion::V2, parts)
    }

    /// Mint a fresh, process-unique `ConnectionId` for tests.
    ///
    /// `ConnectionId` has no public constructor; a new one is assigned to
    /// every `ClientRequest`, so build a throwaway request and take its id.
    fn test_connection_id() -> ConnectionId {
        client_request(&SecretKey::generate(), None).connection_id()
    }

    #[test]
    fn live_tokens_survive_prune_refresh() {
        let config = AuthConfig {
            authentication_hook_server: Some("http://example.com".into()),
            auth_token_idle_timeout: std::time::Duration::from_millis(100),
        };
        let tracker = AuthTokenTracker::default();
        tracker.register_token("live-tok".into());
        tracker.register_token("idle-tok".into());

        let conns = RelayConnTracker::default();
        conns.insert(test_connection_id(), "live-tok".into());

        std::thread::sleep(std::time::Duration::from_millis(150));

        // Mirrors the prune worker: refresh live tokens, then prune.
        for token in conns.live_tokens() {
            tracker.register_token(token);
        }
        let expired = tracker.prune_expired(&config);

        assert_eq!(expired, vec![Arc::<str>::from("idle-tok")]);
        assert!(tracker.is_valid(&Some("live-tok".into()), &config));
    }

    #[tokio::test]
    async fn valid_token_is_allowed_and_tracked() {
        let tracker = AuthTokenTracker::default();
        tracker.register_token("tok-1".into());
        let conns = RelayConnTracker::default();
        let access = TokenAccess::new(
            auth_config(),
            tracker,
            conns.clone(),
            RelayAllowlist::default(),
        );

        let req = client_request(&SecretKey::generate(), Some("tok-1"));
        assert!(matches!(access.on_connect(&req).await, Access::Allow));
        assert_eq!(conns.live_tokens(), vec![Arc::<str>::from("tok-1")]);
    }

    #[tokio::test]
    async fn missing_or_invalid_token_is_denied_with_reason() {
        let access = TokenAccess::new(
            auth_config(),
            AuthTokenTracker::default(),
            RelayConnTracker::default(),
            RelayAllowlist::default(),
        );

        for bearer in [None, Some("unknown-token")] {
            let req = client_request(&SecretKey::generate(), bearer);
            match access.on_connect(&req).await {
                Access::Deny { reason } => {
                    assert_eq!(reason.as_deref(), Some("not authorized"))
                }
                Access::Allow => panic!("must deny"),
            }
        }
    }

    #[tokio::test]
    async fn registered_key_with_stale_token_is_allowed() {
        let allowlist = RelayAllowlist::default();
        let key = SecretKey::generate();
        allowlist.register(key.public(), "fresh-tok".into());
        let access = TokenAccess::new(
            auth_config(),
            AuthTokenTracker::default(),
            RelayConnTracker::default(),
            allowlist,
        );

        // The actor still presents an expired token, so recovery must admit
        // it by its registered public key.
        let req = client_request(&key, Some("stale-tok"));
        assert!(matches!(access.on_connect(&req).await, Access::Allow));
    }

    #[tokio::test]
    async fn registered_key_without_token_is_denied() {
        let allowlist = RelayAllowlist::default();
        let key = SecretKey::generate();
        allowlist.register(key.public(), "fresh-tok".into());
        let access = TokenAccess::new(
            auth_config(),
            AuthTokenTracker::default(),
            RelayConnTracker::default(),
            allowlist,
        );

        // Recovery only applies to actors that present a (stale) token;
        // a connection with no token at all is denied even when the key
        // is registered.
        let req = client_request(&key, None);
        assert!(matches!(access.on_connect(&req).await, Access::Deny { .. }));
    }

    #[tokio::test]
    async fn disconnect_removes_live_token() {
        let tracker = AuthTokenTracker::default();
        tracker.register_token("tok-1".into());
        let conns = RelayConnTracker::default();
        let access = TokenAccess::new(
            auth_config(),
            tracker,
            conns.clone(),
            RelayAllowlist::default(),
        );

        let key = SecretKey::generate();
        let req = client_request(&key, Some("tok-1"));
        access.on_connect(&req).await;
        access.on_disconnect(req.endpoint_id(), req.connection_id());
        assert!(conns.live_tokens().is_empty());
    }

    #[tokio::test]
    async fn on_connect_refreshes_token_last_used() {
        let tracker = AuthTokenTracker::default();
        tracker.register_token("tok-1".into());
        let last_used_before = tracker.last_used("tok-1").unwrap();
        let access = TokenAccess::new(
            auth_config(),
            tracker.clone(),
            RelayConnTracker::default(),
            RelayAllowlist::default(),
        );

        // Ensure the monotonic clock advances before on_connect refreshes it.
        std::thread::sleep(std::time::Duration::from_millis(10));
        let req = client_request(&SecretKey::generate(), Some("tok-1"));
        assert!(matches!(access.on_connect(&req).await, Access::Allow));
        let last_used_after = tracker.last_used("tok-1").unwrap();

        assert!(last_used_after > last_used_before);
    }
}
