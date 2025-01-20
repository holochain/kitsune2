use kitsune2_api::{
    DynPeerMetaStore, K2Error, K2Result, SpaceId, Timestamp, Url,
};
use std::time::Duration;

#[derive(Debug)]
pub(crate) struct K2PeerMetaStore {
    inner: DynPeerMetaStore,
    space: SpaceId,
}

impl K2PeerMetaStore {
    /// Create a new peer meta store for the given space.
    pub(crate) fn new(inner: DynPeerMetaStore, space: SpaceId) -> Self {
        Self { inner, space }
    }

    /// When did we last gossip with the given peer?
    pub(crate) async fn last_gossip_timestamp(
        &self,
        peer: Url,
    ) -> K2Result<Option<Timestamp>> {
        self.get(peer, "gossip:last_timestamp").await
    }

    /// Set the last gossip timestamp for the given agent.
    pub(crate) async fn set_last_gossip_timestamp(
        &self,
        peer: Url,
        timestamp: Timestamp,
    ) -> K2Result<()> {
        self.put(
            peer,
            "gossip:last_timestamp",
            timestamp,
            // Ideally, this value would be higher than the gossip interval, so that we don't
            // permit gossip more often than the configured interval. However, this should be
            // an acceptable minimum if we forget and accept gossip after 10 minutes.
            Some(Timestamp::now() + Duration::from_secs(10 * 60)),
        )
        .await
    }

    /// Get the "new ops" bookmark for the given agent.
    ///
    /// This is a value provided by the agent to indicate the last op they have
    /// sent us. We can use this to request an incremental update from them.
    pub(crate) async fn new_ops_bookmark(
        &self,
        peer: Url,
    ) -> K2Result<Option<Timestamp>> {
        self.get(peer, "gossip:new_ops_bookmark").await
    }

    /// Set the "new ops" bookmark for the given agent.
    pub(crate) async fn set_new_ops_bookmark(
        &self,
        peer: Url,
        timestamp: Timestamp,
    ) -> K2Result<()> {
        self.put(
            peer,
            "gossip:new_ops_bookmark",
            timestamp,
            // Try to keep this value around for 24 hours. If we check in once a day, it seems
            // reasonable to remember where we were syncing from. After that, fall back to catchup
            // logic.
            Some(Timestamp::now() + Duration::from_secs(24 * 60 * 60)),
        )
        .await
    }

    async fn get<T: serde::de::DeserializeOwned>(
        &self,
        peer: Url,
        name: &str,
    ) -> Result<Option<T>, K2Error> {
        self.inner
            .get(self.space.clone(), peer, name.to_string())
            .await?
            .map(|v| {
                serde_json::from_slice::<T>(&v).map_err(|e| {
                    K2Error::other_src(
                        format!(
                            "failed to deserialize peer meta value for {}",
                            name
                        ),
                        e,
                    )
                })
            })
            .transpose()
    }

    async fn put<T: serde::Serialize>(
        &self,
        peer: Url,
        name: &str,
        value: T,
        expiry: Option<Timestamp>,
    ) -> K2Result<()> {
        let value = serde_json::to_vec(&value).map_err(|e| {
            K2Error::other_src("failed to serialize peer meta value", e)
        })?;

        self.inner
            .put(
                self.space.clone(),
                peer,
                name.to_string(),
                value.into(),
                expiry,
            )
            .await?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;
    use kitsune2_core::default_test_builder;
    use kitsune2_core::factories::MemPeerMetaStoreFactory;
    use std::sync::Arc;

    async fn test_store() -> K2PeerMetaStore {
        let space = SpaceId::from(Bytes::from_static(b"test"));
        let builder =
            Arc::new(default_test_builder().with_default_config().unwrap());
        let inner = MemPeerMetaStoreFactory::create()
            .create(builder)
            .await
            .unwrap();
        K2PeerMetaStore::new(inner, space)
    }

    #[tokio::test]
    async fn round_trip_last_gossip_timestamp() {
        let store = test_store().await;
        let peer = Url::from_str("ws://test-host:80/1").unwrap();
        let timestamp = Timestamp::now();
        store
            .set_last_gossip_timestamp(peer.clone(), timestamp)
            .await
            .unwrap();

        assert_eq!(
            Some(timestamp),
            store.last_gossip_timestamp(peer.clone()).await.unwrap()
        );
    }

    #[tokio::test]
    async fn round_trip_new_ops_bookmark() {
        let store = test_store().await;
        let peer = Url::from_str("ws://test-host:80/1").unwrap();
        let timestamp = Timestamp::now();
        store
            .set_new_ops_bookmark(peer.clone(), timestamp)
            .await
            .unwrap();

        assert_eq!(
            Some(timestamp),
            store.new_ops_bookmark(peer.clone()).await.unwrap()
        );
    }
}
