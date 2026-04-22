use iroh::{EndpointAddr, EndpointId, RelayUrl, TransportAddr};
use kitsune2_api::{K2Error, K2Result, Url};
use std::str::FromStr;

pub(super) fn get_url_with_first_relay(
    endpoint_addr: &EndpointAddr,
) -> Option<Url> {
    endpoint_addr.relay_urls().find_map(
        |relay_url| match canonicalize_relay_url(relay_url, endpoint_addr.id) {
            Ok(url) => Some(url),
            Err(err) => {
                tracing::error!(
                    ?relay_url,
                    ?err,
                    "could not canonicalize RelayUrl"
                );
                None
            }
        },
    )
}

/// Construct a kitsune2 peer URL from a relay URL and endpoint ID.
///
/// The relay path is preserved in the peer URL so that the full relay
/// URL can be reconstructed later without external lookup maps.
/// For example, relay `https://boot.example.com:443/relay/` with
/// endpoint ID `abc` produces `https://boot.example.com:443/relay/abc`.
pub(super) fn canonicalize_relay_url(
    relay_url: &RelayUrl,
    endpoint_id: EndpointId,
) -> K2Result<Url> {
    let relay_host = relay_url
        .host()
        .ok_or_else(|| K2Error::other("relay url must have host"))?;
    let relay_host = match relay_host {
        ::url::Host::Ipv6(addr) => format!("[{addr}]"),
        other => other.to_string(),
    };

    let relay_port = relay_url.port_or_known_default().ok_or_else(|| {
        K2Error::other("relay url must have known default port")
    })?;

    let relay_path = relay_url.path().trim_end_matches('/');

    let canonical_relay_url = format!(
        "{}://{}:{}{}/{}",
        relay_url.scheme(),
        relay_host,
        relay_port,
        relay_path,
        endpoint_id
    );

    Url::from_str(canonical_relay_url)
}

/// Extract the relay URL from a kitsune2 peer URL.
///
/// Everything before the last path segment is the relay URL.
pub(super) fn relay_url_from_peer_url(url: &Url) -> K2Result<RelayUrl> {
    let s = url.as_str();
    let last_slash = s
        .rfind('/')
        .ok_or_else(|| K2Error::other("url has no path"))?;
    let relay_url_str = format!("{}/", &s[..last_slash]);
    RelayUrl::from_str(&relay_url_str)
        .map_err(|err| K2Error::other_src("invalid relay url", err))
}

/// Reconstruct an iroh EndpointAddr from a kitsune2 peer URL.
///
/// The peer URL encodes the full relay path, so the relay URL can be
/// extracted directly: everything before the last path segment is the
/// relay URL, and the last segment is the endpoint ID.
pub(super) fn endpoint_from_url(url: &Url) -> K2Result<EndpointAddr> {
    let peer_id = url
        .peer_id()
        .ok_or_else(|| K2Error::other("url must have peer id"))?;
    let endpoint_id = EndpointId::from_str(peer_id).map_err(|err| {
        K2Error::other_src("failed to convert peer id to endpoint id", err)
    })?;

    let relay_url = relay_url_from_peer_url(url)?;

    tracing::info!(
        peer_url = %url,
        resolved_relay_url = %relay_url,
        %endpoint_id,
        "endpoint_from_url: resolved peer -> relay"
    );

    Ok(EndpointAddr::from_parts(
        endpoint_id,
        [TransportAddr::Relay(relay_url)],
    ))
}
