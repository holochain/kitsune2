use kitsune2_test_utils::agent::*;
use std::sync::{Arc, Mutex};

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
        verifier: Arc::new(TestVerifier),
        ..crate::default_builder()
    }
    .with_default_config()
    .unwrap()
    .build(k)
    .await
    .unwrap();
    let s1 = k1.space(TEST_SPACE.clone()).await.unwrap();
    let u1 = u_r.recv().await.unwrap();

    let k: DynKitsuneHandler = Arc::new(K(recv.clone(), u_s.clone()));
    let k2 = builder::Builder {
        verifier: Arc::new(TestVerifier),
        ..crate::default_builder()
    }
    .with_default_config()
    .unwrap()
    .build(k)
    .await
    .unwrap();
    let s2 = k2.space(TEST_SPACE.clone()).await.unwrap();
    let u2 = u_r.recv().await.unwrap();

    println!("url: {u1}, {u2}");

    let bob = Arc::new(TestLocalAgent::default()) as agent::DynLocalAgent;
    let bob_info = AgentBuilder {
        url: Some(Some(u2)),
        ..Default::default()
    }
    .build(bob.clone());
    let ned = Arc::new(TestLocalAgent::default()) as agent::DynLocalAgent;
    let ned_info = AgentBuilder {
        url: Some(Some(u1)),
        ..Default::default()
    }
    .build(ned.clone());

    s1.peer_store().insert(vec![bob_info]).await.unwrap();
    s2.peer_store().insert(vec![ned_info]).await.unwrap();

    s1.send_notify(
        bob.agent().clone(),
        ned.agent().clone(),
        bytes::Bytes::from_static(b"hello"),
    )
    .await
    .unwrap();

    let (t, f, s, d) = recv.lock().unwrap().remove(0);
    assert_eq!(bob.agent(), &t);
    assert_eq!(ned.agent(), &f);
    assert_eq!(TEST_SPACE, s);
    assert_eq!("hello", String::from_utf8_lossy(&d));

    s2.send_notify(
        ned.agent().clone(),
        bob.agent().clone(),
        bytes::Bytes::from_static(b"world"),
    )
    .await
    .unwrap();

    let (t, f, s, d) = recv.lock().unwrap().remove(0);
    assert_eq!(ned.agent(), &t);
    assert_eq!(bob.agent(), &f);
    assert_eq!(TEST_SPACE, s);
    assert_eq!("world", String::from_utf8_lossy(&d));
}
