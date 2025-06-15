use crate::{builder, config, BoxFut, K2Error, K2Result, Timestamp, Url};
use bytes::Bytes;
use futures::future::BoxFuture;
use std::{collections::HashMap, sync::Arc};

/// Key prefix for items at the root level of the peer meta store.
pub const KEY_PREFIX_ROOT: &str = "root";

/// Meta key for unresponsive URLs.
pub const META_KEY_UNRESPONSIVE: &str = "unresponsive";

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

    /// Get all peers for a given key.
    fn get_all_by_key(
        &self,
        key: String,
    ) -> BoxFuture<'_, K2Result<HashMap<Url, bytes::Bytes>>>;

    /// Mark a peer url unresponsive with an expiration timestamp.
    ///
    /// The value that will be stored with the peer key is the passed in timestamp
    /// from when the URL became unresponsive.
    ///
    /// After the expiry timestamp has passed, the peer url is supposed to be removed
    /// from the store.
    fn set_unresponsive(
        &self,
        peer: Url,
        expiry: Timestamp,
        when: Timestamp,
    ) -> BoxFuture<'_, K2Result<()>> {
        Box::pin(async move {
            self.put(
                peer.clone(),
                format!("{KEY_PREFIX_ROOT}:{META_KEY_UNRESPONSIVE}"),
                serde_json::to_vec(&when).map_err(K2Error::other)?.into(),
                Some(expiry),
            )
            .await?;
            Ok(())
        })
    }

    /// Get the timestamp of when a peer URL last was marked unresponsive, if it is present in the
    /// store.
    fn get_unresponsive(
        &self,
        peer: Url,
    ) -> BoxFuture<'_, K2Result<Option<Timestamp>>> {
        Box::pin(async move {
            let maybe_value = self
                .get(peer, format!("{KEY_PREFIX_ROOT}:{META_KEY_UNRESPONSIVE}"))
                .await?;
            match maybe_value {
                None => Ok(None),
                Some(value) => {
                    match serde_json::from_slice::<Timestamp>(&value) {
                        Ok(when) => Ok(Some(when)),
                        Err(err) => Err(K2Error::other(err)),
                    }
                }
            }
        })
    }

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
