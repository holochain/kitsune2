//! Fetch is a Kitsune2 module for fetching ops from peers.
//!
//! In particular it tracks which ops need to be fetched from which agents,
//! sends fetch requests and processes incoming responses to these requests.
//! It consists of multiple parts:
//! - Data object that tracks op and agent ids in memory
//! - Fetch task that requests tracked ops from agents
//! - Incoming op task that processes incoming responses to op requests by
//!     - persisting ops to the data store
//!     - removing op ids from in-memory data object
//!
//! ### Data object [`Kitsune2Fetch`]
//!
//! - Exposes public method [`Kitsune2Fetch::add_ops`] that takes a list of ops and an agent id.
//! - Stores pairs of (op id, agent id) in the order that ops have been added.
//!
//! ### Fetch task
//!
//! - Pops pairs of (op id, agent id) pairs from the beginning of the queue and requests the op from the agent.
//! - Appends popped element to the end of the queue.
//! - Put unresponsive peers on a back-off list.
//!
//! ### Incoming op task
//!
//! - Incoming op is written to the data store.
//! - Once persisted successfully, op is removed from the data object.

use std::{sync::Arc, time::Duration};

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

/// Configuration parameters for [Q]
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Kitsune2FetchConfig {
    /// How many parallel op fetch requests can be made at once. Default: 2.  
    parallel_request_count: u8,
    /// Duration in ms to pause when parallel request count is reached. Default: 10.
    parallel_request_pause: u64,
    /// Duration in ms to sleep when idle. Default: 1000.
    fetch_loop_sleep: u64,
}

impl Default for Kitsune2FetchConfig {
    fn default() -> Self {
        Self {
            parallel_request_count: 2,
            parallel_request_pause: 10,
            fetch_loop_sleep: 1000,
        }
    }
}

impl ModConfig for Kitsune2FetchConfig {}

impl Kitsune2FetchConfig {
    pub fn parallel_request_count(&self) -> u8 {
        self.parallel_request_count
    }
    pub fn parallel_request_pause(&self) -> u64 {
        self.parallel_request_pause
    }
    pub fn fetch_loop_pause(&self) -> u64 {
        self.fetch_loop_sleep
    }
}

// TODO: Temporary trait object of a transport module to facilitate unit tests.
type DynTransport = Arc<Mutex<dyn Transport>>;

#[derive(Debug)]
struct Kitsune2Fetch(Inner);

impl Kitsune2Fetch {
    fn new(config: Kitsune2FetchConfig, transport: DynTransport) -> Self {
        Self(Inner::spawn_fetch_task(config, transport))
    }
}

impl Fetch for Kitsune2Fetch {
    fn add_ops(
        &mut self,
        op_list: Vec<OpId>,
        source: AgentId,
    ) -> BoxFut<'_, K2Result<()>> {
        Box::pin(async move {
            // Add ops to map.
            let mut ops = self.0.ops.lock().await;
            op_list.clone().into_iter().for_each(|op_id| {
                ops.insert((op_id, source.clone()), ());
            });
            drop(ops);

            // Wake up fetch task in case it is sleeping.
            // self.0
            //     .fetch_request_tx
            //     .send(())
            //     .await
            //     .map_err(|err| K2Error::other(err))?;
            Ok(())
        })
    }
}

#[derive(Clone, Debug)]
struct Inner {
    config: Kitsune2FetchConfig,
    // An `IndexMap` is used to retain order of insertion and have the ability to look up elements
    // by key efficiently. Ops may be added redundantly to the map with different sources to fetch from, so
    // the map is keyed by op and agent id together.
    // The value field could be used to track number of attempts for example.
    ops: Arc<Mutex<IndexMap<(OpId, AgentId), ()>>>,
    current_request_count: Arc<Mutex<u8>>,
    // A sender to wake up a sleeping fetch task.
    fetch_request_tx: Sender<()>,
}

impl Inner {
    pub fn spawn_fetch_task(
        config: Kitsune2FetchConfig,
        transport: DynTransport,
    ) -> Self {
        // Create a channel to wake up sleeping loop.
        let (fetch_request_tx, mut fetch_request_rx) =
            tokio::sync::mpsc::channel::<()>(1);

        let inner = Self {
            config,
            ops: Arc::new(Mutex::new(IndexMap::new())),
            current_request_count: Arc::new(Mutex::new(0)),
            fetch_request_tx,
        };

        tokio::spawn({
            let current_request_count = inner.current_request_count.clone();
            let inner = inner.clone();

            async move {
                let Kitsune2FetchConfig {
                    parallel_request_count,
                    parallel_request_pause,
                    fetch_loop_sleep: fetch_loop_pause,
                } = inner.config;

                loop {
                    // Send new request if parallel request count is not reached.
                    if parallel_request_count
                        > (*current_request_count.lock().await)
                    {
                        // Pop first element from the start of the queue and release lock on map.
                        let first_elem =
                            inner.ops.lock().await.shift_remove_index(0);
                        if let Some(((op_id, agent_id), ())) = first_elem {
                            // Register new request.
                            (*inner.current_request_count.lock().await) += 1;

                            // Make new request in a separate task.
                            tokio::spawn({
                                let tx = transport.clone();
                                let op_id = op_id.clone();
                                let agent_id = agent_id.clone();
                                let current_request_count =
                                    inner.current_request_count.clone();
                                let ops = inner.ops.clone();
                                async move {
                                    let _ = tx
                                        .lock()
                                        .await
                                        .send_op_request(
                                            op_id.clone(),
                                            agent_id.clone(),
                                        )
                                        .await;

                                    // Request is complete, reduce request count.
                                    (*current_request_count.lock().await) -= 1;

                                    // Push op and source back to the end of the queue.
                                    ops.lock()
                                        .await
                                        .insert((op_id, agent_id), ());
                                }
                            });
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
                                fetch_loop_pause,
                            )) => {},
                            _ = fetch_request_rx.recv() => {}
                        }
                    }
                }
            }
        });

        inner
    }
}

#[derive(Debug)]
pub struct Kitsune2FetchFactory {}

impl Kitsune2FetchFactory {
    pub fn create() -> DynFetchFactory {
        Arc::new(Self {})
    }
}

impl FetchFactory for Kitsune2FetchFactory {
    fn default_config(
        &self,
        config: &mut kitsune2_api::config::Config,
    ) -> K2Result<()> {
        config.add_default_module_config::<Kitsune2FetchConfig>(
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
            let out: DynFetch = Arc::new(Kitsune2Fetch::new(config, tx));
            Ok(out)
        })
    }
}

#[cfg(test)]
mod test;
