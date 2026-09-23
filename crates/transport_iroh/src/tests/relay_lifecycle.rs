//! A space may name the transport's own relay in order to present auth material
//! of its own. Releasing that space must not take the relay with it:
//! `remove_relay` drops the endpoint's active relay entry, which would cut the
//! node off from the relay everything else is still using.
//!
//! Asserted on the endpoint's relay map rather than on connectivity, because on
//! loopback a lost relay is masked by iroh connecting directly.

use super::*;
use crate::test_utils::MockTxHandler;
use base64::Engine as _;
use kitsune2_api::{Config, DynTxHandler, SpaceId, TxImp, TxImpHnd};
use kitsune2_bootstrap_srv::{AuthConfig, BootstrapSrv};
use kitsune2_test_utils::auth::AuthHookServer;
use kitsune2_test_utils::space::TEST_SPACE_ID;
use std::time::Duration;

/// Start a local bootstrap server with relay authentication enabled.
async fn start_authenticated_relay() -> (AuthHookServer, BootstrapSrv, String) {
    let auth_hook = AuthHookServer::spawn(None);
    let hook_url = auth_hook.url();

    // BootstrapSrv::new() creates its own tokio runtime and cannot be called
    // from within an existing runtime.
    let srv = tokio::task::spawn_blocking(move || {
        BootstrapSrv::new(kitsune2_bootstrap_srv::Config {
            prune_interval: Duration::from_millis(5),
            auth: AuthConfig {
                authentication_hook_server: Some(hook_url),
                ..Default::default()
            },
            ..kitsune2_bootstrap_srv::Config::testing()
        })
    })
    .await
    .expect("spawn_blocking panicked")
    .expect("failed to start local bootstrap server");

    let relay_url = format!("http://{}/relay", srv.listen_addrs()[0]);
    (auth_hook, srv, relay_url)
}

fn space_config(relay_url: &str, auth_material: &[u8]) -> Config {
    let config = Config::default();
    config
        .set_module_config(&IrohTransportModConfig {
            iroh_transport: IrohTransportConfig {
                relay_url: Some(relay_url.to_string()),
                relay_allow_plain_text: true,
                auth_material_relay_base64: Some(
                    base64::engine::general_purpose::STANDARD
                        .encode(auth_material),
                ),
                ..Default::default()
            },
        })
        .unwrap();
    config
}

#[tokio::test(flavor = "multi_thread")]
async fn releasing_a_space_keeps_the_transport_own_relay() {
    kitsune2_test_utils::enable_tracing();
    let _ = rustls::crypto::ring::default_provider().install_default();

    let (_auth_hook, _srv, relay_url) = start_authenticated_relay().await;
    let auth_material = b"local-test-auth-material".to_vec();

    let handler: DynTxHandler = Arc::new(MockTxHandler::default());
    let handler = TxImpHnd::new(handler);
    let tx = IrohTransport::create(
        IrohTransportConfig {
            relay_url: Some(relay_url.clone()),
            relay_allow_plain_text: true,
            ..Default::default()
        },
        handler,
        Some(auth_material.clone()),
    )
    .await
    .unwrap();

    // The space names the relay the transport is already on, with auth material
    // of its own, so it is inserted under this space.
    tx.configure_for_space(
        TEST_SPACE_ID,
        &space_config(&relay_url, &auth_material),
    )
    .await
    .unwrap();

    let inserted = tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            if let Some((relay, _)) = tx
                .space_relays
                .read()
                .expect("poisoned")
                .get(&TEST_SPACE_ID)
                .cloned()
            {
                return relay;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("relay was not inserted for the space within 30 s");

    tx.unconfigure_for_space(TEST_SPACE_ID).await.unwrap();

    // remove_relay returns the entry it removed, so a present relay yields Some.
    assert!(
        tx.endpoint.remove_relay(&inserted).await.is_some(),
        "releasing the space took the transport's own relay with it"
    );
}

/// A relay the space named itself is still removed when the space goes.
#[tokio::test(flavor = "multi_thread")]
async fn releasing_a_space_removes_a_relay_it_named() {
    kitsune2_test_utils::enable_tracing();
    let _ = rustls::crypto::ring::default_provider().install_default();

    let (_auth_hook, _srv, relay_url) = start_authenticated_relay().await;
    let auth_material = b"local-test-auth-material".to_vec();

    let handler: DynTxHandler = Arc::new(MockTxHandler::default());
    let handler = TxImpHnd::new(handler);
    // No relay of our own, so the relay the space names is the space's.
    let tx = IrohTransport::create(
        IrohTransportConfig {
            relay_allow_plain_text: true,
            ..Default::default()
        },
        handler,
        None,
    )
    .await
    .unwrap();

    let space_id: SpaceId = TEST_SPACE_ID;
    tx.configure_for_space(
        space_id.clone(),
        &space_config(&relay_url, &auth_material),
    )
    .await
    .unwrap();

    let inserted = tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            if let Some((relay, _)) = tx
                .space_relays
                .read()
                .expect("poisoned")
                .get(&space_id)
                .cloned()
            {
                return relay;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("relay was not inserted for the space within 30 s");

    tx.unconfigure_for_space(space_id).await.unwrap();

    assert!(
        tx.endpoint.remove_relay(&inserted).await.is_none(),
        "a relay the space named should go when the space does"
    );
}
