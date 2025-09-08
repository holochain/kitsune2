//! Kitsune2 space related types.

use crate::fetch::DynFetch;
use crate::*;
use std::sync::Arc;

/// Handler for events coming out of Kitsune2 such as messages from peers.
pub trait SpaceHandler: 'static + Send + Sync + std::fmt::Debug {
    /// The sync handler for receiving notifications sent by a remote
    /// peer in reference to a particular space. If this callback returns
    /// an error, then the connection which sent the message will be closed.
    fn recv_notify(
        &self,
        from_peer: Url,
        space_id: SpaceId,
        data: bytes::Bytes,
    ) -> K2Result<()> {
        drop((from_peer, space_id, data));
        Ok(())
    }
}

/// Trait-object [SpaceHandler].
pub type DynSpaceHandler = Arc<dyn SpaceHandler>;

/// Represents a unique dht space within which to communicate with peers.
///
/// A space in Kitsune2 is largely just responsible for hooking up
/// modules within that space. However, it also has a couple responsibilities:
///
/// - The space provides the space-level notification send/recv ability.
/// - The space manages the generation / publishing of agent infos
///   for joined local agents.
pub trait Space: 'static + Send + Sync + std::fmt::Debug {
    /// Get a reference to the peer store being used by this space.
    /// This could allow you to inject peer info from some source other
    /// than gossip or bootstrap, or to query the store directly if
    /// you have a need to determine peers for direct messaging.
    fn peer_store(&self) -> &peer_store::DynPeerStore;

    /// Get a reference to the local agent store being used by this space.
    fn local_agent_store(&self) -> &DynLocalAgentStore;

    /// Get a reference to the op store of this space. Ops can be injected and
    /// retrieved from the op store.
    fn op_store(&self) -> &DynOpStore;

    /// Get a reference to the fetch module of this space.
    fn fetch(&self) -> &DynFetch;

    /// get a reference to the publish module of this space.
    fn publish(&self) -> &DynPublish;

    /// Get a reference to the gossip module of this space.
    fn gossip(&self) -> &DynGossip;

    /// Get a reference to the peer meta store being used by this space.
    fn peer_meta_store(&self) -> &DynPeerMetaStore;

    /// Get a reference to the blocks module being used by this space.
    fn blocks(&self) -> &DynBlocks;

    /// The URL that this space is currently reachable at, if any.
    fn current_url(&self) -> Option<Url>;

    /// Indicate that an agent is now online, and should begin receiving
    /// messages and exchanging dht information.
    fn local_agent_join(
        &self,
        local_agent: agent::DynLocalAgent,
    ) -> BoxFut<'_, K2Result<()>>;

    /// Indicate that an agent is no longer online, and should stop
    /// receiving messages and exchanging dht information.
    /// Before the agent is actually removed a tombstone agent info will
    /// be generated and sent to the bootstrap server. A best effort will
    /// be made to publish this tombstone to peers in the space as well.
    fn local_agent_leave(&self, local_agent: id::AgentId) -> BoxFut<'_, ()>;

    /// Send a message to a remote peer. The future returned from this
    /// function will track the message all the way down to the low-level
    /// network transport implementation. But once the data is handed off
    /// there, this function will return Ok, and you will not know if
    /// the remote has received it or not.
    fn send_notify(
        &self,
        to_peer: Url,
        data: bytes::Bytes,
    ) -> BoxFut<'_, K2Result<()>>;

    /// Inform the space that a set of ops have been stored.
    ///
    /// This should be called once the ops have been:
    /// - Checked to be correctly structured according to the Kitsune2 host implementation.
    /// - Persisted to the op store.
    ///
    /// Until this is called, gossip will not include these ops in its DHT model, and they will
    /// therefore not be synced with other peers. They may be synced with other peers through the
    /// "new ops" mechanism, depending on the op store implementation, but that only makes them
    /// available briefly.
    fn inform_ops_stored(&self, ops: Vec<StoredOp>)
        -> BoxFut<'_, K2Result<()>>;
}

/// Trait-object [Space].
pub type DynSpace = Arc<dyn Space>;

/// A factory for constructing Space instances.
pub trait SpaceFactory: 'static + Send + Sync + std::fmt::Debug {
    /// Help the builder construct a default config from the chosen
    /// module factories.
    fn default_config(&self, config: &mut config::Config) -> K2Result<()>;

    /// Validate configuration.
    fn validate_config(&self, config: &config::Config) -> K2Result<()>;

    /// Construct a space instance.
    fn create(
        &self,
        builder: Arc<builder::Builder>,
        handler: DynSpaceHandler,
        space_id: SpaceId,
        report: DynReport,
        tx: transport::DynTransport,
    ) -> BoxFut<'static, K2Result<DynSpace>>;
}

/// Trait-object [SpaceFactory].
pub type DynSpaceFactory = Arc<dyn SpaceFactory>;
