use kitsune2_api::*;
use kitsune2_test_utils::{
    agent::*, enable_tracing, iter_check, space::TEST_SPACE_ID,
};
use std::sync::{Arc, Mutex};

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
            _space_id: SpaceId,
            _config_override: Option<&Config>,
        ) -> BoxFut<'_, K2Result<DynSpaceHandler>> {
            Box::pin(async move {
                let s: DynSpaceHandler = Arc::new(S);
                Ok(s)
            })
        }
    }

    let h: DynKitsuneHandler = Arc::new(K);
    let k1 = Builder {
        verifier: Arc::new(TestVerifier),
        ..crate::default_test_builder()
    }
    .with_default_config()
    .unwrap()
    .build()
    .await
    .unwrap();
    k1.register_handler(h).await.unwrap();

    let bob = Arc::new(TestLocalAgent::default()) as DynLocalAgent;
    let ned = Arc::new(TestLocalAgent::default()) as DynLocalAgent;

    assert!(k1.space_if_exists(TEST_SPACE_ID).await.is_none());
    assert_eq!(0, k1.list_spaces().len());

    let s1 = k1.space(TEST_SPACE_ID, None).await.unwrap();

    assert!(k1.space_if_exists(TEST_SPACE_ID).await.is_some());
    assert!(
        k1.space_if_exists(bytes::Bytes::from_static(b"nope").into())
            .await
            .is_none()
    );
    assert_eq!(1, k1.list_spaces().len());

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
    enable_tracing();

    type Item = (Url, SpaceId, bytes::Bytes);
    type Recv = Arc<Mutex<Vec<Item>>>;
    let recv = Arc::new(Mutex::new(Vec::new()));

    #[derive(Debug)]
    struct S(Recv);

    impl SpaceHandler for S {
        fn recv_notify(
            &self,
            from_peer: Url,
            space_id: SpaceId,
            data: bytes::Bytes,
        ) -> K2Result<()> {
            self.0.lock().unwrap().push((from_peer, space_id, data));
            Ok(())
        }
    }

    let (u_s, mut u_r) = tokio::sync::mpsc::unbounded_channel();

    #[derive(Debug)]
    struct K(Recv, tokio::sync::mpsc::UnboundedSender<Url>);

    impl KitsuneHandler for K {
        fn new_listening_address(&self, this_url: Url) -> BoxFut<'static, ()> {
            let _ = self.1.send(this_url);
            Box::pin(async move {})
        }

        fn create_space(
            &self,
            _space_id: SpaceId,
            _config_override: Option<&Config>,
        ) -> BoxFut<'_, K2Result<DynSpaceHandler>> {
            Box::pin(async move {
                let s: DynSpaceHandler = Arc::new(S(self.0.clone()));
                Ok(s)
            })
        }
    }

    let h: DynKitsuneHandler = Arc::new(K(recv.clone(), u_s.clone()));
    let k1 = Builder {
        verifier: Arc::new(TestVerifier),
        ..crate::default_test_builder()
    }
    .with_default_config()
    .unwrap()
    .build()
    .await
    .unwrap();
    k1.register_handler(h).await.unwrap();
    let s1 = k1.space(TEST_SPACE_ID.clone(), None).await.unwrap();
    let u1 = u_r.recv().await.unwrap();

    let h: DynKitsuneHandler = Arc::new(K(recv.clone(), u_s.clone()));
    let k2 = Builder {
        verifier: Arc::new(TestVerifier),
        ..crate::default_test_builder()
    }
    .with_default_config()
    .unwrap()
    .build()
    .await
    .unwrap();
    k2.register_handler(h).await.unwrap();
    let s2 = k2.space(TEST_SPACE_ID.clone(), None).await.unwrap();
    let u2 = u_r.recv().await.unwrap();

    println!("url: {u1}, {u2}");

    let bob = Arc::new(TestLocalAgent::default()) as DynLocalAgent;
    let bob_info = AgentBuilder {
        url: Some(Some(u2.clone())),
        ..Default::default()
    }
    .build(bob.clone());
    let ned = Arc::new(TestLocalAgent::default()) as DynLocalAgent;
    let ned_info = AgentBuilder {
        url: Some(Some(u1.clone())),
        ..Default::default()
    }
    .build(ned.clone());

    s1.peer_store().insert(vec![bob_info]).await.unwrap();
    s2.peer_store().insert(vec![ned_info]).await.unwrap();

    // Join local agents to spaces before sending messages
    s1.local_agent_join(ned.clone()).await.unwrap();
    s2.local_agent_join(bob.clone()).await.unwrap();

    s1.send_notify(u2.clone(), bytes::Bytes::from_static(b"hello"))
        .await
        .unwrap();

    let (f, s, d) = recv.lock().unwrap().remove(0);
    assert_eq!(u1.clone(), f);
    assert_eq!(TEST_SPACE_ID, s);
    assert_eq!("hello", String::from_utf8_lossy(&d));

    s2.send_notify(u1, bytes::Bytes::from_static(b"world"))
        .await
        .unwrap();

    let (f, s, d) = recv.lock().unwrap().remove(0);
    assert_eq!(u2, f);
    assert_eq!(TEST_SPACE_ID, s);
    assert_eq!("world", String::from_utf8_lossy(&d));
}

// this is a bit of an integration test...
// but the space module is a bit of an integration module...
#[tokio::test(flavor = "multi_thread")]
async fn space_local_agent_periodic_re_sign_and_bootstrap() {
    #[derive(Debug)]
    struct B(pub Mutex<Vec<Arc<AgentInfoSigned>>>);

    impl Bootstrap for B {
        fn put(&self, info: Arc<AgentInfoSigned>) {
            self.0.lock().unwrap().push(info);
        }
    }

    #[derive(Debug)]
    struct BF(pub Arc<B>);

    impl BootstrapFactory for BF {
        fn default_config(&self, _config: &mut Config) -> K2Result<()> {
            Ok(())
        }

        fn validate_config(&self, _config: &Config) -> K2Result<()> {
            Ok(())
        }

        fn create(
            &self,
            _builder: Arc<Builder>,
            _peer_store: DynPeerStore,
            _space_id: SpaceId,
        ) -> BoxFut<'static, K2Result<DynBootstrap>> {
            let out: DynBootstrap = self.0.clone();
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
            _space_id: SpaceId,
            _config_override: Option<&Config>,
        ) -> BoxFut<'_, K2Result<DynSpaceHandler>> {
            Box::pin(async move {
                let s: DynSpaceHandler = Arc::new(S);
                Ok(s)
            })
        }
    }

    let b = Arc::new(B(Mutex::new(Vec::new())));

    let builder = Builder {
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

    let h: DynKitsuneHandler = Arc::new(K);
    let k1 = builder.build().await.unwrap();
    k1.register_handler(h).await.unwrap();

    let bob = Arc::new(TestLocalAgent::default()) as DynLocalAgent;

    let s1 = k1.space(TEST_SPACE_ID.clone(), None).await.unwrap();

    s1.local_agent_join(bob.clone()).await.unwrap();

    iter_check!(1000, {
        // see if bootstrap has received at least 5 new updated agent infos
        if b.0.lock().unwrap().len() >= 5 {
            break;
        }
    });
}

#[tokio::test(flavor = "multi_thread")]
async fn broadcast_new_agent_info_on_resign() {
    #[derive(Debug)]
    struct S;

    impl SpaceHandler for S {}

    #[derive(Debug)]
    struct K;

    impl KitsuneHandler for K {
        fn create_space(
            &self,
            _space_id: SpaceId,
            _config_override: Option<&Config>,
        ) -> BoxFut<'_, K2Result<DynSpaceHandler>> {
            Box::pin(async move {
                let s: DynSpaceHandler = Arc::new(S);
                Ok(s)
            })
        }
    }

    #[derive(Debug)]
    struct PublishStub(Mutex<Vec<(Arc<AgentInfoSigned>, Url)>>);

    impl Publish for PublishStub {
        fn publish_ops(
            &self,
            _ops: Vec<PublishOp>,
            _target: Url,
        ) -> BoxFut<'_, K2Result<()>> {
            unimplemented!()
        }

        fn publish_agent(
            &self,
            agent_info: Arc<AgentInfoSigned>,
            target: Url,
        ) -> BoxFut<'_, K2Result<()>> {
            self.0.lock().unwrap().push((agent_info, target));
            Box::pin(async { Ok(()) })
        }
    }

    #[derive(Debug)]
    struct PF(DynPublish);

    impl PublishFactory for PF {
        fn default_config(&self, _config: &mut Config) -> K2Result<()> {
            Ok(())
        }

        fn validate_config(&self, _config: &Config) -> K2Result<()> {
            Ok(())
        }

        fn create(
            &self,
            _builder: Arc<Builder>,
            _space_id: SpaceId,
            _fetch: DynFetch,
            _peer_store: DynPeerStore,
            _peer_meta_store: DynPeerMetaStore,
            _transport: DynTransport,
        ) -> BoxFut<'static, K2Result<DynPublish>> {
            let out: DynPublish = self.0.clone();
            Box::pin(async move { Ok(out) })
        }
    }

    let p = Arc::new(PublishStub(Mutex::new(Vec::new())));

    let builder = Builder {
        verifier: Arc::new(TestVerifier),
        publish: Arc::new(PF(p.clone())),
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

    let h: DynKitsuneHandler = Arc::new(K);
    let k1 = builder.build().await.unwrap();
    k1.register_handler(h).await.unwrap();

    let s1 = k1.space(TEST_SPACE_ID.clone(), None).await.unwrap();

    // Join alice to the space and then remove her from the local agent store.
    // This is a quick way to create a new agent info into the peer store for alice.
    let alice = Arc::new(TestLocalAgent::default()) as DynLocalAgent;
    s1.local_agent_join(alice.clone()).await.unwrap();
    s1.local_agent_store()
        .remove(alice.agent().clone())
        .await
        .unwrap();

    let bob = Arc::new(TestLocalAgent::default()) as DynLocalAgent;
    s1.local_agent_join(bob.clone()).await.unwrap();

    iter_check!(1000, {
        // see if we have done at least 5 broadcasts
        if p.0.lock().unwrap().len() >= 5 {
            break;
        }
    });

    let broadcast = p.0.lock().unwrap().last().cloned().unwrap();
    assert_eq!(bob.agent(), &broadcast.0.agent);
}

/// Transport, kitsune and space handler stubs shared by the tests that build
/// a kitsune instance only to observe what reaches the transport.
mod tx_fixtures {
    use super::*;

    pub(super) type Configured = Arc<Mutex<Vec<SpaceId>>>;
    pub(super) type CapturedTxHnd = Arc<Mutex<Option<Arc<TxImpHnd>>>>;

    /// Records the per-space hook and does nothing else; these tests only
    /// create spaces, they never move data.
    #[derive(Debug)]
    pub(super) struct RecordingTx {
        configured: Configured,
    }

    impl TxImp for RecordingTx {
        fn url(&self) -> Option<Url> {
            None
        }

        fn disconnect(
            &self,
            _peer: Url,
            _payload: Option<(String, bytes::Bytes)>,
        ) -> BoxFut<'_, ()> {
            Box::pin(async {})
        }

        fn send(
            &self,
            _peer: Url,
            _data: bytes::Bytes,
        ) -> BoxFut<'_, K2Result<()>> {
            Box::pin(async { Ok(()) })
        }

        fn get_connected_peers(&self) -> BoxFut<'_, K2Result<Vec<Url>>> {
            Box::pin(async { Ok(Vec::new()) })
        }

        fn dump_network_stats(&self) -> BoxFut<'_, K2Result<TransportStats>> {
            Box::pin(async {
                Ok(TransportStats {
                    backend: "recording".to_string(),
                    peer_urls: Vec::new(),
                    connections: Vec::new(),
                })
            })
        }

        fn configure_for_space(
            &self,
            space_id: SpaceId,
            _config: &Config,
        ) -> BoxFut<'_, K2Result<()>> {
            self.configured.lock().expect("poison").push(space_id);
            Box::pin(async { Ok(()) })
        }
    }

    /// Builds a [`RecordingTx`] and, when `captured` is set, hands out the
    /// `TxImpHnd` it wraps so a test can reach the registered space handlers.
    #[derive(Debug)]
    pub(super) struct RecordingTxFactory {
        pub(super) configured: Configured,
        pub(super) captured: Option<CapturedTxHnd>,
    }

    impl TransportFactory for RecordingTxFactory {
        fn default_config(&self, _config: &mut Config) -> K2Result<()> {
            Ok(())
        }

        fn validate_config(&self, _config: &Config) -> K2Result<()> {
            Ok(())
        }

        fn create(
            &self,
            _builder: Arc<Builder>,
            handler: DynTxHandler,
        ) -> BoxFut<'static, K2Result<DynTransport>> {
            let configured = self.configured.clone();
            let captured = self.captured.clone();
            Box::pin(async move {
                let handler = TxImpHnd::new(handler);
                if let Some(captured) = captured {
                    *captured.lock().expect("poison") = Some(handler.clone());
                }
                let imp: DynTxImp = Arc::new(RecordingTx { configured });
                Ok(DefaultTransport::create(&handler, imp))
            })
        }
    }

    #[derive(Debug)]
    pub(super) struct S;
    impl SpaceHandler for S {}

    #[derive(Debug)]
    pub(super) struct K;
    impl KitsuneHandler for K {
        fn create_space(
            &self,
            _space_id: SpaceId,
            _config_override: Option<&Config>,
        ) -> BoxFut<'_, K2Result<DynSpaceHandler>> {
            Box::pin(async move {
                let s: DynSpaceHandler = Arc::new(S);
                Ok(s)
            })
        }
    }
}

/// The per-space transport hook exists so a space can name a relay of its own.
/// Calling it for a space that overrides nothing hands the transport the global
/// config and presents the defaults as that space's own - which the transport
/// cannot tell apart, and the iroh transport acts on by reconfiguring the relay
/// it is already connected to.
mod configure_for_space_only_when_overridden {
    use super::tx_fixtures::*;
    use super::*;

    async fn configured_spaces_for(
        config_override: Option<Config>,
    ) -> Vec<SpaceId> {
        let configured: Configured = Arc::new(Mutex::new(Vec::new()));
        let builder = Builder {
            verifier: Arc::new(TestVerifier),
            transport: Arc::new(RecordingTxFactory {
                configured: configured.clone(),
                captured: None,
            }),
            ..crate::default_test_builder()
        }
        .with_default_config()
        .unwrap()
        .build()
        .await
        .unwrap();
        builder
            .register_handler(Arc::new(K) as DynKitsuneHandler)
            .await
            .unwrap();

        builder.space(TEST_SPACE_ID, config_override).await.unwrap();

        configured.lock().expect("poison").clone()
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn not_called_when_the_space_overrides_nothing() {
        assert!(
            configured_spaces_for(None).await.is_empty(),
            "the transport was handed the global config as if it were this space's own"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn called_when_the_space_overrides_something() {
        let configured = configured_spaces_for(Some(Config::default())).await;
        assert_eq!(configured, vec![TEST_SPACE_ID]);
    }
}

/// Global bootstrap auth material authenticates the global bootstrap server. A
/// config merge hands a space every field its override did not set, so a space
/// naming its own server is still handed our credential - presenting it there
/// would let that server replay it against ours.
mod bootstrap_auth_material_is_not_carried_to_another_server {
    use super::*;
    use crate::factories::core_space::bootstrap_server_customized;
    use crate::factories::{CoreBootstrapConfig, CoreBootstrapModConfig};

    const GLOBAL_SERVER: &str = "http://global-bootstrap.example";
    const SPACE_SERVER: &str = "http://space-bootstrap.example";

    type Seen = Arc<Mutex<Vec<Option<Vec<u8>>>>>;

    #[derive(Debug)]
    struct NoopBootstrap;
    impl Bootstrap for NoopBootstrap {
        fn put(&self, _info: Arc<AgentInfoSigned>) {}
    }

    /// Records the auth material the space's builder carries by the time the
    /// bootstrap module is built from it.
    #[derive(Debug)]
    struct RecordingBootstrapFactory {
        seen: Seen,
    }

    impl BootstrapFactory for RecordingBootstrapFactory {
        fn default_config(&self, config: &mut Config) -> K2Result<()> {
            config.set_module_config(&CoreBootstrapModConfig {
                core_bootstrap: CoreBootstrapConfig {
                    server_url: Some(GLOBAL_SERVER.into()),
                    ..Default::default()
                },
            })
        }

        fn validate_config(&self, _config: &Config) -> K2Result<()> {
            Ok(())
        }

        fn create(
            &self,
            builder: Arc<Builder>,
            _peer_store: DynPeerStore,
            _space_id: SpaceId,
        ) -> BoxFut<'static, K2Result<DynBootstrap>> {
            self.seen
                .lock()
                .expect("poison")
                .push(builder.auth_material_bootstrap.clone());
            Box::pin(async { Ok(Arc::new(NoopBootstrap) as DynBootstrap) })
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
            _space_id: SpaceId,
            _config_override: Option<&Config>,
        ) -> BoxFut<'_, K2Result<DynSpaceHandler>> {
            Box::pin(async move {
                let s: DynSpaceHandler = Arc::new(S);
                Ok(s)
            })
        }
    }

    /// Build a node holding global bootstrap auth material, create one space
    /// with the given override, and report the material its bootstrap was
    /// built with.
    async fn material_seen_by_space(
        space_server: Option<&str>,
    ) -> Option<Vec<u8>> {
        let seen: Seen = Arc::new(Mutex::new(Vec::new()));
        let builder = Builder {
            verifier: Arc::new(TestVerifier),
            auth_material_bootstrap: Some(b"global-credential".to_vec()),
            bootstrap: Arc::new(RecordingBootstrapFactory {
                seen: seen.clone(),
            }),
            ..crate::default_test_builder()
        }
        .with_default_config()
        .unwrap()
        .build()
        .await
        .unwrap();
        builder
            .register_handler(Arc::new(K) as DynKitsuneHandler)
            .await
            .unwrap();

        let config_override = Config::default();
        config_override
            .set_module_config(&CoreBootstrapModConfig {
                core_bootstrap: CoreBootstrapConfig {
                    server_url: Some(
                        space_server.unwrap_or(GLOBAL_SERVER).into(),
                    ),
                    ..Default::default()
                },
            })
            .unwrap();

        builder
            .space(TEST_SPACE_ID, Some(config_override))
            .await
            .unwrap();

        let seen = seen.lock().expect("poison").clone();
        assert_eq!(seen.len(), 1, "expected exactly one bootstrap to be built");
        seen.into_iter().next().unwrap()
    }

    fn config_with_server(server_url: Option<&str>) -> Config {
        let config = Config::default();
        config
            .set_module_config(&CoreBootstrapModConfig {
                core_bootstrap: CoreBootstrapConfig {
                    server_url: server_url.map(Into::into),
                    ..Default::default()
                },
            })
            .unwrap();
        config
    }

    #[test]
    fn a_different_server_is_a_customized_one() {
        assert!(bootstrap_server_customized(
            &config_with_server(Some(GLOBAL_SERVER)),
            &config_with_server(Some(SPACE_SERVER)),
        ));
    }

    #[test]
    fn a_trailing_slash_is_not_a_different_server() {
        assert!(!bootstrap_server_customized(
            &config_with_server(Some("http://bootstrap.example")),
            &config_with_server(Some("http://bootstrap.example/")),
        ));
    }

    /// Auth material given with no global server of our own was configured for
    /// whatever server the spaces name, so it stays.
    #[test]
    fn no_global_server_means_the_material_was_meant_for_the_space() {
        assert!(!bootstrap_server_customized(
            &config_with_server(None),
            &config_with_server(Some(SPACE_SERVER)),
        ));
    }

    /// A bootstrap module with a config shape we cannot read handles its own
    /// material; we leave it alone rather than guessing.
    #[test]
    fn an_unreadable_bootstrap_config_leaves_the_material_alone() {
        let unknown = Config::default();
        unknown
            .set_module_config(&serde_json::json!({ "somethingElse": {} }))
            .unwrap();

        assert!(!bootstrap_server_customized(
            &unknown,
            &config_with_server(Some(SPACE_SERVER)),
        ));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn cleared_when_the_space_names_its_own_server() {
        assert_eq!(material_seen_by_space(Some(SPACE_SERVER)).await, None);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn kept_when_the_space_names_the_global_server() {
        assert_eq!(
            material_seen_by_space(None).await,
            Some(b"global-credential".to_vec()),
        );
    }
}

/// `TxHandlerTranslator::is_unresponsive` is the space-side getter the
/// transport uses to decide whether to dial. It must mirror the
/// `unresponsive` mark in the space's peer meta store, counting only marks
/// made at or after the time the caller passes.
mod tx_handler_translator_is_unresponsive {
    use super::tx_fixtures::*;
    use super::*;
    use std::sync::Weak;
    use std::time::Duration;

    /// Builds a kitsune instance with one space and returns that space with
    /// the `TxImpHnd` the space's translator is registered against.
    async fn space_and_tx_hnd() -> (DynSpace, Arc<TxImpHnd>) {
        let captured: CapturedTxHnd = Arc::new(Mutex::new(None));
        let kitsune = Builder {
            verifier: Arc::new(TestVerifier),
            transport: Arc::new(RecordingTxFactory {
                configured: Arc::new(Mutex::new(Vec::new())),
                captured: Some(captured.clone()),
            }),
            ..crate::default_test_builder()
        }
        .with_default_config()
        .unwrap()
        .build()
        .await
        .unwrap();
        kitsune
            .register_handler(Arc::new(K) as DynKitsuneHandler)
            .await
            .unwrap();
        let space = kitsune.space(TEST_SPACE_ID, None).await.unwrap();
        let hnd = captured
            .lock()
            .expect("poison")
            .clone()
            .expect("transport factory must have run");
        (space, hnd)
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn translator_reports_unresponsive_mark_from_peer_meta_store() {
        let (space, hnd) = space_and_tx_hnd().await;

        let marked = Url::from_str("ws://marked.example:80/alice").unwrap();
        let other = Url::from_str("ws://other.example:80/bob").unwrap();
        let before_mark = Timestamp::now();

        assert!(
            !hnd.is_unresponsive(marked.clone(), before_mark)
                .await
                .unwrap()
        );

        let agent = Arc::new(TestLocalAgent::default()) as DynLocalAgent;
        let agent_info = AgentBuilder {
            url: Some(Some(marked.clone())),
            ..Default::default()
        }
        .build(agent);
        space.peer_store().insert(vec![agent_info]).await.unwrap();

        let when = Timestamp::now();
        hnd.set_unresponsive(marked.clone(), when).await.unwrap();
        hnd.set_unresponsive(other.clone(), when).await.unwrap();

        assert!(
            hnd.is_unresponsive(marked.clone(), before_mark)
                .await
                .unwrap()
        );
        assert!(hnd.is_unresponsive(marked.clone(), when).await.unwrap());
        assert!(
            !hnd.is_unresponsive(marked, when + Duration::from_secs(1))
                .await
                .unwrap(),
            "a mark older than `since` must not count"
        );
        assert!(!hnd.is_unresponsive(other, before_mark).await.unwrap());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn translator_errors_when_core_space_is_dropped() {
        let translator =
            super::super::TxHandlerTranslator(Arc::new(S), Weak::new());
        let peer = Url::from_str("ws://peer.example:80/alice").unwrap();

        let err = translator
            .is_unresponsive(peer, Timestamp::now())
            .await
            .expect_err("a dropped space must report an error");
        assert!(
            err.to_string().contains("CoreSpace has been dropped"),
            "unexpected error: {err}"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn corrupt_unresponsive_mark_is_reported_as_error() {
        let (space, hnd) = space_and_tx_hnd().await;
        let peer = Url::from_str("ws://corrupt.example:80/alice").unwrap();
        space
            .peer_meta_store()
            .put(
                peer.clone(),
                format!("{KEY_PREFIX_ROOT}:{META_KEY_UNRESPONSIVE}"),
                bytes::Bytes::from_static(b"not a timestamp"),
                None,
            )
            .await
            .unwrap();
        space
            .peer_meta_store()
            .get_unresponsive(peer.clone())
            .await
            .expect_err("the stored value must fail to deserialize");

        // The caller decides how to treat the error; the iroh transport
        // falls back to dialing.
        hnd.is_unresponsive(peer, Timestamp::from_micros(0))
            .await
            .expect_err("an unreadable mark must be reported as an error");
    }
}
