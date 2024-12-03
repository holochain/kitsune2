//! The fetch queue is a Kitsune2 module for tracking ops that need to be fetched from peers
//! and sending fetch requests to them.
//!
//! The fetch queue holds sets of ops and sources where to fetch ops from. Sending fetch
//! requests to peers is executed by a fetch task.

use std::{collections::HashSet, sync::Arc, time::Duration};

use indexmap::{map::Entry, IndexMap};
use kitsune2_api::{
    builder,
    config::ModConfig,
    fetch::{
        DynFetchQueue, DynFetchQueueFactory, FetchQueue, FetchQueueFactory,
    },
    tx::Tx,
    AgentId, BoxFut, K2Error, K2Result, OpId,
};
use tokio::{
    select,
    sync::{mpsc::Sender, Mutex},
};

const MOD_NAME: &str = "FetchQueue";

/// Configuration parameters for [Q]
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QConfig {
    /// How many parallel fetch requests for actual data we should have at once. Default: 2.  
    parallel_request_count: u8,
    /// Duration in ms to pause between fetch loop iterations. Default: 100.
    fetch_loop_pause: u64,
    /// Max hash count to request from one peer at a time. Default: 16.  
    max_hash_count: u8,
    /// Max byte count that we want to get in a single request. Default: 10MiB.  
    /// This will be sent to the remote as part of the request. If the data of the requested ops  
    /// is larger than this amount, they will send back as the ACK the count of hashes  
    /// that they will be responding with that is under this byte count.  
    max_byte_count: u32,
}

impl Default for QConfig {
    fn default() -> Self {
        Self {
            parallel_request_count: 2,
            fetch_loop_pause: 100, // in ms
            max_hash_count: 16,
            max_byte_count: 10 * 1024,
        }
    }
}

impl ModConfig for QConfig {}

impl QConfig {
    pub fn parallel_request_count(&self) -> u8 {
        self.parallel_request_count
    }
    pub fn fetch_loop_pause(&self) -> u64 {
        self.fetch_loop_pause
    }
    pub fn max_hash_count(&self) -> u8 {
        self.max_hash_count
    }
    pub fn max_byte_count(&self) -> u32 {
        self.max_byte_count
    }
}

// TODO: Temporary trait object of a transport module to facilitate unit tests.
type DynTx = Arc<Mutex<dyn Tx>>;

#[derive(Debug)]
struct Q(Inner);

impl Q {
    fn new(config: QConfig, tx: DynTx) -> Self {
        Self(Inner::spawn(config, tx))
    }
}

impl FetchQueue for Q {
    fn add_ops(
        &mut self,
        op_list: Vec<OpId>,
        source: AgentId,
    ) -> BoxFut<'_, K2Result<()>> {
        Box::pin(async move {
            // Add ops to map.
            let mut ops = self.0.ops.lock().await;
            op_list.clone().into_iter().for_each(|op_id| {
                match ops.entry(op_id) {
                    Entry::Occupied(mut o) => {
                        let agent_ids = o.get_mut();
                        agent_ids.insert(source.clone());
                    }
                    Entry::Vacant(v) => {
                        let mut agent_ids = HashSet::new();
                        agent_ids.insert(source.clone());
                        v.insert(agent_ids);
                    }
                }
            });
            drop(ops);

            // Immediately fetch op list from agent.
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
    config: QConfig,
    ops: Arc<Mutex<IndexMap<OpId, HashSet<AgentId>>>>,
    current_request_count: Arc<Mutex<u8>>,
    fetch_request_tx: Sender<()>,
    tx: DynTx,
}

impl Inner {
    pub fn spawn(config: QConfig, tx: DynTx) -> Self {
        // Create a channel to send fetch requests to the loop.
        let (fetch_request_tx, mut fetch_request_rx) =
            tokio::sync::mpsc::channel::<()>(20);

        let inner = Self {
            config,
            ops: Arc::new(Mutex::new(IndexMap::new())),
            current_request_count: Arc::new(Mutex::new(0)),
            fetch_request_tx,
            tx,
        };

        tokio::spawn({
            let ops = inner.ops.clone();
            let current_request_count = inner.current_request_count.clone();
            let inner = inner.clone();

            async move {
                let QConfig {
                    parallel_request_count,
                    max_hash_count: _,
                    fetch_loop_pause,
                    ..
                } = inner.config;

                loop {
                    println!(
                        "current request count {}",
                        current_request_count.lock().await
                    );

                    // Send new request if parallel request count is not reached.
                    if parallel_request_count
                        > (*current_request_count.lock().await)
                    {
                        let _ = inner.clone().send_request().await;
                    }

                    println!(
                        "current request count now {} parallel request count {}",
                        current_request_count.lock().await
                        , parallel_request_count
                    );
                    // Pause if parallel request count is reached or ops is empty.
                    // Pause can be canceled by ops being added.
                    if (*current_request_count.lock().await)
                        == parallel_request_count
                        || ops.lock().await.is_empty()
                    {
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

    fn get_op_id_list_for_agent(
        &self,
        agent_id: &AgentId,
    ) -> BoxFut<'_, K2Result<Vec<OpId>>> {
        Box::pin(async move {
            let ops = self.ops.lock().await;
            Ok(vec![])
        })
    }

    fn send_request(&self) -> BoxFut<'_, K2Result<()>> {
        Box::pin(async move {
            if let Some((op_id, agent_ids)) =
                self.ops.lock().await.shift_remove_index(0)
            {
                println!("op id is {op_id} and agent id {agent_ids:?}");
                if let Some(agent_id) = agent_ids.iter().last() {
                    let ops_batch =
                        self.get_op_id_list_for_agent(agent_id).await?;

                    (*self.current_request_count.lock().await) += 1;
                    tokio::spawn({
                        let tx = self.tx.clone();
                        let agent_id = agent_id.clone();
                        let current_request_count =
                            self.current_request_count.clone();
                        let max_byte_count = self.config.max_byte_count;
                        async move {
                            let result = tx
                                .lock()
                                .await
                                .send_op_request(
                                    agent_id,
                                    ops_batch,
                                    max_byte_count,
                                )
                                .await;
                            println!("result from thread after sending request {result:?}");
                            (*current_request_count.lock().await) -= 1;
                        }
                    });
                }
            }
            Ok(())
        })
    }
}

#[derive(Debug)]
pub struct QFactory {}

impl QFactory {
    pub fn create() -> DynFetchQueueFactory {
        Arc::new(Self {})
    }
}

impl FetchQueueFactory for QFactory {
    fn default_config(
        &self,
        config: &mut kitsune2_api::config::Config,
    ) -> K2Result<()> {
        config.add_default_module_config::<QConfig>(MOD_NAME.to_string())?;
        Ok(())
    }

    fn create(
        &self,
        builder: Arc<builder::Builder>,
    ) -> BoxFut<'static, K2Result<DynFetchQueue>> {
        #[derive(Debug)]
        struct TxPlaceholder;
        impl Tx for TxPlaceholder {
            fn send_op_request(
                &mut self,
                _source: AgentId,
                _op_list: Vec<OpId>,
                _max_byte_count: u32,
            ) -> BoxFut<'static, K2Result<()>> {
                Box::pin(async move { todo!() })
            }
        }
        let tx = Arc::new(Mutex::new(TxPlaceholder));

        Box::pin(async move {
            let config = builder.config.get_module_config(MOD_NAME)?;
            let out: DynFetchQueue = Arc::new(Q::new(config, tx));
            Ok(out)
        })
    }
}

#[cfg(test)]
mod test;
