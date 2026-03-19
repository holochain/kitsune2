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

    // Construct kitsune2 URL with just scheme://host:port/endpoint_id
    // This strips any path from the relay URL (like /relay/)
    let canonical_relay_url = format!(
        "{}://{}:{}/{}",
        relay_url.scheme(),
        relay_host,
        relay_port,
        endpoint_id
    );

    Url::from_str(canonical_relay_url)
}

pub(super) fn endpoint_from_url(
    url: &Url,
    configured_relay_url: Option<&str>,
    inserted_relays: &HashMap<String, String>,
) -> K2Result<EndpointAddr> {
    let peer_id = url
        .peer_id()
        .ok_or_else(|| K2Error::other("url must have peer id"))?;
    let endpoint_id = EndpointId::from_str(peer_id).map_err(|err| {
        K2Error::other_src("failed to convert peer id to endpoint id", err)
    })?;

    let peer_addr = url.addr();
    let peer_host = peer_addr.split(':').next().unwrap_or("");

    // First check if this peer's host matches a dynamically inserted relay.
    // These have the full URL with path (e.g. "https://host/relay/").
    // Then fall back to the configured (global) relay URL if the host matches.
    // Otherwise reconstruct from the peer URL.
    let relay_url = if let Some(inserted_url) = inserted_relays.get(peer_host) {
        tracing::info!(
            %peer_host,
            %inserted_url,
            "endpoint_from_url: using inserted relay URL"
        );
        RelayUrl::from_str(inserted_url).map_err(|err| {
            K2Error::other_src("invalid inserted relay url", err)
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
            // Peer is on an unknown relay — reconstruct from the peer URL
            let relay_scheme = if url.uses_tls() { "https" } else { "http" };
            let relay_url = format!("{relay_scheme}://{peer_addr}");
            let relay_url = ::url::Url::from_str(&relay_url)
                .map_err(|err| K2Error::other_src("invalid relay url", err))?;
            RelayUrl::from(relay_url)
        }
    } else {
        let relay_scheme = if url.uses_tls() { "https" } else { "http" };
        let relay_url = format!("{relay_scheme}://{peer_addr}");
        let relay_url = ::url::Url::from_str(&relay_url)
            .map_err(|err| K2Error::other_src("invalid relay url", err))?;
        RelayUrl::from(relay_url)
    };

    Ok(EndpointAddr::from_parts(
        endpoint_id,
        [TransportAddr::Relay(relay_url)],
    ))
}
