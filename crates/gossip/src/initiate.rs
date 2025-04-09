use crate::gossip::K2Gossip;
use crate::peer_meta_store::K2PeerMetaStore;
use crate::K2GossipConfig;
use backon::BackoffBuilder;
use kitsune2_api::*;
use kitsune2_api::{AgentId, DynLocalAgentStore, K2Result, Timestamp, Url};
use rand::prelude::SliceRandom;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;
use tokio::task::AbortHandle;

pub fn spawn_initiate_task(
    config: Arc<K2GossipConfig>,
    gossip: K2Gossip,
) -> AbortHandle {
    tracing::info!("Starting initiate task");

    let mut rush_to_first_initiated_round = true;

    let initial_initiate_interval = config.initial_initiate_interval();
    let mut rush_backoff = backon::ExponentialBuilder::new()
        .with_min_delay(Duration::from_millis(10))
        .with_max_delay(initial_initiate_interval)
        .with_factor(1.2)
        .with_jitter()
        .build();

    let initiate_interval = config.initiate_interval();
    let initiate_jitter_ms = config.initiate_jitter_ms as u64;
    let min_initiate_interval = config.min_initiate_interval();
    tokio::task::spawn(async move {
        loop {
            if rush_to_first_initiated_round {
                tokio::time::sleep(rush_backoff.next().unwrap_or(initiate_interval)).await;
            } else {
                let jitter = if initiate_jitter_ms > 0 {
                    Duration::from_millis(rand::random::<u64>() % initiate_jitter_ms)
                } else {
                    Duration::ZERO
                };
                tokio::time::sleep(initiate_interval + jitter).await;
            }

            if gossip.initiated_round_state.lock().await.is_some() {
                tracing::info!("Not initiating gossip because there is already an initiated round");
                continue;
            }

            match select_next_target(
                min_initiate_interval,
                gossip.peer_store.clone(),
                gossip.local_agent_store.clone(),
                gossip.peer_meta_store.clone(),
            )
            .await
            {
                Ok(Some(url)) => {
                    match gossip.initiate_gossip(url.clone()).await {
                        Ok(true) => {
                            rush_to_first_initiated_round = true;
                            tracing::info!("Initiated gossip with {}", url);
                        }
                        Ok(false) => {
                            // Don't log here, will already have logged the reason
                        }
                        Err(e) => {
                            tracing::error!("Error initiating gossip: {:?}", e);
                        }
                    }
                }
                Ok(None) => {
                    // Nobody to gossip with, expect `select_next_target` to have logged a reason
                }
                Err(e) => {
                    tracing::error!("Error selecting target: {:?}", e);
                }
            }
        }
    })
    .abort_handle()
}

async fn select_next_target(
    min_interval: Duration,
    peer_store: DynPeerStore,
    local_agent_store: DynLocalAgentStore,
    peer_meta_store: Arc<K2PeerMetaStore>,
) -> K2Result<Option<Url>> {
    let current_time = Timestamp::now();

    let local_agents = local_agent_store.get_all().await?;
    if local_agents.is_empty() {
        tracing::info!(
            "Skipping initiating gossip because there are no local agents"
        );
        return Ok(None);
    }

    // Get the TARGET storage arcs for all local agents
    //
    // We should gossip to try to gather data that is within our target storage arcs so
    // that we are working towards a complete set of data and can claim those sectors.
    let local_arcs = local_agents
        .iter()
        .map(|a| a.get_tgt_storage_arc())
        .collect::<HashSet<_>>();

    let local_agent_ids = local_agents
        .into_iter()
        .map(|a| a.agent().clone())
        .collect::<HashSet<_>>();

    // Discover remote agents whose arc overlaps with at least one of our local agents' arcs
    let mut all_agents = HashSet::new();
    for local_arc in local_arcs {
        let by_local_arc_agents =
            peer_store.get_by_overlapping_storage_arc(local_arc).await?;

        all_agents.extend(by_local_arc_agents);
    }

    // Filter local agents out of the list of all agents
    remove_local_agents(&mut all_agents, &local_agent_ids);

    let mut using_overlapping_agents = true;

    // There are no agents with an overlapping arc to gossip with. We should cast the net
    // wider and gossip with agents that might still be growing their arc and have some ops
    // or agent infos that we are missing
    if all_agents.is_empty() {
        tracing::info!(
            "No agents with overlapping arcs available, selecting from all agents"
        );

        all_agents = peer_store.get_all().await?.into_iter().collect();
        remove_local_agents(&mut all_agents, &local_agent_ids);
        using_overlapping_agents = false;
    }

    let mut possible_targets = filter_by_recently_gossiped(
        all_agents,
        peer_meta_store.clone(),
        current_time,
        min_interval,
    )
    .await?;

    // We tried using overlapping agents, but they're all on timeout
    if possible_targets.is_empty() && using_overlapping_agents {
        tracing::info!(
            "All agents with overlapping arcs are on timeout, selecting from all agents"
        );

        all_agents = peer_store.get_all().await?.into_iter().collect();
        remove_local_agents(&mut all_agents, &local_agent_ids);

        possible_targets = filter_by_recently_gossiped(
            all_agents,
            peer_meta_store.clone(),
            current_time,
            min_interval,
        )
        .await?;
    }

    // All options exhausted, give up for now
    if possible_targets.is_empty() {
        tracing::info!("No agents to gossip with");
        return Ok(None);
    }

    // Sort by last gossip timestamp with None first and then by oldest to newest
    possible_targets.sort_by_key(|(timestamp, _)| *timestamp);

    let last_new_peer = possible_targets
        .iter()
        .enumerate()
        .find(|(_, (t, _))| t.is_some())
        .map(|(i, _)| i);
    match last_new_peer {
        Some(0) => {
            // Nothing to do
        }
        Some(end_of_new_peers) => {
            possible_targets[0..end_of_new_peers]
                .shuffle(&mut rand::thread_rng());
        }
        None => {
            possible_targets.shuffle(&mut rand::thread_rng());
        }
    }

    // We've already filtered for missing URLs so this is always `Some`
    Ok(possible_targets[0].1.url.clone())
}

fn remove_local_agents(
    agents: &mut HashSet<Arc<AgentInfoSigned>>,
    local_agents: &HashSet<AgentId>,
) {
    agents.retain(|a| !local_agents.contains(&a.get_agent_info().agent));
}

async fn filter_by_recently_gossiped(
    all_agents: HashSet<Arc<AgentInfoSigned>>,
    peer_meta_store: Arc<K2PeerMetaStore>,
    current_time: Timestamp,
    min_interval: Duration,
) -> K2Result<Vec<(Option<Timestamp>, Arc<AgentInfoSigned>)>> {
    let mut possible_targets = Vec::with_capacity(all_agents.len());
    for agent in all_agents {
        // Agent hasn't provided a URL, we won't be able to gossip with them.
        let Some(url) = agent.url.clone() else {
            continue;
        };

        let timestamp =
            peer_meta_store.last_gossip_timestamp(url.clone()).await?;

        // Too soon to gossip with this peer again
        if let Some(timestamp) = timestamp {
            if (current_time - timestamp).unwrap_or(Duration::MAX)
                < min_interval
            {
                continue;
            }
        }

        possible_targets.push((timestamp, agent));
    }

    Ok(possible_targets)
}

#[cfg(test)]
mod tests {
    use super::*;
    use kitsune2_core::default_test_builder;
    use kitsune2_dht::SECTOR_SIZE;
    use kitsune2_test_utils::agent::{AgentBuilder, TestLocalAgent};
    use kitsune2_test_utils::enable_tracing;
    use std::sync::Arc;

    struct Harness {
        peer_store: DynPeerStore,
        local_agent_store: DynLocalAgentStore,
        peer_meta_store: Arc<K2PeerMetaStore>,
    }

    impl Harness {
        async fn create() -> Self {
            let builder =
                Arc::new(default_test_builder().with_default_config().unwrap());

            Harness {
                peer_store: builder
                    .peer_store
                    .create(builder.clone())
                    .await
                    .unwrap(),
                local_agent_store: builder
                    .local_agent_store
                    .create(builder.clone())
                    .await
                    .unwrap(),
                peer_meta_store: Arc::new(K2PeerMetaStore::new(
                    builder
                        .peer_meta_store
                        .create(
                            builder.clone(),
                            kitsune2_test_utils::space::TEST_SPACE_ID.clone(),
                        )
                        .await
                        .unwrap(),
                )),
            }
        }

        async fn new_local_agent(&self, arc: DhtArc) -> Arc<AgentInfoSigned> {
            let local_agent: DynLocalAgent =
                Arc::new(TestLocalAgent::default());
            local_agent.set_tgt_storage_arc_hint(arc);

            let agent_info_signed = AgentBuilder::default()
                .with_url(Some(Url::from_str("ws://test:80/local").unwrap()))
                .build(local_agent.clone());

            self.local_agent_store.add(local_agent).await.unwrap();
            self.peer_store
                .insert(vec![agent_info_signed.clone()])
                .await
                .unwrap();

            agent_info_signed
        }

        async fn new_remote_agent(
            &self,
            peer_url: Option<Url>,
            storage_arc: Option<DhtArc>,
            target_arc: Option<DhtArc>,
        ) -> Arc<AgentInfoSigned> {
            let local_agent: DynLocalAgent =
                Arc::new(TestLocalAgent::default());
            if let Some(arc) = storage_arc {
                local_agent.set_cur_storage_arc(arc);
            }
            if let Some(arc) = target_arc {
                local_agent.set_tgt_storage_arc_hint(arc);
            }

            let mut agent_builder = AgentBuilder::default().with_url(peer_url);
            if let Some(arc) = storage_arc {
                agent_builder = agent_builder.with_storage_arc(arc);
            }
            let agent_info_signed = agent_builder.build(local_agent.clone());

            self.peer_store
                .insert(vec![agent_info_signed.clone()])
                .await
                .unwrap();

            agent_info_signed
        }
    }

    #[tokio::test]
    async fn skip_when_no_local_agents() {
        enable_tracing();

        let harness = Harness::create().await;

        let url = select_next_target(
            Duration::from_secs(60),
            harness.peer_store.clone(),
            harness.local_agent_store.clone(),
            harness.peer_meta_store.clone(),
        )
        .await
        .unwrap();

        assert_eq!(None, url);
    }

    #[tokio::test]
    async fn select_peer() {
        enable_tracing();

        let harness = Harness::create().await;

        harness.new_local_agent(DhtArc::FULL).await;

        let agent_2 = harness
            .new_remote_agent(
                Some(Url::from_str("ws://test:80/1").unwrap()),
                Some(DhtArc::FULL),
                Some(DhtArc::FULL),
            )
            .await;

        let url = select_next_target(
            Duration::from_secs(60),
            harness.peer_store.clone(),
            harness.local_agent_store.clone(),
            harness.peer_meta_store.clone(),
        )
        .await
        .unwrap();

        assert_eq!(agent_2.url.clone().unwrap(), url.unwrap())
    }

    #[tokio::test]
    async fn select_by_last_gossip_timestamp() {
        enable_tracing();

        let harness = Harness::create().await;

        harness.new_local_agent(DhtArc::FULL).await;
        let agent_2 = harness
            .new_remote_agent(
                Some(Url::from_str("ws://test:80/2").unwrap()),
                Some(DhtArc::FULL),
                Some(DhtArc::FULL),
            )
            .await;
        let agent_3 = harness
            .new_remote_agent(
                Some(Url::from_str("ws://test:80/3").unwrap()),
                Some(DhtArc::FULL),
                Some(DhtArc::FULL),
            )
            .await;

        let min_interval = K2GossipConfig::default().min_initiate_interval();

        // Mark both of the remote agents as having gossiped previously.
        // They should be selected by how long ago they gossiped.
        harness
            .peer_meta_store
            .set_last_gossip_timestamp(
                agent_2.url.clone().unwrap(),
                (Timestamp::now()
                    - Duration::from_secs(min_interval.as_secs() + 30))
                .unwrap(),
            )
            .await
            .unwrap();
        harness
            .peer_meta_store
            .set_last_gossip_timestamp(
                agent_3.url.clone().unwrap(),
                (Timestamp::now()
                    - Duration::from_secs(min_interval.as_secs() + 60))
                .unwrap(),
            )
            .await
            .unwrap();

        let url = select_next_target(
            min_interval,
            harness.peer_store.clone(),
            harness.local_agent_store.clone(),
            harness.peer_meta_store.clone(),
        )
        .await
        .unwrap();

        assert_eq!(agent_3.url.clone().unwrap(), url.unwrap());

        // Now that agent_3 was selected for gossip, mark them as having gossiped recently so that
        // they shouldn't be selected on the next pass.
        harness
            .peer_meta_store
            .set_last_gossip_timestamp(
                agent_3.url.clone().unwrap(),
                Timestamp::now(),
            )
            .await
            .unwrap();

        let url = select_next_target(
            min_interval,
            harness.peer_store.clone(),
            harness.local_agent_store.clone(),
            harness.peer_meta_store.clone(),
        )
        .await
        .unwrap();

        assert_eq!(agent_2.url.clone().unwrap(), url.unwrap());

        // Do the same thing for agent_2, so they aren't a valid pick for the next target
        harness
            .peer_meta_store
            .set_last_gossip_timestamp(
                agent_2.url.clone().unwrap(),
                Timestamp::now(),
            )
            .await
            .unwrap();

        let url = select_next_target(
            min_interval,
            harness.peer_store.clone(),
            harness.local_agent_store.clone(),
            harness.peer_meta_store.clone(),
        )
        .await
        .unwrap();

        assert_eq!(None, url);
    }

    #[tokio::test]
    async fn skip_peer_with_missing_url() {
        enable_tracing();

        let harness = Harness::create().await;

        harness.new_local_agent(DhtArc::FULL).await;

        harness
            .new_remote_agent(None, Some(DhtArc::FULL), Some(DhtArc::FULL))
            .await;
        let agent_3 = harness
            .new_remote_agent(
                Some(Url::from_str("ws://test:80/3").unwrap()),
                Some(DhtArc::FULL),
                Some(DhtArc::FULL),
            )
            .await;

        let url = select_next_target(
            Duration::from_secs(60),
            harness.peer_store.clone(),
            harness.local_agent_store.clone(),
            harness.peer_meta_store.clone(),
        )
        .await
        .unwrap();

        assert_eq!(agent_3.url.clone().unwrap(), url.unwrap());

        harness
            .peer_meta_store
            .set_last_gossip_timestamp(
                agent_3.url.clone().unwrap(),
                Timestamp::now(),
            )
            .await
            .unwrap();

        let url = select_next_target(
            Duration::from_secs(60),
            harness.peer_store.clone(),
            harness.local_agent_store.clone(),
            harness.peer_meta_store.clone(),
        )
        .await
        .unwrap();

        assert_eq!(None, url);
    }

    #[tokio::test]
    async fn prioritise_never_gossiped_peers() {
        enable_tracing();

        let harness = Harness::create().await;

        harness.new_local_agent(DhtArc::FULL).await;

        let agent_2 = harness
            .new_remote_agent(
                Some(Url::from_str("ws://test:80/2").unwrap()),
                Some(DhtArc::FULL),
                Some(DhtArc::FULL),
            )
            .await;
        let agent_3 = harness
            .new_remote_agent(
                Some(Url::from_str("ws://test:80/3").unwrap()),
                Some(DhtArc::FULL),
                Some(DhtArc::FULL),
            )
            .await;

        let min_interval = K2GossipConfig::default().min_initiate_interval();

        // Mark that we've gossiped with agent_3, but not recently
        harness
            .peer_meta_store
            .set_last_gossip_timestamp(
                agent_3.url.clone().unwrap(),
                (Timestamp::now()
                    - Duration::from_secs(min_interval.as_secs() + 90))
                .unwrap(),
            )
            .await
            .unwrap();

        let url = select_next_target(
            min_interval,
            harness.peer_store.clone(),
            harness.local_agent_store.clone(),
            harness.peer_meta_store.clone(),
        )
        .await
        .unwrap();

        assert_eq!(agent_2.url.clone().unwrap(), url.unwrap());
    }

    #[tokio::test]
    async fn randomly_select_from_multiple_new_peers() {
        enable_tracing();

        let harness = Harness::create().await;

        harness.new_local_agent(DhtArc::FULL).await;

        let agent_2_url = Url::from_str("ws://test:80/2").unwrap();
        harness
            .new_remote_agent(
                Some(agent_2_url.clone()),
                Some(DhtArc::FULL),
                Some(DhtArc::FULL),
            )
            .await;
        let agent_3_url = Url::from_str("ws://test:80/3").unwrap();
        harness
            .new_remote_agent(
                Some(agent_3_url.clone()),
                Some(DhtArc::FULL),
                Some(DhtArc::FULL),
            )
            .await;

        let mut seen = HashSet::new();
        for _ in 0..100 {
            let url = select_next_target(
                Duration::from_secs(60),
                harness.peer_store.clone(),
                harness.local_agent_store.clone(),
                harness.peer_meta_store.clone(),
            )
            .await
            .unwrap();

            seen.insert(url.unwrap());
        }

        assert_eq!(2, seen.len());
        assert!(seen.contains(&agent_2_url));
        assert!(seen.contains(&agent_3_url));
    }

    #[tokio::test]
    async fn selects_non_overlapping_when_no_overlapping_peers_available() {
        enable_tracing();

        let harness = Harness::create().await;

        harness
            .new_local_agent(DhtArc::Arc(0, SECTOR_SIZE - 1))
            .await;

        // Two agents where the arcs DO NOT overlap with our local agent
        harness
            .new_remote_agent(
                Some(Url::from_str("ws://test:80/2").unwrap()),
                Some(DhtArc::Arc(SECTOR_SIZE, SECTOR_SIZE * 2 - 1)),
                Some(DhtArc::Arc(SECTOR_SIZE, SECTOR_SIZE * 2 - 1)),
            )
            .await;
        harness
            .new_remote_agent(
                Some(Url::from_str("ws://test:80/3").unwrap()),
                Some(DhtArc::Arc(SECTOR_SIZE, SECTOR_SIZE * 2 - 1)),
                Some(DhtArc::Arc(SECTOR_SIZE, SECTOR_SIZE * 2 - 1)),
            )
            .await;

        let mut seen = HashSet::new();
        while let Some(url) = select_next_target(
            Duration::from_secs(60),
            harness.peer_store.clone(),
            harness.local_agent_store.clone(),
            harness.peer_meta_store.clone(),
        )
        .await
        .unwrap()
        {
            harness
                .peer_meta_store
                .set_last_gossip_timestamp(url.clone(), Timestamp::now())
                .await
                .unwrap();

            seen.insert(url);
        }

        assert_eq!(2, seen.len());
    }

    #[tokio::test]
    async fn prioritises_overlapping_arcs() {
        enable_tracing();

        let harness = Harness::create().await;

        harness
            .new_local_agent(DhtArc::Arc(0, SECTOR_SIZE - 1))
            .await;

        // Overlapping
        let agent_2 = harness
            .new_remote_agent(
                Some(Url::from_str("ws://test:80/2").unwrap()),
                Some(DhtArc::Arc(0, SECTOR_SIZE * 10 - 1)),
                Some(DhtArc::Arc(0, SECTOR_SIZE * 10 - 1)),
            )
            .await;
        // Non-overlapping with our local agent
        let agent_3 = harness
            .new_remote_agent(
                Some(Url::from_str("ws://test:80/3").unwrap()),
                Some(DhtArc::Arc(SECTOR_SIZE, SECTOR_SIZE * 2 - 1)),
                Some(DhtArc::Arc(SECTOR_SIZE, SECTOR_SIZE * 2 - 1)),
            )
            .await;

        let mut seen = Vec::with_capacity(2);
        while let Some(url) = select_next_target(
            Duration::from_secs(60),
            harness.peer_store.clone(),
            harness.local_agent_store.clone(),
            harness.peer_meta_store.clone(),
        )
        .await
        .unwrap()
        {
            harness
                .peer_meta_store
                .set_last_gossip_timestamp(url.clone(), Timestamp::now())
                .await
                .unwrap();

            seen.push(url);
        }

        assert_eq!(2, seen.len());
        assert_eq!(agent_2.url.clone().unwrap(), seen[0]);
        assert_eq!(agent_3.url.clone().unwrap(), seen[1]);
    }
}
