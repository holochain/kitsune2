use super::*;
use axum_server::service::SendService;
use iroh::{EndpointAddr, RelayConfig, RelayMap, RelayUrl};
use std::{str::FromStr, sync::Arc};
use ureq::connect;

const TEST_ALPN: &[u8] = b"alpn";

#[test]
fn use_bootstrap_and_iroh_relay() {
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

    let bootstrap_url = format!("http://{addr:?}/bootstrap/{S1}");
    let res = ureq::get(&bootstrap_url)
        .call()
        .unwrap()
        .into_body()
        .read_to_string()
        .unwrap();
    let res: Vec<DecodeAgent> = serde_json::from_str(&res).unwrap();
    assert_eq!(1, res.len());

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()
        .unwrap();
    rt.block_on(async {
        println!("addrs {addr:?}");
        let relay_url =
            RelayUrl::from_str(&format!("http://{addr:?}")).unwrap();
        let mut relay_map = RelayMap::empty();
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
        .bind()
        .await
        .unwrap();
        let ep_2 =
            iroh::Endpoint::empty_builder(iroh::RelayMode::Custom(relay_map))
                .alpns(vec![TEST_ALPN.to_vec()])
                .bind()
                .await
                .unwrap();
        // Endpoint address is the endpoint ID and the relay URL.
        let ep_2_addr =
            EndpointAddr::new(ep_2.id()).with_relay_url(relay_url.clone());
        println!("ep 2 addr {ep_2_addr:?}");

        let message = b"hello";

        let message_received = tokio::spawn(async move {
            let conn = ep_2.accept().await.unwrap().await.unwrap();
            println!("2 connected");
            match conn.accept_uni().await {
                Ok(mut recv_stream) => {
                    println!("opened stream 2");
                    let message_received =
                        recv_stream.read_to_end(1_000).await.unwrap();
                    println!("received meessage {message_received:?}");
                    println!("got message, closing ep 2");
                    conn.close(0u8.into(), b"");
                    ep_2.close().await;
                    println!("closed ep 2");
                    return message_received;
                }
                Err(err) => {
                    println!("err {err:?}");
                    panic!("dddd");
                }
            }
        });

        println!("opening connection");
        let conn = ep_1.connect(ep_2_addr, &TEST_ALPN).await.unwrap();
        println!("connected");

        let mut send_stream = conn.open_uni().await.unwrap();
        println!("opened stream");
        send_stream.write_all(b"hello").await.unwrap();
        send_stream.finish().unwrap();
        println!("written to stream");

        let mr = message_received.await.unwrap();
        println!("mr");
        assert_eq!(mr, message);
        ep_1.close().await;
    });
}
