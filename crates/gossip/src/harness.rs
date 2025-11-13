//! Functional test harness for K2Gossip.

use crate::harness::op_store::K2GossipMemOpStoreFactory;
use crate::peer_meta_store::K2PeerMetaStore;
use crate::{K2GossipConfig, K2GossipFactory, K2GossipModConfig};
use kitsune2_api::{
    AgentId, AgentInfoSigned, DhtArc, DynGossip, DynSpace,
    GossipStateSummaryRequest, LocalAgent, OpId, SpaceHandler, SpaceId,
    StoredOp, UNIX_TIMESTAMP,
};
use kitsune2_core::factories::MemoryOp;
use kitsune2_core::{default_test_builder, Ed25519LocalAgent, Ed25519Verifier};
use kitsune2_test_utils::noop_bootstrap::NoopBootstrapFactory;
use kitsune2_test_utils::tx_handler::TestTxHandler;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;
use tokio::task::AbortHandle;

mod op_store;

pub use op_store::{GossipOpStore, MemoryOpRecord};

/// A functional test harness for K2Gossip.
///
/// Create instances of this using the [`K2GossipFunctionalTestFactory`].
#[derive(Debug, Clone)]
pub struct K2GossipFunctionalTestHarness {
    /// The inner gossip instance.
    pub gossip: DynGossip,
    /// The space instance.
    pub space: DynSpace,
    /// The op store instance.
    ///
    /// This can be used to author data directly to the op store.
    pub op_store: GossipOpStore,
    /// The peer meta store.
    pub peer_meta_store: Arc<K2PeerMetaStore>,
    /// The abort handle for the process task.
    ///
    /// This simulates the process of checking incoming ops and adding them to the DHT model.
    pub process_abort_handle: AbortHandle,
}

impl Drop for K2GossipFunctionalTestHarness {
    fn drop(&mut self) {
        // Abort the process task
        self.process_abort_handle.abort();
    }
}

impl K2GossipFunctionalTestHarness {
    /// Join a local agent to the space.
    pub async fn join_local_agent(
        &self,
        target_arc: DhtArc,
    ) -> Arc<AgentInfoSigned> {
        let local_agent = Arc::new(Ed25519LocalAgent::default());
        local_agent.set_tgt_storage_arc_hint(target_arc);

        self.space
            .local_agent_join(local_agent.clone())
            .await
            .unwrap();

        // Wait for the agent info to be published
        // This means tests can rely on the agent being available in the peer store
        tokio::time::timeout(Duration::from_secs(5), {
            let agent_id = local_agent.agent().clone();
            let peer_store = self.space.peer_store().clone();
            async move {
                while peer_store
                    .get_all()
                    .await
                    .unwrap()
                    .iter()
                    .all(|a| a.agent.clone() != agent_id)
                {
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
            }
        })
        .await
        .unwrap();

        self.space
            .peer_store()
            .get(local_agent.agent().clone())
            .await
            .unwrap()
            .unwrap()
    }

    /// Wait for this agent to be in our peer store.
    pub async fn wait_for_agent_in_peer_store(&self, agent: AgentId) {
        tokio::time::timeout(Duration::from_millis(100), {
            let this = self.clone();
            async move {
                loop {
                    let has_agent = this
                        .space
                        .peer_store()
                        .get(agent.clone())
                        .await
                        .unwrap()
                        .is_some();

                    if has_agent {
                        break;
                    }

                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
            }
        })
        .await
        .unwrap();
    }

    /// Wait for the given ops to be in our op store.
    pub async fn wait_for_ops(&self, op_ids: Vec<OpId>) -> Vec<MemoryOp> {
        tokio::time::timeout(Duration::from_millis(500), {
            let this = self.clone();
            async move {
                loop {
                    tokio::time::sleep(Duration::from_millis(10)).await;

                    let ops = this
                        .space
                        .op_store()
                        .retrieve_ops(op_ids.clone())
                        .await
                        .unwrap();

                    if ops.len() != op_ids.len() {
                        tracing::info!(
                            "Have {}/{} requested ops",
                            ops.len(),
                            op_ids.len()
                        );
                        continue;
                    }

                    return ops
                        .into_iter()
                        .map(|op| {
                            let out: MemoryOp = op.op_data.into();

                            out
                        })
                        .collect::<Vec<_>>();
                }
            }
        })
        .await
        .expect("Timed out waiting for ops")
    }

    /// Wait until ops are either stored or are being fetched.
    ///
    /// This is a soft check for "will be in sync soon" that lets tests check the progress of
    /// gossip without having to wait for fetching to complete.
    pub async fn wait_for_ops_discovered(
        &self,
        other: &Self,
        timeout: Duration,
    ) {
        let this = self.clone();
        let other = other.clone();

        tokio::time::timeout(timeout, async move {
            loop {
                // Known ops
                let mut our_op_ids = this
                    .space
                    .op_store()
                    .retrieve_op_ids_bounded(
                        DhtArc::FULL,
                        UNIX_TIMESTAMP,
                        u32::MAX,
                    )
                    .await
                    .unwrap()
                    .0
                    .into_iter()
                    .collect::<HashSet<_>>();

                let our_stored_op_ids = our_op_ids.clone();

                our_op_ids.extend(
                    this.space
                        .fetch()
                        .get_state_summary()
                        .await
                        .unwrap()
                        .pending_requests
                        .keys()
                        .cloned(),
                );

                let mut other_op_ids = other
                    .space
                    .op_store()
                    .retrieve_op_ids_bounded(
                        DhtArc::FULL,
                        UNIX_TIMESTAMP,
                        u32::MAX,
                    )
                    .await
                    .unwrap()
                    .0
                    .into_iter()
                    .collect::<HashSet<_>>();

                let other_stored_op_ids = other_op_ids.clone();

                other_op_ids.extend(
                    other
                        .space
                        .fetch()
                        .get_state_summary()
                        .await
                        .unwrap()
                        .pending_requests
                        .keys()
                        .cloned(),
                );

                if our_op_ids == other_op_ids {
                    tracing::info!("Both peers now know about {} op ids", our_op_ids.len());
                    break;
                } else {
                    tracing::info!("Waiting for op ids to be discovered: {} != {}. Have stored {}/{} and {}/{}", our_op_ids.len(), other_op_ids.len(), our_stored_op_ids.len(), our_op_ids.len(), other_stored_op_ids.len(), other_op_ids.len());
                }

                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        })
            .await
            .expect("Timed out waiting for ops to be discovered");
    }

    /// Wait for two instances to sync their op stores.
    pub async fn wait_for_sync_with(&self, other: &Self, timeout: Duration) {
        let this = self.clone();
        let other = other.clone();

        tokio::time::timeout(timeout, async move {
            loop {
                let our_op_ids = this
                    .space
                    .op_store()
                    .retrieve_op_ids_bounded(
                        DhtArc::FULL,
                        UNIX_TIMESTAMP,
                        u32::MAX,
                    )
                    .await
                    .unwrap()
                    .0;
                let other_op_ids = other
                    .space
                    .op_store()
                    .retrieve_op_ids_bounded(
                        DhtArc::FULL,
                        UNIX_TIMESTAMP,
                        u32::MAX,
                    )
                    .await
                    .unwrap()
                    .0;

                if our_op_ids.len() == other_op_ids.len() {
                    let our_op_ids_set =
                        our_op_ids.into_iter().collect::<HashSet<_>>();
                    let other_op_ids_set =
                        other_op_ids.into_iter().collect::<HashSet<_>>();

                    if our_op_ids_set == other_op_ids_set {
                        break;
                    }
                } else {
                    tracing::info!(
                        "Waiting for more ops to sync: {} != {}",
                        our_op_ids.len(),
                        other_op_ids.len(),
                    );
                }

                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await
        .expect("Timed out waiting for instances to sync");
    }

    /// Wait for all remote agents to declare a full arc to us.
    pub async fn wait_for_full_arc_for_all(&self, timeout: Duration) {
        let this = self.clone();

        tokio::time::timeout(timeout, async move {
            loop {
                let summary = this
                    .gossip
                    .get_state_summary(GossipStateSummaryRequest {
                        include_dht_summary: false,
                    })
                    .await
                    .unwrap();

                if summary
                    .peer_meta
                    .values()
                    .all(|meta| meta.storage_arc == DhtArc::FULL)
                {
                    break;
                }

                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await
        .expect("Timed out waiting for peers to declare full arc");
    }

    /// Force the storage arc of the given agent to be the given arc.
    ///
    /// This will publish a new agent info with the given arc, and wait for it to be available in
    /// the peer store. The latest agent info will be returned.
    pub async fn force_storage_arc(
        &self,
        agent_id: AgentId,
        arc: DhtArc,
    ) -> Arc<AgentInfoSigned> {
        let local_agents =
            self.space.local_agent_store().get_all().await.unwrap();
        let local_agent = local_agents
            .iter()
            .find(|a| a.agent() == &agent_id)
            .unwrap();

        local_agent.set_cur_storage_arc(arc);
        local_agent.invoke_cb();

        // Wait for the agent info to be published
        tokio::time::timeout(Duration::from_secs(5), {
            let peer_store = self.space.peer_store().clone();
            let agent_id = agent_id.clone();
            async move {
                loop {
                    // Assume the agent was already present.
                    let agent = peer_store.get(agent_id.clone()).await.unwrap();

                    let Some(agent) = agent else {
                        continue;
                    };

                    if agent.storage_arc == arc {
                        break;
                    }

                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
            }
        }).await.expect("Timed out waiting for agent to be in peer store with the requested arc");

        // Return the latest agent info
        self.space
            .peer_store()
            .get(agent_id)
            .await
            .unwrap()
            .unwrap()
    }
}

/// Functional test factory for K2Gossip.
pub struct K2GossipFunctionalTestFactory {
    /// The space ID to use for the test.
    space_id: SpaceId,
    /// Whether bootstrap is enabled.
    bootstrap: bool,
    /// The Kitsune2 configuration to use.
    config: K2GossipModConfig,
}

impl K2GossipFunctionalTestFactory {
    /// Create a new functional test factory.
    pub async fn create(
        space_id: SpaceId,
        bootstrap: bool,
        config: Option<K2GossipConfig>,
    ) -> K2GossipFunctionalTestFactory {
        K2GossipFunctionalTestFactory {
            space_id,
            bootstrap,
            config: K2GossipModConfig {
                k2_gossip: if let Some(config) = config {
                    config
                } else {
                    K2GossipConfig {
                        initiate_interval_ms: 10,
                        min_initiate_interval_ms: 10,
                        initial_initiate_interval_ms: 10,
                        initiate_jitter_ms: 30,
                        ..Default::default()
                    }
                },
            },
        }
    }

    /// Create a new instance of the test harness.
    pub async fn new_instance(&self) -> K2GossipFunctionalTestHarness {
        #[derive(Debug)]
        struct NoopHandler;
        impl SpaceHandler for NoopHandler {}

        let mut builder = default_test_builder();
        // Replace the core builder with a real gossip factory
        builder.gossip = K2GossipFactory::create();

        if !self.bootstrap {
            builder.bootstrap = Arc::new(NoopBootstrapFactory);
        }

        let op_store: GossipOpStore = Default::default();
        builder.op_store = Arc::new(K2GossipMemOpStoreFactory {
            store: op_store.clone(),
        });

        // Default config should be set after module overrides are done.
        let builder = builder.with_default_config().unwrap();
        // Then custom configuration is applied.
        builder.config.set_module_config(&self.config).unwrap();

        let builder = Arc::new(builder);

        let space_lock = Arc::new(tokio::sync::OnceCell::new());
        let tx_handler = TestTxHandler::create(
            space_lock.clone(),
            Arc::new(Ed25519Verifier),
        );

        let transport = builder
            .transport
            .create(builder.clone(), tx_handler)
            .await
            .unwrap();

        let report = builder
            .report
            .create(builder.clone(), transport.clone())
            .await
            .unwrap();

        let space = builder
            .space
            .create(
                builder.clone(),
                Arc::new(NoopHandler),
                self.space_id.clone(),
                report,
                transport.clone(),
            )
            .await
            .unwrap();

        space_lock
            .set(Arc::downgrade(&space))
            .expect("Failed to set space in tx handler");

        let peer_meta_store =
            K2PeerMetaStore::new(space.peer_meta_store().clone());

        let process_abort_handle = tokio::task::spawn({
            let op_store = op_store.clone();
            let space = space.clone();

            async move {
                loop {
                    tokio::time::sleep(Duration::from_millis(50)).await;

                    let new_ops = op_store
                        .read()
                        .await
                        .op_list
                        .iter()
                        .filter_map(|(op_id, op)| {
                            if !op.processed {
                                Some(StoredOp {
                                    op_id: op_id.clone(),
                                    created_at: op.created_at,
                                })
                            } else {
                                None
                            }
                        })
                        .collect::<Vec<_>>();

                    if new_ops.is_empty() {
                        continue;
                    }

                    space.inform_ops_stored(new_ops.clone()).await.unwrap();

                    let mut write_lock = op_store.write().await;
                    for op in new_ops {
                        if let Some(op) = write_lock.op_list.get_mut(&op.op_id)
                        {
                            op.processed = true;
                        }
                    }
                }
            }
        })
        .abort_handle();

        K2GossipFunctionalTestHarness {
            gossip: space.gossip().clone(),
            space,
            op_store,
            peer_meta_store: Arc::new(peer_meta_store),
            process_abort_handle,
        }
    }
}
