use super::*;
use iroh::{EndpointAddr, RelayConfig, RelayMap, RelayUrl};
use iroh_base::SecretKey;
use kitsune2_test_utils::enable_tracing;
use rand_chacha::rand_core::SeedableRng;
use std::{net::SocketAddr, str::FromStr, sync::Arc, time::Duration};

const TEST_ALPN: &[u8] = b"alpn";

#[test]
fn use_bootstrap_and_iroh_relay() {
    enable_tracing();
    // We have mixed features between ring and aws_lc so the "lookup by crate features" doesn't
    // return a default.
    // If this is called twice due to parallel tests, ignore result, because it'll fail.
    let _ = rustls::crypto::ring::default_provider().install_default();

    let s = BootstrapSrv::new(Config {
        prune_interval: std::time::Duration::from_millis(5),
        ..Config::testing()
    })
    .unwrap();
    let addr = s.listen_addrs()[0];

    PutInfo {
        addr: s.listen_addrs()[0],
        ..Default::default()
    }
    .call()
    .unwrap();

    let bootstrap_url = format!("http://{addr}/bootstrap/{S1}");
    let res = ureq::get(&bootstrap_url)
        .call()
        .unwrap()
        .into_body()
        .read_to_string()
        .unwrap();
    let res: Vec<DecodeAgent> = serde_json::from_str(&res).unwrap();
    assert_eq!(1, res.len());

    create_two_endpoints_and_assert_message_is_received(false, addr);
}

#[test]
fn use_bootstrap_and_iroh_relay_with_tls() {
    enable_tracing();
    // We have mixed features between ring and aws_lc so the "lookup by crate features" doesn't
    // return a default.
    // If this is called twice due to parallel tests, ignore result, because it'll fail.
    let _ = rustls::crypto::ring::default_provider().install_default();

    let cert =
        rcgen::generate_simple_self_signed(vec!["bootstrap.test".to_string()])
            .unwrap();

    let cert_dir = tempfile::tempdir().unwrap();

    let cert_path = cert_dir.path().join("test_cert.pem");
    File::create_new(&cert_path)
        .unwrap()
        .write_all(cert.cert.pem().as_bytes())
        .unwrap();

    let key_path = cert_dir.path().join("test_key.pem");
    File::create_new(&key_path)
        .unwrap()
        .write_all(cert.key_pair.serialize_pem().as_bytes())
        .unwrap();

    let mut config = Config::testing();
    config.tls_cert = Some(cert_path);
    config.tls_key = Some(key_path);

    let s = BootstrapSrv::new(Config {
        prune_interval: std::time::Duration::from_millis(5),
        ..config
    })
    .unwrap();
    let addr = s.listen_addrs()[0];

    PutInfo {
        addr: s.listen_addrs()[0],
        use_tls: true,
        ..Default::default()
    }
    .call()
    .unwrap();

    let bootstrap_url = format!("https://{addr}/bootstrap/{S1}");
    let res = ureq::get(&bootstrap_url)
        .config()
        .tls_config(
            ureq::tls::TlsConfig::builder()
                .disable_verification(true)
                .build(),
        )
        .build()
        .call()
        .unwrap()
        .into_body()
        .read_to_string()
        .unwrap();
    let res: Vec<DecodeAgent> = serde_json::from_str(&res).unwrap();
    assert_eq!(1, res.len());

    create_two_endpoints_and_assert_message_is_received(true, addr);
}

#[test]
fn use_bootstrap_and_iroh_relay_with_auth() {
    enable_tracing();
    let _ = rustls::crypto::ring::default_provider().install_default();

    // Start a fake authentication hook server in its own thread and runtime.
    // BootstrapSrv creates its own multi-thread tokio runtime internally, so
    // we keep the fake hook server isolated in a separate runtime.
    let (auth_addr_tx, auth_addr_rx) = std::sync::mpsc::channel();
    let _auth_server = std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_io()
            .enable_time()
            .build()
            .unwrap();
        rt.block_on(async {
            let listener =
                tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            let _ = auth_addr_tx.send(addr);

            let app = axum::Router::new().route(
                "/authenticate",
                axum::routing::put(|| async {
                    axum::Json(
                        serde_json::json!({"authToken": "iroh-relay-test-token"}),
                    )
                }),
            );
            let _ = axum::serve(listener, app).await;
        });
    });

    let auth_addr = auth_addr_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("auth hook server did not start in time");
    let hook_url = format!("http://{auth_addr}");

    let s = BootstrapSrv::new(Config {
        prune_interval: std::time::Duration::from_millis(5),
        auth: crate::auth::AuthConfig {
            authentication_hook_server: Some(hook_url),
            ..Default::default()
        },
        ..Config::testing()
    })
    .unwrap();
    let addr = s.listen_addrs()[0];

    create_two_authenticated_endpoints_and_assert_message_is_received(addr);
}

#[test]
fn use_bootstrap_and_iroh_relay_auth_rejects_unregistered() {
    enable_tracing();
    let _ = rustls::crypto::ring::default_provider().install_default();

    let (auth_addr_tx, auth_addr_rx) = std::sync::mpsc::channel();
    let _auth_server = std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_io()
            .enable_time()
            .build()
            .unwrap();
        rt.block_on(async {
            let listener =
                tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            let _ = auth_addr_tx.send(addr);

            let app = axum::Router::new().route(
                "/authenticate",
                axum::routing::put(|| async {
                    axum::Json(
                        serde_json::json!({"authToken": "iroh-relay-test-token"}),
                    )
                }),
            );
            let _ = axum::serve(listener, app).await;
        });
    });

    let auth_addr = auth_addr_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("auth hook server did not start in time");
    let hook_url = format!("http://{auth_addr}");

    let s = BootstrapSrv::new(Config {
        prune_interval: std::time::Duration::from_millis(5),
        auth: crate::auth::AuthConfig {
            authentication_hook_server: Some(hook_url),
            ..Default::default()
        },
        ..Config::testing()
    })
    .unwrap();
    let addr = s.listen_addrs()[0];

    assert_authenticated_relay_rejects_unregistered(addr);
}

fn assert_authenticated_relay_rejects_unregistered(addr: SocketAddr) {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()
        .unwrap();
    rt.block_on(async {
        let relay_url =
            RelayUrl::from_str(&format!("http://{addr:?}")).unwrap();

        let mut rng = rand_chacha::ChaCha8Rng::seed_from_u64(42u64);
        let ep_1_secret = SecretKey::generate(&mut rng);
        let ep_2_secret = SecretKey::generate(&mut rng);
        let ep_3_secret = SecretKey::generate(&mut rng);

        let build_endpoint = |secret_key: SecretKey| {
            iroh::Endpoint::empty_builder(iroh::RelayMode::Custom(
                RelayMap::empty(),
            ))
            .secret_key(secret_key)
            .alpns(vec![TEST_ALPN.to_vec()])
            .insecure_skip_relay_cert_verify(true)
        };

        let ep_1 = build_endpoint(ep_1_secret).bind().await.unwrap();
        let ep_2 = build_endpoint(ep_2_secret).bind().await.unwrap();
        let ep_3 = build_endpoint(ep_3_secret).bind().await.unwrap();

        let authenticate_and_register = |key_bytes: [u8; 32]| async move {
            let token_resp = ureq::put(&format!("http://{addr}/authenticate"))
                .send(b"test-auth-material".as_ref())
                .unwrap()
                .into_body()
                .read_to_string()
                .unwrap();
            let token = serde_json::from_str::<serde_json::Value>(&token_resp)
                .unwrap()["authToken"]
                .as_str()
                .unwrap()
                .to_owned();
            ureq::put(&format!("http://{addr}/relay/register"))
                .header("Authorization", &format!("Bearer {token}"))
                .send(key_bytes.as_ref())
                .unwrap();
        };

        authenticate_and_register(*ep_1.id().as_bytes()).await;
        authenticate_and_register(*ep_2.id().as_bytes()).await;

        // ep_3 attempts registration with the correct key but an invalid bearer
        // token. Assert the server rejects it with 401; ep_3 remains absent
        // from the allowlist.
        let err = ureq::put(&format!("http://{addr}/relay/register"))
            .header("Authorization", "Bearer invalid-token")
            .send(ep_3.id().as_bytes().as_ref())
            .unwrap_err();
        assert!(
            matches!(
                err,
                ureq::Error::StatusCode(401)
            ),
            "expected 401 Unauthorized, got {err:?}"
        );

        let relay_cfg = Arc::new(RelayConfig {
            url: relay_url.clone(),
            quic: None,
        });
        ep_1.insert_relay(relay_url.clone(), relay_cfg.clone()).await;
        ep_2.insert_relay(relay_url.clone(), relay_cfg.clone()).await;
        ep_3.insert_relay(relay_url.clone(), relay_cfg.clone()).await;

        let ep_1_addr =
            EndpointAddr::new(ep_1.id()).with_relay_url(relay_url.clone());
        let ep_2_addr =
            EndpointAddr::new(ep_2.id()).with_relay_url(relay_url.clone());

        // ep_3 has no registered key, so the relay denies its WebSocket
        // connection, leaving it with no path to reach ep_1 or ep_2.
        // iroh retries the denied relay connection with backoff rather than
        // returning an explicit error from connect(), so the expected outcome
        // is that the attempt does not succeed within the timeout window.
        let ep_3_to_ep_1 = tokio::time::timeout(
            Duration::from_secs(5),
            ep_3.connect(ep_1_addr, TEST_ALPN),
        )
        .await;
        assert!(
            !matches!(ep_3_to_ep_1, Ok(Ok(_))),
            "ep_3 (unregistered) must not connect to ep_1 via authenticated relay"
        );

        let ep_3_to_ep_2 = tokio::time::timeout(
            Duration::from_secs(5),
            ep_3.connect(ep_2_addr.clone(), TEST_ALPN),
        )
        .await;
        assert!(
            !matches!(ep_3_to_ep_2, Ok(Ok(_))),
            "ep_3 (unregistered) must not connect to ep_2 via authenticated relay"
        );

        // ep_1 and ep_2 are both registered so they can reach each other.
        let message = b"hello from ep1";
        let accept_task = tokio::spawn(async move {
            let conn = ep_2.accept().await.unwrap().await.unwrap();
            let mut recv = conn.accept_uni().await.unwrap();
            let data = recv.read_to_end(1_000).await.unwrap();
            conn.close(0u8.into(), b"");
            ep_2.close().await;
            data
        });

        let conn = ep_1.connect(ep_2_addr, TEST_ALPN).await.unwrap();
        let mut send_stream = conn.open_uni().await.unwrap();
        send_stream.write_all(message).await.unwrap();
        send_stream.finish().unwrap();

        assert_eq!(accept_task.await.unwrap(), message);

        ep_1.close().await;
        ep_3.close().await;
    });
}

fn create_two_authenticated_endpoints_and_assert_message_is_received(
    addr: SocketAddr,
) {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()
        .unwrap();
    rt.block_on(async {
        let relay_url =
            RelayUrl::from_str(&format!("http://{addr:?}")).unwrap();

        // Create two iroh endpoints without relay initially.
        // We must register each endpoint's public key with the server before
        // connecting to the relay.
        let mut rng = rand_chacha::ChaCha8Rng::seed_from_u64(99u64);
        let ep_1_secret = SecretKey::generate(&mut rng);
        let ep_2_secret = SecretKey::generate(&mut rng);

        let build_endpoint_without_relay = |secret_key: SecretKey| {
            // Start with an empty relay map so the endpoint binds without
            // immediately connecting to the relay. The relay transport is
            // kept so that insert_relay (called after registration) works.
            iroh::Endpoint::empty_builder(iroh::RelayMode::Custom(
                RelayMap::empty(),
            ))
            .secret_key(secret_key)
            .alpns(vec![TEST_ALPN.to_vec()])
            .insecure_skip_relay_cert_verify(true)
        };

        let ep_1 = build_endpoint_without_relay(ep_1_secret)
            .bind()
            .await
            .unwrap();
        let ep_2 = build_endpoint_without_relay(ep_2_secret)
            .bind()
            .await
            .unwrap();

        // Authenticate and register an endpoint's public key directly via HTTP.
        let authenticate_and_register = |key_bytes: [u8; 32]| async move {
            let auth_url = format!("http://{addr}/authenticate");
            let token_resp = ureq::put(&auth_url)
                .send(b"test-auth-material".as_ref())
                .unwrap()
                .into_body()
                .read_to_string()
                .unwrap();
            let token = serde_json::from_str::<serde_json::Value>(&token_resp)
                .unwrap()["authToken"]
                .as_str()
                .unwrap()
                .to_owned();

            let register_url = format!("http://{addr}/relay/register");
            ureq::put(&register_url)
                .header("Authorization", &format!("Bearer {token}"))
                .send(key_bytes.as_ref())
                .unwrap();
        };

        authenticate_and_register(*ep_1.id().as_bytes()).await;
        authenticate_and_register(*ep_2.id().as_bytes()).await;

        // Insert the relay into both endpoints. The relay WebSocket connection
        // is established lazily when traffic is needed.
        let relay_cfg = Arc::new(RelayConfig {
            url: relay_url.clone(),
            quic: None,
        });
        ep_1.insert_relay(relay_url.clone(), relay_cfg.clone())
            .await;
        ep_2.insert_relay(relay_url.clone(), relay_cfg.clone())
            .await;

        let ep_2_addr =
            EndpointAddr::new(ep_2.id()).with_relay_url(relay_url.clone());

        let message = b"hello from ep1";

        let message_received = tokio::spawn(async move {
            let conn = ep_2.accept().await.unwrap().await.unwrap();
            match conn.accept_uni().await {
                Ok(mut recv_stream) => {
                    let data = recv_stream.read_to_end(1_000).await.unwrap();
                    conn.close(0u8.into(), b"");
                    ep_2.close().await;
                    data
                }
                Err(err) => {
                    panic!("failed to accept incoming stream: {err}");
                }
            }
        });

        let conn = ep_1.connect(ep_2_addr, TEST_ALPN).await.unwrap();
        let mut send_stream = conn.open_uni().await.unwrap();
        send_stream.write_all(message).await.unwrap();
        send_stream.finish().unwrap();

        let received = message_received.await.unwrap();
        assert_eq!(received, message);
        ep_1.close().await;
    });
}

fn create_two_endpoints_and_assert_message_is_received(
    use_tls: bool,
    addr: SocketAddr,
) {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()
        .unwrap();
    rt.block_on(async {
        // Relay URL uses https
        let relay_url = RelayUrl::from_str(&format!(
            "http{}://{addr:?}",
            if use_tls { "s" } else { "" }
        ))
        .unwrap();
        let relay_map = RelayMap::empty();
        relay_map.insert(
            relay_url.clone(),
            Arc::new(RelayConfig {
                quic: None,
                url: relay_url.clone(),
            }),
        );

        let ep_1 = iroh::Endpoint::empty_builder(iroh::RelayMode::Custom(
            relay_map.clone(),
        ))
        .alpns(vec![TEST_ALPN.to_vec()])
        .insecure_skip_relay_cert_verify(true)
        .bind()
        .await
        .unwrap();
        let ep_2 =
            iroh::Endpoint::empty_builder(iroh::RelayMode::Custom(relay_map))
                .alpns(vec![TEST_ALPN.to_vec()])
                .insecure_skip_relay_cert_verify(true)
                .bind()
                .await
                .unwrap();
        // Endpoint address is composed of the endpoint ID and the relay URL.
        let ep_2_addr =
            EndpointAddr::new(ep_2.id()).with_relay_url(relay_url.clone());

        let message = b"hello";

        let message_received = tokio::spawn(async move {
            let conn = ep_2.accept().await.unwrap().await.unwrap();
            match conn.accept_uni().await {
                Ok(mut recv_stream) => {
                    let message_received =
                        recv_stream.read_to_end(1_000).await.unwrap();
                    conn.close(0u8.into(), b"");
                    ep_2.close().await;
                    message_received
                }
                Err(err) => {
                    panic!("failed to accept incoming stream: {err}");
                }
            }
        });

        let conn = ep_1.connect(ep_2_addr, TEST_ALPN).await.unwrap();
        let mut send_stream = conn.open_uni().await.unwrap();
        send_stream.write_all(message).await.unwrap();
        send_stream.finish().unwrap();

        let received_message = message_received.await.unwrap();
        assert_eq!(received_message, message);
        ep_1.close().await;
    });
}
