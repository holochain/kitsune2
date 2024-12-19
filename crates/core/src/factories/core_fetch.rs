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
//! ### State object CoreFetch
//!
//! - Exposes public method CoreFetch::add_ops that takes a list of op ids and an agent id.
//! - Stores pairs of ([OpId][AgentId]) in a set.
//! - A hash set is used to look up elements by key efficiently. Ops may be added redundantly
//!   to the set with different sources to fetch from, so the set is keyed by op and agent id together.
//!
//! ### Fetch tasks
//!
//! A channel acts as the queue structure for the fetch tasks. Requests to send are passed
//! one by one through the channel to the receiving tasks running in parallel. The flow
//! of sending fetch requests is as follows:
//!
//! - Await fetch requests for ([OpId], [AgentId]) from the queue.
//! - Check if request is still on the list of requests to send.
//!     - In case the op to request has been received in the meantime and no longer needs to be fetched,
//!       do nothing and await next request.
//!     - Otherwise proceed.
//! - Check if agent is on a back off list of unresponsive agents. If so, do not send request.
//! - Dispatch request for op id from agent to transport module.
//! - If agent is unresponsive, put them on back off list. If maximum back off has been reached, remove
//!   request from the set.
//! - Re-insert requested ([OpId], [AgentId]) into the queue. It will be removed
//!   from the set of requests if it is received in the meantime, and thus prevent a redundant
//!   fetch request.
//!
//! ### Incoming op task
//!
//! - Incoming op is written to the data store.
//! - Once persisted successfully, op is removed from the set of ops to fetch.

use std::{
    collections::{hash_map::Entry, HashMap, HashSet},
    sync::{Arc, Mutex},
    time::Instant,
};

use kitsune2_api::{
    builder,
    fetch::{serialize_op_ids, DynFetch, DynFetchFactory, Fetch, FetchFactory},
    peer_store,
    transport::DynTransport,
    AgentId, BoxFut, K2Result, OpId, SpaceId, Url,
};
use tokio::{
    sync::mpsc::{channel, Receiver, Sender},
    task::JoinHandle,
};

const MOD_NAME: &str = "Fetch";

/// CoreFetch configuration types.
pub mod config {
    /// Configuration parameters for [CoreFetchFactory](super::CoreFetchFactory).
    #[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct CoreFetchConfig {
        /// How many parallel op fetch requests can be made at once. Default: 2.  
        pub parallel_request_count: u8,
        /// Duration in ms to keep an unresponsive agent on the cool-down list. Default: 120_000.
        pub cool_down_interval_ms: u64,
    }

    impl Default for CoreFetchConfig {
        fn default() -> Self {
            Self {
                parallel_request_count: 2,
                cool_down_interval_ms: 120_000,
            }
        }
    }

    /// Module-level configuration for CoreFetch.
    #[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct CoreFetchModConfig {
        /// CoreFetch configuration.
        pub core_fetch: CoreFetchConfig,
    }
}

use config::*;

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
        config.set_module_config(&CoreFetchModConfig::default())?;
        Ok(())
    }

    fn create(
        &self,
        builder: Arc<builder::Builder>,
        space_id: SpaceId,
        peer_store: peer_store::DynPeerStore,
        transport: DynTransport,
    ) -> BoxFut<'static, K2Result<DynFetch>> {
        Box::pin(async move {
            let config: CoreFetchModConfig =
                builder.config.get_module_config()?;
            let out: DynFetch = Arc::new(CoreFetch::new(
                config.core_fetch,
                space_id,
                peer_store,
                transport,
            ));
            Ok(out)
        })
    }
}

type FetchRequest = (OpId, AgentId);

#[derive(Debug)]
struct State {
    requests: HashSet<FetchRequest>,
    back_off_list: BackOffList,
}

#[derive(Debug)]
struct CoreFetch {
    state: Arc<Mutex<State>>,
    fetch_queue_tx: Sender<FetchRequest>,
    fetch_tasks: Vec<JoinHandle<()>>,
}

impl CoreFetch {
    fn new(
        config: CoreFetchConfig,
        space_id: SpaceId,
        peer_store: peer_store::DynPeerStore,
        transport: DynTransport,
    ) -> Self {
        Self::spawn_fetch_tasks(config, space_id, peer_store, transport)
    }
}

impl Fetch for CoreFetch {
    fn add_ops(
        &self,
        op_list: Vec<OpId>,
        source: AgentId,
    ) -> BoxFut<'_, K2Result<()>> {
        Box::pin(async move {
            // Add requests to set.
            {
                let requests = &mut self.state.lock().unwrap().requests;
                requests.extend(
                    op_list
                        .clone()
                        .into_iter()
                        .map(|op_id| (op_id.clone(), source.clone())),
                );
            }

            // Pass requests to fetch tasks.
            for op_id in op_list {
                if let Err(err) =
                    self.fetch_queue_tx.send((op_id, source.clone())).await
                {
                    tracing::warn!(
                        "could not pass fetch request to fetch task: {err}"
                    );
                }
            }

            Ok(())
        })
    }
}

impl CoreFetch {
    pub fn spawn_fetch_tasks(
        config: CoreFetchConfig,
        space_id: SpaceId,
        peer_store: peer_store::DynPeerStore,
        transport: DynTransport,
    ) -> Self {
        // Create a channel to send new requests to fetch to the tasks. This is in effect the fetch queue.
        let (fetch_queue_tx, fetch_queue_rx) = channel::<FetchRequest>(16_384);
        let fetch_queue_rx = Arc::new(tokio::sync::Mutex::new(fetch_queue_rx));

        let state = Arc::new(Mutex::new(State {
            requests: HashSet::new(),
            back_off_list: BackOffList::new(
                config.back_off_interval_ms,
                config.max_back_off_exponent,
            ),
        }));

        let mut fetch_tasks = Vec::new();
        for _ in 0..config.parallel_request_count {
            let task = tokio::task::spawn(CoreFetch::fetch_task(
                state.clone(),
                fetch_queue_tx.clone(),
                fetch_queue_rx.clone(),
                peer_store.clone(),
                space_id.clone(),
                transport.clone(),
            ));
            fetch_tasks.push(task);
        }

        Self {
            state,
            fetch_queue_tx,
            fetch_tasks,
        }
    }

    async fn fetch_task(
        state: Arc<Mutex<State>>,
        fetch_request_tx: Sender<FetchRequest>,
        fetch_request_rx: Arc<tokio::sync::Mutex<Receiver<FetchRequest>>>,
        peer_store: peer_store::DynPeerStore,
        space_id: SpaceId,
        transport: DynTransport,
    ) {
        while let Some((op_id, agent_id)) =
            fetch_request_rx.lock().await.recv().await
        {
            let is_agent_on_back_off = {
                let mut lock = state.lock().unwrap();

                // Do nothing if op id is no longer in the set of requests to send.
                if !lock.requests.contains(&(op_id.clone(), agent_id.clone())) {
                    continue;
                }

                lock.back_off_list.is_agent_backing_off(&agent_id)
            };

            // Send request if agent is not on back off list.
            if !is_agent_on_back_off {
                let peer = match CoreFetch::get_peer_url_from_store(
                    &agent_id,
                    peer_store.clone(),
                )
                .await
                {
                    Some(url) => url,
                    None => {
                        // Agent not in peer store. Remove all associated op ids.
                        let mut lock = state.lock().unwrap();
                        lock.requests = lock
                            .requests
                            .clone()
                            .into_iter()
                            .filter(|(_, a)| *a != agent_id)
                            .collect();
                        continue;
                    }
                };

                let data = serialize_op_ids(vec![op_id.clone()]);

                // Send fetch request to agent.
                match transport
                    .send_module(
                        peer,
                        space_id.clone(),
                        MOD_NAME.to_string(),
                        data,
                    )
                    .await
                {
                    Ok(()) => {
                        state
                            .lock()
                            .unwrap()
                            .back_off_list
                            .remove_agent(&agent_id);
                    }
                    Err(err) => {
                        tracing::warn!("could not send fetch request for op {op_id} to agent {agent_id}: {err}");
                        let mut lock = state.lock().unwrap();
                        lock.back_off_list.back_off_agent(&agent_id);

                        if lock
                            .back_off_list
                            .is_agent_at_max_back_off(&agent_id)
                        {
                            lock.requests
                                .remove(&(op_id.clone(), agent_id.clone()));
                        }
                    }
                }
            }

            // Re-insert the fetch request into the queue.
            if let Err(err) =
                fetch_request_tx.try_send((op_id.clone(), agent_id.clone()))
            {
                tracing::warn!("could not re-insert fetch request for op {op_id} to agent {agent_id} into queue: {err}");
                // Remove op id/agent id from set to prevent build-up of state.
                state.lock().unwrap().requests.remove(&(op_id, agent_id));
            }
        }
    }

    async fn get_peer_url_from_store(
        agent_id: &AgentId,
        peer_store: peer_store::DynPeerStore,
    ) -> Option<Url> {
        match peer_store.get(agent_id.clone()).await {
            Ok(Some(peer)) => match &peer.url {
                Some(url) => Some(url.clone()),
                None => {
                    tracing::warn!("agent {agent_id} no longer on the network");
                    None
                }
            },
            Ok(None) => {
                tracing::warn!(
                    "could not find agent id {agent_id} in peer store"
                );
                None
            }
            Err(err) => {
                tracing::warn!(
                    "could not get agent id {agent_id} from peer store: {err}"
                );
                None
            }
        }
    }
}

impl Drop for CoreFetch {
    fn drop(&mut self) {
        for t in self.fetch_tasks.iter() {
            t.abort();
        }
    }
}

#[derive(Debug)]
struct BackOffList {
    state: HashMap<AgentId, (Instant, u32)>,
    back_off_interval: u64,
    max_back_off_exponent: u32,
}

impl BackOffList {
    pub fn new(back_off_interval: u64, max_back_off_exponent: u32) -> Self {
        Self {
            state: HashMap::new(),
            back_off_interval,
            max_back_off_exponent,
        }
    }

    pub fn back_off_agent(&mut self, agent_id: &AgentId) {
        match self.state.entry(agent_id.clone()) {
            Entry::Occupied(mut o) => {
                o.get_mut().0 = Instant::now();
                o.get_mut().1 = self.max_back_off_exponent.min(o.get().1 + 1);
            }
            Entry::Vacant(v) => {
                v.insert((Instant::now(), 0));
            }
        }
    }

    pub fn is_agent_backing_off(&mut self, agent_id: &AgentId) -> bool {
        match self.state.get(agent_id) {
            Some((instant, exponent)) => {
                instant.elapsed().as_millis()
                    < (self.back_off_interval * 2_u64.pow(*exponent)) as u128
            }
            None => false,
        }
    }

    pub fn is_agent_at_max_back_off(&self, agent_id: &AgentId) -> bool {
        self.state
            .get(agent_id)
            .map(|v| v.1 == self.max_back_off_exponent)
            .unwrap_or(false)
    }

    pub fn remove_agent(&mut self, agent_id: &AgentId) {
        self.state.remove(agent_id);
    }
}

#[cfg(test)]
mod test;
