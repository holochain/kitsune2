//! A client for the Kitsune2 bootstrap server.

#![deny(missing_docs)]

use base64::Engine;
use kitsune2_api::{AgentInfoSigned, DynVerifier, K2Error, K2Result, SpaceId};
use std::sync::{Arc, Mutex};
use url::Url;

enum AuthType {
    IfUninit,
    Force,
}

/// Authentication material.
pub struct AuthMaterial {
    auth_material: Vec<u8>,
    auth_token: Mutex<Option<String>>,
}

impl AuthMaterial {
    /// Provide authentication material.
    pub fn new(auth_material: Vec<u8>) -> Self {
        Self {
            auth_material,
            auth_token: Mutex::new(None),
        }
    }

    fn priv_authenticate(
        &self,
        auth_url: &str,
        auth_type: AuthType,
    ) -> K2Result<()> {
        if matches!(auth_type, AuthType::IfUninit)
            && self.auth_token.lock().unwrap().is_some()
        {
            return Ok(());
        }

        let token = ureq::put(auth_url)
            .send(&self.auth_material[..])
            .map_err(|err| K2Error::other_src("Authenticate Failed", err))?
            .into_string()
            .map_err(|err| K2Error::other_src("Authenticate Failed", err))?;

        #[derive(serde::Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct AuthToken {
            auth_token: String,
        }

        let auth_token: AuthToken = serde_json::from_str(&token)
            .map_err(|err| K2Error::other_src("Authenticate Failed", err))?;

        *self.auth_token.lock().unwrap() = Some(auth_token.auth_token);

        Ok(())
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

enum Res<T> {
    Ok(T),
    Auth,
    Err(K2Error),
}

impl<T> Res<T> {
    fn needs_auth(&self) -> bool {
        matches!(self, Self::Auth)
    }
}

impl<T> From<std::result::Result<T, ureq::Error>> for Res<T> {
    fn from(r: std::result::Result<T, ureq::Error>) -> Self {
        match r {
            Ok(t) => Self::Ok(t),
            Err(ureq::Error::Status(401, _)) => Self::Auth,
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
            Res::Err(err) => Err(err),
        }
    }
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
            let token =
                auth_material.auth_token.lock().unwrap().clone().unwrap();
            req = req.set("Authenticate", &format!("Bearer {token}"));
        }

        req.send_string(encoded).map(|_| ()).into()
    }

    let mut res = priv_put(&put_url, &encoded, &auth_material);

    if let Some(auth_material) = auth_material {
        if res.needs_auth() {
            auth_material.priv_authenticate(&auth_url, AuthType::Force)?;
            res = priv_put(&put_url, &encoded, &Some(auth_material));
        }
    }

    res.into()
}

/// Get all agent infos from the bootstrap server for the given space.
///
/// Note the `blocking_` prefix. This is a hint to the caller that if the
/// function is used in an async context, it should be treated as a blocking
/// operation.
pub fn blocking_get(
    server_url: Url,
    space: SpaceId,
    verifier: DynVerifier,
) -> K2Result<Vec<Arc<AgentInfoSigned>>> {
    blocking_get_auth(server_url, space, verifier, None)
}

/// Get all agent infos from the bootstrap server for the given space.
///
/// Note the `blocking_` prefix. This is a hint to the caller that if the
/// function is used in an async context, it should be treated as a blocking
/// operation.
pub fn blocking_get_auth(
    mut server_url: Url,
    space: SpaceId,
    verifier: DynVerifier,
    mut auth_material: Option<&AuthMaterial>,
) -> K2Result<Vec<Arc<AgentInfoSigned>>> {
    server_url.set_path("authenticate");
    let auth_url = server_url.as_str().to_string();

    if let Some(auth_material) = &mut auth_material {
        auth_material.priv_authenticate(&auth_url, AuthType::IfUninit)?;
    }

    server_url.set_path(&format!(
        "bootstrap/{}",
        base64::prelude::BASE64_URL_SAFE_NO_PAD.encode(&**space)
    ));
    let get_url = server_url.as_str().to_string();

    fn priv_get(
        get_url: &str,
        auth_material: &Option<&AuthMaterial>,
    ) -> Res<String> {
        let mut req = ureq::get(get_url);

        if let Some(auth_material) = auth_material {
            let token =
                auth_material.auth_token.lock().unwrap().clone().unwrap();
            req = req.set("Authenticate", &format!("Bearer {token}"));
        }

        match req.call() {
            Ok(r) => r.into_string().into(),
            Err(err) => Err(err).into(),
        }
    }

    let mut res = priv_get(&get_url, &auth_material);

    if let Some(auth_material) = auth_material {
        if res.needs_auth() {
            auth_material.priv_authenticate(&auth_url, AuthType::Force)?;
            res = priv_get(&get_url, &Some(auth_material));
        }
    }

    let res = K2Result::from(res)?;

    Ok(AgentInfoSigned::decode_list(&verifier, res.as_bytes())?
        .into_iter()
        .filter_map(|l| {
            l.inspect_err(|err| {
                tracing::debug!(?err, "failure decoding bootstrap agent info");
            })
            .ok()
        })
        .collect::<Vec<_>>())
}
