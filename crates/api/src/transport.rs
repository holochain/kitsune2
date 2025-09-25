//! Kitsune2 transport related types.

use crate::{protocol::*, *};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, Weak};

/// This is the low-level backend transport handler designed to work
/// with [DefaultTransport].
/// Construct using ([TxImpHnd::new]), with a high-level [DynTxHandler],
/// then call [DefaultTransport::create] to return the high-level handler
/// from the [TransportFactory].
pub struct TxImpHnd {
    handler: DynTxHandler,
    space_map: Arc<Mutex<HashMap<SpaceId, DynTxSpaceHandler>>>,
    mod_map: Arc<Mutex<HashMap<(SpaceId, String), DynTxModuleHandler>>>,
    blocked_message_counts: Arc<Mutex<HashMap<(Url, SpaceId), u32>>>,
}

impl TxImpHnd {
    /// When constructing a [Transport] from a [TransportFactory],
    /// you need a [TxImpHnd] for calling transport events.
    /// Pass the handler into here to construct one.
    pub fn new(handler: DynTxHandler) -> Arc<Self> {
        Arc::new(Self {
            handler,
            space_map: Arc::new(Mutex::new(HashMap::new())),
            mod_map: Arc::new(Mutex::new(HashMap::new())),
            blocked_message_counts: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    /// Call this when you receive or bind a new address at which
    /// this local node can be reached by peers
    pub fn new_listening_address(&self, this_url: Url) -> BoxFut<'static, ()> {
        let handler = self.handler.clone();
        let space_map = self
            .space_map
            .clone()
            .lock()
            .unwrap()
            .values()
            .cloned()
            .collect::<Vec<_>>();

        Box::pin(async move {
            handler.new_listening_address(this_url.clone()).await;
            for s in space_map {
                s.new_listening_address(this_url.clone()).await;
            }
        })
    }

    /// Call this when you establish an outgoing connection and
    /// when you establish an incoming connection. If this call
    /// returns an error, the connection should be closed immediately.
    /// On success, this function returns bytes that should be
    /// sent as a preflight message for additional connection validation.
    /// (The preflight data should be sent even if it is zero length).
    pub fn peer_connect(
        &self,
        peer: Url,
    ) -> BoxFut<'_, K2Result<bytes::Bytes>> {
        Box::pin(async {
            for mod_handler in self.mod_map.lock().unwrap().values() {
                mod_handler.peer_connect(peer.clone())?;
            }
            for space_handler in self.space_map.lock().unwrap().values() {
                space_handler.peer_connect(peer.clone())?;
            }
            self.handler.peer_connect(peer.clone())?;
            let preflight =
                self.handler.preflight_gather_outgoing(peer).await?;
            let enc = (K2Proto {
                ty: K2WireType::Preflight as i32,
                data: preflight,
                space_id: None,
                module_id: None,
            })
            .encode()?;
            Ok(enc)
        })
    }

    /// Call this whenever a connection is closed.
    pub fn peer_disconnect(&self, peer: Url, reason: Option<String>) {
        for h in self.mod_map.lock().unwrap().values() {
            h.peer_disconnect(peer.clone(), reason.clone());
        }
        for h in self.space_map.lock().unwrap().values() {
            h.peer_disconnect(peer.clone(), reason.clone());
        }
        self.handler.peer_disconnect(peer, reason);
    }

    /// Call this whenever data is received on an open connection.
    pub fn recv_data(
        &self,
        peer: Url,
        data: bytes::Bytes,
    ) -> BoxFut<'_, K2Result<()>> {
        Box::pin(async move {
            let data = K2Proto::decode(&data)?;
            let ty = data.ty();
            let K2Proto {
                space_id,
                module_id,
                data,
                ..
            } = data;

            match ty {
                K2WireType::Unspecified => Ok(()),
                K2WireType::Preflight => {
                    self.handler.preflight_validate_incoming(peer, data).await
                }
                K2WireType::Notify => {
                    if let Some(space_id) = space_id {
                        let space_id = SpaceId::from(space_id);
                        if let Some(h) =
                            self.space_map.lock().unwrap().get(&space_id)
                        {
                            h.recv_space_notify(peer, space_id, data)?;
                        }
                    }
                    Ok(())
                }
                K2WireType::Module => {
                    if let (Some(space_id), Some(module)) =
                        (space_id, module_id)
                    {
                        let space_id = SpaceId::from(space_id);
                        if let Some(h) = self
                            .mod_map
                            .lock()
                            .unwrap()
                            .get(&(space_id.clone(), module.clone()))
                        {
                            h.recv_module_msg(peer, space_id, module.clone(), data).inspect_err(|e| {
                            tracing::warn!(?module, "Error in recv_module_msg, peer connection will be closed: {e}");
                        })?;
                        }
                    }
                    Ok(())
                }
                K2WireType::Disconnect => {
                    let reason = String::from_utf8_lossy(&data).to_string();
                    Err(K2Error::other(format!("Remote Disconnect: {reason}")))
                }
            }
        })
    }

    /// Call this whenever a connection to a peer fails to get established,
    /// sending a message to a peer fails or when we get a disconnected
    /// event from a peer.
    pub fn set_unresponsive(
        &self,
        peer: Url,
        when: Timestamp,
    ) -> BoxFut<'_, K2Result<()>> {
        let space_map = self.space_map.lock().unwrap().clone();
        Box::pin(async move {
            for (space_id, space_handler) in space_map.iter() {
                if let Err(e) =
                    space_handler.set_unresponsive(peer.clone(), when).await
                {
                    tracing::error!("Failed to mark peer with url {peer} as unresponsive in space {space_id}: {e}");
                };
            }
            Ok(())
        })
    }
}

impl std::fmt::Debug for TxImpHnd {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f,"TxImpHnd {{ handler: {:?}, space_map: [{} entries], mod_map: [{} entries] }}", self.handler, self.space_map.lock().unwrap().len(), self.mod_map.lock().unwrap().len() )
    }
}

/// A low-level transport implementation.
pub trait TxImp: 'static + Send + Sync + std::fmt::Debug {
    /// Get the current url if any.
    fn url(&self) -> Option<Url>;

    /// Indicates that the implementation should close any open connections to
    /// the given peer. If a payload is provided, the implementation can
    /// make a best effort to send it to the remote first on a short timeout.
    /// Regardless of the success of the payload send, the connection should
    /// be closed.
    fn disconnect(
        &self,
        peer: Url,
        payload: Option<(String, bytes::Bytes)>,
    ) -> BoxFut<'_, ()>;

    /// Indicates that the implementation should send the payload to the remote
    /// peer, opening a connection if needed.
    fn send(&self, peer: Url, data: bytes::Bytes) -> BoxFut<'_, K2Result<()>>;

    /// Get the list of connected peers.
    fn get_connected_peers(&self) -> BoxFut<'_, K2Result<Vec<Url>>>;

    /// Dump network stats.
    fn dump_network_stats(&self) -> BoxFut<'_, K2Result<TransportStats>>;
}

/// Trait-object [TxImp].
pub type DynTxImp = Arc<dyn TxImp>;

/// A high-level wrapper around a low-level [DynTxImp] transport implementation.
#[cfg_attr(any(test, feature = "mockall"), mockall::automock)]
pub trait Transport: 'static + Send + Sync + std::fmt::Debug {
    /// Register a space handler for receiving incoming notifications.
    ///
    /// Panics if you attempt to register a duplicate handler for
    /// a space.
    ///
    /// Returns the current url if any.
    fn register_space_handler(
        &self,
        space_id: SpaceId,
        handler: DynTxSpaceHandler,
    ) -> Option<Url>;

    /// Register a module handler for receiving incoming module messages.
    ///
    /// Panics if you attempt to register a duplicate handler for the
    /// same (space_id, module).
    fn register_module_handler(
        &self,
        space_id: SpaceId,
        module: String,
        handler: DynTxModuleHandler,
    );

    /// Make a best effort to notify a peer that we are disconnecting and why.
    /// After a short time out, the connection will be closed even if the
    /// disconnect reason message is still pending.
    fn disconnect(&self, peer: Url, reason: Option<String>) -> BoxFut<'_, ()>;

    /// Notify a remote peer within a space. This is a fire-and-forget
    /// type message. The future this call returns will indicate any errors
    /// that occur up to the point where the message is handed off to
    /// the transport backend. After that, the future will return `Ok(())`
    /// but the remote peer may or may not actually receive the message.
    fn send_space_notify(
        &self,
        peer: Url,
        space_id: SpaceId,
        data: bytes::Bytes,
    ) -> BoxFut<'_, K2Result<()>>;

    /// Notify a remote peer module within a space. This is a fire-and-forget
    /// type message. The future this call returns will indicate any errors
    /// that occur up to the point where the message is handed off to
    /// the transport backend. After that, the future will return `Ok(())`
    /// but the remote peer may or may not actually receive the message.
    fn send_module(
        &self,
        peer: Url,
        space_id: SpaceId,
        module: String,
        data: bytes::Bytes,
    ) -> BoxFut<'_, K2Result<()>>;

    /// Get the list of connected peers.
    fn get_connected_peers(&self) -> BoxFut<'_, K2Result<Vec<Url>>>;

    /// Unregister a space handler and all module handlers for that space.
    fn unregister_space(&self, space_id: SpaceId) -> BoxFut<'_, ()>;

    /// Dump network stats.
    fn dump_network_stats(&self) -> BoxFut<'_, K2Result<TransportStats>>;

    /// Increment the count of blocked messages for the given peer URL.
    fn incr_blocked_message_count(
        &self,
        peer_url: Url,
        space_id: SpaceId,
    ) -> BoxFut<'_, K2Result<()>>;

    /// Read the count of blocked messages for a given peer URL and space.
    fn blocked_message_count(
        &self,
        peer_url: Url,
        space_id: SpaceId,
    ) -> BoxFut<'_, K2Result<u32>>;
}

/// Trait-object [Transport].
pub type DynTransport = Arc<dyn Transport>;

/// A weak trait-object [Transport].
///
/// This is provided in the API as a suggestion for modules that store a reference to the transport
/// for sending messages but also implement [`TxModuleHandler`]. When registering as a module
/// handler, the transport keeps a reference to your module. If you then store an owned reference
/// to the transport, you create a circular reference. By using a weak reference instead, you can
/// create a well-behaved module that will be dropped when a space shuts down.
pub type WeakDynTransport = Weak<dyn Transport>;

/// A high-level wrapper around a low-level [DynTxImp] transport implementation.
#[derive(Clone, Debug)]
pub struct DefaultTransport {
    imp: DynTxImp,
    space_map: Arc<Mutex<HashMap<SpaceId, DynTxSpaceHandler>>>,
    mod_map: Arc<Mutex<HashMap<(SpaceId, String), DynTxModuleHandler>>>,
    blocked_message_counts: Arc<Mutex<HashMap<(Url, SpaceId), u32>>>,
}

impl DefaultTransport {
    /// When constructing a [Transport] from a [TransportFactory],
    /// this function does the actual wrapping of your implemementation
    /// to produce the [Transport] struct.
    ///
    /// [DefaultTransport] is built to be used with the provided [TxImpHnd].
    pub fn create(hnd: &TxImpHnd, imp: DynTxImp) -> DynTransport {
        let out: DynTransport = Arc::new(DefaultTransport {
            imp,
            space_map: hnd.space_map.clone(),
            mod_map: hnd.mod_map.clone(),
            blocked_message_counts: hnd.blocked_message_counts.clone(),
        });
        out
    }
}

impl Transport for DefaultTransport {
    fn register_space_handler(
        &self,
        space_id: SpaceId,
        handler: DynTxSpaceHandler,
    ) -> Option<Url> {
        let mut lock = self.space_map.lock().unwrap();
        if lock.insert(space_id.clone(), handler).is_some() {
            panic!("Attempted to register duplicate space handler! {space_id}");
        }
        // keep the lock locked while we fetch the url for atomicity.
        self.imp.url()
    }

    fn register_module_handler(
        &self,
        space_id: SpaceId,
        module: String,
        handler: DynTxModuleHandler,
    ) {
        if self
            .mod_map
            .lock()
            .unwrap()
            .insert((space_id.clone(), module.clone()), handler)
            .is_some()
        {
            panic!(
                "Attempted to register duplicate module handler! {space_id} {module}"
            );
        }
    }

    fn disconnect(&self, peer: Url, reason: Option<String>) -> BoxFut<'_, ()> {
        Box::pin(async move {
            let payload = match reason {
                None => None,
                Some(reason) => match (K2Proto {
                    ty: K2WireType::Disconnect as i32,
                    data: bytes::Bytes::copy_from_slice(reason.as_bytes()),
                    space_id: None,
                    module_id: None,
                })
                .encode()
                {
                    Ok(payload) => Some((reason, payload)),
                    Err(_) => None,
                },
            };

            self.imp.disconnect(peer, payload).await;
        })
    }

    fn send_space_notify(
        &self,
        peer: Url,
        space_id: SpaceId,
        data: bytes::Bytes,
    ) -> BoxFut<'_, K2Result<()>> {
        Box::pin(async move {
            let enc = (K2Proto {
                ty: K2WireType::Notify as i32,
                data,
                space_id: Some(space_id.into()),
                module_id: None,
            })
            .encode()?;
            self.imp.send(peer, enc).await
        })
    }

    fn send_module(
        &self,
        peer: Url,
        space_id: SpaceId,
        module: String,
        data: bytes::Bytes,
    ) -> BoxFut<'_, K2Result<()>> {
        Box::pin(async move {
            let enc = (K2Proto {
                ty: K2WireType::Module as i32,
                data,
                space_id: Some(space_id.into()),
                module_id: Some(module),
            })
            .encode()?;
            self.imp.send(peer, enc).await
        })
    }

    fn get_connected_peers(&self) -> BoxFut<'_, K2Result<Vec<Url>>> {
        self.imp.get_connected_peers()
    }

    fn unregister_space(&self, space_id: SpaceId) -> BoxFut<'_, ()> {
        Box::pin(async move {
            // Remove the space handler.
            self.space_map.lock().unwrap().remove(&space_id);

            // Remove all module handlers for this space.
            // Done by keeping all module handlers that are not for this space.
            self.mod_map
                .lock()
                .unwrap()
                .retain(|(s, _), _| s != &space_id);
        })
    }

    fn dump_network_stats(&self) -> BoxFut<'_, K2Result<TransportStats>> {
        self.imp.dump_network_stats()
    }

    fn incr_blocked_message_count(
        &self,
        peer_url: Url,
        space_id: SpaceId,
    ) -> BoxFut<'_, K2Result<()>> {
        Box::pin(async {
            let mut blocked_message_counts =
                self.blocked_message_counts.lock().expect("poisoned");
            blocked_message_counts
                .entry((peer_url, space_id))
                .and_modify(|c| *c += 1)
                .or_insert(1);
            Ok(())
        })
    }

    /// Read the count of blocked messages for a given peer URL and space.
    fn blocked_message_count(
        &self,
        peer_url: Url,
        space_id: SpaceId,
    ) -> BoxFut<'_, K2Result<u32>> {
        Box::pin(async {
            let blocked_message_counts =
                self.blocked_message_counts.lock().expect("poisoned");
            Ok(*blocked_message_counts
                .get(&(peer_url, space_id))
                .unwrap_or(&0))
        })
    }
}

/// Base trait for transport handler events.
/// The other three handler types are all based on this trait.
pub trait TxBaseHandler: 'static + Send + Sync + std::fmt::Debug {
    /// A notification that a new listening address has been bound.
    /// Peers should now go to this new address to reach this node.
    fn new_listening_address(&self, this_url: Url) -> BoxFut<'static, ()> {
        drop(this_url);
        Box::pin(async move {})
    }

    /// A peer has connected to us. In addition to the preflight
    /// logic in [TxHandler], this callback allows space and module
    /// logic to block connections to peers. Simply return an Err here.
    fn peer_connect(&self, peer: Url) -> K2Result<()> {
        drop(peer);
        Ok(())
    }

    /// A peer has disconnected from us. If they did so gracefully
    /// the reason will be is_some().
    fn peer_disconnect(&self, peer: Url, reason: Option<String>) {
        drop((peer, reason));
    }
}

/// Handler for whole transport-level events.
pub trait TxHandler: TxBaseHandler {
    /// Gather preflight data to send to a new opening connection.
    /// Returning an Err result will close this connection.
    ///
    /// The default implementation sends an empty preflight message.
    fn preflight_gather_outgoing(
        &self,
        peer_url: Url,
    ) -> BoxFut<'_, K2Result<bytes::Bytes>> {
        drop(peer_url);
        Box::pin(async { Ok(bytes::Bytes::new()) })
    }

    /// Validate preflight data sent by a remote peer on a new connection.
    /// Returning an Err result will close this connection.
    ///
    /// The default implementation ignores the preflight data,
    /// and considers it valid.
    fn preflight_validate_incoming(
        &self,
        peer_url: Url,
        data: bytes::Bytes,
    ) -> BoxFut<'_, K2Result<()>> {
        drop((peer_url, data));
        Box::pin(async { Ok(()) })
    }
}

/// Trait-object [TxHandler].
pub type DynTxHandler = Arc<dyn TxHandler>;

/// Handler for space-related events.
pub trait TxSpaceHandler: TxBaseHandler {
    /// The sync handler for receiving notifications sent by a remote
    /// peer in reference to a particular space. If this callback returns
    /// an error, then the connection which sent the message will be closed.
    fn recv_space_notify(
        &self,
        peer: Url,
        space_id: SpaceId,
        data: bytes::Bytes,
    ) -> K2Result<()> {
        drop((peer, space_id, data));
        Ok(())
    }

    /// Mark a peer as unresponsive in the space's peer meta store
    fn set_unresponsive(
        &self,
        peer: Url,
        when: Timestamp,
    ) -> BoxFut<'_, K2Result<()>> {
        drop((peer, when));
        Box::pin(async move { Ok(()) })
    }
}

/// Trait-object [TxSpaceHandler].
pub type DynTxSpaceHandler = Arc<dyn TxSpaceHandler>;

/// Handler for module-related events.
pub trait TxModuleHandler: TxBaseHandler {
    /// The sync handler for receiving module messages sent by a remote
    /// peer in reference to a particular space. If this callback returns
    /// an error, then the connection which sent the message will be closed.
    fn recv_module_msg(
        &self,
        peer: Url,
        space_id: SpaceId,
        module: String,
        data: bytes::Bytes,
    ) -> K2Result<()> {
        drop((peer, space_id, module, data));
        Ok(())
    }
}

/// Trait-object [TxModuleHandler].
pub type DynTxModuleHandler = Arc<dyn TxModuleHandler>;

/// A factory for constructing Transport instances.
pub trait TransportFactory: 'static + Send + Sync + std::fmt::Debug {
    /// Help the builder construct a default config from the chosen
    /// module factories.
    fn default_config(&self, config: &mut config::Config) -> K2Result<()>;

    /// Validate configuration.
    fn validate_config(&self, config: &config::Config) -> K2Result<()>;

    /// Construct a transport instance.
    fn create(
        &self,
        builder: Arc<builder::Builder>,
        handler: DynTxHandler,
    ) -> BoxFut<'static, K2Result<DynTransport>>;
}

/// Trait-object [TransportFactory].
pub type DynTransportFactory = Arc<dyn TransportFactory>;

/// Stats for a transport connection.
///
/// This is intended to be a state dump that gives some insight into what the transport is doing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransportStats {
    /// The networking backend that is in use.
    pub backend: String,

    /// The list of peer urls that this Kitsune2 instance can currently be reached at.
    pub peer_urls: Vec<Url>,

    /// The list of current connections.
    pub connections: Vec<TransportConnectionStats>,
}

/// Stats for a single transport connection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransportConnectionStats {
    /// The public key of the remote peer.
    pub pub_key: String,

    /// The message count sent on this connection.
    pub send_message_count: u64,

    /// The bytes sent on this connection.
    pub send_bytes: u64,

    /// The message count received on this connection.
    pub recv_message_count: u64,

    /// The bytes received on this connection
    pub recv_bytes: u64,

    /// UNIX epoch timestamp in seconds when this connection was opened.
    pub opened_at_s: u64,

    /// True if this connection has successfully upgraded to webrtc.
    pub is_webrtc: bool,
}
