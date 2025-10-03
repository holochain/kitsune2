use kitsune2_api::{DynPeerMetaStore, K2Error, K2Result, Timestamp, Url};
use std::time::Duration;

/// A K2Gossip-specific peer meta store.
///
/// This is a wrapper around any [`DynPeerMetaStore`] that provides a set of get and set operations
/// for peer metadata that K2 gossip tracks.
#[derive(Debug)]
pub struct K2PeerMetaStore {
    inner: DynPeerMetaStore,
}

impl K2PeerMetaStore {
    /// Create a new peer meta store.
    pub(crate) fn new(inner: DynPeerMetaStore) -> Self {
        Self { inner }
    }

    /// When did we last gossip with the given peer?
    pub async fn last_gossip_timestamp(
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
    pub async fn new_ops_bookmark(
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

    /// Get the number of peer behavior errors.
    ///
    /// These are gossip rounds that have failed for a reason that we consider to be a wrong
    /// behavior by the peer. This might indicate malicious behavior or a protocol mismatch.
    pub async fn peer_behavior_errors(
        &self,
        peer: Url,
    ) -> K2Result<Option<u32>> {
        self.get(peer, "gossip:peer_behavior_errors").await
    }

    /// Increment the number of peer behavior errors for the given peer.
    pub(crate) async fn incr_peer_behavior_errors(
        &self,
        peer: Url,
    ) -> K2Result<()> {
        let value = self.peer_behavior_errors(peer.clone()).await?;
        self.put(
            peer,
            "gossip:peer_behavior_errors",
            value.unwrap_or_default() + 1,
            // If no more errors after 1 hour, remove
            Some(Timestamp::now() + Duration::from_secs(60 * 60)),
        )
        .await
    }

    /// Get the number of local errors.
    ///
    /// These are gossip rounds that have failed because of something that happened on our
    /// instance. Record by peer to discover patterns of forced errors.
    pub async fn local_errors(&self, peer: Url) -> K2Result<Option<u32>> {
        self.get(peer, "gossip:local_errors").await
    }

    /// Increment the number of local errors with the given peer.
    pub(crate) async fn incr_local_errors(&self, peer: Url) -> K2Result<()> {
        let value = self.local_errors(peer.clone()).await?;
        self.put(
            peer,
            "gossip:local_errors",
            value.unwrap_or_default() + 1,
            // If no more errors after 1 hour, remove
            Some(Timestamp::now() + Duration::from_secs(60 * 60)),
        )
        .await
    }

    /// Get the number of peer busy messages for the given peer.
    ///
    /// These are returned when the peer has reached their limit for number of concurrent
    /// accepted sessions and has asked us to wait until later to initiate gossip.
    pub async fn peer_busy(&self, peer: Url) -> K2Result<Option<u32>> {
        self.get(peer, "gossip:peer_busy").await
    }

    /// Increment the number of peer busy messages from the given peer.
    pub(crate) async fn incr_peer_busy(&self, peer: Url) -> K2Result<()> {
        let value = self.peer_busy(peer.clone()).await?;
        self.put(
            peer,
            "gossip:peer_busy",
            value.unwrap_or_default() + 1,
            // If no more errors after 1 hour, remove
            Some(Timestamp::now() + Duration::from_secs(60 * 60)),
        )
        .await
    }

    /// Get the number of times the given peer has terminated gossip rounds.
    ///
    /// This may happen for legitimate reasons. If it is happening frequently then check the logs
    /// for the reasons being given.
    pub async fn peer_terminated(&self, peer: Url) -> K2Result<Option<u32>> {
        self.get(peer, "gossip:peer_terminated").await
    }

    /// Increment the number of gossip rounds terminated by the given peer.
    pub(crate) async fn incr_peer_terminated(&self, peer: Url) -> K2Result<()> {
        let value = self.peer_terminated(peer.clone()).await?;
        self.put(
            peer,
            "gossip:peer_terminated",
            value.unwrap_or_default() + 1,
            // If no more errors after 1 hour, remove
            Some(Timestamp::now() + Duration::from_secs(60 * 60)),
        )
        .await
    }

    /// Get the number of completed gossip rounds with the given peer.
    ///
    /// These are rounds that have ended naturally through some exchange of messages. It does not
    /// include rounds that ended with some error.
    pub async fn completed_rounds(&self, peer: Url) -> K2Result<Option<u32>> {
        self.get(peer, "gossip:completed_rounds").await
    }

    /// Increment the number of gossip rounds terminated by the given peer.
    pub(crate) async fn incr_completed_rounds(
        &self,
        peer: Url,
    ) -> K2Result<()> {
        let value = self.completed_rounds(peer.clone()).await?;
        self.put(
            peer,
            "gossip:completed_rounds",
            value.unwrap_or_default() + 1,
            // If no more errors after 24 hours, remove
            Some(Timestamp::now() + Duration::from_secs(24 * 60 * 60)),
        )
        .await
    }

    /// Get the number of times the given peer has timed out gossip rounds.
    pub async fn peer_timeouts(&self, peer: Url) -> K2Result<Option<u32>> {
        self.get(peer, "gossip:peer_timeouts").await
    }

    /// Increment the number of gossip rounds terminated by the given peer.
    pub(crate) async fn incr_peer_timeout(&self, peer: Url) -> K2Result<()> {
        let value = self.peer_timeouts(peer.clone()).await?;
        self.put(
            peer,
            "gossip:peer_timeouts",
            value.unwrap_or_default() + 1,
            // If no more errors after 24 hours, remove
            Some(Timestamp::now() + Duration::from_secs(24 * 60 * 60)),
        )
        .await
    }

    /// Mark a peer url unresponsive with an expiration timestamp.
    pub async fn set_unresponsive(
        &self,
        peer: Url,
        expiry: Timestamp,
        when: Timestamp,
    ) -> K2Result<()> {
        self.inner.set_unresponsive(peer, expiry, when).await
    }

    /// Check if a peer is marked unresponsive in the store.
    pub async fn get_unresponsive(
        &self,
        peer: Url,
    ) -> K2Result<Option<Timestamp>> {
        self.inner.get_unresponsive(peer).await
    }

    async fn get<T: serde::de::DeserializeOwned>(
        &self,
        peer: Url,
        name: &str,
    ) -> Result<Option<T>, K2Error> {
        self.inner
            .get(peer, name.to_string())
            .await?
            .map(|v| {
                serde_json::from_slice::<T>(&v).map_err(|e| {
                    K2Error::other_src(
                        format!(
                            "failed to deserialize peer meta value for {name}"
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
            .put(peer, name.to_string(), value.into(), expiry)
            .await?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kitsune2_core::default_test_builder;
    use kitsune2_core::factories::MemPeerMetaStoreFactory;
    use std::sync::Arc;

    async fn test_store() -> K2PeerMetaStore {
        let builder =
            Arc::new(default_test_builder().with_default_config().unwrap());
        let inner = MemPeerMetaStoreFactory::create()
            .create(builder, kitsune2_test_utils::space::TEST_SPACE_ID.clone())
            .await
            .unwrap();
        K2PeerMetaStore::new(inner)
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
