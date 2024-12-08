//! Fetch is a Kitsune2 module for fetching ops from peers.
//!
//! In particular it tracks which ops need to be fetched from which agents,
//! sends fetch requests and processes incoming responses to these requests.
//!
//! It consists of multiple parts:
//! - Data object that tracks op and agent ids in memory
//! - Fetch tasks that request tracked ops from agents
//! - Incoming op task that processes incoming responses to op requests by
//!     - persisting ops to the data store
//!     - removing op ids from in-memory data object
//!
//! ### Data object [CoreFetch]
//!
//! - Exposes public method [CoreFetch::add_ops] that takes a list of ops and an agent id.
//! - Stores pairs of ([OpId][AgentId]) in a set.
//!
//! ### Fetch tasks
//!
//! A channel acts as the queue structure for the fetch tasks. Ops to fetch are sent
//! one by one through the channel to the receiving tasks running in parallel. The flow
//! of sending fetch requests is as follows:
//!
//! - Await fetch requests for ([OpId], [AgentId]) from an internal channel.
//! - Check if requested op id/agent id is still on the list of ops to fetch.
//!     - In case the op has been received in the meantime and no longer needs to be fetched,
//! do nothing.
//!     - Otherwise proceed.
//! - Check if agent is on a cool-down list of unresponsive agents.
//! - Dispatch request for op id from agent to transport module.
//! - If agent is unresponsive, put them on cool-down list.
//! - Re-send requested ([OpId], [AgentId]) to the internal channel again. It will be removed
//! from the list of ops to fetch if it is received in the meantime, and thus prevent a redundant
//! fetch request.
//!
//! ### Incoming op task
//!
//! - Incoming op is written to the data store.
//! - Once persisted successfully, op is removed from the data object.

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::{Duration, Instant},
};

use kitsune2_api::{
    builder,
    config::ModConfig,
    fetch::{DynFetch, DynFetchFactory, Fetch, FetchFactory},
    tx::Transport,
    AgentId, BoxFut, K2Error, K2Result, OpId,
};
use tokio::{select, sync::mpsc::Sender};

const MOD_NAME: &str = "Fetch";

/// Configuration parameters for [CoreFetchFactory].
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CoreFetchConfig {
    /// How many parallel op fetch requests can be made at once. Default: 2.  
    pub parallel_request_count: u8,
    /// Duration in ms to sleep when idle. Default: 1_000.
    pub fetch_loop_sleep: u64,
    /// Duration in ms to keep an unresponsive agent on the cool-down list. Default: 10_000.
    pub cool_down_interval: u64,
}

impl Default for CoreFetchConfig {
    fn default() -> Self {
        Self {
            parallel_request_count: 2,
            fetch_loop_sleep: 1_000,
            cool_down_interval: 10_000,
        }
    }
}

impl ModConfig for CoreFetchConfig {}

// TODO: Temporary trait object of a transport module to facilitate unit tests.
type DynTransport = Arc<tokio::sync::Mutex<dyn Transport>>;

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
            // Add ops to set.
            let mut ops = self.0.ops.lock().await;
            op_list.clone().into_iter().for_each(|op_id| {
                ops.insert((op_id, source.clone()));
            });
            drop(ops);

            // Pass ops to fetch tasks.
            let futures = op_list.into_iter().map(|op_id| async {
                if let Err(err) =
                    self.0.fetch_request_tx.send((op_id, source.clone())).await
                {
                    eprintln!(
                        "could not pass fetch request to fetch task: {err}"
                    );
                }
            });
            futures::future::join_all(futures).await;

            Ok(())
        })
    }
}

#[derive(Debug)]
struct Inner {
    config: CoreFetchConfig,
    // A hash set` is used to look up elements by key efficiently. Ops may be added redundantly
    // to the set with different sources to fetch from, so the set is keyed by op and agent id together.
    ops: Arc<tokio::sync::Mutex<HashSet<(OpId, AgentId)>>>,
    cool_down_list: Arc<tokio::sync::Mutex<HashMap<AgentId, Instant>>>,
    fetch_request_tx: tokio::sync::mpsc::Sender<(OpId, AgentId)>,
}

impl Inner {
    pub fn spawn_fetch_task(
        config: CoreFetchConfig,
        transport: DynTransport,
    ) -> Self {
        // Create a channel to send new ops to fetch to the tasks.
        let (fetch_request_tx, fetch_request_rx) =
            tokio::sync::mpsc::channel::<(OpId, AgentId)>(1024);
        let fetch_request_rx =
            Arc::new(tokio::sync::Mutex::new(fetch_request_rx));

        let inner = Self {
            config: config.clone(),
            ops: Arc::new(tokio::sync::Mutex::new(HashSet::new())),
            cool_down_list: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            fetch_request_tx: fetch_request_tx.clone(),
        };

        for i in 0..config.parallel_request_count {
            tokio::task::spawn({
                let fetch_request_tx = fetch_request_tx.clone();
                let fetch_request_rx = fetch_request_rx.clone();
                let transport = transport.clone();
                let ops = inner.ops.clone();
                async move {
                    while let Some((op_id, agent_id)) =
                        fetch_request_rx.lock().await.recv().await
                    {
                        // Ensure op is still in the set of ops to fetch.
                        if !ops
                            .lock()
                            .await
                            .contains(&(op_id.clone(), agent_id.clone()))
                        {
                            continue;
                        }

                        // Send fetch request to agent.
                        // If successful, re-insert the fetch request into the channel queue.
                        println!("task {i} fetch request coming in for op {op_id} to agent {agent_id}");
                        if let Err(err) = transport
                            .lock()
                            .await
                            .send_op_request(op_id.clone(), agent_id.clone())
                            .await
                        {
                            eprintln!("could not send fetch request for op {op_id} to agent {agent_id}: {err}");
                        } else if let Err(err) = fetch_request_tx
                            .send((op_id.clone(), agent_id.clone()))
                            .await
                        {
                            eprintln!("could not re-insert fetch request for op {op_id} to agent {agent_id} in queue: {err}");
                        }
                    }
                }
            });
        }

        inner

        //                     // Remove agents from cool-down list after configured interval has elapsed.
        //                     inner.purge_cool_down_list(Instant::now()).await;

        //                     // Do not send request to agent on cool-down list.
        //                     if !inner
        //                         .cool_down_list
        //                         .lock()
        //                         .await
        //                         .contains_key(&agent_id)
        //                     {
        //                     // Push op and source back to the end of the queue.
        //                     inner
        //                         .ops
        //                         .lock()
        //                         .await
        //                         .insert((op_id, agent_id), ());
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
        let tx = Arc::new(tokio::sync::Mutex::new(TransportPlaceholder));

        Box::pin(async move {
            let config = builder.config.get_module_config(MOD_NAME)?;
            let out: DynFetch = Arc::new(CoreFetch::new(config, tx));
            Ok(out)
        })
    }
}

#[cfg(test)]
mod test;
