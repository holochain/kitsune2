use crate::*;

const S1: &str = "2o79pTXHaK1FTPZeBiJo2lCgXW_P0ULjX_5Div_2qxU";

const K1: &str = "m-U7gdxW1A647O-4wkuCWOvtGGVfHEsxNScFKiL8-k8";
const K2: &str = "v9I5GT3xVKPcaa4uyd2pcuJromf5zv1-OaahYOLBAWY";

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_micros() as i64
}

pub struct GenInfo {
    info: String,
    agent: String,
}

fn gen_info(s: &str, k: &str, c: i64, e: i64, t: bool) -> GenInfo {
    use base64::prelude::*;
    use ed25519_dalek::*;

    let seed: [u8; 32] = BASE64_URL_SAFE_NO_PAD
        .decode(k)
        .unwrap()
        .try_into()
        .unwrap();
    let sign = SigningKey::from_bytes(&seed);
    let pk = BASE64_URL_SAFE_NO_PAD
        .encode(VerifyingKey::from(&sign).as_bytes());

    let agent_info = serde_json::to_string(&serde_json::json!({
        "space": s,
        "agent": pk,
        "createdAt": c.to_string(),
        "expiresAt": e.to_string(),
        "isTombstone": t
    }))
    .unwrap();

    let signature = BASE64_URL_SAFE_NO_PAD
        .encode(&sign.sign(agent_info.as_bytes()).to_bytes());

    let signed = serde_json::to_string(&serde_json::json!({
        "agentInfo": agent_info,
        "signature": signature
    }))
    .unwrap();

    GenInfo {
        info: signed,
        agent: pk,
    }
}

#[test]
fn happy_bootstrap_put_get() {
    let s = BootSrv::new(Config::testing()).unwrap();

    let c = now();
    let e = c + 72_000_000_000;
    let GenInfo { info, agent } = gen_info(S1, K1, c, e, false);

    let addr = format!("http://{:?}/bootstrap/{}/{}", s.listen_addr(), S1, agent);

    println!("{addr}: {info}");

    let res = ureq::put(&addr)
        .send(std::io::Cursor::new(info.into_bytes()))
        .unwrap()
        .into_string()
        .unwrap();
    println!("{res}");
    assert_eq!("{}", res);
}

#[test]
fn happy_empty_server_health() {
    let s = BootSrv::new(Config::testing()).unwrap();
    let addr = format!("http://{:?}/health", s.listen_addr());
    println!("{addr}");
    let res = ureq::get(&addr).call().unwrap().into_string().unwrap();
    println!("{res}");
    assert_eq!("{}", res);
}

#[test]
fn happy_empty_server_bootstrap_get() {
    let s = BootSrv::new(Config::testing()).unwrap();
    let addr = format!("http://{:?}/bootstrap/{}", s.listen_addr(), S1);
    println!("{addr}");
    let res = ureq::get(&addr).call().unwrap().into_string().unwrap();
    println!("{res}");
    assert_eq!("[]", res);
}
