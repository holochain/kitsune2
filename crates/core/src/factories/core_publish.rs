//! Publish is a Kitsune2 module for publishing new ops and agent infos to peers.
//!
//! It consists of multiple parts:
//! - A task that sends Op ids to peers
//! - A task that adds received Op ids from other peers to the fetch queue
//! - A task that sends Agent infos to peers
//! - A task that verifies the signature of received agent infos and inserts
//!   them into the peer store.
//!
//!
//! ### Publish Ops
//!
//! #### Outgoing publishing of Ops
//!
//! A channel acts as the queue for outgoing ops to be published. Lists of
//! Ops to be published are sent one by one through the channel to the receiving task,
//! processing publish requests in incoming order and dispatching them to the
//! transport module.
//!
//! #### Incoming of published Ops task
//!
//! Similarly to outgoing ops to be published, a channel serves as a queue
//! for incoming published ops. The queue processes ops lists in order of the
//! incoming messages and adds the associated Op ids to the fetch queue.
//!
//!
//! ### Publish Agent Info
//!
//! #### Outgoing publishing of agent infos
//!
//! A channel acts as the queue for outgoing agent infos to be published.
//! Agent infos to be published are sent one by one through the channel
//! to the receiving task, processing requests in incoming order and
//! dispatching them to the transport module.
//!
//! #### Incoming of published agent infos task
//!
//! Similarly to outgoing agent infos to be published, a channel serves as a
//! queue for incoming published agent infos. The queue processes items in
//! order of the incoming messages and decodes and verifies associated
//! agent infos before inserting them into the peer store.
//!
//!

use kitsune2_api::*;
use message_handler::PublishMessageHandler;
use std::sync::Arc;
use tokio::{
    sync::mpsc::{channel, Receiver, Sender},
    task::AbortHandle,
};

mod message_handler;

#[cfg(test)]
mod test;

/// CorePublish module name.
pub const PUBLISH_MOD_NAME: &str = "Publish";

/// CorePublish configuration types.
mod config {
    /// Configuration parameters for [CorePublishFactory](super::CorePublishFactory).
    #[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
    #[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
    #[serde(rename_all = "camelCase")]
    pub struct CorePublishConfig {}

    /// Module-level configuration for CorePublish.
    #[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
    #[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
    #[serde(rename_all = "camelCase")]
    pub struct CorePublishModConfig {
        /// CorePublish configuration.
        pub core_publish: CorePublishConfig,
    }
}

pub use config::*;

/// A production-ready publish module.
#[derive(Debug)]
pub struct CorePublishFactory {}

impl CorePublishFactory {
    /// Construct a new CorePublishFactory.
    pub fn create() -> DynPublishFactory {
        Arc::new(Self {})
    }
}

impl PublishFactory for CorePublishFactory {
    fn default_config(&self, config: &mut Config) -> K2Result<()> {
        config.set_module_config(&CorePublishModConfig::default())?;
        Ok(())
    }

    fn validate_config(&self, _config: &Config) -> K2Result<()> {
        Ok(())
    }
    fn create(
        &self,
        builder: Arc<Builder>,
        space_id: SpaceId,
        fetch: DynFetch,
        peer_store: DynPeerStore,
        peer_meta_store: DynPeerMetaStore,
        transport: DynTransport,
    ) -> BoxFut<'static, K2Result<DynPublish>> {
        Box::pin(async move {
            let config: CorePublishModConfig =
                builder.config.get_module_config()?;
            let out: DynPublish = Arc::new(CorePublish::new(
                config.core_publish,
                space_id,
                builder,
                fetch,
                peer_store,
                peer_meta_store,
                transport,
            ));
            Ok(out)
        })
    }
}

type OutgoingPublishOps = (Vec<OpId>, Url);
type IncomingPublishOps = (Vec<OpId>, Url);
type OutgoingAgentInfo = (Arc<AgentInfoSigned>, Url);
type IncomingAgentInfoEncoded = String;

#[derive(Debug)]
struct CorePublish {
    outgoing_publish_ops_tx: Sender<OutgoingPublishOps>,
    outgoing_publish_agent_tx: Sender<OutgoingAgentInfo>,
    tasks: Vec<AbortHandle>,
}

impl CorePublish {
    fn new(
        config: CorePublishConfig,
        space_id: SpaceId,
        builder: Arc<Builder>,
        fetch: DynFetch,
        peer_store: DynPeerStore,
        peer_meta_store: DynPeerMetaStore,
        transport: DynTransport,
    ) -> Self {
        Self::spawn_tasks(
            config,
            space_id,
            builder,
            fetch,
            peer_store,
            peer_meta_store,
            transport,
        )
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
            if let Err(err) = self
                .outgoing_publish_ops_tx
                .send((op_ids, target.clone()))
                .await
            {
                tracing::warn!(
                    "could not insert ops into ops publish queue: {err}"
                );
            }

            Ok(())
        })
    }

    fn publish_agent(
        &self,
        agent_info: Arc<AgentInfoSigned>,
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
        builder: Arc<Builder>,
        fetch: DynFetch,
        peer_store: DynPeerStore,
        peer_meta_store: DynPeerMetaStore,
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
            channel::<IncomingAgentInfoEncoded>(16_384);

        let mut tasks = Vec::new();

        // Spawn outgoing publish ops task.
        let outgoing_publish_ops_task =
            tokio::task::spawn(CorePublish::outgoing_publish_ops_task(
                outgoing_publish_ops_rx,
                space_id.clone(),
                peer_meta_store,
                Arc::downgrade(&transport),
            ))
            .abort_handle();
        tasks.push(outgoing_publish_ops_task);

        // Spawn incoming publish ops task.
        let incoming_publish_ops_task =
            tokio::task::spawn(CorePublish::incoming_publish_ops_task(
                incoming_publish_ops_rx,
                fetch,
            ))
            .abort_handle();
        tasks.push(incoming_publish_ops_task);

        // Spawn outgoing publish agent task.
        let outgoing_publish_agent_task =
            tokio::task::spawn(CorePublish::outgoing_publish_agent_task(
                outgoing_publish_agent_rx,
                space_id.clone(),
                Arc::downgrade(&transport),
            ))
            .abort_handle();
        tasks.push(outgoing_publish_agent_task);

        // Spawn incoming publish agent task.
        let incoming_publish_agent_task =
            tokio::task::spawn(CorePublish::incoming_publish_agent_task(
                incoming_publish_agent_rx,
                peer_store,
                builder,
            ))
            .abort_handle();
        tasks.push(incoming_publish_agent_task);

        // Register transport module handler for incoming op and agent publishes.
        let message_handler = Arc::new(PublishMessageHandler {
            incoming_publish_ops_tx,
            incoming_publish_agent_tx,
        });

        transport.register_module_handler(
            space_id.clone(),
            PUBLISH_MOD_NAME.to_string(),
            message_handler.clone(),
        );

        Self {
            outgoing_publish_ops_tx,
            outgoing_publish_agent_tx,
            tasks,
        }
    }

    async fn outgoing_publish_ops_task(
        mut outgoing_publish_ops_rx: Receiver<OutgoingPublishOps>,
        space_id: SpaceId,
        peer_meta_store: DynPeerMetaStore,
        transport: WeakDynTransport,
    ) {
        while let Some((op_ids, peer_url)) =
            outgoing_publish_ops_rx.recv().await
        {
            let Some(transport) = transport.upgrade() else {
                tracing::warn!("Transport dropped, stopping publish ops task");
                return;
            };

            // Check if peer URL to publish to is unresponsive.
            let peer_url_unresponsive = match peer_meta_store
                .get_unresponsive(peer_url.clone())
                .await
            {
                Ok(maybe_value) => maybe_value.is_some(),
                Err(err) => {
                    tracing::warn!(?err, "could not query peer meta store");
                    false
                }
            };
            if peer_url_unresponsive {
                // Peer URL is unresponsive, do not publish.
                continue;
            }

            // Send ops publish message to peer.
            let data = serialize_publish_ops_message(op_ids.clone());
            if let Err(err) = transport
                .send_module(
                    peer_url.clone(),
                    space_id.clone(),
                    PUBLISH_MOD_NAME.to_string(),
                    data,
                )
                .await
            {
                tracing::warn!(
                    ?op_ids,
                    ?peer_url,
                    "could not send publish ops: {err}"
                );
            }
        }
    }

    async fn incoming_publish_ops_task(
        mut response_rx: Receiver<IncomingPublishOps>,
        fetch: DynFetch,
    ) {
        while let Some((op_ids, peer)) = response_rx.recv().await {
            tracing::debug!(?peer, ?op_ids, "incoming publish ops");

            // Add incoming op ids to the fetch queue to let that retrieve the op data
            if let Err(err) = fetch.request_ops(op_ids.clone(), peer).await {
                tracing::warn!(
                    "could not insert publish ops request into fetch queue: {err}"
                );
            };
        }
    }

    async fn outgoing_publish_agent_task(
        mut outgoing_publish_agent_rx: Receiver<OutgoingAgentInfo>,
        space_id: SpaceId,
        transport: WeakDynTransport,
    ) {
        while let Some((agent_info, peer_url)) =
            outgoing_publish_agent_rx.recv().await
        {
            let Some(transport) = transport.upgrade() else {
                tracing::warn!(
                    "Transport dropped, stopping publish agent task"
                );
                return;
            };

            // Send fetch request to peer.
            match serialize_publish_agent_message(&agent_info) {
                Ok(data) => {
                    if let Err(err) = transport
                        .send_module(
                            peer_url.clone(),
                            space_id.clone(),
                            PUBLISH_MOD_NAME.to_string(),
                            data,
                        )
                        .await
                    {
                        tracing::debug!(
                            ?agent_info,
                            ?peer_url,
                            "could not send publish agent: {err}"
                        );
                    }
                }
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
        mut response_rx: Receiver<IncomingAgentInfoEncoded>,
        peer_store: DynPeerStore,
        builder: Arc<Builder>,
    ) {
        while let Some(agent_info_encoded) = response_rx.recv().await {
            match AgentInfoSigned::decode(
                &builder.verifier,
                agent_info_encoded.as_bytes(),
            ) {
                Ok(agent_info) => {
                    if let Err(err) = peer_store.insert(vec![agent_info]).await
                    {
                        tracing::warn!(
                            "could not insert published agent info into peer store: {err}"
                        );
                    }
                }
                Err(err) => {
                    tracing::warn!("Failed to decode signed agent info: {err}");
                }
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
