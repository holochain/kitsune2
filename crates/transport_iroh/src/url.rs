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

pub(super) fn canonicalize_relay_url(
    relay_url: &RelayUrl,
    endpoint_id: EndpointId,
) -> K2Result<Url> {
    let canonical_relay_url = if relay_url.port().is_none() {
        let relay_host = relay_url
            .host()
            .ok_or_else(|| K2Error::other("relay url must have host"))?;
        let relay_host = match relay_host {
            ::url::Host::Ipv6(addr) => format!("[{addr}]"),
            other => other.to_string(),
        };
        let relay_port =
            relay_url.port_or_known_default().ok_or_else(|| {
                K2Error::other("relay url must have known default port")
            })?;
        format!(
            "{}://{}:{}/{}",
            relay_url.scheme(),
            relay_host,
            relay_port,
            endpoint_id
        )
    } else {
        format!("{relay_url}{endpoint_id}")
    };
    Url::from_str(canonical_relay_url)
}

pub(super) fn endpoint_from_url(url: &Url) -> K2Result<EndpointAddr> {
    let peer_id = url
        .peer_id()
        .ok_or_else(|| K2Error::other("url must have peer id"))?;
    let endpoint_id = EndpointId::from_str(peer_id).map_err(|err| {
        K2Error::other_src("failed to convert peer id to endpoint id", err)
    })?;
    let relay_addr = url.addr();
    let relay_scheme = if url.uses_tls() { "https" } else { "http" };
    let relay_url = format!("{relay_scheme}://{relay_addr}");
    let relay_url = ::url::Url::from_str(&relay_url)
        .map_err(|err| K2Error::other_src("invalid relay url", err))?;
    let relay_url = RelayUrl::from(relay_url);
    Ok(EndpointAddr::from_parts(
        endpoint_id,
        [TransportAddr::Relay(relay_url)],
    ))
}
