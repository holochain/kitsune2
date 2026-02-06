use super::*;

#[test]
fn invalid_auth() {
    let s = BootstrapSrv::new(Config::testing()).unwrap();
    let addr = format!("http://{}/bootstrap/{}", s.listen_addrs()[0], S1);
    let res = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .build()
        .new_agent()
        .get(&addr)
        .header("Authorization", "Bearer bob")
        .call()
        .unwrap()
        .into_body()
        .read_to_string()
        .unwrap();
    assert!(format!("{res:?}").contains("Unauthorized"), "Got: {res:?}");
}

#[test]
fn valid_auth() {
    let s = BootstrapSrv::new(Config::testing()).unwrap();
    let addr = format!("http://{}/authenticate", s.listen_addrs()[0]);
    let response = ureq::put(&addr)
        .send(&b"hello"[..])
        .unwrap()
        .into_body()
        .read_to_string()
        .unwrap();

    // Parse JSON response
    let json: serde_json::Value = serde_json::from_str(&response).unwrap();
    let token = json["authToken"].as_str().unwrap();

    let addr = format!("http://{}/bootstrap/{}", s.listen_addrs()[0], S1);
    let res = ureq::get(&addr)
        .header("Authorization", &format!("Bearer {token}"))
        .call()
        .unwrap()
        .into_body()
        .read_to_string()
        .unwrap();

    assert_eq!("[]", res);
}

#[test]
fn use_bootstrap_and_sbd() {
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

    async fn connect(
        addr: std::net::SocketAddr,
    ) -> (sbd_client::SbdClient, sbd_client::MsgRecv) {
        sbd_client::SbdClient::connect_config(
            &format!("ws://{addr}"),
            &sbd_client::DefaultCrypto::default(),
            sbd_client::SbdClientConfig {
                allow_plain_text: true,
                ..Default::default()
            },
        )
        .await
        .unwrap()
    }

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()
        .unwrap();
    rt.block_on(async move {
        let (c1, _r1) = connect(addr).await;
        let p1 = c1.pub_key().clone();
        let (c2, mut r2) = connect(addr).await;
        let p2 = c2.pub_key().clone();

        c1.send(&p2, b"hello").await.unwrap();
        let received = r2.recv().await.unwrap().0;
        assert_eq!(p1.as_slice(), &received[0..32]);
        assert_eq!(b"hello".as_slice(), &received[32..]);

        c1.close().await;
        c2.close().await;
    });
}
