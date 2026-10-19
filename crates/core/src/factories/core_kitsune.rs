//! The core kitsune implementation provided by Kitsune2.

use kitsune2_api::*;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};

/// The core kitsune implementation provided by Kitsune2.
/// You probably will have no reason to use something other than this.
/// This abstraction is mainly here for testing purposes.
#[derive(Debug)]
pub struct CoreKitsuneFactory {}

impl CoreKitsuneFactory {
    /// Construct a new CoreKitsuneFactory.
    pub fn create() -> DynKitsuneFactory {
        let out: DynKitsuneFactory = Arc::new(CoreKitsuneFactory {});
        out
    }
}

impl KitsuneFactory for CoreKitsuneFactory {
    fn default_config(&self, _config: &mut Config) -> K2Result<()> {
        Ok(())
    }

    fn validate_config(&self, _config: &Config) -> K2Result<()> {
        Ok(())
    }

    fn create(
        &self,
        builder: Arc<Builder>,
    ) -> BoxFut<'static, K2Result<DynKitsune>> {
        Box::pin(async move {
            let out = CoreKitsune::new(builder).await?;
            let out: DynKitsune = Arc::new(out);
            Ok(out)
        })
    }
}

#[derive(Debug)]
struct TxHandlerTranslator(DynKitsuneHandler);

impl TxBaseHandler for TxHandlerTranslator {
    fn new_listening_address(&self, this_url: Url) -> BoxFut<'static, ()> {
        self.0.new_listening_address(this_url)
    }

    fn peer_disconnect(&self, peer: Url, reason: Option<String>) {
        self.0.peer_disconnect(peer, reason);
    }
}

impl TxHandler for TxHandlerTranslator {
    fn preflight_gather_outgoing(
        &self,
        peer_url: Url,
    ) -> BoxFut<'_, K2Result<bytes::Bytes>> {
        self.0.preflight_gather_outgoing(peer_url)
    }

    fn preflight_validate_incoming(
        &self,
        peer_url: Url,
        data: bytes::Bytes,
    ) -> BoxFut<'_, K2Result<()>> {
        self.0.preflight_validate_incoming(peer_url, data)
    }
}

type SpaceFut = futures::future::Shared<BoxFut<'static, K2Result<DynSpace>>>;

/// A space that is constructed or under construction.
#[derive(Debug)]
struct SpaceEntry {
    fut: SpaceFut,
    /// Aborts the construction, so that its future and everything it captured
    /// are dropped even while a `space` call still awaits it.
    abort: futures::future::AbortHandle,
}

type Map = HashMap<SpaceId, SpaceEntry>;

#[derive(Debug)]
struct CoreKitsune {
    builder: Arc<Builder>,
    close_once: CloseOnce,
    handler: std::sync::Mutex<Option<DynKitsuneHandler>>,
    map: std::sync::Mutex<Map>,
    tx: std::sync::Mutex<Option<DynTransport>>,
    report: std::sync::Mutex<Option<DynReport>>,
}

impl CoreKitsune {
    pub async fn new(builder: Arc<Builder>) -> K2Result<Self> {
        Ok(Self {
            builder,
            close_once: CloseOnce::new(),
            handler: std::sync::Mutex::new(None),
            map: std::sync::Mutex::new(HashMap::new()),
            tx: std::sync::Mutex::new(None),
            report: std::sync::Mutex::new(None),
        })
    }

    /// Gets the transport for `remove_space`, or `None` once this instance
    /// is closed, because `close` has then removed every space.
    async fn transport_unless_closed(&self) -> K2Result<Option<DynTransport>> {
        match self.transport().await {
            Err(_) if self.close_once.is_requested() => Ok(None),
            result => result.map(Some),
        }
    }
}

impl Kitsune for CoreKitsune {
    fn register_handler(
        &self,
        handler: DynKitsuneHandler,
    ) -> BoxFut<'_, K2Result<()>> {
        Box::pin(async move {
            const ERR: &str = "handler already registered";

            if self.close_once.is_requested() {
                return Err(K2Error::other(KITSUNE_CLOSED_ERR));
            }

            if self.handler.lock().expect("poison").is_some() {
                return Err(K2Error::other(ERR));
            }

            let tx = self
                .builder
                .transport
                .create(
                    self.builder.clone(),
                    Arc::new(TxHandlerTranslator(handler.clone())),
                )
                .await?;

            // A transport that is not stored below is never reachable by
            // `close`, so it is closed here on every failure path.
            let report = match self
                .builder
                .report
                .create(self.builder.clone(), tx.clone())
                .await
            {
                Ok(report) => report,
                Err(err) => {
                    tx.close().await;
                    return Err(err);
                }
            };

            // `close` marks the close as requested before reading or taking
            // these slots under the handler lock, so checking the request under
            // that lock means a racing close either sees every value stored
            // here or makes this registration fail.
            let rejected = {
                let mut handler_slot = self.handler.lock().expect("poison");
                if self.close_once.is_requested() {
                    Some(KITSUNE_CLOSED_ERR)
                } else if handler_slot.is_some() {
                    Some(ERR)
                } else {
                    *handler_slot = Some(handler);
                    *self.tx.lock().expect("poison") = Some(tx.clone());
                    *self.report.lock().expect("poison") = Some(report);
                    None
                }
            };
            if let Some(err) = rejected {
                tx.close().await;
                return Err(K2Error::other(err));
            }

            Ok(())
        })
    }

    fn list_spaces(&self) -> Vec<SpaceId> {
        self.map.lock().unwrap().keys().cloned().collect()
    }

    fn space(
        &self,
        space_id: SpaceId,
        config_override: Option<Config>,
    ) -> BoxFut<'_, K2Result<DynSpace>> {
        Box::pin(async move {
            const ERR: &str = "handler not registered";
            use std::collections::hash_map::Entry;

            // This is quick, we don't hold the lock very long,
            // because we're just constructing the future here,
            // not awaiting it.
            // The close request is checked under the map lock, so a racing
            // `close` either drains the entry inserted here or makes this call
            // fail.
            let fut = {
                let mut map = self.map.lock().expect("poison");
                if self.close_once.is_requested() {
                    return Err(K2Error::other(KITSUNE_CLOSED_ERR));
                }
                match map.entry(space_id.clone()) {
                    Entry::Occupied(e) => e.get().fut.clone(),
                    Entry::Vacant(e) => {
                        let builder = self.builder.clone();
                        let handler = self
                            .handler
                            .lock()
                            .expect("poison")
                            .clone()
                            .ok_or_else(|| K2Error::other(ERR))?;
                        let tx = self
                            .tx
                            .lock()
                            .expect("poison")
                            .clone()
                            .ok_or_else(|| K2Error::other(ERR))?;
                        let report = self
                            .report
                            .lock()
                            .expect("poison")
                            .clone()
                            .ok_or_else(|| K2Error::other(ERR))?;
                        let (abort, registration) =
                            futures::future::AbortHandle::new_pair();
                        let construct = futures::future::Abortable::new(
                            async move {
                                let sh = handler
                                    .create_space(
                                        space_id.clone(),
                                        config_override.as_ref(),
                                    )
                                    .await?;
                                let s = builder
                                    .space
                                    .create(
                                        builder.clone(),
                                        config_override,
                                        sh,
                                        space_id,
                                        report,
                                        tx,
                                    )
                                    .await?;
                                Ok(s)
                            },
                            registration,
                        );
                        let fut: BoxFut<'static, K2Result<DynSpace>> =
                            Box::pin(async move {
                                construct.await.unwrap_or_else(|_| {
                                    Err(K2Error::other(KITSUNE_CLOSED_ERR))
                                })
                            });
                        e.insert(SpaceEntry {
                            fut: futures::future::FutureExt::shared(fut),
                            abort,
                        })
                        .fut
                        .clone()
                    }
                }
            };

            let space = fut.await?;
            // A space whose construction finished while `close` ran belongs
            // to a closed instance.
            if self.close_once.is_requested() {
                return Err(K2Error::other(KITSUNE_CLOSED_ERR));
            }
            Ok(space)
        })
    }

    fn space_if_exists(
        &self,
        space_id: SpaceId,
    ) -> BoxFut<'_, Option<DynSpace>> {
        Box::pin(async move {
            let fut =
                self.map.lock().expect("poison").get(&space_id)?.fut.clone();
            let space = fut.await.ok()?;
            // A space whose construction finished while `close` ran belongs
            // to a closed instance.
            if self.close_once.is_requested() {
                return None;
            }
            Some(space)
        })
    }

    fn remove_space(&self, space_id: SpaceId) -> BoxFut<'_, K2Result<()>> {
        Box::pin(async move {
            let fut = self
                .map
                .lock()
                .expect("poison")
                .get(&space_id)
                .map(|entry| entry.fut.clone());
            if let Some(fut) = fut {
                if let Ok(s) = fut.await {
                    // Just a point in time check, try to help the user out if they accidentally
                    // try to remove a space with active local agents.
                    // Those agents need to leave the space before it should be removed.
                    if !s.local_agent_store().get_all().await?.is_empty() {
                        return Err(K2Error::other(
                            "Cannot remove space with local agents",
                        ));
                    }

                    // Checks passed, remove our reference to the space.
                    self.map.lock().expect("poison").remove(&space_id);

                    // Unregister the space and its module handlers from the transport.
                    let Some(transport) =
                        self.transport_unless_closed().await?
                    else {
                        return Ok(());
                    };
                    transport.unregister_space(space_id).await;

                    // Get all the peer URLs of connected peers.
                    let Some(transport) =
                        self.transport_unless_closed().await?
                    else {
                        return Ok(());
                    };
                    let connected_peer_urls = Arc::new(RwLock::new(
                        transport
                            .get_connected_peers()
                            .await?
                            .into_iter()
                            .collect::<HashSet<_>>(),
                    ));

                    // Remove any peer URLs that are connected for another space.
                    let spaces = self
                        .map
                        .lock()
                        .expect("poison")
                        .iter()
                        .map(|s| s.1.fut.clone())
                        .collect::<Vec<_>>();
                    futures::future::join_all(spaces.into_iter().map(
                        |fut| -> BoxFut<'_, K2Result<()>> {
                            let connected_peer_urls =
                                connected_peer_urls.clone();
                            Box::pin(async move {
                                if let Ok(s) = fut.await {
                                    let check_our_peers = connected_peer_urls
                                        .read()
                                        .unwrap()
                                        .clone();
                                    let space_agent_infos =
                                        s.peer_store().get_all().await?;

                                    let to_remove = space_agent_infos
                                        .into_iter()
                                        .filter_map(|a| {
                                            if let Some(url) = &a.url {
                                                check_our_peers
                                                    .contains(url)
                                                    .then_some(url.clone())
                                            } else {
                                                None
                                            }
                                        })
                                        .collect::<HashSet<_>>();

                                    connected_peer_urls
                                        .write()
                                        .unwrap()
                                        .retain(|u| !to_remove.contains(u));
                                }

                                Ok(())
                            })
                        },
                    ))
                    .await;

                    // Disconnect from any remaining peers. I.e. those that aren't in use by
                    // another space.
                    let Some(transport) =
                        self.transport_unless_closed().await?
                    else {
                        return Ok(());
                    };
                    let to_remove = connected_peer_urls.read().unwrap().clone();
                    for url in to_remove {
                        tracing::info!(
                            "Disconnecting peer {url} from transport"
                        );
                        transport
                            .disconnect(
                                url.clone(),
                                Some("Space is being removed".to_string()),
                            )
                            .await;
                    }
                } else if self.close_once.is_requested() {
                    // `close` cancelled the construction, so there is nothing
                    // left to remove.
                    tracing::debug!(
                        "Space construction was cancelled by close"
                    );
                } else {
                    tracing::warn!(
                        "Space exists, but failed to get a handle to it"
                    );
                }
            } else {
                tracing::info!("Space with id {space_id:?} not found");
            }
            Ok(())
        })
    }

    fn transport(&self) -> BoxFut<'_, K2Result<DynTransport>> {
        Box::pin(async move {
            if self.close_once.is_requested() {
                return Err(K2Error::other(KITSUNE_CLOSED_ERR));
            }
            self.tx
                .lock()
                .expect("poison")
                .clone()
                .ok_or_else(|| K2Error::other("Transport not registered yet"))
        })
    }

    fn report(&self) -> K2Result<DynReport> {
        if self.close_once.is_requested() {
            return Err(K2Error::other(KITSUNE_CLOSED_ERR));
        }
        self.report
            .lock()
            .expect("poison")
            .clone()
            .ok_or_else(|| K2Error::other("Report not registered yet"))
    }

    fn close(&self) -> BoxFut<'_, ()> {
        Box::pin(async move {
            // `run` rejects new work right away, even while another close is
            // running.
            self.close_once
                .run(async {
                    // Abort and drop the spaces without awaiting the ones still
                    // under construction, which could hang on the handler's
                    // `create_space`. Aborting also cancels a construction that
                    // a pending `space` call still awaits, so the handler,
                    // transport and report it captured are released once that
                    // call is next polled.
                    let spaces =
                        std::mem::take(&mut *self.map.lock().expect("poison"));
                    for entry in spaces.into_values() {
                        entry.abort.abort();
                    }

                    // `register_handler` stores its values under the handler
                    // lock after checking for a close request, so once that lock
                    // is taken a racing registration has either stored every
                    // value or will fail.
                    let tx = {
                        let _handler = self.handler.lock().expect("poison");
                        self.tx.lock().expect("poison").clone()
                    };
                    // Close the transport before clearing its slot, so a
                    // cancelled close leaves it in place for the next call to
                    // close.
                    if let Some(tx) = tx {
                        tx.close().await;
                    }

                    let (handler, tx, report) = {
                        let mut handler = self.handler.lock().expect("poison");
                        (
                            handler.take(),
                            self.tx.lock().expect("poison").take(),
                            self.report.lock().expect("poison").take(),
                        )
                    };
                    drop(report);
                    drop(handler);
                    drop(tx);
                    tracing::info!("Kitsune closed");
                })
                .await;
        })
    }
}

#[cfg(test)]
mod test {
    use kitsune2_api::*;
    use kitsune2_test_utils::space::TEST_SPACE_ID;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    #[derive(Debug)]
    struct S;

    impl SpaceHandler for S {
        fn recv_notify(
            &self,
            _peer: Url,
            _space_id: SpaceId,
            _data: bytes::Bytes,
        ) -> K2Result<()> {
            // this test is a bit of a stub for now until we have the
            // transport module implemented and can send/receive messages.
            Ok(())
        }
    }

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

    /// Hands out each pre-built transport once, after its gate opens, so a
    /// test can control both the transports and when they are created.
    #[derive(Debug)]
    struct GatedTxFactory {
        txs: Mutex<Vec<DynTransport>>,
        gate: tokio::sync::watch::Receiver<bool>,
        create_calls: Arc<AtomicUsize>,
    }

    impl TransportFactory for GatedTxFactory {
        fn default_config(&self, _config: &mut Config) -> K2Result<()> {
            Ok(())
        }

        fn validate_config(&self, _config: &Config) -> K2Result<()> {
            Ok(())
        }

        fn create(
            &self,
            _builder: Arc<Builder>,
            _handler: DynTxHandler,
        ) -> BoxFut<'static, K2Result<DynTransport>> {
            self.create_calls.fetch_add(1, Ordering::SeqCst);
            let tx = self
                .txs
                .lock()
                .expect("poison")
                .pop()
                .expect("one transport per create");
            let mut gate = self.gate.clone();
            Box::pin(async move {
                gate.wait_for(|open| *open).await.expect("gate dropped");
                Ok(tx)
            })
        }
    }

    /// A report factory whose `create` always fails.
    #[derive(Debug)]
    struct FailingReportFactory;

    impl ReportFactory for FailingReportFactory {
        fn default_config(&self, _config: &mut Config) -> K2Result<()> {
            Ok(())
        }

        fn validate_config(&self, _config: &Config) -> K2Result<()> {
            Ok(())
        }

        fn create(
            &self,
            _builder: Arc<Builder>,
            _tx: DynTransport,
        ) -> BoxFut<'static, K2Result<DynReport>> {
            Box::pin(async { Err(K2Error::other("report create failed")) })
        }
    }

    /// A kitsune handler whose `create_space` never completes.
    #[derive(Debug)]
    struct HangingK;

    impl KitsuneHandler for HangingK {
        fn create_space(
            &self,
            _space_id: SpaceId,
            _config_override: Option<&Config>,
        ) -> BoxFut<'_, K2Result<DynSpaceHandler>> {
            Box::pin(std::future::pending())
        }
    }

    /// A kitsune handler whose `create_space` completes once its gate opens.
    #[derive(Debug)]
    struct GatedK {
        gate: tokio::sync::watch::Receiver<bool>,
    }

    impl KitsuneHandler for GatedK {
        fn create_space(
            &self,
            _space_id: SpaceId,
            _config_override: Option<&Config>,
        ) -> BoxFut<'_, K2Result<DynSpaceHandler>> {
            let mut gate = self.gate.clone();
            Box::pin(async move {
                gate.wait_for(|open| *open).await.expect("gate dropped");
                let s: DynSpaceHandler = Arc::new(S);
                Ok(s)
            })
        }
    }

    /// A local agent store without agents whose `get_all` only finishes once
    /// its gate is open.
    #[derive(Debug)]
    struct GatedLocalAgentStore {
        gate: tokio::sync::watch::Receiver<bool>,
    }

    impl LocalAgentStore for GatedLocalAgentStore {
        fn add(&self, _local_agent: DynLocalAgent) -> BoxFut<'_, K2Result<()>> {
            Box::pin(async { Ok(()) })
        }

        fn remove(
            &self,
            _local_agent: AgentId,
        ) -> BoxFut<'_, Option<DynLocalAgent>> {
            Box::pin(async { None })
        }

        fn get_all(&self) -> BoxFut<'_, K2Result<Vec<DynLocalAgent>>> {
            let mut gate = self.gate.clone();
            Box::pin(async move {
                gate.wait_for(|open| *open).await.expect("gate dropped");
                Ok(Vec::new())
            })
        }
    }

    /// Creates [GatedLocalAgentStore]s that share one gate.
    #[derive(Debug)]
    struct GatedLocalAgentStoreFactory {
        gate: tokio::sync::watch::Receiver<bool>,
    }

    impl LocalAgentStoreFactory for GatedLocalAgentStoreFactory {
        fn default_config(&self, _config: &mut Config) -> K2Result<()> {
            Ok(())
        }

        fn validate_config(&self, _config: &Config) -> K2Result<()> {
            Ok(())
        }

        fn create(
            &self,
            _builder: Arc<Builder>,
        ) -> BoxFut<'static, K2Result<DynLocalAgentStore>> {
            let store: DynLocalAgentStore = Arc::new(GatedLocalAgentStore {
                gate: self.gate.clone(),
            });
            Box::pin(async move { Ok(store) })
        }
    }

    /// A test builder whose transport is `tx`, created as soon as a handler
    /// is registered.
    fn builder_with_transport(tx: MockTransport) -> Builder {
        builder_with_gated_transport(
            tx,
            tokio::sync::watch::channel(true).1,
            Arc::new(AtomicUsize::new(0)),
        )
    }

    /// A test builder whose transport is `tx`, created once `gate` is `true`.
    /// Every transport creation increments `create_calls`.
    fn builder_with_gated_transport(
        tx: MockTransport,
        gate: tokio::sync::watch::Receiver<bool>,
        create_calls: Arc<AtomicUsize>,
    ) -> Builder {
        builder_with_gated_transports(vec![tx], gate, create_calls)
    }

    /// A test builder whose transport creations each take one of `txs`, once
    /// `gate` is `true`. Every transport creation increments `create_calls`.
    fn builder_with_gated_transports(
        txs: Vec<MockTransport>,
        gate: tokio::sync::watch::Receiver<bool>,
        create_calls: Arc<AtomicUsize>,
    ) -> Builder {
        Builder {
            transport: Arc::new(GatedTxFactory {
                txs: Mutex::new(
                    txs.into_iter()
                        .map(|tx| Arc::new(tx) as DynTransport)
                        .collect(),
                ),
                gate,
                create_calls,
            }),
            ..crate::default_test_builder()
        }
    }

    /// Builds a kitsune instance whose transport is `tx`, created as soon as
    /// a handler is registered.
    async fn kitsune_with_transport(tx: MockTransport) -> DynKitsune {
        builder_with_transport(tx)
            .with_default_config()
            .unwrap()
            .build()
            .await
            .unwrap()
    }

    /// A [MockTransport] whose `close` counts calls and only finishes once
    /// the returned gate sender has sent `true`.
    fn gated_close_transport() -> (
        MockTransport,
        tokio::sync::watch::Sender<bool>,
        Arc<AtomicUsize>,
    ) {
        let (gate_send, gate) = tokio::sync::watch::channel(false);
        let close_calls = Arc::new(AtomicUsize::new(0));
        let mut tx = MockTransport::new();
        tx.expect_close().returning({
            let close_calls = close_calls.clone();
            move || {
                close_calls.fetch_add(1, Ordering::SeqCst);
                let mut gate = gate.clone();
                Box::pin(async move {
                    gate.wait_for(|open| *open).await.expect("gate dropped");
                })
            }
        });
        (tx, gate_send, close_calls)
    }

    /// Asserts that `result` failed because the kitsune instance is closed.
    fn assert_closed<T>(result: K2Result<T>) {
        let err = result.err().expect("call must fail after close");
        assert!(
            err.to_string().contains(KITSUNE_CLOSED_ERR),
            "unexpected error: {err}"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn happy_space_construct() {
        let h: DynKitsuneHandler = Arc::new(K);

        let k = crate::default_test_builder()
            .with_default_config()
            .unwrap()
            .build()
            .await
            .unwrap();

        k.register_handler(h).await.unwrap();

        k.space(TEST_SPACE_ID, None).await.unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn close_releases_spaces_transport_and_handler() {
        let k = crate::default_test_builder()
            .with_default_config()
            .unwrap()
            .build()
            .await
            .unwrap();
        let h: DynKitsuneHandler = Arc::new(K);
        let weak_h = Arc::downgrade(&h);
        k.register_handler(h).await.unwrap();
        let weak_space =
            Arc::downgrade(&k.space(TEST_SPACE_ID, None).await.unwrap());
        let weak_tx = Arc::downgrade(&k.transport().await.unwrap());
        let weak_report = Arc::downgrade(&k.report().unwrap());

        k.close().await;

        assert!(k.list_spaces().is_empty());
        assert!(k.space_if_exists(TEST_SPACE_ID).await.is_none());
        assert_closed(k.space(TEST_SPACE_ID, None).await);
        assert_closed(k.transport().await);
        assert_closed(k.report());
        assert_closed(k.register_handler(Arc::new(K)).await);
        k.remove_space(TEST_SPACE_ID).await.unwrap();

        // The space, handler, transport and report must be released even
        // though `k` is still alive.
        kitsune2_test_utils::retry_fn_until_timeout(
            || async {
                weak_space.upgrade().is_none()
                    && weak_h.upgrade().is_none()
                    && weak_tx.upgrade().is_none()
                    && weak_report.upgrade().is_none()
            },
            Some(5000),
            Some(50),
        )
        .await
        .expect(
            "space, handler, transport and report must be released after close",
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn close_without_handler() {
        let k = crate::default_test_builder()
            .with_default_config()
            .unwrap()
            .build()
            .await
            .unwrap();

        tokio::time::timeout(std::time::Duration::from_secs(1), k.close())
            .await
            .expect("close without a handler must complete");

        assert!(k.list_spaces().is_empty());
        assert_closed(k.transport().await);
        assert_closed(k.report());
        assert_closed(k.register_handler(Arc::new(K)).await);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn close_cancels_spaces_under_construction() {
        let mut tx = MockTransport::new();
        tx.expect_close().times(1).returning(|| Box::pin(async {}));
        let k = kitsune_with_transport(tx).await;
        let h: DynKitsuneHandler = Arc::new(HangingK);
        let weak_h = Arc::downgrade(&h);
        k.register_handler(h).await.unwrap();
        let pending_space = tokio::spawn({
            let k = k.clone();
            async move { k.space(TEST_SPACE_ID, None).await }
        });
        kitsune2_test_utils::retry_fn_until_timeout(
            || async { !k.list_spaces().is_empty() },
            Some(1000),
            Some(10),
        )
        .await
        .expect("space construction must start");
        // Polling once makes `remove_space` await the construction that
        // `close` is about to cancel.
        let mut pending_remove = k.remove_space(TEST_SPACE_ID);
        assert!(futures::poll!(&mut pending_remove).is_pending());

        tokio::time::timeout(std::time::Duration::from_secs(1), k.close())
            .await
            .expect("close must not wait for a hanging create_space");

        tokio::time::timeout(std::time::Duration::from_secs(1), pending_remove)
            .await
            .expect("pending remove_space call must finish after close")
            .expect("remove_space must succeed for a cancelled construction");

        // The pending `space` call fails instead of handing out a space of
        // the closed instance, and the construction it awaited no longer
        // holds the handler.
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            pending_space,
        )
        .await
        .expect("pending space call must finish after close")
        .unwrap();
        assert_closed(result);
        assert!(
            weak_h.upgrade().is_none(),
            "kitsune handler must be released after close"
        );
        // mockall verifies `times(1)` on drop.
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn remove_space_succeeds_when_closed_after_space_is_built() {
        let (gate_send, gate) = tokio::sync::watch::channel(true);
        let k = Builder {
            local_agent_store: Arc::new(GatedLocalAgentStoreFactory { gate }),
            ..crate::default_test_builder()
        }
        .with_default_config()
        .unwrap()
        .build()
        .await
        .unwrap();
        k.register_handler(Arc::new(K)).await.unwrap();
        k.space(TEST_SPACE_ID, None).await.unwrap();

        // Hold `remove_space` at its local agent check, past the point where
        // it awaited the built space.
        gate_send.send(false).unwrap();
        let mut pending_remove = k.remove_space(TEST_SPACE_ID);
        assert!(futures::poll!(&mut pending_remove).is_pending());

        k.close().await;
        gate_send.send(true).unwrap();

        tokio::time::timeout(std::time::Duration::from_secs(1), pending_remove)
            .await
            .expect("pending remove_space call must finish after close")
            .expect("remove_space must succeed when close removed the space");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn space_constructed_while_closing_is_rejected() {
        let k = crate::default_test_builder()
            .with_default_config()
            .unwrap()
            .build()
            .await
            .unwrap();
        let (gate_send, gate) = tokio::sync::watch::channel(false);
        k.register_handler(Arc::new(GatedK { gate })).await.unwrap();

        // Both calls await the same construction, which is held at the gate.
        let mut driver = k.space(TEST_SPACE_ID, None);
        assert!(futures::poll!(&mut driver).is_pending());
        let mut waiter = k.space(TEST_SPACE_ID, None);
        assert!(futures::poll!(&mut waiter).is_pending());

        // The construction completes before `close` runs, but `waiter` only
        // observes its result after the instance is closed.
        gate_send.send(true).unwrap();
        driver
            .await
            .expect("space must be constructed before close");
        k.close().await;

        assert_closed(waiter.await);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn space_if_exists_constructed_while_closing_is_none() {
        let k = crate::default_test_builder()
            .with_default_config()
            .unwrap()
            .build()
            .await
            .unwrap();
        let (gate_send, gate) = tokio::sync::watch::channel(false);
        k.register_handler(Arc::new(GatedK { gate })).await.unwrap();

        // Both calls await the same construction, which is held at the gate.
        let mut driver = k.space(TEST_SPACE_ID, None);
        assert!(futures::poll!(&mut driver).is_pending());
        let mut waiter = k.space_if_exists(TEST_SPACE_ID);
        assert!(futures::poll!(&mut waiter).is_pending());

        // The construction completes before `close` runs, but `waiter` only
        // observes its result after the instance is closed.
        gate_send.send(true).unwrap();
        driver
            .await
            .expect("space must be constructed before close");
        k.close().await;

        assert!(
            waiter.await.is_none(),
            "space_if_exists returned Some after close"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn close_after_cancelled_close_closes_transport() {
        let (tx, gate_send, close_calls) = gated_close_transport();
        let k = kitsune_with_transport(tx).await;
        k.register_handler(Arc::new(K)).await.unwrap();

        tokio::time::timeout(std::time::Duration::from_millis(50), k.close())
            .await
            .expect_err("close must wait for the transport to close");

        gate_send.send(true).unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(1), k.close())
            .await
            .expect("close after a cancelled close must complete");

        assert_eq!(close_calls.load(Ordering::SeqCst), 2);
        assert_closed(k.transport().await);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn concurrent_close_waits_for_running_close() {
        let (tx, gate_send, close_calls) = gated_close_transport();
        let k = kitsune_with_transport(tx).await;
        k.register_handler(Arc::new(K)).await.unwrap();

        let first = tokio::spawn({
            let k = k.clone();
            async move { k.close().await }
        });
        kitsune2_test_utils::retry_fn_until_timeout(
            || async { close_calls.load(Ordering::SeqCst) == 1 },
            Some(1000),
            Some(10),
        )
        .await
        .expect("first close must reach the transport");

        tokio::time::timeout(std::time::Duration::from_millis(100), k.close())
            .await
            .expect_err("a concurrent close must wait for the running close");

        gate_send.send(true).unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(1), first)
            .await
            .expect("first close must complete once the transport closes")
            .unwrap();
        k.close().await;

        assert_eq!(close_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn register_handler_closes_transport_when_report_fails() {
        let mut tx = MockTransport::new();
        tx.expect_close().times(1).returning(|| Box::pin(async {}));
        let k = Builder {
            report: Arc::new(FailingReportFactory),
            ..builder_with_transport(tx)
        }
        .with_default_config()
        .unwrap()
        .build()
        .await
        .unwrap();

        let err = k
            .register_handler(Arc::new(K))
            .await
            .expect_err("register_handler must fail with the report error");
        assert!(
            err.to_string().contains("report create failed"),
            "unexpected error: {err}"
        );
        // mockall verifies `times(1)` on drop.
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn register_handler_racing_close_closes_its_transport() {
        let close_calls = Arc::new(AtomicUsize::new(0));
        let mut tx = MockTransport::new();
        tx.expect_close().returning({
            let close_calls = close_calls.clone();
            move || {
                close_calls.fetch_add(1, Ordering::SeqCst);
                Box::pin(async {})
            }
        });
        let (gate_send, gate) = tokio::sync::watch::channel(false);
        let create_calls = Arc::new(AtomicUsize::new(0));
        let k = builder_with_gated_transport(tx, gate, create_calls.clone())
            .with_default_config()
            .unwrap()
            .build()
            .await
            .unwrap();

        let register = tokio::spawn({
            let k = k.clone();
            async move { k.register_handler(Arc::new(K)).await }
        });
        kitsune2_test_utils::retry_fn_until_timeout(
            || async { create_calls.load(Ordering::SeqCst) == 1 },
            Some(1000),
            Some(10),
        )
        .await
        .expect("register_handler must start creating the transport");

        // Close while the transport is still being created, then let the
        // registration finish.
        k.close().await;
        gate_send.send(true).unwrap();

        assert_closed(register.await.unwrap());
        assert_eq!(close_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn duplicate_register_handler_closes_the_losing_transport() {
        let counted_close_transport = || {
            let close_calls = Arc::new(AtomicUsize::new(0));
            let mut tx = MockTransport::new();
            tx.expect_close().returning({
                let close_calls = close_calls.clone();
                move || {
                    close_calls.fetch_add(1, Ordering::SeqCst);
                    Box::pin(async {})
                }
            });
            (tx, close_calls)
        };
        let (tx_a, close_calls_a) = counted_close_transport();
        let (tx_b, close_calls_b) = counted_close_transport();
        let (gate_send, gate) = tokio::sync::watch::channel(false);
        let create_calls = Arc::new(AtomicUsize::new(0));
        let k = builder_with_gated_transports(
            vec![tx_a, tx_b],
            gate,
            create_calls.clone(),
        )
        .with_default_config()
        .unwrap()
        .build()
        .await
        .unwrap();

        let register = || {
            let k = k.clone();
            tokio::spawn(async move { k.register_handler(Arc::new(K)).await })
        };
        let first = register();
        let second = register();
        kitsune2_test_utils::retry_fn_until_timeout(
            || async { create_calls.load(Ordering::SeqCst) == 2 },
            Some(1000),
            Some(10),
        )
        .await
        .expect("both registrations must start creating a transport");

        gate_send.send(true).unwrap();
        let results = [first.await.unwrap(), second.await.unwrap()];

        let errors: Vec<_> =
            results.iter().filter_map(|r| r.as_ref().err()).collect();
        assert_eq!(errors.len(), 1, "exactly one registration must fail");
        assert!(
            errors[0].to_string().contains("handler already registered"),
            "unexpected error: {err}",
            err = errors[0]
        );
        let close_calls = || {
            [
                close_calls_a.load(Ordering::SeqCst),
                close_calls_b.load(Ordering::SeqCst),
            ]
        };
        assert_eq!(close_calls().iter().sum::<usize>(), 1);

        // Closing the instance closes the winning transport, so each
        // transport ends up closed exactly once.
        k.close().await;
        assert_eq!(close_calls(), [1, 1]);
    }
}
