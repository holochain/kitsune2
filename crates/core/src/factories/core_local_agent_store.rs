use kitsune2_api::agent::DynLocalAgent;
use kitsune2_api::{
    AgentId, BoxFut, K2Result, LocalAgentStore, LocalAgentStoreFactory,
};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

type Inner = HashMap<AgentId, DynLocalAgent>;

/// Default, production ready implementation of [LocalAgentStore].
#[derive(Debug)]
pub struct CoreLocalAgentStore {
    inner: Arc<Mutex<Inner>>,
}

impl CoreLocalAgentStore {
    fn create() -> Arc<dyn LocalAgentStore> {
        Arc::new(CoreLocalAgentStore {
            inner: Arc::new(Mutex::new(HashMap::new())),
        })
    }
}

impl LocalAgentStore for CoreLocalAgentStore {
    fn add(&self, local_agent: DynLocalAgent) -> BoxFut<'_, K2Result<()>> {
        self.inner
            .lock()
            .unwrap()
            .insert(local_agent.agent().clone(), local_agent.clone());

        Box::pin(async { Ok(()) })
    }

    fn remove(
        &self,
        local_agent: AgentId,
    ) -> BoxFut<'_, Option<DynLocalAgent>> {
        let out = self.inner.lock().unwrap().remove(&local_agent);
        Box::pin(async { out })
    }

    fn get_all(&self) -> BoxFut<'_, K2Result<Vec<DynLocalAgent>>> {
        let inner = self.inner.lock().unwrap();
        let out = inner.values().cloned().collect();
        Box::pin(async { Ok(out) })
    }
}

/// Factory for creating [CoreLocalAgentStore] instances.
#[derive(Debug)]
pub struct CoreLocalAgentStoreFactory;

impl CoreLocalAgentStoreFactory {
    pub(crate) fn create() -> Arc<dyn LocalAgentStoreFactory> {
        Arc::new(CoreLocalAgentStoreFactory)
    }
}

impl LocalAgentStoreFactory for CoreLocalAgentStoreFactory {
    fn default_config(
        &self,
        _config: &mut kitsune2_api::config::Config,
    ) -> K2Result<()> {
        Ok(())
    }

    fn create(
        &self,
        _builder: Arc<crate::builder::Builder>,
    ) -> BoxFut<'static, K2Result<Arc<dyn LocalAgentStore>>> {
        Box::pin(async { Ok(CoreLocalAgentStore::create()) })
    }
}
