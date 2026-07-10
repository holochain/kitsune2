//! Integration test for the authenticated relay flow.
//!
//! Runs locally by default (in-process auth hook + bootstrap server).
//! Set `KITSUNE2_TEST_RELAY_URL` and `KITSUNE2_TEST_AUTH_MATERIAL`
//! (base64url, no-pad) to run against a real deployment instead.

use std::{sync::Arc, time::Duration};

use base64::Engine as _;
use bytes::Bytes;
use kitsune2_api::Builder;
use kitsune2_bootstrap_srv::{AuthConfig, BootstrapSrv, Config};
use kitsune2_test_utils::{
    auth::AuthHookServer, enable_tracing, retry_fn_until_timeout,
    space::TEST_SPACE_ID,
};
use kitsune2_transport_iroh::{
    IrohTransportConfig, IrohTransportFactory, IrohTransportModConfig,
    test_utils::{MockTxHandler, dummy_url},
};

async fn build_auth_transport(
    relay_url: &str,
    auth_bytes: Vec<u8>,
    allow_plain_text: bool,
    keepalive_interval_s: u32,
    handler: Arc<MockTxHandler>,
) -> kitsune2_api::DynTransport {
    let mut builder = Builder {
        transport: IrohTransportFactory::create(),
        ..kitsune2_core::default_test_builder()
    }
    .with_default_config()
    .unwrap();

    builder.auth_material_relay = Some(auth_bytes);
    builder
        .config
        .set_module_config(&IrohTransportModConfig {
            iroh_transport: IrohTransportConfig {
                relay_url: Some(relay_url.to_string()),
                relay_allow_plain_text: allow_plain_text,
                relay_keepalive_interval_s: keepalive_interval_s,
                ..Default::default()
            },
        })
        .unwrap();

    let builder = Arc::new(builder);
    builder
        .transport
        .create(builder.clone(), handler)
        .await
        .unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn authenticated_relay_two_transports_can_communicate() {
    enable_tracing();
    // iroh uses rustls; ensure a crypto provider is installed.
    let _ = rustls::crypto::ring::default_provider().install_default();

    let real_relay_url = std::env::var("KITSUNE2_TEST_RELAY_URL").ok();
    let real_auth_material = std::env::var("KITSUNE2_TEST_AUTH_MATERIAL").ok();

    // _servers keeps the local servers alive for the duration of the test.
    let (relay_url, auth_bytes, allow_plain_text, _servers) = match (
        real_relay_url,
        real_auth_material,
    ) {
        (Some(url), Some(auth_b64)) => {
            let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
                .decode(&auth_b64)
                .expect(
                    "KITSUNE2_TEST_AUTH_MATERIAL must be base64url (no-pad) encoded",
                );
            tracing::info!(%url, "using real relay deployment");
            (url, bytes, false, None)
        }
        _ => {
            let auth_hook = AuthHookServer::spawn(None);
            let hook_url = auth_hook.url();
            tracing::info!(%hook_url, "started local auth hook");

            // BootstrapSrv::new() creates its own tokio runtime and cannot
            // be called from within an existing runtime.
            let srv = tokio::task::spawn_blocking(move || {
                BootstrapSrv::new(Config {
                    prune_interval: Duration::from_millis(5),
                    auth: AuthConfig {
                        authentication_hook_server: Some(hook_url),
                        ..Default::default()
                    },
                    ..Config::testing()
                })
            })
            .await
            .expect("spawn_blocking panicked")
            .expect("failed to start local bootstrap server");

            let relay_url = format!("http://{}/relay", srv.listen_addrs()[0]);
            tracing::info!(%relay_url, "started local bootstrap+relay");

            let auth_bytes = b"local-test-auth-material".to_vec();
            (relay_url, auth_bytes, true, Some((auth_hook, srv)))
        }
    };

    let dummy = dummy_url();

    let handler_1 = Arc::new(MockTxHandler::default());
    let ep_1 = build_auth_transport(
        &relay_url,
        auth_bytes.clone(),
        allow_plain_text,
        120,
        handler_1.clone(),
    )
    .await;
    ep_1.register_space_handler(TEST_SPACE_ID, handler_1.clone());

    let (msg_tx, mut msg_rx) = tokio::sync::mpsc::unbounded_channel::<Bytes>();
    let handler_2 = Arc::new(MockTxHandler {
        recv_space_notify: Arc::new(move |_peer, _space_id, data| {
            msg_tx.send(data).ok();
            Ok(())
        }),
        ..Default::default()
    });
    let ep_2 = build_auth_transport(
        &relay_url,
        auth_bytes,
        allow_plain_text,
        120,
        handler_2.clone(),
    )
    .await;
    ep_2.register_space_handler(TEST_SPACE_ID, handler_2.clone());

    retry_fn_until_timeout(
        || async {
            *handler_1.current_url.lock().unwrap() != dummy
                && *handler_2.current_url.lock().unwrap() != dummy
        },
        Some(30_000),
        Some(200),
    )
    .await
    .expect("transports did not obtain listening addresses within 30 s");

    let ep2_url = handler_2.current_url.lock().unwrap().clone();
    tracing::info!(%ep2_url, "ep_2 listening; sending test message");

    let message = Bytes::from_static(b"hello via authenticated relay");
    ep_1.send_space_notify(ep2_url, TEST_SPACE_ID, message.clone())
        .await
        .unwrap();

    tokio::time::timeout(Duration::from_secs(30), async {
        let received = msg_rx.recv().await.unwrap();
        assert_eq!(received, message);
    })
    .await
    .expect("message was not received by ep_2 within 30 s");
}

/// The bootstrap server restarts, wiping its in-memory token state. The
/// transport's registration keepalive must re-authenticate, re-register
/// the endpoint key, and restore relay connectivity without intervention.
///
#[tokio::test(flavor = "multi_thread")]
async fn relay_auth_recovers_after_server_restart() {
    enable_tracing();
    let _ = rustls::crypto::ring::default_provider().install_default();

    let auth_hook = AuthHookServer::spawn(None);
    let hook_url = auth_hook.url();

    let make_config =
        |hook_url: String, listen: Vec<std::net::SocketAddr>| -> Config {
            Config {
                prune_interval: Duration::from_millis(100),
                listen_address_list: listen,
                auth: AuthConfig {
                    authentication_hook_server: Some(hook_url),
                    ..Default::default()
                },
                ..Config::testing()
            }
        };

    // BootstrapSrv::new() creates its own tokio runtime and cannot be
    // called from within an existing runtime.
    let hook_url_1 = hook_url.clone();
    let srv = tokio::task::spawn_blocking(move || {
        BootstrapSrv::new(make_config(
            hook_url_1,
            vec![(std::net::Ipv4Addr::LOCALHOST, 0).into()],
        ))
    })
    .await
    .expect("spawn_blocking panicked")
    .expect("failed to start local bootstrap server");

    let srv_addr = srv.listen_addrs()[0];
    let relay_url = format!("http://{srv_addr}/relay");
    tracing::info!(%relay_url, "started local bootstrap+relay");

    let dummy = dummy_url();
    let auth_bytes = b"local-test-auth-material".to_vec();

    // ep_1 authenticates and connects to the relay.
    let (msg_tx, mut msg_rx) = tokio::sync::mpsc::unbounded_channel::<Bytes>();
    let handler_1 = Arc::new(MockTxHandler {
        recv_space_notify: Arc::new(move |_peer, _space_id, data| {
            msg_tx.send(data).ok();
            Ok(())
        }),
        ..Default::default()
    });
    // A short re-registration interval keeps the recovery window (and the
    // test) fast; production defaults to 120 s.
    let ep_1 = build_auth_transport(
        &relay_url,
        auth_bytes.clone(),
        true,
        1,
        handler_1.clone(),
    )
    .await;
    ep_1.register_space_handler(TEST_SPACE_ID, handler_1.clone());

    retry_fn_until_timeout(
        || async { *handler_1.current_url.lock().unwrap() != dummy },
        Some(30_000),
        Some(200),
    )
    .await
    .expect("ep_1 did not obtain a listening address within 30 s");
    let ep1_url = handler_1.current_url.lock().unwrap().clone();

    // Restart the server on the same address: all in-memory token and
    // allowlist state is gone, so ep_1's relay reconnect with its cached
    // token is denied until the registration heartbeat re-authenticates
    // and re-registers ep_1's public key on the allowlist. (The relay
    // actor inside iroh keeps dialing with the stale token — it cannot be
    // refreshed while the actor lives — so recovery comes from the
    // allowlist admitting the handshake-proven key.)
    tracing::info!("restarting bootstrap server");
    let hook_url_2 = hook_url.clone();
    let srv = tokio::task::spawn_blocking(move || {
        drop(srv);
        BootstrapSrv::new(make_config(hook_url_2, vec![srv_addr]))
    })
    .await
    .expect("spawn_blocking panicked")
    .expect("failed to restart local bootstrap server on the same address");
    assert_eq!(srv.listen_addrs()[0], srv_addr);
    tracing::info!("bootstrap server restarted on the same address");

    // ep_2 joins after the restart with fresh authentication and must be
    // able to reach ep_1 via the relay — which requires ep_1 to have
    // re-registered and restored its relay connection.
    let handler_2 = Arc::new(MockTxHandler::default());
    let ep_2 = build_auth_transport(
        &relay_url,
        auth_bytes,
        true,
        1,
        handler_2.clone(),
    )
    .await;
    ep_2.register_space_handler(TEST_SPACE_ID, handler_2.clone());

    retry_fn_until_timeout(
        || async { *handler_2.current_url.lock().unwrap() != dummy },
        Some(30_000),
        Some(200),
    )
    .await
    .expect("ep_2 did not obtain a listening address within 30 s");

    // Recovery needs one heartbeat tick (1 s here) plus the relay
    // reconnect backoff, so allow a generous window and keep re-sending
    // until the message lands.
    let message = Bytes::from_static(b"hello after server restart");
    let ep_1_recovered = retry_fn_until_timeout(
        || async {
            ep_2.send_space_notify(
                ep1_url.clone(),
                TEST_SPACE_ID,
                message.clone(),
            )
            .await
            .ok();
            !msg_rx.is_empty()
        },
        Some(90_000),
        Some(2_000),
    )
    .await;
    ep_1_recovered
        .expect("ep_1 did not recover relay connectivity within 90 s");

    let received = msg_rx.recv().await.unwrap();
    assert_eq!(received, message);
}
