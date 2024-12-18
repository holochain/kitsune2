use kitsune2_api::*;
use std::sync::{Arc, Mutex};

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

const S1: SpaceId = SpaceId(id::Id(bytes::Bytes::from_static(b"space1")));

fn make_agent_info(id: AgentId, url: Url) -> Arc<agent::AgentInfoSigned> {
    let created_at = Timestamp::now();
    let expires_at = created_at + std::time::Duration::from_secs(60 * 20);
    futures::executor::block_on(agent::AgentInfoSigned::sign(
        &TestCrypto,
        agent::AgentInfo {
            agent: id,
            space: S1.clone(),
            created_at,
            expires_at,
            is_tombstone: false,
            url: Some(url),
            storage_arc: DhtArc::FULL,
        },
    ))
    .unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn space_notify_send_recv() {
    use kitsune2_api::{kitsune::*, space::*, *};

    type Item = (AgentId, AgentId, SpaceId, bytes::Bytes);
    type Recv = Arc<Mutex<Vec<Item>>>;
    let recv = Arc::new(Mutex::new(Vec::new()));

    #[derive(Debug)]
    struct S(Recv);

    impl SpaceHandler for S {
        fn recv_notify(
            &self,
            to_agent: AgentId,
            from_agent: AgentId,
            space: SpaceId,
            data: bytes::Bytes,
        ) -> K2Result<()> {
            self.0
                .lock()
                .unwrap()
                .push((to_agent, from_agent, space, data));
            Ok(())
        }
    }

    let (u_s, mut u_r) = tokio::sync::mpsc::unbounded_channel();

    #[derive(Debug)]
    struct K(Recv, tokio::sync::mpsc::UnboundedSender<Url>);

    impl KitsuneHandler for K {
        fn new_listening_address(&self, this_url: Url) {
            let _ = self.1.send(this_url);
        }

        fn create_space(
            &self,
            _space: SpaceId,
        ) -> BoxFut<'_, K2Result<space::DynSpaceHandler>> {
            Box::pin(async move {
                let s: DynSpaceHandler = Arc::new(S(self.0.clone()));
                Ok(s)
            })
        }
    }

    let k: DynKitsuneHandler = Arc::new(K(recv.clone(), u_s.clone()));
    let k1 = builder::Builder {
        verifier: Arc::new(TestCrypto),
        ..crate::default_builder()
    }
    .build(k)
    .await
    .unwrap();
    let s1 = k1.space(S1.clone()).await.unwrap();
    let u1 = u_r.recv().await.unwrap();

    let k: DynKitsuneHandler = Arc::new(K(recv.clone(), u_s.clone()));
    let k2 = builder::Builder {
        verifier: Arc::new(TestCrypto),
        ..crate::default_builder()
    }
    .build(k)
    .await
    .unwrap();
    let s2 = k2.space(S1.clone()).await.unwrap();
    let u2 = u_r.recv().await.unwrap();

    println!("url: {u1}, {u2}");

    let bob = AgentId::from(bytes::Bytes::from_static(b"bob"));
    let ned = AgentId::from(bytes::Bytes::from_static(b"ned"));

    s1.peer_store()
        .insert(vec![make_agent_info(bob.clone(), u2)])
        .await
        .unwrap();
    s2.peer_store()
        .insert(vec![make_agent_info(ned.clone(), u1)])
        .await
        .unwrap();

    s1.send_notify(
        bob.clone(),
        ned.clone(),
        bytes::Bytes::from_static(b"hello"),
    )
    .await
    .unwrap();

    let (t, f, s, d) = recv.lock().unwrap().remove(0);
    assert_eq!(&bob, &t);
    assert_eq!(&ned, &f);
    assert_eq!(S1, s);
    assert_eq!("hello", String::from_utf8_lossy(&d));

    s2.send_notify(
        ned.clone(),
        bob.clone(),
        bytes::Bytes::from_static(b"world"),
    )
    .await
    .unwrap();

    let (t, f, s, d) = recv.lock().unwrap().remove(0);
    assert_eq!(&ned, &t);
    assert_eq!(&bob, &f);
    assert_eq!(S1, s);
    assert_eq!("world", String::from_utf8_lossy(&d));
}
