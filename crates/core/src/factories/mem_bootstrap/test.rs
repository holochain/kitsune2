use kitsune2_api::{id::*, *};
use std::sync::Arc;

#[derive(Debug)]
struct TestCrypto;

const SIG: bytes::Bytes = bytes::Bytes::from_static(b"TEST-SIGNATURE");

impl agent::Signer for TestCrypto {
    fn sign(
        &self,
        _agent_info: &agent::AgentInfo,
        _encoded: &[u8],
    ) -> BoxFut<'_, K2Result<bytes::Bytes>> {
        Box::pin(async move { Ok(SIG.clone()) })
    }
}

impl agent::Verifier for TestCrypto {
    fn verify(
        &self,
        _agent_info: &agent::AgentInfo,
        _message: &[u8],
        signature: &[u8],
    ) -> bool {
        signature == &SIG[..]
    }
}

const S1: SpaceId = SpaceId(Id(bytes::Bytes::from_static(b"space-1")));

struct Test {
    peer_store: peer_store::DynPeerStore,
    boot: bootstrap::DynBootstrap,
}

impl Test {
    pub async fn new() -> Self {
        let builder = Arc::new(
            builder::Builder {
                verifier: Arc::new(TestCrypto),
                ..crate::default_builder()
            }
            .with_default_config()
            .unwrap(),
        );
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

        static NXT: AtomicU64 = AtomicU64::new(1);
        let nxt = NXT.fetch_add(1, Ordering::Relaxed);
        let agent =
            AgentId::from(bytes::Bytes::copy_from_slice(&nxt.to_le_bytes()));

        let url = None;
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

#[tokio::test(flavor = "multi_thread")]
async fn mem_bootstrap_sanity() {
    let t1 = Test::new().await;
    let t2 = Test::new().await;

    let a1 = t1.push_agent().await;
    let a2 = t2.push_agent().await;

    for _ in 0..5 {
        super::MemBootstrapFactory::trigger_immediate_poll();

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
