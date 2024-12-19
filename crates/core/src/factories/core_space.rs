//! The core space implementation provided by Kitsune2.

use kitsune2_api::{config::*, space::*, *};
use std::sync::Arc;

mod protocol;

/// The core space implementation provided by Kitsune2.
/// You probably will have no reason to use something other than this.
/// This abstraction is mainly here for testing purposes.
#[derive(Debug)]
pub struct CoreSpaceFactory {}

impl CoreSpaceFactory {
    /// Construct a new CoreSpaceFactory.
    pub fn create() -> DynSpaceFactory {
        let out: DynSpaceFactory = Arc::new(CoreSpaceFactory {});
        out
    }
}

impl SpaceFactory for CoreSpaceFactory {
    fn default_config(&self, _config: &mut Config) -> K2Result<()> {
        Ok(())
    }

    fn create(
        &self,
        builder: Arc<builder::Builder>,
        handler: DynSpaceHandler,
        space: SpaceId,
        tx: transport::DynTransport,
    ) -> BoxFut<'static, K2Result<DynSpace>> {
        Box::pin(async move {
            let peer_store = builder.peer_store.create(builder.clone()).await?;
            tx.register_space_handler(
                space.clone(),
                Arc::new(TxHandlerTranslator(handler)),
            );
            let out: DynSpace = Arc::new(CoreSpace::new(space, tx, peer_store));
            Ok(out)
        })
    }
}

#[derive(Debug)]
struct TxHandlerTranslator(DynSpaceHandler);

impl transport::TxBaseHandler for TxHandlerTranslator {}
impl transport::TxSpaceHandler for TxHandlerTranslator {
    fn recv_space_notify(
        &self,
        _peer: Url,
        space: SpaceId,
        data: bytes::Bytes,
    ) -> K2Result<()> {
        let dec = protocol::K2SpaceProto::decode(&data)?;
        self.0.recv_notify(
            dec.to_agent.into(),
            dec.from_agent.into(),
            space,
            dec.data,
        )
    }
}

#[derive(Debug)]
struct CoreSpace {
    space: SpaceId,
    tx: transport::DynTransport,
    peer_store: peer_store::DynPeerStore,
}

impl CoreSpace {
    pub fn new(
        space: SpaceId,
        tx: transport::DynTransport,
        peer_store: peer_store::DynPeerStore,
    ) -> Self {
        Self {
            space,
            tx,
            peer_store,
        }
    }
}

impl Space for CoreSpace {
    fn peer_store(&self) -> &peer_store::DynPeerStore {
        &self.peer_store
    }

    fn local_agent_join(
        &self,
        _local_agent: agent::DynLocalAgent,
    ) -> BoxFut<'_, K2Result<()>> {
        Box::pin(async move { todo!() })
    }

    fn local_agent_leave(&self, _local_agent: id::AgentId) -> BoxFut<'_, ()> {
        Box::pin(async move { todo!() })
    }

    fn send_notify(
        &self,
        to_agent: AgentId,
        from_agent: AgentId,
        data: bytes::Bytes,
    ) -> BoxFut<'_, K2Result<()>> {
        Box::pin(async move {
            let info = match self.peer_store.get(to_agent.clone()).await? {
                Some(info) => info,
                None => {
                    // TODO - once discovery is implemented try to
                    //        look up the peer from the network.
                    return Err(K2Error::other(format!(
                        "to_agent {to_agent} not found"
                    )));
                }
            };
            let url = match &info.url {
                Some(url) => url.clone(),
                None => {
                    return Err(K2Error::other(format!(
                        "to_agent {to_agent} is offline"
                    )));
                }
            };

            let enc = protocol::K2SpaceProto {
                to_agent: to_agent.into(),
                from_agent: from_agent.into(),
                data,
            }
            .encode()?;

            self.tx
                .send_space_notify(url, self.space.clone(), enc)
                .await
        })
    }
}

#[cfg(test)]
mod test;
