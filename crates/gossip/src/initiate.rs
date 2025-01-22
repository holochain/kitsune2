use crate::peer_meta_store::K2PeerMetaStore;
use crate::K2GossipConfig;
use futures::StreamExt;
use kitsune2_api::agent::LocalAgent;
use kitsune2_api::peer_store::DynPeerStore;
use kitsune2_api::{DynLocalAgentStore, DynPeerMetaStore, SpaceId, Timestamp};
use rand::prelude::SliceRandom;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;
use tokio::task::AbortHandle;

pub fn initiate_task(
    space_id: SpaceId,
    config: Arc<K2GossipConfig>,
    peer_store: DynPeerStore,
    local_agent_store: DynLocalAgentStore,
    peer_meta_store: Arc<K2PeerMetaStore>,
) -> AbortHandle {
    tracing::info!("Start initiate task");

    let interval = config.interval();
    tokio::task::spawn(async move {
        loop {
            tokio::time::sleep(interval.clone()).await;

            let current_time = Timestamp::now();

            let Ok(local_agents) = local_agent_store.get_all().await else {
                tracing::error!("Failed to get local agents");
                continue;
            };
            if local_agents.is_empty() {
                tracing::info!("Skipping initiating gossip because there are no local agents");
                continue;
            }

            // Get the TARGET storage arcs for all local agents
            //
            // We should gossip to try to gather data that is within our target storage arcs so
            // that we are working towards a complete set of data and can claim those sectors.
            let local_arcs = local_agents.iter().map(|a| a.get_tgt_storage_arc()).collect::<HashSet<_>>();

            // Discover remote agents that overlap with at least one of our local agents
            let mut all_agents = HashSet::new();
            for local_arc in local_arcs {
                let Ok(by_local_arc_agents) = peer_store.get_by_overlapping_storage_arc(local_arc).await else {
                    tracing::error!("Failed to get all agents");
                    continue;
                };

                all_agents.extend(by_local_arc_agents);
            }

            // Filter local agents out of the list of all agents
            let local_agents = local_agents.into_iter().map(|a| a.agent().clone()).collect::<HashSet<_>>();
            all_agents.retain(|a| !local_agents.contains(&a.get_agent_info().agent));

            for agent in all_agents {
                // Agent hasn't provided a URL, we won't be able to gossip with them.
                let Some(url) = agent.url.clone() else {
                    continue;
                };

                let Ok(timestamp) = peer_meta_store.last_gossip_timestamp(url).await else {
                    tracing::warn!("Failed to get last gossip timestamp for {}", url);
                    continue;
                };

                // Too soon to gossip with this peer again
                if let Some(timestamp) = timestamp {
                    if (current_time - timestamp).unwrap_or(Duration::MAX) < interval {
                        continue;
                    }
                }

                
            }

        }
    })
    .abort_handle()
}
