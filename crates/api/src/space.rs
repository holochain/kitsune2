//! Kitsune2 space related types.

use crate::*;
use std::sync::Arc;

/// Handler for events coming out of Kitsune2 such as messages from peers.
pub trait SpaceHandler: 'static + Send + Sync + std::fmt::Debug {
    /// We have received an incomming message from a remote peer.
    ///
    /// If this callback handler returns an `Err` response, the connection
    /// will be closed immediately.
    //
    // Note: this is the minimal low-level messaging unit. We can decide
    //       later if we want to handle request/response tracking in
    //       kitsune itself as a convenience or if users of this lib
    //       should have to implement that if they want it.
    fn incoming_message(
        &self,
        peer: AgentId,
        data: bytes::Bytes,
    ) -> K2Result<()>;
}

/// Trait-object [SpaceHandler].
pub type DynSpaceHandler = Arc<dyn SpaceHandler>;

/// Represents a unique dht space within which to communicate with peers.
pub trait Space: 'static + Send + Sync + std::fmt::Debug {
    /// Get a reference to the peer store being used by this space.
    /// This could allow you to inject peer info from some source other
    /// than gossip or bootstrap, or to query the store directly if
    /// you have a need to determine peers for direct messaging.
    fn peer_store(&self) -> &peer_store::DynPeerStore;

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
    /// be made to publish this tomstone to peers in the space as well.
    fn local_agent_leave(&self, local_agent: id::AgentId) -> BoxFut<'_, ()>;

    /// Send a message to a remote peer. The future returned from this
    /// function will track the message all the way down to the low-level
    /// network transport implementation. But once the data is handed off
    /// there, this function will return Ok, and you will not know if
    /// the remote has received it or not.
    //
    // Note: this is the minimal low-level messaging unit. We can decide
    //       later if we want to handle request/response tracking in
    //       kitsune itself as a convenience or if users of this lib
    //       should have to implement that if they want it.
    fn send_message(&self, peer: AgentId, data: bytes::Bytes)
        -> BoxFut<'_, ()>;
}

/// Trait-object [Space].
pub type DynSpace = Arc<dyn Space>;

/// A factory for constructing Space instances.
pub trait SpaceFactory: 'static + Send + Sync + std::fmt::Debug {
    /// Help the builder construct a default config from the chosen
    /// module factories.
    fn default_config(&self, config: &mut config::Config) -> K2Result<()>;

    /// Construct a space instance.
    fn create(
        &self,
        builder: Arc<builder::Builder>,
        handler: DynSpaceHandler,
        space: SpaceId,
    ) -> BoxFut<'static, K2Result<DynSpace>>;
}

/// Trait-object [SpaceFactory].
pub type DynSpaceFactory = Arc<dyn SpaceFactory>;
