use crate::IrohTransport;
use crate::url::endpoint_from_url;
use crate::url::{
    SpaceRelay, canonicalize_relay_url, get_url_with_first_relay,
    is_transport_own_relay, per_space_relay,
};
use iroh::{EndpointAddr, EndpointId, RelayUrl, TransportAddr};
use kitsune2_api::{Id, SpaceId, Url};
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
    let result = endpoint_from_url(&url).unwrap();
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
    let result = endpoint_from_url(&url).unwrap();
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
    let result = endpoint_from_url(&url);
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("url must have peer id"));
}

#[test]
fn canonicalize_relay_url_preserves_path() {
    // Relay URLs with paths like /relay/ are preserved so that
    // endpoint_from_url can reconstruct the full relay URL.
    let relay_url =
        RelayUrl::from_str("https://example.com:443/relay/").unwrap();
    let endpoint_id = test_endpoint_id();
    let result = canonicalize_relay_url(&relay_url, endpoint_id).unwrap();
    let expected =
        Url::from_str(format!("https://example.com:443/relay/{endpoint_id}"))
            .unwrap();
    assert_eq!(result, expected);
}

#[test]
fn endpoint_from_url_extracts_relay_with_path() {
    // When the peer URL includes a relay path, endpoint_from_url
    // reconstructs the full relay URL (with path) directly.
    let endpoint_id = test_endpoint_id();
    let url =
        Url::from_str(format!("https://example.com:443/relay/{endpoint_id}"))
            .unwrap();
    let result = endpoint_from_url(&url).unwrap();
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
fn endpoint_from_url_roundtrip_without_path() {
    // Relays without a path roundtrip correctly
    let relay_url =
        RelayUrl::from_str("https://relay.example.com:443/").unwrap();
    let endpoint_id = test_endpoint_id();
    let peer_url = canonicalize_relay_url(&relay_url, endpoint_id).unwrap();
    let result = endpoint_from_url(&peer_url).unwrap();
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
fn endpoint_from_url_roundtrip_with_path() {
    // Relays with a path roundtrip correctly since the path is preserved
    let relay_url =
        RelayUrl::from_str("http://bootstrap.example.com:4433/relay/").unwrap();
    let endpoint_id = test_endpoint_id();
    let peer_url = canonicalize_relay_url(&relay_url, endpoint_id).unwrap();
    let result = endpoint_from_url(&peer_url).unwrap();
    let actual_relay = result.addrs.iter().next().unwrap();
    assert!(
        matches!(
            actual_relay,
            TransportAddr::Relay(r) if *r == relay_url
        ),
        "roundtrip failed: expected {relay_url:?} but got {actual_relay:?}"
    );
}

fn space(name: &[u8]) -> SpaceId {
    SpaceId(Id(bytes::Bytes::copy_from_slice(name)))
}

#[test]
fn own_url_for_preflight_matches_space_relay() {
    let eid = test_endpoint_id();
    let relay =
        RelayUrl::from_str("https://space-relay.com:443/relay/").unwrap();
    let our_space_url =
        Url::from_str(format!("https://space-relay.com:443/relay/{eid}"))
            .unwrap();
    let peer_url = Url::from_str(
        "https://space-relay.com:443/relay/bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
    )
    .unwrap();
    let global_url = Some(
        Url::from_str(format!("https://global-relay.com:443/{eid}")).unwrap(),
    );
    let mut space_relays = HashMap::new();
    space_relays.insert(space(b"s1"), (relay, Some(our_space_url.clone())));

    let result = IrohTransport::own_url_for_preflight(
        &peer_url,
        &space_relays,
        &global_url,
    );
    assert_eq!(result, Some(our_space_url));
}

#[test]
fn own_url_for_preflight_matches_global_relay() {
    let eid = test_endpoint_id();
    let global_url = Some(
        Url::from_str(format!("https://global-relay.com:443/{eid}")).unwrap(),
    );
    let peer_url = Url::from_str(
        "https://global-relay.com:443/bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
    )
    .unwrap();
    let space_relays = HashMap::new();

    let result = IrohTransport::own_url_for_preflight(
        &peer_url,
        &space_relays,
        &global_url,
    );
    assert_eq!(result, global_url);
}

#[test]
fn own_url_for_preflight_unknown_relay_returns_none() {
    let eid = test_endpoint_id();
    let global_url = Some(
        Url::from_str(format!("https://global-relay.com:443/{eid}")).unwrap(),
    );
    let peer_url = Url::from_str(
        "https://unknown-relay.com:443/bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
    )
    .unwrap();
    let space_relays = HashMap::new();

    let result = IrohTransport::own_url_for_preflight(
        &peer_url,
        &space_relays,
        &global_url,
    );
    assert_eq!(result, None);
}

#[test]
fn own_url_for_preflight_space_relay_takes_precedence() {
    let eid = test_endpoint_id();
    let relay = RelayUrl::from_str("https://shared-relay.com:443/").unwrap();
    let our_space_url =
        Url::from_str(format!("https://shared-relay.com:443/{eid}")).unwrap();
    let global_url = Some(
        Url::from_str(format!("https://shared-relay.com:443/{eid}")).unwrap(),
    );
    let peer_url = Url::from_str(
        "https://shared-relay.com:443/bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
    )
    .unwrap();
    let mut space_relays = HashMap::new();
    space_relays.insert(space(b"s1"), (relay, Some(our_space_url.clone())));

    let result = IrohTransport::own_url_for_preflight(
        &peer_url,
        &space_relays,
        &global_url,
    );
    assert_eq!(result, Some(our_space_url));
}

/// A space overriding nothing is handed the global config, so its `relay_url` is
/// the relay already in use. Acting on it would insert that relay as a custom
/// override. At best, that's pointless, at worst it loses the mapping to auth
/// material and results in a broken configuration.
#[test]
fn per_space_relay_ignores_the_relay_already_in_use() {
    assert_eq!(
        per_space_relay(
            Some("https://relay.example/"),
            None,
            Some("https://relay.example/"),
            None,
        ),
        None
    );
}

#[test]
fn per_space_relay_ignores_trailing_slash_differences() {
    assert_eq!(
        per_space_relay(
            Some("https://relay.example"),
            None,
            Some("https://relay.example/"),
            None,
        ),
        None
    );
    assert_eq!(
        per_space_relay(
            Some("https://relay.example///"),
            None,
            Some("https://relay.example"),
            None,
        ),
        None
    );
}

#[test]
fn per_space_relay_keeps_a_genuine_override() {
    assert_eq!(
        per_space_relay(
            Some("https://space-relay.example"),
            None,
            Some("https://relay.example"),
            None,
        ),
        Some(SpaceRelay {
            url: "https://space-relay.example".to_string(),
            auth_material_base64: None,
        })
    );
}

/// With no relay of our own, whatever the space names is its own relay.
#[test]
fn per_space_relay_keeps_an_override_when_the_transport_has_no_relay() {
    assert_eq!(
        per_space_relay(Some("https://space-relay.example"), None, None, None),
        Some(SpaceRelay {
            url: "https://space-relay.example".to_string(),
            auth_material_base64: None,
        })
    );
}

/// A space that brings auth material of its own is naming its own relay even
/// when the URL matches ours - skipping it would leave that space
/// unauthenticated.
#[test]
fn per_space_relay_keeps_a_matching_relay_that_brings_its_own_auth_material() {
    assert_eq!(
        per_space_relay(
            Some("https://relay.example"),
            Some("c3BhY2U="),
            Some("https://relay.example/"),
            None,
        ),
        Some(SpaceRelay {
            url: "https://relay.example".to_string(),
            auth_material_base64: Some("c3BhY2U=".to_string()),
        })
    );
}

/// The leak this guards against: a config merge hands a space every field its
/// override did not set, so a space naming only a relay is handed the global
/// auth material. Carrying it would PUT our credential to a host the space
/// chose, which can then replay it against the relay it was issued for.
#[test]
fn per_space_relay_never_carries_inherited_auth_material_to_another_relay() {
    assert_eq!(
        per_space_relay(
            Some("https://space-relay.example"),
            Some("Z2xvYmFs"),
            Some("https://relay.example"),
            Some("Z2xvYmFs"),
        ),
        Some(SpaceRelay {
            url: "https://space-relay.example".to_string(),
            auth_material_base64: None,
        })
    );
}

/// Inherited material naming the relay it was issued for is still nothing to do.
#[test]
fn per_space_relay_ignores_inherited_auth_material_for_the_same_relay() {
    assert_eq!(
        per_space_relay(
            Some("https://relay.example"),
            Some("Z2xvYmFs"),
            Some("https://relay.example"),
            Some("Z2xvYmFs"),
        ),
        None
    );
}

/// A space may still authenticate a relay of its own, so long as the material
/// came with it.
#[test]
fn per_space_relay_keeps_its_own_auth_material_for_its_own_relay() {
    assert_eq!(
        per_space_relay(
            Some("https://space-relay.example"),
            Some("c3BhY2U="),
            Some("https://relay.example"),
            Some("Z2xvYmFs"),
        ),
        Some(SpaceRelay {
            url: "https://space-relay.example".to_string(),
            auth_material_base64: Some("c3BhY2U=".to_string()),
        })
    );
}

#[test]
fn per_space_relay_is_none_when_the_space_names_no_relay() {
    assert_eq!(
        per_space_relay(None, None, Some("https://relay.example"), None),
        None
    );
    assert_eq!(per_space_relay(None, None, None, None), None);
    // Auth material without a relay to attach it to is still nothing to do.
    assert_eq!(per_space_relay(None, Some("c3BhY2U="), None, None), None);
}

#[test]
fn is_transport_own_relay_ignores_trailing_slashes() {
    assert!(is_transport_own_relay(
        "https://relay.example///",
        Some("https://relay.example")
    ));
    assert!(!is_transport_own_relay(
        "https://space-relay.example",
        Some("https://relay.example")
    ));
}

/// With no relay of our own, nothing is ours to keep.
#[test]
fn is_transport_own_relay_is_false_without_a_transport_relay() {
    assert!(!is_transport_own_relay("https://relay.example", None));
}
