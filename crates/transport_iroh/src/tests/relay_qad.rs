//! `relay_config_with_token` builds the relay config used when authentication
//! is enabled on the default relay or a relay is customized per space. It must
//! keep QUIC address discovery (QAD) enabled: without it the endpoint cannot
//! learn its public address, so NAT traversal only advertises local candidates
//! and peers behind different NATs never go direct. That cannot be reproduced
//! on loopback, so these tests assert the relay config instead.

use super::*;

#[test]
fn relay_config_keeps_qad_enabled_with_and_without_token() {
    let url = RelayUrl::from_str("http://127.0.0.1:1/relay/").unwrap();
    let default_quic = RelayConfig::from(url.clone()).quic;
    assert!(default_quic.is_some(), "iroh enables QAD by default");

    let unauthenticated = IrohTransport::relay_config_with_token(&url, None);
    assert_eq!(unauthenticated.quic, default_quic);
    assert_eq!(unauthenticated.auth_token, None);

    let authenticated =
        IrohTransport::relay_config_with_token(&url, Some("test-token"));
    assert_eq!(authenticated.quic, default_quic);
    assert_eq!(authenticated.auth_token.as_deref(), Some("test-token"));
}

#[tokio::test(flavor = "multi_thread")]
async fn reinserting_relay_keeps_qad_enabled() {
    let url = RelayUrl::from_str("http://127.0.0.1:1/relay/").unwrap();
    let raw = Endpoint::builder(Minimal)
        .relay_mode(RelayMode::Custom(RelayMap::from_iter([url.clone()])))
        .bind()
        .await
        .unwrap();
    let endpoint: DynIrohEndpoint = Arc::new(IrohEndpoint::new(raw));
    let startup_quic = endpoint
        .remove_relay(&url)
        .await
        .expect("relay configured at startup")
        .quic
        .clone();
    assert!(startup_quic.is_some(), "startup relay has QAD enabled");
    endpoint
        .insert_relay(url.clone(), RelayConfig::from(url.clone()).into())
        .await;

    IrohTransport::do_insert_relay(endpoint.clone(), url.to_string(), None)
        .await
        .unwrap();

    let reinserted = endpoint
        .remove_relay(&url)
        .await
        .expect("relay still present after re-insert");
    assert_eq!(
        reinserted.quic, startup_quic,
        "re-inserting the relay must not disable QAD"
    );

    endpoint.close().await;
}
