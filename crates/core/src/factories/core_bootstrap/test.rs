use kitsune2_api::{id::*, *};
use std::sync::Arc;

const SIG: &[u8] = b"fake-signature";

#[derive(Debug)]
struct TestCrypto;

impl agent::Signer for TestCrypto {
    fn sign(
        &self,
        _agent_info: &agent::AgentInfo,
        _encoded: &[u8],
    ) -> BoxFut<'_, K2Result<bytes::Bytes>> {
        Box::pin(async move { Ok(bytes::Bytes::from_static(SIG)) })
    }
}

impl agent::Verifier for TestCrypto {
    fn verify(
        &self,
        _agent_info: &agent::AgentInfo,
        _message: &[u8],
        signature: &[u8],
    ) -> bool {
        signature == SIG
    }
}

const S1: SpaceId = SpaceId(Id(bytes::Bytes::from_static(b"space-1")));

struct Test {
    peer_store: peer_store::DynPeerStore,
    boot: bootstrap::DynBootstrap,
}

impl Test {
    pub async fn new() -> Self {
        let mut builder = builder::Builder {
            verifier: Arc::new(TestCrypto),
            ..crate::default_builder()
        };
        builder.set_default_config().unwrap();
        let builder = Arc::new(builder);
        println!("{}", serde_json::to_string(&builder.config).unwrap());

        let peer_store =
            builder.peer_store.create(builder.clone()).await.unwrap();

        let boot = builder
            .bootstrap
            .create(builder.clone(), peer_store.clone(), S1.clone())
            .await
            .unwrap();

        Self { peer_store, boot }
    }

    pub async fn push_agent(&self) -> AgentId {
        use std::sync::atomic::*;

        static N: AtomicU64 = AtomicU64::new(1);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let agent = AgentId::from(bytes::Bytes::copy_from_slice(
            n.to_string().as_bytes(),
        ));

        let url = Some(Url::from_str("ws://test.com:80/test-url").unwrap());
        let storage_arc = DhtArc::Arc(42, u32::MAX / 13);

        let info = agent::AgentInfoSigned::sign(
            &TestCrypto,
            agent::AgentInfo {
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
        self.peer_store.get(agent).await.map(|_| ())
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn bootstrap_sanity() {
    let t1 = Test::new().await;
    let t2 = Test::new().await;

    let a1 = t1.push_agent().await;
    let a2 = t2.push_agent().await;

    for _ in 0..5 {
        println!("checking...");
        if t1.check_agent(a2.clone()).await.is_ok()
            && t2.check_agent(a1.clone()).await.is_ok()
        {
            println!("found!");
            return;
        }
        println!("not found :(");

        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }

    panic!("failed to bootstrap both created agents in time");
}
