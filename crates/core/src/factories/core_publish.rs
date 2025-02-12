//! Fetch is a Kitsune2 module for fetching ops from peers.
//!
//! In particular it tracks which ops need to be fetched from which peers,
//! sends fetch requests and processes incoming requests and responses.
//!
//! It consists of multiple parts:
//! - State object that tracks op and peer urls in memory
//! - Fetch tasks that request tracked ops from peers
//! - An incoming request task that retrieves ops from the op store and responds with the
//!   ops to the requester
//! - An incoming response task that writes ops to the op store and removes their ids
//!   from the state object
//!
//!
//! ### Publish task
//!
//! #### Outgoing publish requests
//!
//!
//!
//!
//!
//!
//!

use kitsune2_api::*;
use message_handler::PublishMessageHandler;
use prost::Message;
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    sync::mpsc::{channel, Receiver, Sender},
    task::JoinHandle,
};

#[cfg(test)]
mod test;

mod message_handler;

/// CorePublish module name.
pub const MOD_NAME: &str = "Publish";

/// CoreFetch configuration types.
mod config {
    /// Configuration parameters for [CoreFetchFactory](super::CoreFetchFactory).
    #[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct CorePublishConfig {}

    impl Default for CorePublishConfig {
        // Maximum back off is 11:40 min.
        fn default() -> Self {
            Self {}
        }
    }

    /// Module-level configuration for CoreFetch.
    #[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct CorePublishModConfig {
        /// CoreFetch configuration.
        pub core_publish: CorePublishConfig,
    }
}

pub use config::*;

/// A production-ready publish module.
#[derive(Debug)]
pub struct CorePublishFactory {}

impl CorePublishFactory {
    fn default_config(&self, config: &mut Config) -> K2Result<()> {
        config.set_module_config(&CorePublishModConfig::default())?;
        Ok(())
    }

    fn validate_config(&self, _config: &Config) -> K2Result<()> {
        Ok(())
    }

    /// Construct a new CorePublishFactory.
    pub fn create() -> DynPublishFactory {
        Arc::new(Self {})
    }
}

impl PublishFactory for CorePublishFactory {
    fn create(
        &self,
        builder: Arc<Builder>,
        space_id: SpaceId,
        fetch: DynFetch,
        peer_store: DynPeerStore,
        transport: DynTransport,
    ) -> BoxFut<'static, K2Result<DynPublish>> {
        Box::pin(async move {
            let config: CorePublishModConfig =
                builder.config.get_module_config()?;
            let out: DynPublish = Arc::new(CorePublish::new(
                config.core_publish,
                space_id,
                fetch,
                peer_store,
                transport,
            ));
            Ok(out)
        })
    }
}

// type OutgoingRequest = (OpId, Url);
// type IncomingRequest = (Vec<OpId>, Url);
// type IncomingResponse = Vec<Op>;
type OutgoingPublishOps = (OpId, Url);
type OutgoingAgentInfo = (AgentInfoSigned, Url);
type IncomingPublishOps = (Vec<OpId>, Url);

#[derive(Debug)]
struct CorePublish {
    outgoing_publish_ops_tx: Sender<OutgoingPublishOps>,
    outgoing_publish_agent_tx: Sender<OutgoingAgentInfo>,
    tasks: Vec<JoinHandle<()>>,
    #[cfg(test)]
    message_handler: DynTxModuleHandler,
}

impl CorePublish {
    fn new(
        config: CorePublishConfig,
        space_id: SpaceId,
        fetch: DynFetch,
        peer_store: DynPeerStore,
        transport: DynTransport,
    ) -> Self {
        Self::spawn_tasks(config, space_id, fetch, peer_store, transport)
    }
}

impl Publish for CorePublish {
    fn publish_ops(
        &self,
        op_ids: Vec<OpId>,
        target: Url,
    ) -> BoxFut<'_, K2Result<()>> {
        Box::pin(async move {
            // Insert requests into publish queue.
            for op_id in op_ids {
                if let Err(err) = self
                    .outgoing_publish_ops_tx
                    .send((op_id, target.clone()))
                    .await
                {
                    tracing::warn!(
                        "could not insert ops into ops publish queue: {err}"
                    );
                }
            }

            Ok(())
        })
    }

    fn publish_agent(
        &self,
        agent_info: AgentInfoSigned,
        target: Url,
    ) -> BoxFut<'_, K2Result<()>> {
        Box::pin(async move {
            // Insert requests into publish queue.
            if let Err(err) = self
                .outgoing_publish_agent_tx
                .send((agent_info, target))
                .await
            {
                tracing::warn!(
                    "could not insert signed agent info into agent publish queue: {err}"
                );
            }

            Ok(())
        })
    }
}

impl CorePublish {
    pub fn spawn_tasks(
        _config: CorePublishConfig,
        space_id: SpaceId,
        fetch: DynFetch,
        peer_store: DynPeerStore,
        transport: DynTransport,
    ) -> Self {
        // Create a queue to process outgoing op publishes. Publishes are sent to peers.
        let (outgoing_publish_ops_tx, outgoing_publish_ops_rx) =
            channel::<OutgoingPublishOps>(16_384);

        // Create a queue to process incoming op publishes. Incoming publishes are added to the
        // fetch queue
        let (incoming_publish_ops_tx, incoming_publish_ops_rx) =
            channel::<IncomingPublishOps>(16_384);

        // Create a queue to process outgoing agent publishes. Publishes are sent to peers.
        let (outgoing_publish_agent_tx, outgoing_publish_agent_rx) =
            channel::<OutgoingAgentInfo>(16_384);

        // Create a queue to process incoming agent publishes. Incoming agent infos are added to the peer store.
        let (incoming_publish_agent_tx, incoming_publish_agent_rx) =
            channel::<AgentInfoSigned>(16_384);

        let mut tasks = Vec::new();

        // Spawn outgoing publish ops task.
        let outgoing_publish_ops_task =
            tokio::task::spawn(CorePublish::outgoing_publish_ops_task(
                outgoing_publish_ops_rx,
                space_id.clone(),
                transport.clone(),
            ));
        tasks.push(outgoing_publish_ops_task);

        // Spawn incoming publish ops task.
        let incoming_publish_ops_task =
            tokio::task::spawn(CorePublish::incoming_publish_ops_task(
                incoming_publish_ops_rx,
                fetch,
            ));
        tasks.push(incoming_publish_ops_task);

        // Spawn outgoing publish agent task.
        let outgoing_publish_agent_task =
            tokio::task::spawn(CorePublish::outgoing_publish_agent_task(
                outgoing_publish_agent_rx,
                space_id.clone(),
                transport.clone(),
            ));
        tasks.push(outgoing_publish_agent_task);

        // Spawn incoming publish agent task.
        let incoming_publish_agent_task =
            tokio::task::spawn(CorePublish::incoming_publish_agent_task(
                incoming_publish_agent_rx,
                peer_store,
            ));
        tasks.push(incoming_publish_agent_task);

        // Register transport module handler for incoming op and agent publishes.
        let message_handler = Arc::new(PublishMessageHandler {
            incoming_publish_ops_tx,
            incoming_publish_agent_tx,
        });

        transport.register_module_handler(
            space_id.clone(),
            MOD_NAME.to_string(),
            message_handler.clone(),
        );

        Self {
            outgoing_publish_ops_tx,
            outgoing_publish_agent_tx,
            tasks,
            #[cfg(test)]
            message_handler,
        }
    }

    async fn outgoing_publish_ops_task(
        mut outgoing_publish_ops_rx: Receiver<OutgoingPublishOps>,
        space_id: SpaceId,
        transport: DynTransport,
    ) {
        while let Some((op_id, peer_url)) = outgoing_publish_ops_rx.recv().await
        {
            // Send fetch request to peer.
            let data = serialize_publish_ops_message(vec![op_id.clone()]);
            match transport
                .send_module(
                    peer_url.clone(),
                    space_id.clone(),
                    MOD_NAME.to_string(),
                    data,
                )
                .await
            {
                Err(err) => {
                    tracing::warn!(
                        ?op_id,
                        ?peer_url,
                        "could not send publish ops: {err}"
                    );
                }
                Ok(()) => (),
            }
        }
    }

    async fn incoming_publish_ops_task(
        mut response_rx: Receiver<IncomingPublishOps>,
        fetch: DynFetch,
    ) {
        while let Some((op_ids, peer)) = response_rx.recv().await {
            tracing::debug!(?peer, ?op_ids, "incoming publish ops");

            // Retrieve ops to send from store.
            match fetch.request_ops(op_ids.clone(), peer).await {
                Err(err) => {
                    tracing::warn!(
                        "could not insert publish ops request into fetch queue: {err}"
                    );
                    continue;
                }
                Ok(()) => (),
            };
        }
    }

    async fn outgoing_publish_agent_task(
        mut outgoing_publish_agent_rx: Receiver<OutgoingAgentInfo>,
        space_id: SpaceId,
        transport: DynTransport,
    ) {
        while let Some((agent_info, peer_url)) =
            outgoing_publish_agent_rx.recv().await
        {
            // Send fetch request to peer.
            match serialize_publish_agent_message(&agent_info) {
                Ok(data) => match transport
                    .send_module(
                        peer_url.clone(),
                        space_id.clone(),
                        MOD_NAME.to_string(),
                        data,
                    )
                    .await
                {
                    Err(err) => {
                        tracing::warn!(
                            ?agent_info,
                            ?peer_url,
                            "could not send publish agent: {err}"
                        );
                    }
                    Ok(()) => (),
                },
                Err(err) => {
                    tracing::warn!(
                        ?agent_info,
                        ?peer_url,
                        "Failed to serialize publish agent message: {err}"
                    )
                }
            };
        }
    }

    async fn incoming_publish_agent_task(
        mut response_rx: Receiver<AgentInfoSigned>,
        peer_store: DynPeerStore,
    ) {
        while let Some(agent_info) = response_rx.recv().await {
            // QUESTION: Do we need to update the peer meta store as well here, i.e. move stuff
            // from the
            match peer_store.insert(vec![Arc::new(agent_info)]).await {
                Err(err) => {
                    tracing::warn!(
                        "could not insert published agent info into peer store: {err}"
                    );
                    continue;
                }
                Ok(()) => (),
            }
        }
    }
}

impl Drop for CorePublish {
    fn drop(&mut self) {
        for t in self.tasks.iter() {
            t.abort();
        }
    }
}
