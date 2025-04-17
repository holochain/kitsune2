//! Functional test harness for K2Gossip.

use crate::peer_meta_store::K2PeerMetaStore;
use crate::{K2GossipConfig, K2GossipFactory, K2GossipModConfig};
use bytes::Bytes;
use kitsune2_api::{
    AgentId, AgentInfoSigned, BoxFut, Builder, Config, DhtArc, DynGossip,
    DynOpStore, DynSpace, K2Error, K2Result, LocalAgent, MetaOp, OpId, OpStore,
    OpStoreFactory, SpaceHandler, SpaceId, StoredOp, Timestamp, TxBaseHandler,
    TxHandler, TxSpaceHandler, UNIX_TIMESTAMP,
};
use kitsune2_core::factories::{
    Kitsune2MemoryOpStore, MemoryOp, MemoryOpRecord,
};
use kitsune2_core::{default_test_builder, Ed25519LocalAgent};
use kitsune2_test_utils::noop_bootstrap::NoopBootstrapFactory;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

/// A functional test harness for K2Gossip.
///
/// Create instances of this using the [`K2GossipFunctionalTestFactory`].
#[derive(Debug, Clone)]
pub struct K2GossipFunctionalTestHarness {
    /// The inner gossip instance.
    pub gossip: DynGossip,
    /// The space instance.
    pub space: DynSpace,
    /// The peer meta store.
    pub peer_meta_store: Arc<K2PeerMetaStore>,
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
                while !peer_store
                    .get_all()
                    .await
                    .unwrap()
                    .iter()
                    .any(|a| a.agent.clone() == agent_id)
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
    /// The builder for creating new instances.
    builder: Arc<Builder>,
    /// A sender for ops stored events.
    ///
    /// This must not be used for sending. If it is provided then the K2 mem op store is in use
    /// and that will send ops through this channel. Use the sender to create new listeners so
    /// that each space can update its DHT.
    ops_stored_sender: Option<tokio::sync::broadcast::Sender<Vec<StoredOp>>>,
}

impl K2GossipFunctionalTestFactory {
    /// Create a new functional test factory.
    pub async fn create(
        space: SpaceId,
        bootstrap: bool,
        gossip_mem_op_store: bool,
        config: Option<K2GossipConfig>,
    ) -> K2GossipFunctionalTestFactory {
        let mut builder = default_test_builder().with_default_config().unwrap();
        // Replace the core builder with a real gossip factory
        builder.gossip = K2GossipFactory::create();

        if !bootstrap {
            builder.bootstrap = Arc::new(NoopBootstrapFactory);
        }

        let ops_stored_sender = if gossip_mem_op_store {
            let (tx, _) =
                tokio::sync::broadcast::channel::<Vec<StoredOp>>(5000);
            builder.op_store = Arc::new(K2GossipMemOpStoreFactory {
                ops_stored: tx.clone(),
            });

            Some(tx)
        } else {
            None
        };

        builder
            .config
            .set_module_config(&K2GossipModConfig {
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
            })
            .unwrap();

        let builder = Arc::new(builder);

        K2GossipFunctionalTestFactory {
            space_id: space,
            builder,
            ops_stored_sender,
        }
    }

    /// Create a new instance of the test harness.
    pub async fn new_instance(&self) -> K2GossipFunctionalTestHarness {
        #[derive(Debug)]
        struct NoopHandler;
        impl TxBaseHandler for NoopHandler {}
        impl TxHandler for NoopHandler {}
        impl TxSpaceHandler for NoopHandler {}
        impl SpaceHandler for NoopHandler {}

        let transport = self
            .builder
            .transport
            .create(self.builder.clone(), Arc::new(NoopHandler))
            .await
            .unwrap();

        let space = self
            .builder
            .space
            .create(
                self.builder.clone(),
                Arc::new(NoopHandler),
                self.space_id.clone(),
                transport.clone(),
            )
            .await
            .unwrap();

        let peer_meta_store =
            K2PeerMetaStore::new(space.peer_meta_store().clone());

        if let Some(ref sender) = self.ops_stored_sender {
            let space = space.clone();
            let mut receiver = sender.subscribe();
            tokio::spawn(async move {
                tracing::info!("Starting ops stored space informer");
                while let Ok(op_ids) = receiver.recv().await {
                    space.inform_ops_stored(op_ids).await.unwrap();
                }

                tracing::info!("Ops stored sender closed");
            });
        }

        K2GossipFunctionalTestHarness {
            gossip: space.gossip().clone(),
            space,
            peer_meta_store: Arc::new(peer_meta_store),
        }
    }
}

/// A factory for creating [K2GossipMemOpStore] instances.
#[derive(Debug)]
pub struct K2GossipMemOpStoreFactory {
    /// The sender for ops stored events.
    pub ops_stored: tokio::sync::broadcast::Sender<Vec<StoredOp>>,
}

impl OpStoreFactory for K2GossipMemOpStoreFactory {
    fn default_config(&self, _config: &mut Config) -> K2Result<()> {
        Ok(())
    }

    fn validate_config(&self, _config: &Config) -> K2Result<()> {
        Ok(())
    }

    fn create(
        &self,
        _builder: Arc<Builder>,
        space: SpaceId,
    ) -> BoxFut<'static, K2Result<DynOpStore>> {
        let sender = self.ops_stored.clone();
        Box::pin(async move {
            let op_store: DynOpStore =
                Arc::new(K2GossipMemOpStore::create(space, sender));
            Ok(op_store)
        })
    }
}

/// A K2Gossip specialization of the [Kitsune2MemoryOpStore].
///
/// This implementation deliberately breaks the op store contract by storing ops with their
/// "stored at" time set to the "create at" time of the op. This allows gossip tests to create
/// historical data that won't immediately be synced by the "what's new" mechanism.
///
/// The other behaviour change is that this op store can automatically notify the space when ops
/// are stored, os the DHT model can be updated immediately.
#[derive(Debug)]
pub struct K2GossipMemOpStore {
    inner: Kitsune2MemoryOpStore,
    ops_stored: tokio::sync::broadcast::Sender<Vec<StoredOp>>,
}

impl K2GossipMemOpStore {
    /// Create a new K2GossipMemOpStore.
    pub fn create(
        space_id: SpaceId,
        ops_stored: tokio::sync::broadcast::Sender<Vec<StoredOp>>,
    ) -> Self {
        Self {
            inner: Kitsune2MemoryOpStore::new(space_id),
            ops_stored,
        }
    }
}

impl OpStore for K2GossipMemOpStore {
    fn process_incoming_ops(
        &self,
        op_list: Vec<Bytes>,
    ) -> BoxFut<'_, K2Result<Vec<OpId>>> {
        Box::pin(async move {
            let ops_to_add = op_list
                .iter()
                .map(|op| -> serde_json::Result<(OpId, MemoryOpRecord)> {
                    let mut op = MemoryOpRecord::from(op.clone());

                    // Behaviour difference from the inner implementation.
                    op.stored_at = op.created_at;

                    Ok((op.op_id.clone(), op))
                })
                .collect::<Result<Vec<_>, _>>().map_err(|e| {
                K2Error::other_src("Failed to deserialize op data, are you using `Kitsune2MemoryOp`s?", e)
            })?;

            let mut op_ids = Vec::with_capacity(ops_to_add.len());
            let mut newly_stored = Vec::with_capacity(ops_to_add.len());
            let mut lock = self.inner.write().await;
            for (op_id, record) in ops_to_add {
                lock.op_list.entry(op_id.clone()).or_insert_with(|| {
                    newly_stored.push(StoredOp {
                        op_id: op_id.clone(),
                        created_at: record.created_at,
                    });
                    record
                });
                op_ids.push(op_id);
            }

            // Notify the space that the ops have been stored.
            // This is also a deviation from the inner implementation.
            if !newly_stored.is_empty() {
                if let Err(err) = self.ops_stored.send(newly_stored) {
                    tracing::warn!(
                        ?err,
                        "Failed to send ops stored notification"
                    );
                }
            }

            Ok(op_ids)
        })
    }

    fn retrieve_op_hashes_in_time_slice(
        &self,
        arc: DhtArc,
        start: Timestamp,
        end: Timestamp,
    ) -> BoxFut<'_, K2Result<(Vec<OpId>, u32)>> {
        self.inner.retrieve_op_hashes_in_time_slice(arc, start, end)
    }

    fn retrieve_ops(
        &self,
        op_ids: Vec<OpId>,
    ) -> BoxFut<'_, K2Result<Vec<MetaOp>>> {
        self.inner.retrieve_ops(op_ids)
    }

    fn filter_out_existing_ops(
        &self,
        op_ids: Vec<OpId>,
    ) -> BoxFut<'_, K2Result<Vec<OpId>>> {
        self.inner.filter_out_existing_ops(op_ids)
    }

    fn retrieve_op_ids_bounded(
        &self,
        arc: DhtArc,
        start: Timestamp,
        limit_bytes: u32,
    ) -> BoxFut<'_, K2Result<(Vec<OpId>, u32, Timestamp)>> {
        self.inner.retrieve_op_ids_bounded(arc, start, limit_bytes)
    }

    fn earliest_timestamp_in_arc(
        &self,
        arc: DhtArc,
    ) -> BoxFut<'_, K2Result<Option<Timestamp>>> {
        self.inner.earliest_timestamp_in_arc(arc)
    }

    fn store_slice_hash(
        &self,
        arc: DhtArc,
        slice_index: u64,
        slice_hash: Bytes,
    ) -> BoxFut<'_, K2Result<()>> {
        self.inner.store_slice_hash(arc, slice_index, slice_hash)
    }

    fn slice_hash_count(&self, arc: DhtArc) -> BoxFut<'_, K2Result<u64>> {
        self.inner.slice_hash_count(arc)
    }

    fn retrieve_slice_hash(
        &self,
        arc: DhtArc,
        slice_index: u64,
    ) -> BoxFut<'_, K2Result<Option<Bytes>>> {
        self.inner.retrieve_slice_hash(arc, slice_index)
    }

    fn retrieve_slice_hashes(
        &self,
        arc: DhtArc,
    ) -> BoxFut<'_, K2Result<Vec<(u64, Bytes)>>> {
        self.inner.retrieve_slice_hashes(arc)
    }
}
