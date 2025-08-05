use kitsune2_api::*;
use std::sync::Arc;

#[derive(Debug)]
struct TestCrypto;

const SIG: bytes::Bytes = bytes::Bytes::from_static(b"TEST-SIGNATURE");

impl Signer for TestCrypto {
    fn sign<'a, 'b: 'a, 'c: 'a>(
        &'a self,
        _agent_info: &'b AgentInfo,
        _encoded: &'c [u8],
    ) -> BoxFut<'a, K2Result<bytes::Bytes>> {
        Box::pin(async move { Ok(SIG.clone()) })
    }
}

impl Verifier for TestCrypto {
    fn verify(
        &self,
        _agent_info: &AgentInfo,
        _message: &[u8],
        signature: &[u8],
    ) -> bool {
        signature == &SIG[..]
    }
}

const S1: SpaceId = SpaceId(Id(bytes::Bytes::from_static(b"space-1")));
const S2: SpaceId = SpaceId(Id(bytes::Bytes::from_static(b"space-2")));

struct Test {
    peer_store: DynPeerStore,
    boot: DynBootstrap,
}

impl Test {
    pub async fn new(space_id: SpaceId) -> Self {
        let builder = Arc::new(
            Builder {
                verifier: Arc::new(TestCrypto),
                ..crate::default_test_builder()
            }
            .with_default_config()
            .unwrap(),
        );
        println!("{}", serde_json::to_string(&builder.config).unwrap());

        let blocks = builder
            .blocks
            .create(builder.clone(), space_id.clone())
            .await
            .unwrap();

        let peer_store = builder
            .peer_store
            .create(builder.clone(), space_id.clone(), blocks)
            .await
            .unwrap();

        let boot = builder
            .bootstrap
            .create(builder.clone(), peer_store.clone(), space_id.clone())
            .await
            .unwrap();

        Self { peer_store, boot }
    }

    pub async fn push_agent(&self, space_id: SpaceId) -> AgentId {
        use std::sync::atomic::*;

        static NXT: AtomicU64 = AtomicU64::new(1);
        let nxt = NXT.fetch_add(1, Ordering::Relaxed);
        let agent =
            AgentId::from(bytes::Bytes::copy_from_slice(&nxt.to_le_bytes()));

        let url = None;
        let storage_arc = DhtArc::Arc(42, u32::MAX / 13);

        let info = AgentInfoSigned::sign(
            &TestCrypto,
            AgentInfo {
                agent: agent.clone(),
                space: space_id,
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
    let t1 = Test::new(S1.clone()).await;
    let t2 = Test::new(S1.clone()).await;

    let a1 = t1.push_agent(S1.clone()).await;
    let a2 = t2.push_agent(S1.clone()).await;

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

#[tokio::test(flavor = "multi_thread")]
async fn mem_bootstrap_no_crossover() {
    let t1 = Test::new(S1.clone()).await;
    let t2 = Test::new(S2.clone()).await;

    let a1 = t1.push_agent(S1.clone()).await;
    let a2 = t2.push_agent(S2.clone()).await;

    for _ in 0..5 {
        super::MemBootstrapFactory::trigger_immediate_poll();

        println!("checking...");
        if t1.check_agent(a1.clone()).await.is_ok()
            && t2.check_agent(a2.clone()).await.is_ok()
        {
            println!("found!");

            assert!(t1.check_agent(a2.clone()).await.is_err());
            assert!(t2.check_agent(a1.clone()).await.is_err());

            return;
        }
        println!("not found :(");

        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }

    panic!("failed to bootstrap both created agents in time");
}
