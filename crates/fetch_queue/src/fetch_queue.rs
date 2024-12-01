use std::sync::Arc;

use kitsune2_api::{
    builder,
    config::ModConfig,
    fetch::{
        DynFetchQueue, DynFetchQueueFactory, FetchQueue, FetchQueueFactory,
    },
    AgentId, BoxFut, K2Result, OpId,
};

const MOD_NAME: &str = "FetchQueue";

/// Configuration parameters for [Q]
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QConfig {
    /// How many parallel fetch requests for actual data we should have at once. Default: 2.  
    parallel_request_count: u8,
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
    pub fn max_hash_count(&self) -> u8 {
        self.max_hash_count
    }
    pub fn max_byte_count(&self) -> u32 {
        self.max_byte_count
    }
}

#[derive(Debug)]
struct Q(Inner);

impl Q {
    fn new(config: QConfig) -> Self {
        Self(Inner::new(config))
    }
}

impl FetchQueue for Q {
    fn add_ops(&mut self, op_list: Vec<OpId>, source: AgentId) {
        todo!()
    }
}

#[derive(Debug)]
struct Inner {
    config: QConfig,
}

impl Inner {
    fn new(config: QConfig) -> Self {
        Self { config }
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
mod tests {
    use bytes::Bytes;
    use kitsune2_api::{id::Id, AgentId, OpId};
    use rand::Rng;

    use crate::fetch_queue::FetchQueue;

    #[test]
    fn add_ops() {}

    fn random_id() -> Id {
        let mut rng = rand::thread_rng();
        let mut bytes = [0u8; 32];
        rng.fill(&mut bytes);
        let bytes = Bytes::from(bytes.to_vec());
        Id(bytes)
    }

    fn random_op_id() -> OpId {
        OpId(random_id())
    }

    fn random_agent_id() -> AgentId {
        AgentId(random_id())
    }

    fn create_op_list(num_ops: u16) -> Vec<OpId> {
        let mut ops = Vec::new();
        for _ in 0..num_ops {
            let op = random_op_id();
            ops.push(op.clone());
        }
        ops
    }
}
