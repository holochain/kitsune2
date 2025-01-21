use kitsune2_api::{kitsune::*, space::*, *};
use kitsune2_test_utils::agent::*;
use std::sync::{Arc, Mutex};

macro_rules! iter_check {
    ($millis:literal, $code:block) => {
        tokio::time::timeout(
            std::time::Duration::from_millis($millis),
            async {
                loop {
                    $code
                    tokio::time::sleep(
                        std::time::Duration::from_millis(1)
                    ).await;
                }
            }
        ).await.unwrap();
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn space_local_agent_join_leave() {
    #[derive(Debug)]
    struct S;

    impl SpaceHandler for S {}

    #[derive(Debug)]
    struct K;

    impl KitsuneHandler for K {
        fn create_space(
            &self,
            _space: SpaceId,
        ) -> BoxFut<'_, K2Result<space::DynSpaceHandler>> {
            Box::pin(async move {
                let s: DynSpaceHandler = Arc::new(S);
                Ok(s)
            })
        }
    }

    let k: DynKitsuneHandler = Arc::new(K);
    let k1 = builder::Builder {
        verifier: Arc::new(TestVerifier),
        ..crate::default_test_builder()
    }
    .with_default_config()
    .unwrap()
    .build(k)
    .await
    .unwrap();

    let bob = Arc::new(TestLocalAgent::default()) as agent::DynLocalAgent;
    let ned = Arc::new(TestLocalAgent::default()) as agent::DynLocalAgent;

    let s1 = k1.space(TEST_SPACE.clone()).await.unwrap();

    s1.local_agent_join(bob.clone()).await.unwrap();
    s1.local_agent_join(ned.clone()).await.unwrap();

    let mut active_peer_count = 0;

    iter_check!(1000, {
        active_peer_count = 0;
        for peer in s1.peer_store().get_all().await.unwrap() {
            if !peer.is_tombstone {
                active_peer_count += 1;
            }
        }
        if active_peer_count == 2 {
            break;
        }
    });

    if active_peer_count != 2 {
        panic!("expected 2 active agents, got {active_peer_count}");
    }

    s1.local_agent_leave(bob.agent().clone()).await;

    iter_check!(1000, {
        active_peer_count = 0;
        for peer in s1.peer_store().get_all().await.unwrap() {
            if !peer.is_tombstone {
                active_peer_count += 1;
            }
        }
        if active_peer_count == 1 {
            break;
        }
    });

    if active_peer_count != 1 {
        panic!("expected 1 active agents, got {active_peer_count}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn space_notify_send_recv() {
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
        fn new_listening_address(
            &self,
            this_url: Url,
        ) -> BoxFut<'static, K2Result<()>> {
            let _ = self.1.send(this_url);
            Box::pin(async move { Ok(()) })
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
        ..crate::default_test_builder()
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
        ..crate::default_test_builder()
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

// this is a bit of an integration test...
// but the space module is a bit of an integration module...
#[tokio::test(flavor = "multi_thread")]
async fn space_local_agent_periodic_re_sign_and_bootstrap() {
    #[derive(Debug)]
    struct B(pub Mutex<Vec<Arc<agent::AgentInfoSigned>>>);

    impl bootstrap::Bootstrap for B {
        fn put(&self, info: Arc<agent::AgentInfoSigned>) {
            self.0.lock().unwrap().push(info);
        }
    }

    #[derive(Debug)]
    struct BF(pub Arc<B>);

    impl bootstrap::BootstrapFactory for BF {
        fn default_config(&self, _config: &mut config::Config) -> K2Result<()> {
            Ok(())
        }

        fn create(
            &self,
            _builder: Arc<builder::Builder>,
            _peer_store: peer_store::DynPeerStore,
            _space: SpaceId,
        ) -> BoxFut<'static, K2Result<bootstrap::DynBootstrap>> {
            let out: bootstrap::DynBootstrap = self.0.clone();
            Box::pin(async move { Ok(out) })
        }
    }

    #[derive(Debug)]
    struct S;

    impl SpaceHandler for S {}

    #[derive(Debug)]
    struct K;

    impl KitsuneHandler for K {
        fn create_space(
            &self,
            _space: SpaceId,
        ) -> BoxFut<'_, K2Result<space::DynSpaceHandler>> {
            Box::pin(async move {
                let s: DynSpaceHandler = Arc::new(S);
                Ok(s)
            })
        }
    }

    let b = Arc::new(B(Mutex::new(Vec::new())));

    let builder = builder::Builder {
        verifier: Arc::new(TestVerifier),
        bootstrap: Arc::new(BF(b.clone())),
        ..crate::default_test_builder()
    }
    .with_default_config()
    .unwrap();

    builder
        .config
        .set_module_config(&super::CoreSpaceModConfig {
            core_space: super::CoreSpaceConfig {
                // check every 5 millis if we need to re-sign
                re_sign_freq_ms: 5,
                // setting this to a big number like 60 minutes makes
                // it so we *always* re-sign agent infos, because the
                // 20min+now expiry times are always within this time range
                re_sign_expire_time_ms: 1000 * 60 * 60,
            },
        })
        .unwrap();

    let k: DynKitsuneHandler = Arc::new(K);
    let k1 = builder.build(k).await.unwrap();

    let bob = Arc::new(TestLocalAgent::default()) as agent::DynLocalAgent;

    let s1 = k1.space(TEST_SPACE.clone()).await.unwrap();

    s1.local_agent_join(bob.clone()).await.unwrap();

    iter_check!(1000, {
        // see if bootstrap has received at least 5 new updated agent infos
        if b.0.lock().unwrap().len() >= 5 {
            break;
        }
    });
}
