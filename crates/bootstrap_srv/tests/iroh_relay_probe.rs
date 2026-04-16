use std::{str::FromStr, time::Instant};

use iroh::{EndpointAddr, RelayMap, RelayMode, RelayUrl};
const ALPN: &[u8] = b"kitsune2-iroh-relay-probe";
const DEFAULT_PUBLIC_RELAY_URL: &str =
    "https://use1-1.relay.n0.iroh-canary.iroh.link./";

#[tokio::test(flavor = "multi_thread")]
#[ignore = "network probe; set KITSUNE2_IROH_RELAY_PROBE_URL to override the relay URL"]
async fn iroh_relay_probe_sends_payload() {
    let relay_url = std::env::var("KITSUNE2_IROH_RELAY_PROBE_URL")
        .unwrap_or_else(|_| DEFAULT_PUBLIC_RELAY_URL.to_string());
    let relay_url = RelayUrl::from_str(&relay_url).unwrap();
    let payload = vec![42u8; 64 * 1024];

    let elapsed = send_payload_through_relay(relay_url.clone(), payload).await;
    println!("relay_url={relay_url}");
    println!("elapsed_ms={}", elapsed.as_millis());
}

async fn send_payload_through_relay(
    relay_url: RelayUrl,
    payload: Vec<u8>,
) -> std::time::Duration {
    let relay_map = RelayMap::from_iter([relay_url.clone()]);
    let start = Instant::now();

    let ep_1 = iroh::Endpoint::empty_builder()
        .relay_mode(RelayMode::Custom(relay_map.clone()))
        .alpns(vec![ALPN.to_vec()])
        .bind()
        .await
        .unwrap();
    let ep_2 = iroh::Endpoint::empty_builder()
        .relay_mode(RelayMode::Custom(relay_map))
        .alpns(vec![ALPN.to_vec()])
        .bind()
        .await
        .unwrap();

    let ep_2_addr = EndpointAddr::new(ep_2.id()).with_relay_url(relay_url);
    let expected = payload.clone();
    let expected_len = expected.len();

    let message_received = tokio::spawn(async move {
        let conn = ep_2.accept().await.unwrap().await.unwrap();
        let mut recv_stream = conn.accept_uni().await.unwrap();
        let message_received =
            recv_stream.read_to_end(expected_len).await.unwrap();
        conn.close(0u8.into(), b"");
        ep_2.close().await;
        message_received
    });

    let conn = ep_1.connect(ep_2_addr, ALPN).await.unwrap();
    let mut send_stream = conn.open_uni().await.unwrap();
    send_stream.write_all(&payload).await.unwrap();
    send_stream.finish().unwrap();

    assert_eq!(message_received.await.unwrap(), expected);
    ep_1.close().await;

    start.elapsed()
}
