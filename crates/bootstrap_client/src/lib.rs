//! A client for the Kitsune2 bootstrap server.

#![deny(missing_docs)]

use kitsune2_api::{AgentInfoSigned, DynVerifier, K2Error, K2Result, SpaceId};
use std::sync::Arc;

/// Send the agent info, for the given space, to the bootstrap server.
///
/// Note the `blocking_` prefix. This is a hint to the caller that if the function is used in
/// an async context, it should be treated as a blocking operation.
pub fn blocking_put(
    server_url: &str,
    agent_info: &AgentInfoSigned,
) -> K2Result<()> {
    let url = format!(
        "{server_url}/bootstrap/{}/{}",
        &agent_info.space, &agent_info.agent
    );

    let encoded = agent_info.encode()?;
    ureq::put(&url)
        .send_string(&encoded)
        .map_err(|e| K2Error::other_src("Failed to put agent info", e))?;

    Ok(())
}

/// Get all agent infos from the bootstrap server for the given space.
///
/// Note the `blocking_` prefix. This is a hint to the caller that if the function is used in
/// an async context, it should be treated as a blocking operation.
pub fn blocking_get(
    server_url: &str,
    space: SpaceId,
    verifier: DynVerifier,
) -> K2Result<Vec<Arc<AgentInfoSigned>>> {
    let url = format!("{server_url}/bootstrap/{}", space);

    let encoded = ureq::get(&url)
        .call()
        .map_err(K2Error::other)?
        .into_string()
        .map_err(K2Error::other)?;

    Ok(AgentInfoSigned::decode_list(&verifier, encoded.as_bytes())?
        .into_iter()
        .filter_map(|l| {
            l.inspect_err(|err| {
                tracing::debug!(?err, "failure decoding bootstrap agent info");
            })
            .ok()
        })
        .collect::<Vec<_>>())
}
