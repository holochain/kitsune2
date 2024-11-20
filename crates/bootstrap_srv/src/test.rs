use crate::*;

const S1: &str = "2o79pTXHaK1FTPZeBiJo2lCgXW_P0ULjX_5Div_2qxU";

const K1: &str = "m-U7gdxW1A647O-4wkuCWOvtGGVfHEsxNScFKiL8-k8";
const K2: &str = "v9I5GT3xVKPcaa4uyd2pcuJromf5zv1-OaahYOLBAWY";

#[derive(Debug)]
#[allow(dead_code)]
struct DecodeAgent {
    space: String,
    agent: String,
    created_at: i64,
    expires_at: i64,
    is_tombstone: bool,
    encoded: String,
    signature: String,
}

impl<'de> serde::Deserialize<'de> for DecodeAgent {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(serde::Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Out {
            agent_info: String,
            signature: String,
        }

        let out: Out = serde::Deserialize::deserialize(deserializer)?;

        #[derive(serde::Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Inn {
            space: String,
            agent: String,
            created_at: String,
            expires_at: String,
            is_tombstone: bool,
        }

        let inn: Inn = serde_json::from_str(&out.agent_info).unwrap();

        Ok(Self {
            space: inn.space,
            agent: inn.agent,
            created_at: inn.created_at.parse().unwrap(),
            expires_at: inn.expires_at.parse().unwrap(),
            is_tombstone: inn.is_tombstone,
            encoded: out.agent_info,
            signature: out.signature,
        })
    }
}

struct PutInfo {
    info: String,
    agent: String,
}

fn put_info(
    addr: std::net::SocketAddr,
    space: &str,
    agent_seed: &str,
    created_at: i64,
    expires_at: i64,
    is_tombstone: bool,
) -> PutInfo {
    use base64::prelude::*;
    use ed25519_dalek::*;

    let seed: [u8; 32] = BASE64_URL_SAFE_NO_PAD
        .decode(agent_seed)
        .unwrap()
        .try_into()
        .unwrap();
    let sign = SigningKey::from_bytes(&seed);
    let pk =
        BASE64_URL_SAFE_NO_PAD.encode(VerifyingKey::from(&sign).as_bytes());

    let agent_info = serde_json::to_string(&serde_json::json!({
        "space": space,
        "agent": pk,
        "createdAt": created_at.to_string(),
        "expiresAt": expires_at.to_string(),
        "isTombstone": is_tombstone
    }))
    .unwrap();

    let signature = BASE64_URL_SAFE_NO_PAD
        .encode(&sign.sign(agent_info.as_bytes()).to_bytes());

    let info = serde_json::to_string(&serde_json::json!({
        "agentInfo": agent_info,
        "signature": signature
    }))
    .unwrap();

    let addr = format!("http://{:?}/bootstrap/{}/{}", addr, space, pk);
    let res = ureq::put(&addr)
        .send(std::io::Cursor::new(info.as_bytes()))
        .unwrap()
        .into_string()
        .unwrap();
    assert_eq!("{}", res);

    PutInfo { info, agent: pk }
}

#[test]
fn happy_bootstrap_put_get() {
    let s = BootSrv::new(Config::testing()).unwrap();

    let c = now();
    let e = c + std::time::Duration::from_secs(60 * 20).as_micros() as i64;

    let PutInfo { info, .. } = put_info(s.listen_addr(), S1, K1, c, e, false);

    let addr = format!("http://{:?}/bootstrap/{}", s.listen_addr(), S1);
    println!("{addr}");
    let res = ureq::get(&addr).call().unwrap().into_string().unwrap();
    println!("{res}");

    // make sure it is valid json and only contains one entry
    let r: Vec<DecodeAgent> = serde_json::from_str(&res).unwrap();
    assert_eq!(1, r.len());

    // make sure that info byte-wise matches our put
    assert_eq!(format!("[{info}]"), res);
}

#[test]
fn happy_empty_server_health() {
    let s = BootSrv::new(Config::testing()).unwrap();
    let addr = format!("http://{:?}/health", s.listen_addr());
    let res = ureq::get(&addr).call().unwrap().into_string().unwrap();
    assert_eq!("{}", res);
}

#[test]
fn happy_empty_server_bootstrap_get() {
    let s = BootSrv::new(Config::testing()).unwrap();
    let addr = format!("http://{:?}/bootstrap/{}", s.listen_addr(), S1);
    let res = ureq::get(&addr).call().unwrap().into_string().unwrap();
    assert_eq!("[]", res);
}

#[test]
fn tombstone_will_not_put() {
    let s = BootSrv::new(Config::testing()).unwrap();

    let c = now();
    let e = c + std::time::Duration::from_secs(60 * 20).as_micros() as i64;

    let _ = put_info(s.listen_addr(), S1, K1, c, e, true);

    let addr = format!("http://{:?}/bootstrap/{}", s.listen_addr(), S1);
    let res = ureq::get(&addr).call().unwrap().into_string().unwrap();
    assert_eq!("[]", res);
}

#[test]
fn tombstone_deletes_correct_agent() {
    let s = BootSrv::new(Config::testing()).unwrap();

    // -- put agent1 -- //

    let c = now();
    let e = c + std::time::Duration::from_secs(60 * 20).as_micros() as i64;

    let PutInfo {
        info: info1,
        agent: agent1,
    } = put_info(s.listen_addr(), S1, K1, c, e, false);

    // -- put agent2 -- //

    let c = now();
    let e = c + std::time::Duration::from_secs(60 * 20).as_micros() as i64;

    let PutInfo {
        info: info2,
        agent: agent2,
    } = put_info(s.listen_addr(), S1, K2, c, e, false);

    // -- tombstone agent1 -- //

    let c = now();
    let e = c + std::time::Duration::from_secs(60 * 20).as_micros() as i64;

    let PutInfo {
        info: info1_t,
        agent: agent1_t,
    } = put_info(s.listen_addr(), S1, K1, c, e, true);

    // -- validate test -- //

    assert_eq!(agent1, agent1_t);
    assert_ne!(agent1, agent2);
    assert_ne!(info1, info2);
    assert_ne!(info1, info1_t);

    // -- get the result -- //

    let addr = format!("http://{:?}/bootstrap/{}", s.listen_addr(), S1);
    let res = ureq::get(&addr).call().unwrap().into_string().unwrap();
    let mut res: Vec<DecodeAgent> = serde_json::from_str(&res).unwrap();

    assert_eq!(1, res.len());
    let one = res.pop().unwrap();
    assert_eq!(one.agent, agent2);
}
