//! A client for the Kitsune2 bootstrap server.

#![deny(missing_docs)]

use base64::Engine;
use kitsune2_api::{AgentInfoSigned, DynVerifier, K2Error, K2Result, SpaceId};
use std::sync::{Arc, Mutex, PoisonError};
use url::Url;

/// Determine how we should handle an internal request for authorization
/// on the [AuthMaterial].
enum AuthType {
    /// Only authenticate if we don't currently have any token at all.
    IfUninit,

    /// Authenticate even if we have a token. Basically, the token has expired.
    Force,
}

/// Authentication material.
pub struct AuthMaterial {
    auth_material: Vec<u8>,
    auth_token: Mutex<Option<String>>,
}

impl std::fmt::Debug for AuthMaterial {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("AuthMaterial")
    }
}

impl AuthMaterial {
    /// Provide authentication material.
    pub fn new(auth_material: Vec<u8>) -> Self {
        Self {
            auth_material,
            auth_token: Mutex::new(None),
        }
    }

    /// This is mainly a testing api.
    pub fn danger_access_token(&self) -> &Mutex<Option<String>> {
        &self.auth_token
    }

    /// Returns the currently cached auth token, if any.
    pub fn token(&self) -> Option<String> {
        self.auth_token
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn priv_authenticate(
        &self,
        auth_url: &str,
        auth_type: AuthType,
    ) -> K2Result<()> {
        if matches!(auth_type, AuthType::IfUninit)
            && self
                .auth_token
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .is_some()
        {
            return Ok(());
        }

        tracing::debug!(url = auth_url, "Authenticating with bootstrap server");

        let response = ureq::put(auth_url)
            .send(&self.auth_material[..])
            .map_err(|err| K2Error::other_src("Authenticate Failed", err))?;

        // A 202 Accepted response means the server received the credentials
        // but the key is pending approval (e.g. waiting for an operator to
        // allowlist it). There is no token yet, so we cannot proceed.
        if response.status() == 202 {
            return Err(K2Error::other(
                "Authentication pending: key awaiting approval on the server",
            ));
        }

        let token = response
            .into_body()
            .read_to_string()
            .map_err(|err| K2Error::other_src("Authenticate Failed", err))?;

        #[derive(serde::Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct AuthToken {
            auth_token: String,
        }

        let auth_token: AuthToken = serde_json::from_str(&token)
            .map_err(|err| K2Error::other_src("Authenticate Failed", err))?;

        *self
            .auth_token
            .lock()
            .unwrap_or_else(PoisonError::into_inner) =
            Some(auth_token.auth_token);

        tracing::debug!("Authentication successful, token acquired");
        Ok(())
    }
}

enum Res<T> {
    Ok(T),
    Auth,
    HttpErr(u16),
    Err(K2Error),
}

impl<T> Res<T> {
    fn needs_auth(&self) -> bool {
        matches!(self, Self::Auth)
    }
}

impl<T> From<Result<T, ureq::Error>> for Res<T> {
    fn from(r: Result<T, ureq::Error>) -> Self {
        match r {
            Ok(t) => Self::Ok(t),
            Err(ureq::Error::StatusCode(401)) => Self::Auth,
            Err(ureq::Error::StatusCode(code)) => Self::HttpErr(code),
            Err(err) => Self::Err(K2Error::other(err)),
        }
    }
}

impl<T> From<std::io::Result<T>> for Res<T> {
    fn from(r: std::io::Result<T>) -> Self {
        match r {
            Ok(t) => Self::Ok(t),
            Err(err) => Self::Err(K2Error::other(err)),
        }
    }
}

impl<T> From<Res<T>> for K2Result<T> {
    fn from(r: Res<T>) -> Self {
        match r {
            Res::Ok(t) => Ok(t),
            Res::Auth => Err(K2Error::other("Unauthorized")),
            Res::HttpErr(code) => Err(K2Error::other(format!(
                "Bootstrap server returned HTTP {code}"
            ))),
            Res::Err(err) => Err(err),
        }
    }
}

/// Send the agent info, for the given space, to the bootstrap server.
///
/// Note the `blocking_` prefix. This is a hint to the caller that if the
/// function is used in an async context, it should be treated as a blocking
/// operation.
pub fn blocking_put(
    server_url: Url,
    agent_info: &AgentInfoSigned,
) -> K2Result<()> {
    blocking_put_auth(server_url, agent_info, None)
}

/// Send the agent info, for the given space, to the bootstrap server.
///
/// Note the `blocking_` prefix. This is a hint to the caller that if the
/// function is used in an async context, it should be treated as a blocking
/// operation.
pub fn blocking_put_auth(
    mut server_url: Url,
    agent_info: &AgentInfoSigned,
    auth_material: Option<&AuthMaterial>,
) -> K2Result<()> {
    tracing::debug!(
        space = %base64::prelude::BASE64_URL_SAFE_NO_PAD.encode(&**agent_info.space),
        agent = %base64::prelude::BASE64_URL_SAFE_NO_PAD.encode(&**agent_info.agent),
        "Putting agent info to bootstrap server",
    );

    server_url.set_path("authenticate");
    let auth_url = server_url.as_str().to_string();

    server_url.set_path(&format!(
        "bootstrap/{}/{}",
        base64::prelude::BASE64_URL_SAFE_NO_PAD.encode(&**agent_info.space),
        base64::prelude::BASE64_URL_SAFE_NO_PAD.encode(&**agent_info.agent),
    ));
    let put_url = server_url.as_str().to_string();

    if let Some(auth_material) = &auth_material {
        auth_material.priv_authenticate(&auth_url, AuthType::IfUninit)?;
    }

    let encoded = agent_info.encode()?;

    fn priv_put(
        put_url: &str,
        encoded: &str,
        auth_material: &Option<&AuthMaterial>,
    ) -> Res<()> {
        let mut req = ureq::put(put_url);

        if let Some(auth_material) = auth_material {
            let token = auth_material
                .auth_token
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone()
                .expect("authenticated token must be cached");
            req = req.header("Authorization", &format!("Bearer {token}"));
        }

        req.send(encoded).map(|_| ()).into()
    }

    let mut res = priv_put(&put_url, &encoded, &auth_material);

    if let Some(auth_material) = auth_material
        && res.needs_auth()
    {
        auth_material.priv_authenticate(&auth_url, AuthType::Force)?;
        res = priv_put(&put_url, &encoded, &Some(auth_material));
    }

    if let Res::HttpErr(code) = &res {
        tracing::warn!(
            url = put_url,
            status = code,
            "Bootstrap PUT returned HTTP error"
        );
    }

    res.into()
}

/// Fetch the bearer token for the relay on the bootstrap server.
///
/// The returned token should be presented on the relay WebSocket upgrade via
/// `RelayConfig::with_auth_token`, which sends it as an
/// `Authorization: Bearer` header that the relay validates at connect time.
///
/// A cached token is reused without contacting the server. Operations that
/// receive a 401 response refresh the token internally before retrying.
///
/// Note the `blocking_` prefix. This is a hint to the caller that if the
/// function is used in an async context, it should be treated as a blocking
/// operation.
///
/// # Errors
/// Returns an error if the authentication request fails, if the key is
/// pending approval on the hook server, or if the response is malformed.
pub fn blocking_fetch_relay_token(
    mut server_url: Url,
    auth_material: &AuthMaterial,
) -> K2Result<String> {
    server_url.set_path("authenticate");
    auth_material.priv_authenticate(server_url.as_str(), AuthType::IfUninit)?;
    auth_material.token().ok_or_else(|| {
        K2Error::other("authentication succeeded but no token was cached")
    })
}

/// Keep an iroh endpoint public key registered with the relay on the
/// bootstrap server.
///
/// After authenticating (which yields a bearer token), this function
/// registers the 32-byte iroh public key with the server's relay allowlist
/// so that the endpoint stays permitted to connect to the relay.
///
/// The allowlist complements the bearer token presented on the relay
/// WebSocket upgrade (see [`blocking_fetch_relay_token`]): iroh captures
/// that token once per relay connection actor and cannot refresh it while
/// the actor lives, so the allowlist — keyed on the handshake-proven public
/// key — is what re-admits an actor whose token has gone stale. After a
/// bootstrap server restart, this call first obtains a fresh token and then
/// repopulates the allowlist. Call it periodically to keep the entry alive.
///
/// This function should only be called when the server is configured with an
/// auth hook server. Open relays do not expose the `relay/keepalive`
/// endpoint, and the keepalive is not required when the relay has no access
/// restrictions.
///
/// Note the `blocking_` prefix. This is a hint to the caller that if the
/// function is used in an async context, it should be treated as a blocking
/// operation.
pub fn blocking_relay_keepalive(
    mut server_url: Url,
    auth_material: &AuthMaterial,
    key_bytes: &[u8; 32],
) -> K2Result<()> {
    tracing::info!(
        server_url = %server_url,
        iroh_key = %base64::prelude::BASE64_URL_SAFE_NO_PAD.encode(key_bytes),
        "Keeping iroh endpoint key registered with relay service",
    );

    server_url.set_path("authenticate");
    let auth_url = server_url.as_str().to_string();
    auth_material.priv_authenticate(&auth_url, AuthType::IfUninit)?;

    server_url.set_path("relay/keepalive");
    let keepalive_url = server_url.as_str().to_string();

    fn priv_keepalive(
        keepalive_url: &str,
        key_bytes: &[u8; 32],
        auth_material: &AuthMaterial,
    ) -> Res<()> {
        let token = auth_material
            .auth_token
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
            .expect("authenticated token must be cached");
        ureq::put(keepalive_url)
            .header("Content-Type", "application/octet-stream")
            .header("Authorization", &format!("Bearer {token}"))
            .send(key_bytes.as_ref())
            .map(|_| ())
            .into()
    }

    let mut res = priv_keepalive(&keepalive_url, key_bytes, auth_material);

    if res.needs_auth() {
        tracing::debug!("Relay keepalive returned 401, re-authenticating");
        server_url.set_path("authenticate");
        let auth_url = server_url.as_str().to_string();
        auth_material.priv_authenticate(&auth_url, AuthType::Force)?;
        res = priv_keepalive(&keepalive_url, key_bytes, auth_material);
    }

    let result: K2Result<()> = res.into();
    match &result {
        Ok(()) => tracing::info!("Iroh relay keepalive succeeded"),
        Err(e) => tracing::warn!(?e, "Iroh relay keepalive failed"),
    }
    result
}

/// Get all agent infos from the bootstrap server for the given space.
///
/// Note the `blocking_` prefix. This is a hint to the caller that if the
/// function is used in an async context, it should be treated as a blocking
/// operation.
pub fn blocking_get(
    server_url: Url,
    space_id: SpaceId,
    verifier: DynVerifier,
) -> K2Result<Vec<Arc<AgentInfoSigned>>> {
    blocking_get_auth(server_url, space_id, verifier, None)
}

/// Get all agent infos from the bootstrap server for the given space.
///
/// Note the `blocking_` prefix. This is a hint to the caller that if the
/// function is used in an async context, it should be treated as a blocking
/// operation.
pub fn blocking_get_auth(
    mut server_url: Url,
    space_id: SpaceId,
    verifier: DynVerifier,
    mut auth_material: Option<&AuthMaterial>,
) -> K2Result<Vec<Arc<AgentInfoSigned>>> {
    tracing::debug!(
        space = %base64::prelude::BASE64_URL_SAFE_NO_PAD.encode(&**space_id),
        "Getting agent infos from bootstrap server",
    );

    server_url.set_path("authenticate");
    let auth_url = server_url.as_str().to_string();

    if let Some(auth_material) = &mut auth_material {
        auth_material.priv_authenticate(&auth_url, AuthType::IfUninit)?;
    }

    server_url.set_path(&format!(
        "bootstrap/{}",
        base64::prelude::BASE64_URL_SAFE_NO_PAD.encode(&**space_id)
    ));
    let get_url = server_url.as_str().to_string();

    fn priv_get(
        get_url: &str,
        auth_material: &Option<&AuthMaterial>,
    ) -> Res<String> {
        let mut req = ureq::get(get_url);

        if let Some(auth_material) = auth_material {
            let token = auth_material
                .auth_token
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone()
                .expect("authenticated token must be cached");
            req = req.header("Authorization", &format!("Bearer {token}"));
        }

        match req.call() {
            Ok(r) => r.into_body().read_to_string().into(),
            Err(err) => Err(err).into(),
        }
    }

    let mut res = priv_get(&get_url, &auth_material);

    if let Some(auth_material) = auth_material
        && res.needs_auth()
    {
        tracing::debug!(
            url = get_url,
            "Bootstrap GET returned 401, re-authenticating"
        );
        auth_material.priv_authenticate(&auth_url, AuthType::Force)?;
        res = priv_get(&get_url, &Some(auth_material));
    }

    match &res {
        Res::Auth => tracing::warn!(
            url = get_url,
            "Bootstrap GET returned 401 Unauthorized (even after re-auth)"
        ),
        Res::HttpErr(code) => tracing::warn!(
            url = get_url,
            status = code,
            "Bootstrap GET returned HTTP error"
        ),
        Res::Err(_) | Res::Ok(_) => {}
    }
    let res = K2Result::from(res)?;

    let agents = AgentInfoSigned::decode_list(&verifier, res.as_bytes())
        .map_err(|e| {
            tracing::warn!(url = get_url, err = ?e, "Failed to decode bootstrap GET response body");
            e
        })?
        .into_iter()
        .filter_map(|l| {
            l.inspect_err(|err| {
                tracing::debug!(?err, "failure decoding bootstrap agent info");
            })
            .ok()
        })
        .collect::<Vec<_>>();

    tracing::debug!("Bootstrap GET complete");
    Ok(agents)
}
