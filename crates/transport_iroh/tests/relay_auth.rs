//! Integration test for the authenticated relay flow.
//!
//! Runs locally by default (in-process auth hook + bootstrap server).
//! Set `KITSUNE2_TEST_RELAY_URL` and `KITSUNE2_TEST_AUTH_MATERIAL`
//! (base64url, no-pad) to run against a real deployment instead.

use std::{sync::Arc, time::Duration};

use axum::routing;
use base64::Engine as _;
use bytes::Bytes;
use kitsune2_api::Builder;
use kitsune2_bootstrap_srv::{AuthConfig, BootstrapSrv, Config};
use kitsune2_test_utils::{
    enable_tracing, retry_fn_until_timeout, space::TEST_SPACE_ID,
};
use kitsune2_transport_iroh::{
    IrohTransportConfig, IrohTransportFactory, IrohTransportModConfig,
    test_utils::{MockTxHandler, dummy_url},
};

fn start_local_auth_hook() -> (std::net::SocketAddr, std::thread::JoinHandle<()>)
{
    let (tx, rx) = std::sync::mpsc::channel();
    let handle = std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_io()
            .enable_time()
            .build()
            .unwrap();
        rt.block_on(async {
            let listener =
                tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            tx.send(listener.local_addr().unwrap()).unwrap();
            let app = axum::Router::new().route(
                "/authenticate",
                routing::put(|| async {
                    axum::Json(
                        serde_json::json!({"authToken": "relay-auth-test-token"}),
                    )
                }),
            );
            axum::serve(listener, app).await.ok();
        });
    });
    let addr = rx
        .recv_timeout(Duration::from_secs(5))
        .expect("auth hook did not start in time");
    (addr, handle)
}

async fn build_auth_transport(
    relay_url: &str,
    auth_bytes: Vec<u8>,
    allow_plain_text: bool,
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
            let (auth_addr, auth_handle) = start_local_auth_hook();
            let hook_url = format!("http://{auth_addr}");
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
            (relay_url, auth_bytes, true, Some((auth_handle, srv)))
        }
    };

    let dummy = dummy_url();

    let handler_1 = Arc::new(MockTxHandler::default());
    let ep_1 = build_auth_transport(
        &relay_url,
        auth_bytes.clone(),
        allow_plain_text,
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
