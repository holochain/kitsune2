use iroh::{EndpointAddr, EndpointId, RelayUrl, TransportAddr};
use kitsune2_api::{K2Error, K2Result, Url};
use std::collections::HashMap;
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

pub(super) fn canonicalize_relay_url(
    relay_url: &RelayUrl,
    endpoint_id: EndpointId,
) -> K2Result<Url> {
    // Extract host from relay URL
    let relay_host = relay_url
        .host()
        .ok_or_else(|| K2Error::other("relay url must have host"))?;
    let relay_host = match relay_host {
        ::url::Host::Ipv6(addr) => format!("[{addr}]"),
        other => other.to_string(),
    };

    // Get port (either explicit or default for the scheme)
    let relay_port = relay_url.port_or_known_default().ok_or_else(|| {
        K2Error::other("relay url must have known default port")
    })?;

    // Construct kitsune2 URL: scheme://host:port/endpoint_id
    // The relay path (e.g. /relay/) is not included here because the
    // kitsune2 Url format only allows a single path segment.
    // When reconstructing the relay URL from a peer URL, we look up the
    // relay host in a map of known relays to recover the full URL with path.
    let canonical_relay_url = format!(
        "{}://{}:{}/{}",
        relay_url.scheme(),
        relay_host,
        relay_port,
        endpoint_id
    );

    Url::from_str(canonical_relay_url)
}

/// Reconstruct an iroh EndpointAddr from a kitsune2 peer URL.
///
/// The peer URL has the format `scheme://host:port/endpoint_id`.
/// To reconstruct the full relay URL (which may have a path like `/relay/`),
/// we check `known_relays` (host -> full relay URL) first, then
/// `configured_relay_url` if the host matches, then fall back to
/// constructing `scheme://host:port/` (no path).
pub(super) fn endpoint_from_url(
    url: &Url,
    configured_relay_url: Option<&str>,
    known_relays: &HashMap<String, String>,
) -> K2Result<EndpointAddr> {
    let peer_id = url
        .peer_id()
        .ok_or_else(|| K2Error::other("url must have peer id"))?;
    let endpoint_id = EndpointId::from_str(peer_id).map_err(|err| {
        K2Error::other_src("failed to convert peer id to endpoint id", err)
    })?;

    let peer_addr = url.addr();
    let peer_host = peer_addr.split(':').next().unwrap_or("");

    // Look up the relay URL for this peer's host:
    // 1. Check known_relays map (populated by insert_relay with the
    //    exact URL that was used to connect, preserving any path)
    // 2. Check if configured relay host matches (use configured URL as-is)
    // 3. Fall back to reconstructing from peer URL (no path)
    let relay_url = if let Some(relay_url_str) = known_relays.get(peer_host)
    {
        RelayUrl::from_str(relay_url_str).map_err(|err| {
            K2Error::other_src("invalid known relay url", err)
        })?
    } else if let Some(configured) = configured_relay_url {
        let configured_parsed = ::url::Url::from_str(configured).ok();
        let configured_host = configured_parsed
            .as_ref()
            .and_then(|u| u.host_str().map(|h| h.to_string()));

        if configured_host.as_deref() == Some(peer_host) {
            RelayUrl::from_str(configured).map_err(|err| {
                K2Error::other_src("invalid configured relay url", err)
            })?
        } else {
            relay_url_from_peer_url(url, peer_addr)?
        }
    } else {
        relay_url_from_peer_url(url, peer_addr)?
    };

    tracing::info!(
        peer_url = %url,
        ?configured_relay_url,
        resolved_relay_url = %relay_url,
        %endpoint_id,
        "endpoint_from_url: resolved peer -> relay"
    );

    Ok(EndpointAddr::from_parts(
        endpoint_id,
        [TransportAddr::Relay(relay_url)],
    ))
}

/// Construct a RelayUrl from a peer URL by using scheme://host:port/
fn relay_url_from_peer_url(url: &Url, peer_addr: &str) -> K2Result<RelayUrl> {
    let relay_scheme = if url.uses_tls() { "https" } else { "http" };
    let relay_url = format!("{relay_scheme}://{peer_addr}/");
    let relay_url = ::url::Url::from_str(&relay_url)
        .map_err(|err| K2Error::other_src("invalid relay url", err))?;
    Ok(RelayUrl::from(relay_url))
}
