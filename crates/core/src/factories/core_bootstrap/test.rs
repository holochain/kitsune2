use kitsune2_api::*;
use kitsune2_test_utils::bootstrap::TestBootstrapSrv;
use std::sync::Arc;

#[derive(Debug)]
struct TestCrypto;

impl Signer for TestCrypto {
    fn sign<'a, 'b: 'a, 'c: 'a>(
        &'a self,
        agent_info: &'b AgentInfo,
        encoded: &'c [u8],
    ) -> BoxFut<'a, K2Result<bytes::Bytes>> {
        use ed25519_dalek::*;

        let s1: AgentId = serde_json::from_str(&format!(
            "\"{}\"",
            agent_info.url.as_ref().unwrap().peer_id().unwrap()
        ))
        .unwrap();

        let mut s2 = [0_u8; 32];
        s2.copy_from_slice(&s1[..]);
        let s3 = SigningKey::from_bytes(&s2);
        let sig = s3.sign(encoded);
        let sig = bytes::Bytes::copy_from_slice(&sig.to_bytes());
        Box::pin(async move { Ok(sig) })
    }
}

impl Verifier for TestCrypto {
    fn verify(
        &self,
        agent_info: &AgentInfo,
        message: &[u8],
        signature: &[u8],
    ) -> bool {
        crate::Ed25519Verifier.verify(agent_info, message, signature)
    }
}

const S1: SpaceId = SpaceId(Id(bytes::Bytes::from_static(b"space-1")));

struct Test {
    peer_store: DynPeerStore,
    boot: DynBootstrap,
}

impl Test {
    pub async fn new(server: &str) -> Self {
        let builder = Builder {
            verifier: Arc::new(TestCrypto),
            bootstrap: super::CoreBootstrapFactory::create(),
            ..crate::default_test_builder()
        }
        .with_default_config()
        .unwrap();
        builder
            .config
            .set_module_config(&super::CoreBootstrapModConfig {
                core_bootstrap: super::CoreBootstrapConfig {
                    server_url: server.into(),
                    backoff_min_ms: 10,
                    backoff_max_ms: 10,
                },
            })
            .unwrap();
        let builder = Arc::new(builder);
        println!("{}", serde_json::to_string(&builder.config).unwrap());

        let peer_store = builder
            .peer_store
            .create(builder.clone(), S1.clone())
            .await
            .unwrap();

        let boot = builder
            .bootstrap
            .create(builder.clone(), peer_store.clone(), S1.clone())
            .await
            .unwrap();

        Self { peer_store, boot }
    }

    pub async fn push_agent(&self) -> AgentId {
        let secret =
            ed25519_dalek::SigningKey::generate(&mut rand::thread_rng());
        let pubkey = secret.verifying_key();

        let agent =
            AgentId::from(bytes::Bytes::copy_from_slice(pubkey.as_bytes()));

        let secret =
            AgentId::from(bytes::Bytes::copy_from_slice(secret.as_bytes()));

        let url =
            Some(Url::from_str(format!("ws://test.com:80/{secret}")).unwrap());
        let storage_arc = DhtArc::Arc(42, u32::MAX / 13);

        let info = AgentInfoSigned::sign(
            &TestCrypto,
            AgentInfo {
                agent: agent.clone(),
                space: S1.clone(),
                created_at: Timestamp::now(),
                expires_at: Timestamp::now()
                    + std::time::Duration::from_secs(60 * 20),
                is_tombstone: false,
                url,
                storage_arc,
            },
        )
        .await
        .unwrap();

        self.boot.put(info);

        agent
    }

    pub async fn check_agent(&self, agent: AgentId) -> K2Result<()> {
        self.peer_store.get(agent.clone()).await.map(|a| {
            a.ok_or_else(|| {
                let err = K2Error::other(format!("{agent} not found"));
                println!("{err}");
                err
            })
            .map(|a| {
                println!("GOT AGENT: {a:?}");
            })
        })?
    }
}

#[test]
fn validate_bad_server_url() {
    let builder = Builder {
        bootstrap: super::CoreBootstrapFactory::create(),
        ..crate::default_test_builder()
    };

    builder
        .config
        .set_module_config(&super::config::CoreBootstrapModConfig {
            core_bootstrap: super::config::CoreBootstrapConfig {
                server_url: "<bad-url>".into(),
                ..Default::default()
            },
        })
        .unwrap();

    assert!(builder.validate_config().is_err());
}

#[tokio::test(flavor = "multi_thread")]
async fn bootstrap_delayed_online() {
    // this custom server will reject all requests with 500 errors
    // until we call set_halt(false) on it.
    let srv = TestBootstrapSrv::new(true).await;

    println!("addr: {}", srv.addr());

    let t1 = Test::new(srv.addr()).await;
    let t2 = Test::new(srv.addr()).await;

    let a1 = t1.push_agent().await;
    let a2 = t2.push_agent().await;

    // we should NOT get the infos yet, the server is erroring

    for _ in 0..5 {
        println!("checking...");
        if t1.check_agent(a2.clone()).await.is_ok()
            && t2.check_agent(a1.clone()).await.is_ok()
        {
            println!("found too soon!");
            panic!("the server is halting!! how did we get the data?!?!?!");
        }
        println!("not found - yay, this is what we want here.");

        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }

    // set_halt(false). now the push retry loop will successfully push agents
    //                  now the poll retry loop will successfully pull agents

    srv.set_halt(false);

    for _ in 0..5 {
        println!("checking...");
        if t1.check_agent(a2.clone()).await.is_ok()
            && t2.check_agent(a1.clone()).await.is_ok()
        {
            println!("found!");
            return;
        }
        println!("not found :(");

        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }

    panic!("failed to bootstrap both created agents in time");
}
