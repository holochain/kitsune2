use std::{sync::Arc, time::Duration};

use indexmap::IndexMap;
use kitsune2_api::{
    builder,
    config::ModConfig,
    fetch::{
        DynFetchQueue, DynFetchQueueFactory, FetchQueue, FetchQueueFactory,
    },
    tx::Tx,
    AgentId, BoxFut, K2Result, OpId,
};
use tokio::sync::Mutex;

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

type DynTx = Arc<Mutex<dyn Tx>>;

#[derive(Debug)]
struct Q(Arc<Mutex<Inner>>);

impl Q {
    fn new(config: QConfig) -> Self {
        Self(Arc::new(Mutex::new(Inner::new(config))))
    }

    pub fn spawn_fetch_loop(&self, tx: DynTx) {
        tokio::spawn({
            let inner = self.0.clone();
            let tx = tx.clone();
            async move {
                loop {
                    let mut inner = inner.lock().await;
                    let QConfig {
                        parallel_request_count,
                        max_hash_count,
                        max_byte_count,
                        fetch_loop_pause,
                    } = inner.config;
                    let num_requests = if parallel_request_count
                        > inner.current_request_count
                    {
                        parallel_request_count - inner.current_request_count
                    } else {
                        0
                    };

                    for _ in 0..num_requests {
                        let mut key_to_remove = None;
                        if let Some((source, op_list)) = inner.state.first_mut()
                        {
                            let num_elements =
                                (max_hash_count as usize).min(op_list.len());
                            let ops_batch: Vec<_> =
                                op_list.drain(0..num_elements).collect();
                            tokio::spawn(tx.lock().await.send_op_request(
                                source.clone(),
                                ops_batch,
                                max_byte_count,
                            ));
                            if num_elements < max_hash_count as usize {
                                key_to_remove = Some(source.clone());
                            }
                        }

                        if let Some(key) = key_to_remove {
                            inner.state.shift_remove_entry(&key);
                        }
                    }

                    drop(inner);

                    tokio::time::sleep(Duration::from_millis(fetch_loop_pause))
                        .await;
                }
            }
        });
    }
}

impl FetchQueue for Q {
    fn add_ops(
        &mut self,
        op_list: Vec<OpId>,
        source: AgentId,
    ) -> BoxFut<'_, K2Result<()>> {
        Box::pin(async move {
            self.0.lock().await.state.insert(source, op_list);
            Ok(())
        })
    }
}

#[derive(Debug)]
struct Inner {
    config: QConfig,
    state: IndexMap<AgentId, Vec<OpId>>,
    current_request_count: u8,
}

impl Inner {
    fn new(config: QConfig) -> Self {
        Self {
            config,
            state: IndexMap::new(),
            current_request_count: 0,
        }
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
        Box::pin(async move {
            let config = builder.config.get_module_config(MOD_NAME)?;
            let out: DynFetchQueue = Arc::new(Q::new(config));
            Ok(out)
        })
    }
}

#[cfg(test)]
mod test;
