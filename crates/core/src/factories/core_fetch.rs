//! Fetch is a Kitsune2 module for fetching ops from peers.
//!
//! In particular it tracks which ops need to be fetched from which agents,
//! sends fetch requests and processes incoming responses to these requests.
//!
//! It consists of multiple parts:
//! - Data object that tracks op and agent ids in memory
//! - Fetch task that requests tracked ops from agents
//! - Incoming op task that processes incoming responses to op requests by
//!     - persisting ops to the data store
//!     - removing op ids from in-memory data object
//!
//! ### Data object [`CoreFetch`]
//!
//! - Exposes public method [`CoreFetch::add_ops`] that takes a list of ops and an agent id.
//! - Stores pairs of ([`OpId`][`AgentId`]) in the order that ops have been added.
//!
//! ### Fetch task
//!
//! - Toeks pairs of ([`OpId`], [`AgentId`]) from the beginning of the queue and requests the op from the agent.
//! - Appends removed pair to the end of the queue.
//! - Put unresponsive peers on a cool-down list.
//!
//! ### Incoming op task
//!
//! - Incoming op is written to the data store.
//! - Once persisted successfully, op is removed from the data object.

use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};

use indexmap::IndexMap;
use kitsune2_api::{
    builder,
    config::ModConfig,
    fetch::{DynFetch, DynFetchFactory, Fetch, FetchFactory},
    tx::Transport,
    AgentId, BoxFut, K2Result, OpId,
};
use tokio::{
    select,
    sync::{mpsc::Sender, Mutex},
};

const MOD_NAME: &str = "Fetch";

/// Configuration parameters for [CoreFetchFactory].
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CoreFetchConfig {
    /// How many parallel op fetch requests can be made at once. Default: 2.  
    parallel_request_count: u8,
    /// Duration in ms to pause when parallel request count is reached. Default: 10.
    parallel_request_pause: u64,
    /// Duration in ms to sleep when idle. Default: 1_000.
    fetch_loop_sleep: u64,
    /// Duration in ms to keep an unresponsive agent on the cool-down list. Default: 10_000.
    cool_down_interval: u64,
}

impl Default for CoreFetchConfig {
    fn default() -> Self {
        Self {
            parallel_request_count: 2,
            parallel_request_pause: 10,
            fetch_loop_sleep: 1_000,
            cool_down_interval: 10_000,
        }
    }
}

impl ModConfig for CoreFetchConfig {}

impl CoreFetchConfig {
    /// Get parallel request count.
    pub fn parallel_request_count(&self) -> u8 {
        self.parallel_request_count
    }
    /// Get parallel request pause interval.
    pub fn parallel_request_pause(&self) -> u64 {
        self.parallel_request_pause
    }
    /// Get fetch loop pause interval.
    pub fn fetch_loop_pause(&self) -> u64 {
        self.fetch_loop_sleep
    }
    /// Get cool-down interval.
    pub fn cool_down_interval(&self) -> u64 {
        self.cool_down_interval
    }
}

// TODO: Temporary trait object of a transport module to facilitate unit tests.
type DynTransport = Arc<Mutex<dyn Transport>>;

#[derive(Debug)]
struct CoreFetch(Inner);

impl CoreFetch {
    fn new(config: CoreFetchConfig, transport: DynTransport) -> Self {
        Self(Inner::spawn_fetch_task(config, transport))
    }
}

impl Fetch for CoreFetch {
    fn add_ops(
        &mut self,
        op_list: Vec<OpId>,
        source: AgentId,
    ) -> BoxFut<'_, K2Result<()>> {
        Box::pin(async move {
            // Add ops to queue.
            let mut ops = self.0.ops.lock().await;
            op_list.clone().into_iter().for_each(|op_id| {
                ops.insert((op_id, source.clone()), ());
            });
            drop(ops);

            // Wake up fetch task in case it is sleeping. Time out if the channel buffer
            // is full.
            let _ = tokio::time::timeout(
                Duration::from_millis(5),
                self.0.fetch_request_tx.send(()),
            )
            .await;

            Ok(())
        })
    }
}

#[derive(Clone, Debug)]
struct Inner {
    config: CoreFetchConfig,
    // An `IndexMap` is used to retain order of insertion and have the ability to look up elements
    // by key efficiently. Ops may be added redundantly to the map with different sources to fetch from, so
    // the map is keyed by op and agent id together.
    // The value field could be used to track number of attempts for example.
    ops: Arc<Mutex<IndexMap<(OpId, AgentId), ()>>>,
    current_request_count: Arc<Mutex<u8>>,
    cool_down_list: Arc<Mutex<HashMap<AgentId, Instant>>>,
    // A sender to wake up a sleeping fetch task.
    fetch_request_tx: Sender<()>,
}

impl Inner {
    pub fn spawn_fetch_task(
        config: CoreFetchConfig,
        transport: DynTransport,
    ) -> Self {
        // Create a channel to wake up sleeping loop.
        let (fetch_request_tx, mut fetch_request_rx) =
            tokio::sync::mpsc::channel::<()>(50);

        let inner = Self {
            config,
            ops: Arc::new(Mutex::new(IndexMap::new())),
            current_request_count: Arc::new(Mutex::new(0)),
            cool_down_list: Arc::new(Mutex::new(HashMap::new())),
            fetch_request_tx,
        };

        tokio::spawn({
            let current_request_count = inner.current_request_count.clone();
            let inner = inner.clone();
            let transport = transport.clone();

            async move {
                let CoreFetchConfig {
                    parallel_request_count,
                    parallel_request_pause,
                    fetch_loop_sleep,
                    ..
                } = inner.config;

                // Loop for processing op/agent pairs in the queue. Runs continuously and is paused
                // briefly when the maximum of parallel requests is reached, and sleeps for a
                // longer period when the queue is empty. It is awoken from sleep through a channel
                // when new op ids are added to the queue.
                loop {
                    // Send new request if parallel request count is not reached.
                    if parallel_request_count
                        > (*current_request_count.lock().await)
                    {
                        let inner = inner.clone();

                        // Take first element from the start of the queue and release lock on queue.
                        let first_elem =
                            inner.ops.lock().await.shift_remove_index(0);
                        if let Some(((op_id, agent_id), ())) = first_elem {
                            // Remove agents from cool-down list after configured interval has elapsed.
                            inner.purge_cool_down_list(Instant::now()).await;

                            // Do not send request to agent on cool-down list.
                            if !inner
                                .cool_down_list
                                .lock()
                                .await
                                .contains_key(&agent_id)
                            {
                                // Register new request, increase request count.
                                (*current_request_count.lock().await) += 1;

                                // Make new request in a separate task.
                                tokio::spawn({
                                    let inner = inner.clone();
                                    let op_id = op_id.clone();
                                    let agent_id = agent_id.clone();
                                    let transport = transport.clone();
                                    let current_request_count =
                                        current_request_count.clone();
                                    async move {
                                        let result = transport
                                            .lock()
                                            .await
                                            .send_op_request(
                                                op_id.clone(),
                                                agent_id.clone(),
                                            )
                                            .await;

                                        // Request is complete, decrease request count.
                                        (*current_request_count
                                            .lock()
                                            .await) -= 1;

                                        if let Err(err) = result {
                                            eprintln!("could not send fetch request for op id {op_id} to agent {agent_id}: {err}");
                                            inner
                                                .cool_down_list
                                                .lock()
                                                .await
                                                .insert(
                                                    agent_id,
                                                    Instant::now(),
                                                );
                                        }
                                    }
                                });
                            }

                            // Pause to give other requests a chance to be processed before re-adding this op.
                            tokio::time::sleep(Duration::from_millis(1)).await;

                            // Push op and source back to the end of the queue.
                            inner
                                .ops
                                .lock()
                                .await
                                .insert((op_id, agent_id), ());
                        }
                    }

                    // Pause if maximum parallel request count is reached.
                    if (*current_request_count.lock().await)
                        == parallel_request_count
                    {
                        tokio::time::sleep(Duration::from_millis(
                            parallel_request_pause,
                        ))
                        .await;
                    } else if inner.ops.lock().await.is_empty() {
                        // Sleep if there are no more ops to fetch.
                        // Sleep is canceled by newly added ops.
                        select! {
                            _ = tokio::time::sleep(Duration::from_millis(
                                fetch_loop_sleep,
                            )) => {},
                            _ = fetch_request_rx.recv() => {}
                        }
                    }
                }
            }
        });

        inner
    }

    fn purge_cool_down_list(&self, now: Instant) -> BoxFut<'_, ()> {
        Box::pin(async move {
            let mut list = self.cool_down_list.lock().await;
            list.retain(|_, instant| {
                (now - *instant).as_millis()
                    <= self.config.cool_down_interval as u128
            });
        })
    }
}

/// A production-ready fetch module.
#[derive(Debug)]
pub struct CoreFetchFactory {}

impl CoreFetchFactory {
    /// Construct a new CoreFetchFactory.
    pub fn create() -> DynFetchFactory {
        Arc::new(Self {})
    }
}

impl FetchFactory for CoreFetchFactory {
    fn default_config(
        &self,
        config: &mut kitsune2_api::config::Config,
    ) -> K2Result<()> {
        config.add_default_module_config::<CoreFetchConfig>(
            MOD_NAME.to_string(),
        )?;
        Ok(())
    }

    fn create(
        &self,
        builder: Arc<builder::Builder>,
    ) -> BoxFut<'static, K2Result<DynFetch>> {
        #[derive(Debug)]
        struct TransportPlaceholder;
        impl Transport for TransportPlaceholder {
            fn send_op_request(
                &mut self,
                _op_id: OpId,
                _source: AgentId,
            ) -> BoxFut<'static, K2Result<()>> {
                Box::pin(async move { todo!() })
            }
        }
        let tx = Arc::new(Mutex::new(TransportPlaceholder));

        Box::pin(async move {
            let config = builder.config.get_module_config(MOD_NAME)?;
            let out: DynFetch = Arc::new(CoreFetch::new(config, tx));
            Ok(out)
        })
    }
}

#[cfg(test)]
mod test;
