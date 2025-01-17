//! Fetch is a Kitsune2 module for fetching ops from peers.
//!
//! In particular it tracks which ops need to be fetched from which agents,
//! sends fetch requests and processes incoming requests and responses.
//!
//! It consists of multiple parts:
//! - State object that tracks op and agent ids in memory
//! - Fetch tasks that request tracked ops from agents
//! - An incoming request task that retrieves ops from the op store and responds with the
//!   ops to the requester
//! - An incoming response task that writes ops to the op store and removes their ids
//!   from the state object
//!
//! ### State object CoreFetch
//!
//! - Exposes public method CoreFetch::add_ops that takes a list of op ids and an agent id.
//! - Stores pairs of ([OpId][AgentId]) in a hash set. Hash set is used to look up elements
//!   by key efficiently. Ops may be added redundantly to the set with different sources
//!   to fetch from, so the set is keyed by op and agent id combined.
//!
//! ### Fetch tasks
//!
//! #### Outgoing requests
//!
//! A channel acts as the queue for outgoing fetch requests. Ops to fetch are sent
//! one by one through the channel to the receiving tasks, running in parallel and processing
//! requests in incoming order. The flow of sending a fetch request is as follows:
//!
//! - Check if request for ([OpId][AgentId]) is still in the set of requests to send.
//!     - In case the op to request has been received in the meantime and no longer needs to be fetched,
//!       it will have been removed from the set. Do nothing.
//!     - Otherwise proceed.
//! - Check if agent is on a back off list of unresponsive agents. If so, do not send a request.
//! - Dispatch request for op id from agent to transport module.
//! - If agent is unresponsive, put them on back off list. If maximum back off has been reached, remove
//!   this and all other requests to the agent from the set.
//! - Re-insert requested ([OpId], [AgentId]) into the queue. It will be removed
//!   from the set of requests if it is received in the meantime, and thus prevent redundant
//!   fetch requests.
//!
//! #### Incoming requests
//!
//! Similarly to outgoing requests, a channel serves as a queue for incoming requests. The queue
//! has the following properties:
//! - Simple queue which processes items in the order of the incoming requests.
//! - Requests consist of a list of requested op ids and the URL of the requesting agent.
//! - The task attempts to look up the requested op in the data store and send it in a response.
//! - Requests for data that the host doesn't hold should be logged.
//! - If none of the requested ops could be read from the store, no response is sent.
//! - If sending or receiving the response fails, it's the requester's responsibility to request again.
//!
//! ### Incoming responses
//!
//! A channel acts as a queue for incoming responses. When a response to a requested op is received,
//! it must be processed as follows:
//! - Incoming op is written to the op store.
//! - Once persisted successfully, the op is removed from the set of ops to fetch.
//! - If persisting fails, the op is not removed from the set.

use back_off::BackOffList;
#[cfg(test)]
use kitsune2_api::transport::DynTxModuleHandler;
use kitsune2_api::{
    builder,
    fetch::{
        serialize_request_message, serialize_response_message, DynFetch,
        DynFetchFactory, Fetch, FetchFactory,
    },
    peer_store,
    transport::DynTransport,
    AgentId, BoxFut, DynOpStore, K2Result, Op, OpId, SpaceId, Url,
};
use message_handler::FetchMessageHandler;
use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    sync::mpsc::{channel, Receiver, Sender},
    task::JoinHandle,
};

mod back_off;
mod message_handler;

#[cfg(test)]
mod test;

const MOD_NAME: &str = "Fetch";

/// CoreFetch configuration types.
pub mod config {
    /// Configuration parameters for [CoreFetchFactory](super::CoreFetchFactory).
    #[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct CoreFetchConfig {
        /// How many parallel op fetch requests can be made at once. Default: 2.  
        pub parallel_request_count: u8,
        /// Delay before re-inserting ops to request back into the outgoing request queue.
        /// Default: 100 ms.
        pub re_insert_outgoing_request_delay_ms: u32,
        /// Duration of first interval to back off an unresponsive agent. Default: 20 s.
        pub first_back_off_interval_ms: u32,
        /// Duration of last interval to back off an unresponsive agent. Default: 10 min.
        pub last_back_off_interval_ms: u32,
        /// Number of back off intervals. Default: 4.
        pub num_back_off_intervals: usize,
    }

    impl Default for CoreFetchConfig {
        // Maximum back off is 11:40 min.
        fn default() -> Self {
            Self {
                parallel_request_count: 2,
                re_insert_outgoing_request_delay_ms: 100,
                first_back_off_interval_ms: 1000 * 20,
                last_back_off_interval_ms: 1000 * 60 * 10,
                num_back_off_intervals: 4,
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
        op_store: DynOpStore,
        transport: DynTransport,
    ) -> BoxFut<'static, K2Result<DynFetch>> {
        Box::pin(async move {
            let config: CoreFetchModConfig =
                builder.config.get_module_config()?;
            let out: DynFetch = Arc::new(CoreFetch::new(
                config.core_fetch,
                space_id,
                peer_store,
                op_store,
                transport,
            ));
            Ok(out)
        })
    }
}

type OutgoingRequest = (OpId, AgentId);
type IncomingRequest = (Vec<OpId>, Url);
type IncomingResponse = Vec<Op>;

#[derive(Debug)]
struct State {
    requests: HashSet<OutgoingRequest>,
    back_off_list: BackOffList,
}

#[derive(Debug)]
struct CoreFetch {
    state: Arc<Mutex<State>>,
    outgoing_request_tx: Sender<OutgoingRequest>,
    tasks: Vec<JoinHandle<()>>,
    #[cfg(test)]
    message_handler: DynTxModuleHandler,
}

impl CoreFetch {
    fn new(
        config: CoreFetchConfig,
        space_id: SpaceId,
        peer_store: peer_store::DynPeerStore,
        op_store: DynOpStore,
        transport: DynTransport,
    ) -> Self {
        Self::spawn_tasks(config, space_id, peer_store, op_store, transport)
    }
}

impl Fetch for CoreFetch {
    fn request_ops(
        &self,
        op_ids: Vec<OpId>,
        source: AgentId,
    ) -> BoxFut<'_, K2Result<()>> {
        Box::pin(async move {
            // Add requests to set.
            {
                let requests = &mut self.state.lock().unwrap().requests;
                requests.extend(
                    op_ids
                        .clone()
                        .into_iter()
                        .map(|op_id| (op_id.clone(), source.clone())),
                );
            }

            // Insert requests into fetch queue.
            for op_id in op_ids {
                if let Err(err) =
                    self.outgoing_request_tx.send((op_id, source.clone())).await
                {
                    tracing::warn!(
                        "could not insert fetch request into fetch queue: {err}"
                    );
                }
            }

            Ok(())
        })
    }
}

impl CoreFetch {
    pub fn spawn_tasks(
        config: CoreFetchConfig,
        space_id: SpaceId,
        peer_store: peer_store::DynPeerStore,
        op_store: DynOpStore,
        transport: DynTransport,
    ) -> Self {
        // Create a queue to process outgoing op requests. Requests are sent to peers.
        let (outgoing_request_tx, outgoing_request_rx) =
            channel::<OutgoingRequest>(16_384);
        let outgoing_request_rx =
            Arc::new(tokio::sync::Mutex::new(outgoing_request_rx));

        // Create a queue to process incoming op requests. Requested ops are retrieved from the
        // store and returned to the requester.
        let (incoming_request_tx, incoming_request_rx) =
            channel::<IncomingRequest>(16_384);

        // Create a queue to process incoming op responses. Ops are passed to the op store and op
        // ids removed from the set of ops to fetch.
        let (incoming_response_tx, incoming_response_rx) =
            channel::<IncomingResponse>(16_384);

        let state = Arc::new(Mutex::new(State {
            requests: HashSet::new(),
            back_off_list: BackOffList::new(
                config.first_back_off_interval_ms,
                config.last_back_off_interval_ms,
                config.num_back_off_intervals,
            ),
        }));

        let mut tasks =
            Vec::with_capacity(config.parallel_request_count as usize);
        // Spawn request tasks.
        for _ in 0..config.parallel_request_count {
            let request_task =
                tokio::task::spawn(CoreFetch::outgoing_request_task(
                    state.clone(),
                    outgoing_request_tx.clone(),
                    outgoing_request_rx.clone(),
                    peer_store.clone(),
                    space_id.clone(),
                    transport.clone(),
                    config.re_insert_outgoing_request_delay_ms,
                ));
            tasks.push(request_task);
        }

        // Spawn incoming request task.
        let incoming_request_task =
            tokio::task::spawn(CoreFetch::incoming_request_task(
                incoming_request_rx,
                op_store.clone(),
                transport.clone(),
                space_id.clone(),
            ));
        tasks.push(incoming_request_task);

        // Spawn incoming response task.
        let incoming_response_task =
            tokio::task::spawn(CoreFetch::incoming_response_task(
                incoming_response_rx,
                op_store,
                state.clone(),
            ));
        tasks.push(incoming_response_task);

        // Register transport module handler for incoming op requests and responses.
        let message_handler = Arc::new(FetchMessageHandler {
            incoming_request_tx,
            incoming_response_tx,
        });
        transport.register_module_handler(
            space_id.clone(),
            MOD_NAME.to_string(),
            message_handler.clone(),
        );

        Self {
            state,
            outgoing_request_tx,
            tasks,
            #[cfg(test)]
            message_handler,
        }
    }

    async fn outgoing_request_task(
        state: Arc<Mutex<State>>,
        outgoing_request_tx: Sender<OutgoingRequest>,
        outgoing_request_rx: Arc<tokio::sync::Mutex<Receiver<OutgoingRequest>>>,
        peer_store: peer_store::DynPeerStore,
        space_id: SpaceId,
        transport: DynTransport,
        re_insert_outgoing_request_delay: u32,
    ) {
        while let Some((op_id, agent_id)) =
            outgoing_request_rx.lock().await.recv().await
        {
            let is_agent_on_back_off = {
                let mut lock = state.lock().unwrap();

                // Do nothing if op id is no longer in the set of requests to send.
                if !lock.requests.contains(&(op_id.clone(), agent_id.clone())) {
                    continue;
                }

                lock.back_off_list.is_agent_on_back_off(&agent_id)
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
                        // If agent is not in peer store, remove all requests to them.
                        state
                            .lock()
                            .unwrap()
                            .requests
                            .retain(|(_, a)| *a != agent_id);
                        continue;
                    }
                };

                let data = serialize_request_message(vec![op_id.clone()]);

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
                        // If agent was on back off list, remove them.
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

                        // If max back off interval has expired for the agent,
                        // give up on requesting ops from them.
                        if lock
                            .back_off_list
                            .has_last_back_off_expired(&agent_id)
                        {
                            lock.requests.retain(|(_, a)| *a != agent_id);
                        }
                    }
                }
            }

            // Re-insert the fetch request into the queue after a delay.
            let outgoing_request_tx = outgoing_request_tx.clone();
            let state = state.clone();
            tokio::task::spawn(async move {
                tokio::time::sleep(Duration::from_millis(
                    re_insert_outgoing_request_delay as u64,
                ))
                .await;
                if let Err(err) = outgoing_request_tx
                    .try_send((op_id.clone(), agent_id.clone()))
                {
                    tracing::warn!("could not re-insert fetch request for op {op_id} to agent {agent_id} into queue: {err}");
                    // Remove op id/agent id from set to prevent build-up of state.
                    state.lock().unwrap().requests.remove(&(op_id, agent_id));
                }
            });
        }
    }

    async fn incoming_request_task(
        mut response_rx: Receiver<IncomingRequest>,
        op_store: DynOpStore,
        transport: DynTransport,
        space_id: SpaceId,
    ) {
        while let Some((op_ids, peer)) = response_rx.recv().await {
            tracing::debug!(?op_ids, ?peer, "incoming request");

            // Retrieve ops to send from store.
            let ops = match op_store.retrieve_ops(op_ids.clone()).await {
                Err(err) => {
                    tracing::error!("could not read ops from store: {err}");
                    continue;
                }
                Ok(ops) => {
                    ops.into_iter().map(|op| op.op_data).collect::<Vec<_>>()
                }
            };

            if ops.is_empty() {
                // Do not send a response when no ops could be retrieved.
                continue;
            }

            let data = serialize_response_message(ops);
            if let Err(err) = transport
                .send_module(
                    peer.clone(),
                    space_id.clone(),
                    MOD_NAME.to_string(),
                    data,
                )
                .await
            {
                tracing::warn!(
                    ?op_ids,
                    ?peer,
                    "could not send ops to requesting agent: {err}"
                );
            }
        }
    }

    async fn incoming_response_task(
        mut incoming_response_rx: Receiver<IncomingResponse>,
        op_store: DynOpStore,
        state: Arc<Mutex<State>>,
    ) {
        while let Some(ops) = incoming_response_rx.recv().await {
            let ops_data = ops.clone().into_iter().map(|op| op.data).collect();
            match op_store.process_incoming_ops(ops_data).await {
                Err(err) => {
                    tracing::error!("could not process incoming ops: {err}");
                    // Ops could not be written to the op store. Their ids remain in the set of ops
                    // to fetch.
                    continue;
                }
                Ok(processed_op_ids) => {
                    // Ops were processed successfully by op store. Op ids are returned.
                    // The op ids are removed from the set of ops to fetch.
                    let mut lock = state.lock().unwrap();
                    lock.requests
                        .retain(|(op_id, _)| !processed_op_ids.contains(op_id));
                }
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
        for t in self.tasks.iter() {
            t.abort();
        }
    }
}
