//! The fetch queue is a Kitsune2 module for tracking ops that need to be fetched from peers
//! and sending fetch requests to them.
//!
//! The fetch queue holds sets of ops and sources where to fetch ops from. Sending fetch
//! requests to peers is executed by a fetch task.
//!
//! Purpose: Keep track of ops to be fetched from which agents, send fetch requests and process incoming fetch responses.

//! - Fetch data store which stores ops
//! - Task that requests ops from agents
//! - Task that processes incoming responses to these requests
//!     - persist ops into data store
//!     - remove op hashes from in-memory data store
//!
//! Components:
//! - Fetch queue: Data structure to record ops by hash
//! - Fetch task: To poll the fetch queue and do fetch requests
//! ~~- Fetch response queue: To manage parallel incoming fetch requests and getting op data to respond with~~
//! ~~- Fetch response task: Reads the response queue~~
//!
//! ### Fetch data object
//!
//! - Each op hash has a list of agents who have told us they are holding that data.
//! - There is one public method `add_ops` which takes a list of ops and a source (agent id).
//! - New ops are added to the end of the queue.
//!
//! ### Fetch task
//!
//! First iteration method:
//! - Pop individual op/agent pairs from the queue and send a request for them
//! - Append popped item to the end of the queue
//! - Put unresponsive peers on a backoff/cool-down list
//! - incoming op is written to the data store
//! - once persisted successfully, op is removed from the fetch data object

use std::{sync::Arc, time::Duration};

use indexmap::IndexMap;
use kitsune2_api::{
    builder,
    config::ModConfig,
    fetch::{DynFetch, DynFetchFactory, Fetch, FetchFactory},
    tx::Transport,
    AgentId, BoxFut, K2Error, K2Result, OpId,
};
use tokio::{
    select,
    sync::{mpsc::Sender, Mutex},
};

const MOD_NAME: &str = "Fetch";

/// Configuration parameters for [Q]
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FetchConfig {
    /// How many parallel op fetch requests can be made at once. Default: 2.  
    parallel_request_count: u8,
    /// Duration in ms to sleep when idle. Default: 1000.
    fetch_loop_sleep: u64,
}

impl Default for FetchConfig {
    fn default() -> Self {
        Self {
            parallel_request_count: 2,
            fetch_loop_sleep: 1000, // in ms
        }
    }
}

impl ModConfig for FetchConfig {}

impl FetchConfig {
    pub fn parallel_request_count(&self) -> u8 {
        self.parallel_request_count
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
    fn new(config: FetchConfig, transport: DynTransport) -> Self {
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
            self.0
                .fetch_request_tx
                .send(())
                .await
                .map_err(|err| K2Error::other(err))?;
            Ok(())
        })
    }
}

#[derive(Clone, Debug)]
struct Inner {
    config: FetchConfig,
    // An `IndexMap` is used to retain order of insertion and have the ability to look up elements
    // by key. Ops may be added redundantly to the map with different sources to fetch from, so
    // the map is keyed by op and agent id together.
    // The value field could be used to track number of attempts for example.
    ops: Arc<Mutex<IndexMap<(OpId, AgentId), ()>>>,
    current_request_count: Arc<Mutex<u8>>,
    // A sender to wake up a sleeping fetch task.
    fetch_request_tx: Sender<()>,
}

impl Inner {
    pub fn spawn_fetch_task(
        config: FetchConfig,
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
                let FetchConfig {
                    parallel_request_count,
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
                            tokio::spawn({
                                let tx = transport.clone();
                                let op_id = op_id.clone();
                                let agent_id = agent_id.clone();
                                let current_request_count =
                                    inner.current_request_count.clone();
                                async move {
                                    let result = tx
                                        .lock()
                                        .await
                                        .send_op_request(op_id, agent_id)
                                        .await;
                                    println!("result from thread after sending request {result:?}");

                                    // Request is complete, reduce request count.
                                    (*current_request_count.lock().await) -= 1;
                                }
                            });

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
                        tokio::time::sleep(Duration::from_millis(10)).await;
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
        config
            .add_default_module_config::<FetchConfig>(MOD_NAME.to_string())?;
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
