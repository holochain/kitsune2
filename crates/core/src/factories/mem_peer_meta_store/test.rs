use super::*;
use crate::default_test_builder;
use kitsune2_api::{Timestamp, Url};
use std::time::Duration;

struct TestPeerMetaStore {
    inner: DynPeerMetaStore,
}

impl TestPeerMetaStore {
    async fn new(inner: DynPeerMetaStore) -> K2Result<Self> {
        Ok(Self { inner })
    }

    async fn last_gossip_timestamp(&self, peer: Url) -> Option<Timestamp> {
        self.inner
            .get(peer, "gossip:last_timestamp".to_string())
            .await
            .unwrap()
            .map(|v| {
                Timestamp::from_micros(i64::from_be_bytes(
                    v.to_vec().as_slice().try_into().unwrap(),
                ))
            })
    }

    async fn set_last_gossip_timestamp(
        &mut self,
        peer: Url,
        timestamp: Timestamp,
    ) -> K2Result<()> {
        let value = bytes::Bytes::from(
            timestamp.as_micros().to_be_bytes().as_slice().to_vec(),
        );

        self.inner
            .put(peer, "gossip:last_timestamp".to_string(), value, None)
            .await?;

        Ok(())
    }
}

#[tokio::test]
async fn mem_meta_store() {
    let factory = MemPeerMetaStoreFactory::create();
    let store = factory
        .create(
            Arc::new(default_test_builder().with_default_config().unwrap()),
            kitsune2_test_utils::space::TEST_SPACE_ID.clone(),
        )
        .await
        .unwrap();

    let peer = Url::from_str("ws://test-host:80/1").unwrap();
    let mut agent_store = TestPeerMetaStore::new(store.clone()).await.unwrap();

    assert_eq!(agent_store.last_gossip_timestamp(peer.clone()).await, None);

    let timestamp = Timestamp::now();
    agent_store
        .set_last_gossip_timestamp(peer.clone(), timestamp)
        .await
        .unwrap();

    assert_eq!(
        agent_store.last_gossip_timestamp(peer).await,
        Some(timestamp)
    );
}

#[tokio::test]
async fn store_with_multiple_agents() {
    let factory = MemPeerMetaStoreFactory::create();
    let store = factory
        .create(
            Arc::new(default_test_builder().with_default_config().unwrap()),
            kitsune2_test_utils::space::TEST_SPACE_ID.clone(),
        )
        .await
        .unwrap();

    let peer_1 = Url::from_str("ws://test-host:80/1").unwrap();
    let peer_2 = Url::from_str("ws://test-host:80/2").unwrap();
    let mut agent_store = TestPeerMetaStore::new(store.clone()).await.unwrap();

    let timestamp_1 = Timestamp::now();
    let timestamp_2 = timestamp_1 + Duration::from_secs(1);

    agent_store
        .set_last_gossip_timestamp(peer_1.clone(), timestamp_1)
        .await
        .unwrap();
    agent_store
        .set_last_gossip_timestamp(peer_2.clone(), timestamp_2)
        .await
        .unwrap();

    assert_eq!(
        agent_store.last_gossip_timestamp(peer_1).await,
        Some(timestamp_1)
    );
    assert_eq!(
        agent_store.last_gossip_timestamp(peer_2).await,
        Some(timestamp_2)
    );
}

#[tokio::test]
async fn store_unresponsive_peer() {
    let factory = MemPeerMetaStoreFactory::create();
    let store = factory
        .create(
            Arc::new(default_test_builder().with_default_config().unwrap()),
            kitsune2_test_utils::space::TEST_SPACE_ID.clone(),
        )
        .await
        .unwrap();

    let peer = Url::from_str("ws://test-host:80/1").unwrap();

    let when_set_unresponsive =
        store.get_unresponsive(peer.clone()).await.unwrap();
    assert!(when_set_unresponsive.is_none());

    let when = Timestamp::now();
    store
        .set_unresponsive(peer.clone(), Timestamp::now(), when)
        .await
        .unwrap();

    let when_set_unresponsive =
        store.get_unresponsive(peer.clone()).await.unwrap();
    assert_eq!(when_set_unresponsive, Some(when));
}

#[tokio::test]
async fn get_all_by_key() {
    let factory = MemPeerMetaStoreFactory::create();
    let store = factory
        .create(
            Arc::new(default_test_builder().with_default_config().unwrap()),
            kitsune2_test_utils::space::TEST_SPACE_ID.clone(),
        )
        .await
        .unwrap();

    let peer_1 = Url::from_str("ws://test-host:80/1").unwrap();
    let when = Timestamp::now();
    store
        .set_unresponsive(peer_1.clone(), Timestamp::now(), when)
        .await
        .unwrap();
    let peer_2 = Url::from_str("ws://test-host:80/2").unwrap();
    store
        .set_unresponsive(peer_2.clone(), Timestamp::now(), when)
        .await
        .unwrap();

    let all_unresponsive_urls = store
        .get_all_by_key(
            format!("{KEY_PREFIX_ROOT}:{META_KEY_UNRESPONSIVE}").to_string(),
        )
        .await
        .unwrap();
    assert_eq!(all_unresponsive_urls.len(), 2);
    assert_eq!(
        all_unresponsive_urls.get(&peer_1),
        Some(&serde_json::to_vec(&when).unwrap().into())
    );
    assert_eq!(
        all_unresponsive_urls.get(&peer_2),
        Some(&serde_json::to_vec(&when).unwrap().into())
    );
}
