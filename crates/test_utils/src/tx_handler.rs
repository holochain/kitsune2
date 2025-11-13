//! A test TxHandler implementation that implements pre-flight agent info sharing.

use bytes::BufMut;
use kitsune2_api::{
    AgentInfoSigned, BoxFut, DynTxHandler, DynVerifier, K2Error, K2Result,
    LocalAgent, Space, TxBaseHandler, TxHandler, Url,
};
use std::sync::{Arc, Weak};
use tokio::sync::OnceCell;

/// A test [`TxHandler`] implementation that implements pre-flight agent info sharing.
#[derive(Debug)]
pub struct TestTxHandler {
    space: Arc<OnceCell<Weak<dyn Space>>>,
    verifier: DynVerifier,
}

impl TestTxHandler {
    /// Create a new instance of the [`TestTxHandler`].
    pub fn create(
        space: Arc<OnceCell<Weak<dyn Space>>>,
        verifier: DynVerifier,
    ) -> DynTxHandler {
        Arc::new(Self { space, verifier })
    }
}

impl TxBaseHandler for TestTxHandler {}

impl TxHandler for TestTxHandler {
    fn preflight_gather_outgoing(
        &self,
        _peer_url: Url,
    ) -> BoxFut<'_, K2Result<bytes::Bytes>> {
        let Some(space) = self
            .space
            .get()
            .expect("Space should have been initialized")
            .upgrade()
            .clone()
        else {
            return Box::pin(async {
                Err(K2Error::other("Space has been dropped"))
            });
        };

        Box::pin(async move {
            let agents = space.peer_store().get_all().await?;

            let local_agents = space.local_agent_store().get_all().await?;
            // Filter to only local agents
            let agents: Vec<Arc<AgentInfoSigned>> = agents
                .into_iter()
                .filter(|a| {
                    local_agents.iter().any(|la| la.agent() == &a.agent)
                })
                .collect();

            let mut buf = bytes::BytesMut::new().writer();
            serde_json::to_writer(
                &mut buf,
                &agents
                    .iter()
                    .map(|a| {
                        serde_json::to_value(
                            &serde_json::from_str::<serde_json::Value>(
                                &a.encode()?,
                            )
                            .map_err(K2Error::other)?,
                        )
                        .map_err(K2Error::other)
                    })
                    .collect::<K2Result<Vec<_>>>()?,
            )
            .map_err(K2Error::other)?;

            Ok(buf.into_inner().freeze())
        })
    }

    fn preflight_validate_incoming(
        &self,
        _peer_url: Url,
        data: bytes::Bytes,
    ) -> BoxFut<'_, K2Result<()>> {
        let Some(space) = self
            .space
            .get()
            .expect("Space should have been initialized")
            .upgrade()
            .clone()
        else {
            return Box::pin(async {
                Err(K2Error::other("Space has been dropped"))
            });
        };

        Box::pin(async move {
            let agents = AgentInfoSigned::decode_list(&self.verifier, &data)
                .map_err(K2Error::other)?
                .into_iter()
                .collect::<K2Result<Vec<_>>>()?;

            space.peer_store().insert(agents).await?;

            Ok(())
        })
    }
}
