//! Publish is a Kitsune2 module for publishing new ops and agent infos to peers.
//!
//! It consists of multiple parts:
//! - A task that sends Op ids to peers
//! - A task that adds received Op ids from other peers to the fetch queue
//!
//! ### Publish Ops
//!
//! #### Outgoing publishing of Ops
//!
//! A channel acts as the queue for outgoing ops to be published. Ops to be
//! published are sent one by one through the channel to the receiving task,
//! processing requests in incoming order and dispatching it to the
//! transport module.
//!
//! #### Incoming of published Ops task
//!
//! Similarly to outgoing ops to be published, a channel serves as a queue
//! for incoming published ops. The queue processes items in order of the
//! incoming messages and adds the associated Op ids to the fetch queue.
//!

use kitsune2_api::*;
use message_handler::PublishMessageHandler;
use std::sync::Arc;
use tokio::{
    sync::mpsc::{channel, Receiver, Sender},
    task::AbortHandle,
};

#[cfg(test)]
mod test;

mod message_handler;

/// CorePublish module name.
pub const PUBLISH_MOD_NAME: &str = "Publish";

/// CorePublish configuration types.
mod config {
    /// Configuration parameters for [CorePublishFactory](super::CorePublishFactory).
    #[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
    #[serde(rename_all = "camelCase")]
    pub struct CorePublishConfig {}

    /// Module-level configuration for CorePublish.
    #[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
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
        transport: DynTransport,
    ) -> BoxFut<'static, K2Result<DynPublish>> {
        Box::pin(async move {
            let config: CorePublishModConfig =
                builder.config.get_module_config()?;
            let out: DynPublish = Arc::new(CorePublish::new(
                config.core_publish,
                space_id,
                fetch,
                transport,
            ));
            Ok(out)
        })
    }
}

type OutgoingPublishOps = (OpId, Url);
type IncomingPublishOps = (Vec<OpId>, Url);

#[derive(Debug)]
struct CorePublish {
    outgoing_publish_ops_tx: Sender<OutgoingPublishOps>,
    tasks: Vec<AbortHandle>,
}

impl CorePublish {
    fn new(
        config: CorePublishConfig,
        space_id: SpaceId,
        fetch: DynFetch,
        transport: DynTransport,
    ) -> Self {
        Self::spawn_tasks(config, space_id, fetch, transport)
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
}

impl CorePublish {
    pub fn spawn_tasks(
        _config: CorePublishConfig,
        space_id: SpaceId,
        fetch: DynFetch,
        transport: DynTransport,
    ) -> Self {
        // Create a queue to process outgoing op publishes. Publishes are sent to peers.
        let (outgoing_publish_ops_tx, outgoing_publish_ops_rx) =
            channel::<OutgoingPublishOps>(16_384);

        // Create a queue to process incoming op publishes. Incoming publishes are added to the
        // fetch queue
        let (incoming_publish_ops_tx, incoming_publish_ops_rx) =
            channel::<IncomingPublishOps>(16_384);

        let mut tasks = Vec::new();

        // Spawn outgoing publish ops task.
        let outgoing_publish_ops_task =
            tokio::task::spawn(CorePublish::outgoing_publish_ops_task(
                outgoing_publish_ops_rx,
                space_id.clone(),
                transport.clone(),
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

        // Register transport module handler for incoming op and agent publishes.
        let message_handler = Arc::new(PublishMessageHandler {
            incoming_publish_ops_tx,
        });

        transport.register_module_handler(
            space_id.clone(),
            PUBLISH_MOD_NAME.to_string(),
            message_handler.clone(),
        );

        Self {
            outgoing_publish_ops_tx,
            tasks,
        }
    }

    async fn outgoing_publish_ops_task(
        mut outgoing_publish_ops_rx: Receiver<OutgoingPublishOps>,
        space_id: SpaceId,
        transport: DynTransport,
    ) {
        while let Some((op_id, peer_url)) = outgoing_publish_ops_rx.recv().await
        {
            // Send ops publish message to peer.
            let data = serialize_publish_ops_message(vec![op_id.clone()]);
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
                    ?op_id,
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

            // Retrieve ops to send from store.
            if let Err(err) = fetch.request_ops(op_ids.clone(), peer).await {
                tracing::warn!(
                        "could not insert publish ops request into fetch queue: {err}"
                    );
            };
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
