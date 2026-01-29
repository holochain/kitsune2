//! Authentication module for the bootstrap server.
//!
//! This module provides authentication functionality that's independent
//! of the relay implementation (SBD or Iroh). It implements the
//! authentication hook server specification from:
//! <https://github.com/holochain/sbd/blob/main/spec-auth.md>

use base64::Engine;
use rand::Rng;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// Configuration for authentication.
#[derive(Debug, Clone)]
pub struct AuthConfig {
    /// URL of the authentication hook server (e.g., <http://auth.example.com/authenticate>)
    pub authentication_hook_server: Option<String>,

    /// Idle timeout for authentication tokens
    /// Tokens are invalidated after this period of inactivity
    pub auth_token_idle_timeout: std::time::Duration,
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            authentication_hook_server: None,
            auth_token_idle_timeout: std::time::Duration::from_secs(300), // 5 minutes
        }
    }
}

/// Tracks valid authentication tokens and their last-used timestamps.
#[derive(Clone, Default)]
pub struct AuthTokenTracker {
    tokens: Arc<Mutex<HashMap<Arc<str>, Instant>>>,
}

impl AuthTokenTracker {
    /// Register a new valid token or update an existing one.
    pub fn register_token(&self, token: Arc<str>) {
        let mut tokens = self.tokens.lock().unwrap();
        tokens.insert(token, Instant::now());
    }

    /// Check if a token is valid. Returns true if:
    /// - Token exists in the tracker and hasn't expired
    ///
    /// Updates the last-used timestamp if the token is valid.
    ///
    /// Note: When no hook server is configured, tokens are still generated
    /// and tracked, they're just generated locally without external validation.
    pub fn is_valid(
        &self,
        token: &Option<Arc<str>>,
        config: &AuthConfig,
    ) -> bool {
        let Some(token) = token else {
            // No token provided - only valid if no auth configured
            return config.authentication_hook_server.is_none();
        };

        let mut tokens = self.tokens.lock().unwrap();
        if let Some(last_used) = tokens.get_mut(token.as_ref()) {
            let now = Instant::now();
            if now.duration_since(*last_used) < config.auth_token_idle_timeout {
                *last_used = now; // Update last used time
                return true;
            }
            // Token expired
            tokens.remove(token.as_ref());
        }
        false
    }

    /// Remove expired tokens from the tracker.
    pub fn prune_expired(&self, config: &AuthConfig) {
        let mut tokens = self.tokens.lock().unwrap();
        let now = Instant::now();
        tokens.retain(|_, last_used| {
            now.duration_since(*last_used) < config.auth_token_idle_timeout
        });
    }
}

/// Errors that can occur during authentication.
#[derive(Debug)]
pub enum AuthenticateError {
    /// The client provided invalid authentication credentials.
    Unauthorized,
    /// There was an error communicating with the hook server.
    HookServerError(Box<dyn std::error::Error + Send + Sync>),
    /// Some other internal error occurred.
    OtherError(Box<dyn std::error::Error + Send + Sync>),
}

/// Process an authentication request.
///
/// If a hook server is configured, this will call the hook server's
/// `/authenticate` endpoint with the provided authentication bytes.
/// If successful, the returned token is registered in the tracker.
///
/// If no hook server is configured, a random token is generated
/// (effectively accepting all authentication attempts).
pub async fn process_authenticate(
    config: &AuthConfig,
    token_tracker: &AuthTokenTracker,
    auth_failures: opentelemetry::metrics::Counter<u64>,
    auth_bytes: bytes::Bytes,
) -> Result<Arc<str>, AuthenticateError> {
    if let Some(hook_server_url) = &config.authentication_hook_server {
        // Call hook server
        match call_hook_server(hook_server_url, auth_bytes).await {
            Ok(token) => {
                token_tracker.register_token(token.clone());
                Ok(token)
            }
            Err(err) => {
                auth_failures.add(1, &[]);
                Err(err)
            }
        }
    } else {
        // No hook server - generate random token (accept all)
        let mut token_bytes = [0u8; 32];
        rand::thread_rng().fill(&mut token_bytes);
        let token = Arc::<str>::from(
            base64::prelude::BASE64_URL_SAFE_NO_PAD.encode(token_bytes),
        );
        token_tracker.register_token(token.clone());
        Ok(token)
    }
}

/// Call the authentication hook server.
async fn call_hook_server(
    url: &str,
    auth_bytes: bytes::Bytes,
) -> Result<Arc<str>, AuthenticateError> {
    // Make HTTP request to hook server
    let client = reqwest::Client::new();
    let response = client
        .put(url)
        .header("Content-Type", "application/octet-stream")
        .body(auth_bytes)
        .send()
        .await
        .map_err(|e| AuthenticateError::HookServerError(Box::new(e)))?;

    match response.status() {
        reqwest::StatusCode::OK => {
            // Parse JSON response
            let json: serde_json::Value = response
                .json()
                .await
                .map_err(|e| AuthenticateError::OtherError(Box::new(e)))?;

            let token = json["authToken"].as_str().ok_or_else(|| {
                AuthenticateError::OtherError(
                    "Missing authToken in response".into(),
                )
            })?;

            Ok(Arc::<str>::from(token))
        }
        reqwest::StatusCode::UNAUTHORIZED => {
            Err(AuthenticateError::Unauthorized)
        }
        _ => {
            let status = response.status();
            Err(AuthenticateError::HookServerError(
                format!("Hook server returned {}", status).into(),
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_expiration() {
        let config = AuthConfig {
            authentication_hook_server: Some(
                "http://example.com/auth".to_string(),
            ),
            auth_token_idle_timeout: std::time::Duration::from_millis(100),
        };

        let tracker = AuthTokenTracker::default();
        let token: Arc<str> = Arc::from("test-token");

        // Register token
        tracker.register_token(token.clone());

        // Token should be valid immediately
        assert!(tracker.is_valid(&Some(token.clone()), &config));

        // Wait for token to expire
        std::thread::sleep(std::time::Duration::from_millis(150));

        // Token should now be invalid
        assert!(!tracker.is_valid(&Some(token.clone()), &config));
    }

    #[test]
    fn test_token_lifetime_extension() {
        let config = AuthConfig {
            authentication_hook_server: Some(
                "http://example.com/auth".to_string(),
            ),
            auth_token_idle_timeout: std::time::Duration::from_millis(200),
        };

        let tracker = AuthTokenTracker::default();
        let token: Arc<str> = Arc::from("test-token");

        // Register token
        tracker.register_token(token.clone());

        // Use token after 100ms (half the timeout)
        std::thread::sleep(std::time::Duration::from_millis(100));
        assert!(tracker.is_valid(&Some(token.clone()), &config));

        // Use token again after another 100ms
        // This should extend the lifetime
        std::thread::sleep(std::time::Duration::from_millis(100));
        assert!(tracker.is_valid(&Some(token.clone()), &config));

        // Wait 250ms more - token should now be expired (200ms timeout + some buffer)
        std::thread::sleep(std::time::Duration::from_millis(250));
        assert!(!tracker.is_valid(&Some(token.clone()), &config));
    }

    #[test]
    fn test_no_auth_configured() {
        let config = AuthConfig {
            authentication_hook_server: None,
            auth_token_idle_timeout: std::time::Duration::from_secs(300),
        };

        let tracker = AuthTokenTracker::default();

        // When no hook server is configured, no token is valid (backward compat)
        assert!(tracker.is_valid(&None, &config));

        // But if a token is provided, it still needs to be registered
        assert!(!tracker.is_valid(&Some(Arc::from("any-token")), &config));

        // After registering a token, it should be valid
        let token: Arc<str> = Arc::from("test-token");
        tracker.register_token(token.clone());
        assert!(tracker.is_valid(&Some(token), &config));
    }

    #[test]
    fn test_prune_expired() {
        let config = AuthConfig {
            authentication_hook_server: Some(
                "http://example.com/auth".to_string(),
            ),
            auth_token_idle_timeout: std::time::Duration::from_millis(100),
        };

        let tracker = AuthTokenTracker::default();

        // Register multiple tokens
        tracker.register_token(Arc::from("token1"));
        tracker.register_token(Arc::from("token2"));
        tracker.register_token(Arc::from("token3"));

        // Wait for tokens to expire
        std::thread::sleep(std::time::Duration::from_millis(150));

        // Prune expired tokens
        tracker.prune_expired(&config);

        // All tokens should be gone
        assert!(!tracker.is_valid(&Some(Arc::from("token1")), &config));
        assert!(!tracker.is_valid(&Some(Arc::from("token2")), &config));
        assert!(!tracker.is_valid(&Some(Arc::from("token3")), &config));
    }
}
