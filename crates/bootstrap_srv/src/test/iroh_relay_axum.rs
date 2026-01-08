use super::*;
use iroh::{EndpointAddr, RelayConfig, RelayMap, RelayUrl};
use kitsune2_test_utils::enable_tracing;
use std::{net::SocketAddr, str::FromStr, sync::Arc};

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
