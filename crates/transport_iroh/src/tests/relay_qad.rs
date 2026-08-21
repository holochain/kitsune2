//! Re-inserting a relay must not strip QUIC address discovery (QAD) from it.
//!
//! A relay configured at endpoint creation gets QAD enabled by
//! `RelayMap::from_iter`. `configure_for_space` later inserts the same relay
//! again through `do_insert_relay`, and iroh's `insert_relay` replaces the
//! existing map entry rather than merging into it. If the replacement carries
//! no QAD config, net_report can no longer learn the endpoint's public
//! address, every NAT-traversal round advertises LAN candidates only, and
//! peers behind different NATs never go direct. That symptom cannot show on
//! loopback (the QAD-discovered address is the local address), so this test
//! asserts the relay-map state that the symptom follows from.

use super::*;

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
