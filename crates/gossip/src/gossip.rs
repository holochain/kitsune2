use crate::protocol::k2_gossip_message::GossipMessage;
use crate::protocol::{
    deserialize_gossip_message, serialize_gossip_message,
    K2GossipAcceptMessage, K2GossipMessage,
};
use crate::MOD_NAME;
use kitsune2_api::peer_store::DynPeerStore;
use kitsune2_api::transport::{DynTransport, TxBaseHandler, TxModuleHandler};
use kitsune2_api::{
    AgentId, DynGossip, DynGossipFactory, DynOpStore, Gossip, GossipFactory,
    K2Error, K2Result, SpaceId, Url,
};
use std::sync::Arc;

/// A factory for creating K2Gossip instances.
#[derive(Debug)]
pub struct K2GossipFactory;

impl K2GossipFactory {
    /// Construct a new [K2GossipFactory].
    pub fn create() -> DynGossipFactory {
        Arc::new(K2GossipFactory)
    }
}

impl GossipFactory for K2GossipFactory {
    fn default_config(
        &self,
        _config: &mut kitsune2_api::config::Config,
    ) -> K2Result<()> {
        // TODO config
        Ok(())
    }

    fn create(
        &self,
        _builder: Arc<kitsune2_api::builder::Builder>,
        space: SpaceId,
        peer_store: DynPeerStore,
        op_store: DynOpStore,
        transport: DynTransport,
    ) -> kitsune2_api::BoxFut<'static, K2Result<DynGossip>> {
        let gossip = K2Gossip::create(space, peer_store, op_store, transport);
        Box::pin(async move { Ok(gossip) })
    }
}

struct GossipResponse(bytes::Bytes, Url);

/// The gossip implementation.
///
/// This type acts as both an implementation of the [Gossip] trait and a [TxModuleHandler].
#[derive(Debug, Clone)]
struct K2Gossip {
    space: SpaceId,
    _peer_store: DynPeerStore,
    _op_store: DynOpStore,
    response_tx: tokio::sync::mpsc::Sender<GossipResponse>,
    response_task: tokio::task::AbortHandle,
}

impl Drop for K2Gossip {
    fn drop(&mut self) {
        tracing::info!("K2Gossip shutting down for space: {:?}", self.space);

        self.response_task.abort();
    }
}

impl K2Gossip {
    /// Construct a new [K2Gossip] instance.
    pub fn create(
        space: SpaceId,
        peer_store: DynPeerStore,
        op_store: DynOpStore,
        transport: DynTransport,
    ) -> DynGossip {
        let (response_tx, mut rx) =
            tokio::sync::mpsc::channel::<GossipResponse>(1024);
        let response_task = tokio::task::spawn({
            let space = space.clone();
            let transport = transport.clone();
            async move {
                while let Some(msg) = rx.recv().await {
                    transport
                        .send_module(
                            msg.1,
                            space.clone(),
                            MOD_NAME.to_string(),
                            msg.0,
                        )
                        .await
                        .unwrap();
                }
            }
        })
        .abort_handle();

        let gossip = K2Gossip {
            space: space.clone(),
            _peer_store: peer_store,
            _op_store: op_store,
            response_tx,
            response_task,
        };

        transport.register_module_handler(
            space,
            MOD_NAME.to_string(),
            Arc::new(gossip.clone()),
        );

        Arc::new(gossip)
    }
}

impl K2Gossip {
    /// Handle a gossip message.
    ///
    /// Produces a response message if the input is acceptable and a response is required.
    /// Otherwise, returns None.
    fn handle_gossip_message(
        &self,
        from: AgentId,
        msg: K2GossipMessage,
    ) -> K2Result<Option<K2GossipMessage>> {
        println!("handle_gossip_message from: {:?}, msg: {:?}", from, msg);

        let Some(msg) = msg.gossip_message else {
            return Err(K2Error::other("no gossip message"));
        };

        match msg {
            GossipMessage::Initiate(initiate) => {
                println!("Initiate: {:?}", initiate);

                Ok(Some(K2GossipMessage {
                    gossip_message: Some(GossipMessage::Accept(
                        K2GossipAcceptMessage {
                            participating_agents: vec![],
                            missing_agents: vec![],
                            new_since: 0,
                            new_ops: vec![],
                            updated_new_since: 0,
                        },
                    )),
                }))
            }
            GossipMessage::Accept(accept) => {
                println!("Accept: {:?}", accept);

                Ok(None)
            }
        }
    }
}

impl Gossip for K2Gossip {}

impl TxBaseHandler for K2Gossip {}
impl TxModuleHandler for K2Gossip {
    fn recv_module_msg(
        &self,
        peer: Url,
        space: SpaceId,
        module: String,
        data: bytes::Bytes,
    ) -> K2Result<()> {
        if self.space != space {
            return Err(K2Error::other("wrong space"));
        }

        if module != MOD_NAME {
            return Err(K2Error::other(format!(
                "wrong module name: {}",
                module
            )));
        }

        let peer_id = peer
            .peer_id()
            .ok_or_else(|| K2Error::other("no peer id"))?
            .as_bytes()
            .to_vec();
        let agent_id = AgentId::from(bytes::Bytes::from(peer_id));

        let msg = deserialize_gossip_message(data)?;
        let res = self.handle_gossip_message(agent_id, msg)?;

        if let Some(msg) = res {
            self.response_tx
                .try_send(GossipResponse(serialize_gossip_message(msg)?, peer))
                .map_err(|e| {
                    K2Error::other_src("could not send response", e)
                })?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::protocol::k2_gossip_message::GossipMessage;
    use crate::protocol::{serialize_gossip_message, K2GossipInitiateMessage};
    use kitsune2_api::builder::Builder;
    use kitsune2_api::transport::{TxHandler, TxSpaceHandler};
    use kitsune2_core::default_test_builder;
    use kitsune2_test_utils::enable_tracing;

    struct TestGossipFactory {
        space: SpaceId,
        builder: Arc<Builder>,
    }

    impl TestGossipFactory {
        pub async fn create(space: SpaceId) -> TestGossipFactory {
            let mut builder = default_test_builder();
            // Replace the core builder with a real gossip factory
            builder.gossip = K2GossipFactory::create();
            let builder = Arc::new(builder.with_default_config().unwrap());

            TestGossipFactory { space, builder }
        }

        pub async fn new_instance(&self) -> (DynGossip, DynTransport, Url) {
            #[derive(Debug)]
            struct NoopHandler;
            impl TxBaseHandler for NoopHandler {}
            impl TxHandler for NoopHandler {}

            impl TxSpaceHandler for NoopHandler {}

            let transport = self
                .builder
                .transport
                .create(self.builder.clone(), Arc::new(NoopHandler))
                .await
                .unwrap();
            let gossip = self
                .builder
                .gossip
                .create(
                    self.builder.clone(),
                    self.space.clone(),
                    self.builder
                        .peer_store
                        .create(self.builder.clone())
                        .await
                        .unwrap(),
                    self.builder
                        .op_store
                        .create(self.builder.clone(), self.space.clone())
                        .await
                        .unwrap(),
                    transport.clone(),
                )
                .await
                .unwrap();

            let url = transport.register_space_handler(
                self.space.clone(),
                Arc::new(NoopHandler),
            );

            (gossip, transport, url.unwrap())
        }
    }

    fn test_space() -> SpaceId {
        SpaceId::from(bytes::Bytes::from_static(b"test-space"))
    }

    #[tokio::test]
    async fn create_gossip_instance() {
        let factory = TestGossipFactory::create(test_space()).await;
        factory.new_instance().await;
    }

    #[tokio::test]
    async fn send_initiate() {
        enable_tracing();

        let space = test_space();
        let factory = TestGossipFactory::create(space.clone()).await;
        let (_instance_1, _transport_1, addr_1) = factory.new_instance().await;
        let (_instance_2, transport_2, _) = factory.new_instance().await;

        transport_2
            .send_module(
                addr_1,
                space.clone(),
                MOD_NAME.to_string(),
                serialize_gossip_message(K2GossipMessage {
                    gossip_message: Some(GossipMessage::Initiate(
                        K2GossipInitiateMessage {
                            participating_agents: vec![],
                            new_since: 0,
                        },
                    )),
                })
                .unwrap(),
            )
            .await
            .unwrap();

        // TODO assert the outcome via the op store and peer metadata
    }
}
