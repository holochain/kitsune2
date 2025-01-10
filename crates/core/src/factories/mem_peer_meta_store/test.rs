use super::*;
use crate::default_builder;
use kitsune2_api::Timestamp;
use std::time::Duration;

struct TestPeerMetaStore {
    inner: DynPeerMetaStore,
    space: SpaceId,
}

impl TestPeerMetaStore {
    async fn new(inner: DynPeerMetaStore, space: &str) -> K2Result<Self> {
        let space: SpaceId =
            bytes::Bytes::from(space.as_bytes().to_vec()).into();

        Ok(Self { inner, space })
    }

    async fn last_gossip_timestamp(&self, agent: AgentId) -> Option<Timestamp> {
        self.inner
            .get(
                self.space.clone(),
                agent,
                "gossip:last_timestamp".to_string(),
            )
            .await
            .unwrap()
            .map(|v| {
                Timestamp::from_micros(i64::from_be_bytes(
                    v.as_slice().try_into().unwrap(),
                ))
            })
    }

    async fn set_last_gossip_timestamp(
        &mut self,
        agent: AgentId,
        timestamp: Timestamp,
    ) -> K2Result<()> {
        let value = timestamp.as_micros().to_be_bytes().to_vec();

        self.inner
            .put(
                self.space.clone(),
                agent,
                "gossip:last_timestamp".to_string(),
                value.clone(),
            )
            .await?;

        Ok(())
    }
}

#[tokio::test]
async fn mem_meta_store() {
    let factory = MemPeerMetaStoreFactory::create();
    let store = factory
        .create(Arc::new(default_builder().with_default_config().unwrap()))
        .await
        .unwrap();

    let agent = AgentId::from(bytes::Bytes::from_static(b"agent-1"));
    let mut agent_store = TestPeerMetaStore::new(store.clone(), "space")
        .await
        .unwrap();

    assert_eq!(agent_store.last_gossip_timestamp(agent.clone()).await, None);

    let timestamp = Timestamp::now();
    agent_store
        .set_last_gossip_timestamp(agent.clone(), timestamp)
        .await
        .unwrap();

    assert_eq!(
        agent_store.last_gossip_timestamp(agent).await,
        Some(timestamp)
    );
}

#[tokio::test]
async fn store_with_multiple_agents() {
    let factory = MemPeerMetaStoreFactory::create();
    let store = factory
        .create(Arc::new(default_builder().with_default_config().unwrap()))
        .await
        .unwrap();

    let agent_1 = AgentId::from(bytes::Bytes::from_static(b"agent-1"));
    let agent_2 = AgentId::from(bytes::Bytes::from_static(b"agent-2"));
    let mut agent_store = TestPeerMetaStore::new(store.clone(), "space")
        .await
        .unwrap();

    let timestamp_1 = Timestamp::now();
    let timestamp_2 = timestamp_1 + Duration::from_secs(1);

    agent_store
        .set_last_gossip_timestamp(agent_1.clone(), timestamp_1)
        .await
        .unwrap();
    agent_store
        .set_last_gossip_timestamp(agent_2.clone(), timestamp_2)
        .await
        .unwrap();

    assert_eq!(
        agent_store.last_gossip_timestamp(agent_1).await,
        Some(timestamp_1)
    );
    assert_eq!(
        agent_store.last_gossip_timestamp(agent_2).await,
        Some(timestamp_2)
    );
}
