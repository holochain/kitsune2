use crate::url::endpoint_from_url;
use crate::url::{canonicalize_relay_url, get_url_with_first_relay};
use iroh::{EndpointAddr, EndpointId, RelayUrl, TransportAddr};
use kitsune2_api::Url;
use std::collections::HashMap;
use std::str::FromStr;

fn test_endpoint_id() -> EndpointId {
    EndpointId::from_str(
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    )
    .unwrap()
}

// URLs with invalid scheme or host are tested in url module of kitsune2_api.
// Note: iroh 0.96.1 changed behavior around FQDN trailing dots.
// In 0.95, RelayUrl::from_str("https://example.com:444") would normalize to https://example.com.:444/
// (adding trailing dot). In 0.96, it preserves the input without adding a dot when the port is explicit.
#[test]
fn canonicalize_relay_url_https_without_port() {
    let relay_url =
        RelayUrl::from_str("https://use1-1.relay.n0.iroh-canary.iroh.link./")
            .unwrap();
    let endpoint_id = test_endpoint_id();
    let result = canonicalize_relay_url(&relay_url, endpoint_id).unwrap();
    let expected = Url::from_str(format!(
        "https://use1-1.relay.n0.iroh-canary.iroh.link.:443/{endpoint_id}"
    ))
    .unwrap();
    assert_eq!(result, expected);
}

#[test]
fn canonicalize_relay_url_https_with_port() {
    let relay_url = RelayUrl::from_str("https://example.com:444").unwrap();
    let endpoint_id = test_endpoint_id();
    let result = canonicalize_relay_url(&relay_url, endpoint_id).unwrap();
    let expected =
        Url::from_str(format!("https://example.com:444/{endpoint_id}"))
            .unwrap();
    assert_eq!(result, expected);
}

#[test]
fn canonicalize_relay_url_http_without_port() {
    let relay_url = RelayUrl::from_str("http://example.com").unwrap();
    let endpoint_id = test_endpoint_id();
    let result = canonicalize_relay_url(&relay_url, endpoint_id).unwrap();
    let expected =
        Url::from_str(format!("http://example.com:80/{endpoint_id}")).unwrap();
    assert_eq!(result, expected);
}

#[test]
fn canonicalize_relay_url_http_with_port() {
    let relay_url = RelayUrl::from_str("http://example.com:444").unwrap();
    let endpoint_id = test_endpoint_id();
    let result = canonicalize_relay_url(&relay_url, endpoint_id).unwrap();
    let expected =
        Url::from_str(format!("http://example.com:444/{endpoint_id}")).unwrap();
    assert_eq!(result, expected);
}

#[test]
fn canonicalize_relay_url_ipv6_https_without_port() {
    let relay_url = RelayUrl::from_str("https://[2001:db8::1]").unwrap();
    let endpoint_id = test_endpoint_id();
    let result = canonicalize_relay_url(&relay_url, endpoint_id).unwrap();
    let expected =
        Url::from_str(format!("https://[2001:db8::1]:443/{endpoint_id}"))
            .unwrap();
    assert_eq!(result, expected);
}

#[test]
fn canonicalize_relay_url_ipv6_https_with_port() {
    let relay_url = RelayUrl::from_str("https://[2001:db8::1]:8443").unwrap();
    let endpoint_id = test_endpoint_id();
    let result = canonicalize_relay_url(&relay_url, endpoint_id).unwrap();
    let expected =
        Url::from_str(format!("https://[2001:db8::1]:8443/{endpoint_id}"))
            .unwrap();
    assert_eq!(result, expected);
}

#[test]
fn canonicalize_relay_url_ipv6_http_without_port() {
    let relay_url = RelayUrl::from_str("http://[2001:db8::1]").unwrap();
    let endpoint_id = test_endpoint_id();
    let result = canonicalize_relay_url(&relay_url, endpoint_id).unwrap();
    let expected =
        Url::from_str(format!("http://[2001:db8::1]:80/{endpoint_id}"))
            .unwrap();
    assert_eq!(result, expected);
}

#[test]
fn canonicalize_relay_url_ipv6_http_with_port() {
    let relay_url = RelayUrl::from_str("http://[2001:db8::1]:8080").unwrap();
    let endpoint_id = test_endpoint_id();
    let result = canonicalize_relay_url(&relay_url, endpoint_id).unwrap();
    let expected =
        Url::from_str(format!("http://[2001:db8::1]:8080/{endpoint_id}"))
            .unwrap();
    assert_eq!(result, expected);
}

#[test]
fn get_url_with_first_relay_one_relay() {
    let relay_url = RelayUrl::from_str("https://example.com:443/").unwrap();
    let endpoint_id = test_endpoint_id();
    let endpoint_addr = EndpointAddr::from_parts(
        endpoint_id,
        vec![TransportAddr::Relay(relay_url)],
    );
    let result = get_url_with_first_relay(&endpoint_addr).unwrap();
    let expected =
        Url::from_str(format!("https://example.com:443/{endpoint_id}"))
            .unwrap();
    assert_eq!(result, expected);
}

#[test]
fn get_url_with_first_relay_no_relay() {
    let endpoint_id = test_endpoint_id();
    let endpoint_addr = EndpointAddr::from_parts(
        endpoint_id,
        vec![], // No addresses
    );
    let result = get_url_with_first_relay(&endpoint_addr);
    assert!(result.is_none());
}

#[test]
fn get_url_with_first_relay_multiple_relays() {
    let relay_url1 = RelayUrl::from_str("https://example1.com:443/").unwrap();
    let relay_url2 = RelayUrl::from_str("https://example2.com:443/").unwrap();
    let endpoint_id = test_endpoint_id();
    let endpoint_addr = EndpointAddr::from_parts(
        endpoint_id,
        vec![
            TransportAddr::Relay(relay_url1), // First relay
            TransportAddr::Relay(relay_url2), // Another relay, but should pick first
        ],
    );
    let result = get_url_with_first_relay(&endpoint_addr).unwrap();
    let expected =
        Url::from_str(format!("https://example1.com:443/{endpoint_id}"))
            .unwrap();
    assert_eq!(result, expected);
}

#[test]
fn endpoint_from_url_valid_https() {
    let endpoint_id = test_endpoint_id();
    let url = Url::from_str(format!("https://example.com:443/{endpoint_id}"))
        .unwrap();
    let result = endpoint_from_url(&url, None, &Default::default()).unwrap();
    let expected_id = test_endpoint_id();
    let expected_relay =
        RelayUrl::from_str("https://example.com:443/").unwrap();
    assert_eq!(result.id, expected_id);
    assert_eq!(result.addrs.len(), 1);
    let actual_transport_addr = result.addrs.iter().next().unwrap();
    assert!(
        matches!(
            actual_transport_addr,
            TransportAddr::Relay(r) if *r == expected_relay
        ),
        "expected relay url but got {actual_transport_addr:?}"
    );
}

#[test]
fn endpoint_from_url_valid_http() {
    let endpoint_id = test_endpoint_id();
    let url =
        Url::from_str(format!("http://example.com:80/{endpoint_id}")).unwrap();
    let result = endpoint_from_url(&url, None, &Default::default()).unwrap();
    let expected_id = test_endpoint_id();
    let expected_relay = RelayUrl::from_str("http://example.com:80/").unwrap();
    assert_eq!(result.id, expected_id);
    assert_eq!(result.addrs.len(), 1);
    let actual_transport_addr = result.addrs.iter().next().unwrap();
    assert!(
        matches!(
            actual_transport_addr,
            TransportAddr::Relay(r) if *r == expected_relay
        ),
        "expected relay url but got {actual_transport_addr:?}"
    );
}

#[test]
fn endpoint_from_url_no_peer_id() {
    let url = Url::from_str("https://example.com:443").unwrap();
    let result = endpoint_from_url(&url, None, &Default::default());
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("url must have peer id"));
}

#[test]
fn canonicalize_relay_url_strips_path() {
    // Relay URLs with paths like /relay/ get stripped to just host:port/endpoint_id
    // because kitsune2 Url only allows a single path segment.
    let relay_url =
        RelayUrl::from_str("https://example.com:443/relay/").unwrap();
    let endpoint_id = test_endpoint_id();
    let result = canonicalize_relay_url(&relay_url, endpoint_id).unwrap();
    let expected = Url::from_str(format!(
        "https://example.com:443/{endpoint_id}"
    ))
    .unwrap();
    assert_eq!(result, expected);
}

#[test]
fn endpoint_from_url_uses_known_relays() {
    // When a relay host is in the known_relays map, use its full URL (with path)
    let endpoint_id = test_endpoint_id();
    let url = Url::from_str(format!(
        "https://example.com:443/{endpoint_id}"
    ))
    .unwrap();
    let mut known_relays = HashMap::new();
    known_relays.insert(
        "example.com".to_string(),
        "https://example.com:443/relay/".to_string(),
    );
    let result =
        endpoint_from_url(&url, None, &known_relays).unwrap();
    let expected_relay =
        RelayUrl::from_str("https://example.com:443/relay/").unwrap();
    let actual_transport_addr = result.addrs.iter().next().unwrap();
    assert!(
        matches!(
            actual_transport_addr,
            TransportAddr::Relay(r) if *r == expected_relay
        ),
        "expected relay with /relay/ path but got {actual_transport_addr:?}"
    );
}

#[test]
fn endpoint_from_url_known_relay_takes_precedence_over_configured() {
    let endpoint_id = test_endpoint_id();
    let url = Url::from_str(format!(
        "https://example.com:443/{endpoint_id}"
    ))
    .unwrap();
    let mut known_relays = HashMap::new();
    known_relays.insert(
        "example.com".to_string(),
        "https://example.com:443/custom-path/".to_string(),
    );
    let result = endpoint_from_url(
        &url,
        Some("https://example.com:443/relay/"),
        &known_relays,
    )
    .unwrap();
    let expected_relay =
        RelayUrl::from_str("https://example.com:443/custom-path/").unwrap();
    let actual_transport_addr = result.addrs.iter().next().unwrap();
    assert!(
        matches!(
            actual_transport_addr,
            TransportAddr::Relay(r) if *r == expected_relay
        ),
        "known_relays should take precedence over configured, got {actual_transport_addr:?}"
    );
}

#[test]
fn endpoint_from_url_roundtrip_without_path() {
    // Relays without a path roundtrip correctly without needing the map
    let relay_url =
        RelayUrl::from_str("https://relay.example.com:443/").unwrap();
    let endpoint_id = test_endpoint_id();
    let peer_url =
        canonicalize_relay_url(&relay_url, endpoint_id).unwrap();
    let result =
        endpoint_from_url(&peer_url, None, &Default::default()).unwrap();
    let actual_relay = result.addrs.iter().next().unwrap();
    assert!(
        matches!(
            actual_relay,
            TransportAddr::Relay(r) if *r == relay_url
        ),
        "roundtrip failed: expected {relay_url:?} but got {actual_relay:?}"
    );
}

#[test]
fn endpoint_from_url_roundtrip_with_path_via_known_relays() {
    // Relays with a path need the known_relays map to roundtrip correctly
    let relay_url =
        RelayUrl::from_str("http://bootstrap.example.com:4433/relay/")
            .unwrap();
    let endpoint_id = test_endpoint_id();
    let peer_url =
        canonicalize_relay_url(&relay_url, endpoint_id).unwrap();
    let mut known_relays = HashMap::new();
    known_relays.insert(
        "bootstrap.example.com".to_string(),
        "http://bootstrap.example.com:4433/relay/".to_string(),
    );
    let result =
        endpoint_from_url(&peer_url, None, &known_relays).unwrap();
    let actual_relay = result.addrs.iter().next().unwrap();
    assert!(
        matches!(
            actual_relay,
            TransportAddr::Relay(r) if *r == relay_url
        ),
        "roundtrip failed: expected {relay_url:?} but got {actual_relay:?}"
    );
}
