use crate::{builder, config, BoxFut, K2Result, Timestamp, Url};
use bytes::Bytes;
use futures::future::BoxFuture;
use std::sync::Arc;

/// A store for peer metadata.
///
/// This is expected to be backed by a key-value store that keys by space, peer URL and key.
pub trait PeerMetaStore: 'static + Send + Sync + std::fmt::Debug {
    /// Store a key-value pair for a peer.
    fn put(
        &self,
        peer: Url,
        key: String,
        value: Bytes,
        expiry: Option<Timestamp>,
    ) -> BoxFuture<'_, K2Result<()>>;

    /// Get a value by key for a peer.
    fn get(
        &self,
        peer: Url,
        key: String,
    ) -> BoxFuture<'_, K2Result<Option<bytes::Bytes>>>;

    /// Mark a peer url unresponsive with an expiration timestamp.
    ///
    /// The value that will be stored with the peer key is the passed in timestamp
    /// from when the URL became unresponsive.
    ///
    /// After the expiry timestamp has passed, the peer url is supposed to be removed
    /// from the store.
    fn mark_peer_unresponsive(
        &self,
        peer: Url,
        expiry: Timestamp,
        when: Timestamp,
    ) -> BoxFuture<'_, K2Result<()>>;

    /// Get the timestamp of when a peer URL was marked unresponsive, if it is present in the
    /// store.
    fn get_unresponsive_url(
        &self,
        peer: Url,
    ) -> BoxFuture<'_, K2Result<Option<Timestamp>>>;

    /// Delete a key-value pair for a given space and peer.
    fn delete(&self, peer: Url, key: String) -> BoxFuture<'_, K2Result<()>>;
}

/// Trait-object version of kitsune2 [PeerMetaStore].
pub type DynPeerMetaStore = Arc<dyn PeerMetaStore>;

/// A factory for constructing [PeerMetaStore] instances.
pub trait PeerMetaStoreFactory:
    'static + Send + Sync + std::fmt::Debug
{
    /// Help the builder construct a default config from the chosen
    /// module factories.
    fn default_config(&self, config: &mut config::Config) -> K2Result<()>;

    /// Validate configuration.
    fn validate_config(&self, config: &config::Config) -> K2Result<()>;

    /// Construct a meta store instance.
    fn create(
        &self,
        builder: Arc<builder::Builder>,
        space: crate::SpaceId,
    ) -> BoxFut<'static, K2Result<DynPeerMetaStore>>;
}

/// Trait-object [PeerMetaStoreFactory].
pub type DynPeerMetaStoreFactory = Arc<dyn PeerMetaStoreFactory>;
