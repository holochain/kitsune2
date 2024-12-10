//! Fetch is a Kitsune2 module for fetching ops from peers.
//!
//! In particular it tracks which ops need to be fetched from which agents,
//! sends fetch requests and processes incoming responses to these requests.
//!
//! It consists of multiple parts:
//! - State object that tracks op and agent ids in memory
//! - Fetch tasks that request tracked ops from agents
//! - Incoming op task that processes incoming responses to op requests by
//!     - persisting ops to the data store
//!     - removing op ids from in-memory data object
//!
//! ### State object [CoreFetch]
//!
//! - Exposes public method [CoreFetch::add_ops] that takes a list of op ids and an agent id.
//! - Stores pairs of ([OpId][AgentId]) in a set.
//! - A hash set is used to look up elements by key efficiently. Ops may be added redundantly
//!   to the set with different sources to fetch from, so the set is keyed by op and agent id together.
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
//!       do nothing.
//!     - Otherwise proceed.
//! - Check if agent is on a cool-down list of unresponsive agents.
//! - Dispatch request for op id from agent to transport module.
//! - If agent is unresponsive, put them on cool-down list.
//! - Re-send requested ([OpId], [AgentId]) to the internal channel again. It will be removed
//!   from the list of ops to fetch if it is received in the meantime, and thus prevent a redundant
//!   fetch request.
//!
//! ### Incoming op task
//!
//! - Incoming op is written to the data store.
//! - Once persisted successfully, op is removed from the data object.

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::Instant,
};

use kitsune2_api::{
    builder,
    config::ModConfig,
    fetch::{DynFetch, DynFetchFactory, Fetch, FetchFactory},
    tx::Transport,
    AgentId, BoxFut, K2Result, OpId,
};
use tokio::sync::{
    mpsc::{channel, Receiver, Sender},
    Mutex,
};

const MOD_NAME: &str = "Fetch";

/// Configuration parameters for [CoreFetchFactory].
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CoreFetchConfig {
    /// How many parallel op fetch requests can be made at once. Default: 2.  
    pub parallel_request_count: u8,
    /// Duration in ms to keep an unresponsive agent on the cool-down list. Default: 10_000.
    pub cool_down_interval_ms: u64,
}

impl Default for CoreFetchConfig {
    fn default() -> Self {
        Self {
            parallel_request_count: 2,
            cool_down_interval_ms: 10_000,
        }
    }
}

impl ModConfig for CoreFetchConfig {}

// TODO: Temporary trait object of a transport module to facilitate unit tests.
type DynTransport = Arc<Mutex<dyn Transport>>;

#[derive(Debug)]
struct CoreFetch(Inner);

impl CoreFetch {
    fn new(config: CoreFetchConfig, transport: DynTransport) -> Self {
        let inner = Inner::new();
        inner.create_fetch_queue(config, transport);
        Self(inner)
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
            let ops = &mut self.0.state.lock().await.ops;
            op_list.clone().into_iter().for_each(|op_id| {
                ops.insert((op_id, source.clone()));
            });

            // Pass ops to fetch tasks.
            let futures = op_list.into_iter().map(|op_id| async {
                if let Err(err) =
                    self.0.fetch_queue_tx.send((op_id, source.clone())).await
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

type FetchRequest = (OpId, AgentId);

#[derive(Debug)]
struct State {
    ops: HashSet<FetchRequest>,
    cool_down_list: HashMap<AgentId, Instant>,
}

#[derive(Debug)]
struct Inner {
    state: Arc<Mutex<State>>,
    fetch_queue_tx: Sender<FetchRequest>,
    fetch_queue_rx: Arc<Mutex<Receiver<FetchRequest>>>,
}

impl Inner {
    pub fn new() -> Self {
        // Create a channel to send new ops to fetch to the tasks. This is in effect the fetch queue.
        let (fetch_queue_tx, fetch_queue_rx) = channel::<FetchRequest>(1024);
        let fetch_queue_rx = Arc::new(Mutex::new(fetch_queue_rx));

        let ops = HashSet::new();
        let cool_down_list = HashMap::new();

        Self {
            state: Arc::new(Mutex::new(State {
                ops,
                cool_down_list,
            })),
            fetch_queue_tx,
            fetch_queue_rx,
        }
    }

    pub fn create_fetch_queue(
        &self,
        config: CoreFetchConfig,
        transport: DynTransport,
    ) {
        for _ in 0..config.parallel_request_count {
            tokio::task::spawn(Inner::fetch_task(
                self.fetch_queue_tx.clone(),
                self.fetch_queue_rx.clone(),
                transport.clone(),
                self.state.clone(),
                config.cool_down_interval_ms,
            ));
        }
    }

    async fn fetch_task(
        fetch_request_tx: Sender<FetchRequest>,
        fetch_request_rx: Arc<Mutex<Receiver<FetchRequest>>>,
        transport: DynTransport,
        state: Arc<Mutex<State>>,
        cool_down_interval: u64,
    ) {
        while let Some((op_id, agent_id)) =
            fetch_request_rx.lock().await.recv().await
        {
            // Ensure op is still in the set of ops to fetch.
            if !state
                .lock()
                .await
                .ops
                .contains(&(op_id.clone(), agent_id.clone()))
            {
                continue;
            }

            // Check if agent is on cool-down list.
            if !Inner::is_agent_cooling_down(
                &agent_id,
                &mut state.lock().await.cool_down_list,
                cool_down_interval,
            ) {
                // Send fetch request to agent.
                if let Err(err) = transport
                    .lock()
                    .await
                    .send_op_request(op_id.clone(), agent_id.clone())
                    .await
                {
                    eprintln!("could not send fetch request for op {op_id} to agent {agent_id}: {err}");
                    state
                        .lock()
                        .await
                        .cool_down_list
                        .insert(agent_id.clone(), Instant::now());
                }
            }

            // Re-insert the fetch request into the queue.
            if let Err(err) = fetch_request_tx
                .send((op_id.clone(), agent_id.clone()))
                .await
            {
                eprintln!("could not re-insert fetch request for op {op_id} to agent {agent_id} in queue: {err}");
            }
        }
    }

    fn is_agent_cooling_down(
        agent_id: &AgentId,
        cool_down_list: &mut HashMap<AgentId, Instant>,
        cool_down_interval: u64,
    ) -> bool {
        let a = cool_down_list.get(agent_id);
        match a {
            Some(instant) => {
                let now = Instant::now();
                if (now - *instant).as_millis() > cool_down_interval as u128 {
                    // Cool down interval has elapsed. Remove agent from list.
                    cool_down_list.remove(agent_id);
                    false
                } else {
                    // Cool down interval has not elapsed, still cooling down.
                    true
                }
            }
            None => false,
        }
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
