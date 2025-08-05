use kitsune2_api::{
    AgentInfoSigned, DhtArc, DynLocalAgentStore, DynPeerMetaStore,
    DynPeerStore, K2Result, KEY_PREFIX_ROOT, META_KEY_UNRESPONSIVE,
};
use std::collections::HashSet;
use std::sync::Arc;

/// Get all remote agents from the peer store.
///
/// Uses the [DynPeerStore] to get all agents and the [DynLocalAgentStore] to filter out local
/// agents.
pub async fn get_all_remote_agents(
    peer_store: DynPeerStore,
    local_agent_store: DynLocalAgentStore,
) -> K2Result<Vec<Arc<AgentInfoSigned>>> {
    let all_agents = peer_store.get_all().await?;
    filter_local_agents(local_agent_store, all_agents).await
}

/// Get remote agents by overlapping storage arc.
///
/// Uses the [DynPeerStore] to get agents with overlapping storage arcs and the [DynLocalAgentStore]
/// to filter out local agents.
pub async fn get_remote_agents_by_overlapping_storage_arc(
    peer_store: DynPeerStore,
    local_agent_store: DynLocalAgentStore,
    arc: DhtArc,
) -> K2Result<Vec<Arc<AgentInfoSigned>>> {
    let by_overlapping_storage_arc =
        peer_store.get_by_overlapping_storage_arc(arc).await?;
    filter_local_agents(local_agent_store, by_overlapping_storage_arc).await
}

/// Get remote agents near a specific location.
///
/// Uses the [DynPeerStore] to get agents near a location and the [DynLocalAgentStore] to filter
/// out local agents.
pub async fn get_remote_agents_near_location(
    peer_store: DynPeerStore,
    local_agent_store: DynLocalAgentStore,
    loc: u32,
    limit: usize,
) -> K2Result<Vec<Arc<AgentInfoSigned>>> {
    let near_location = peer_store.get_near_location(loc, limit).await?;
    filter_local_agents(local_agent_store, near_location).await
}

/// Get all responsive remote agents from the peer store.
///
/// Uses the [DynPeerStore] to get all agents, the [DynLocalAgentStore] to filter out local
/// agents and the [DynPeerMetaStore] to filter out unresponsive agents.
pub async fn get_all_responsive_remote_agents(
    peer_store: DynPeerStore,
    local_agent_store: DynLocalAgentStore,
    peer_meta_store: DynPeerMetaStore,
) -> K2Result<Vec<Arc<AgentInfoSigned>>> {
    let all_agents = peer_store.get_all().await?;
    let remote_agents =
        filter_local_agents(local_agent_store, all_agents).await?;
    filter_unresponsive_agents(peer_meta_store, remote_agents).await
}

/// Get responsive remote agents by overlapping storage arc.
///
/// Uses the [DynPeerStore] to get agents with overlapping storage arcs and the [DynLocalAgentStore]
/// to filter out local agents.
pub async fn get_responsive_remote_agents_by_overlapping_storage_arc(
    peer_store: DynPeerStore,
    local_agent_store: DynLocalAgentStore,
    peer_meta_store: DynPeerMetaStore,
    arc: DhtArc,
) -> K2Result<Vec<Arc<AgentInfoSigned>>> {
    let by_overlapping_storage_arc =
        peer_store.get_by_overlapping_storage_arc(arc).await?;
    let remote_agents =
        filter_local_agents(local_agent_store, by_overlapping_storage_arc)
            .await?;
    filter_unresponsive_agents(peer_meta_store, remote_agents).await
}

/// Get responsive remote agents near a specific location.
///
/// Uses the [DynPeerStore] to get agents near a location and the [DynLocalAgentStore] to filter
/// out local agents.
pub async fn get_responsive_remote_agents_near_location(
    peer_store: DynPeerStore,
    local_agent_store: DynLocalAgentStore,
    peer_meta_store: DynPeerMetaStore,
    loc: u32,
    limit: usize,
) -> K2Result<Vec<Arc<AgentInfoSigned>>> {
    let near_location = peer_store.get_near_location(loc, limit).await?;
    let remote_agents =
        filter_local_agents(local_agent_store, near_location).await?;
    filter_unresponsive_agents(peer_meta_store, remote_agents).await
}

async fn filter_local_agents(
    local_agent_store: DynLocalAgentStore,
    agent_list: Vec<Arc<AgentInfoSigned>>,
) -> K2Result<Vec<Arc<AgentInfoSigned>>> {
    let all_local_agents = local_agent_store
        .get_all()
        .await?
        .into_iter()
        .map(|a| a.agent().clone())
        .collect::<HashSet<_>>();

    Ok(agent_list
        .into_iter()
        .filter(|a| !all_local_agents.contains(&a.agent))
        .collect())
}

async fn filter_unresponsive_agents(
    peer_meta_store: DynPeerMetaStore,
    agent_list: Vec<Arc<AgentInfoSigned>>,
) -> K2Result<Vec<Arc<AgentInfoSigned>>> {
    let unresponsive_urls = peer_meta_store
        .get_all_by_key(
            format!("{KEY_PREFIX_ROOT}:{META_KEY_UNRESPONSIVE}").to_string(),
        )
        .await?;

    Ok(agent_list
        .into_iter()
        .filter(|a| {
            if let Some(url) = a.url.clone() {
                !unresponsive_urls.contains_key(&url)
            } else {
                false
            }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::default_test_builder;
    use kitsune2_api::{DynLocalAgent, Timestamp, Url};
    use kitsune2_test_utils::agent::{AgentBuilder, TestLocalAgent};
    use kitsune2_test_utils::space::TEST_SPACE_ID;
    use std::sync::Arc;

    #[tokio::test]
    async fn get_all_remote_agents_filters_local() {
        let remote_agent_1 =
            AgentBuilder::default().build(TestLocalAgent::default());
        let remote_agent_2 =
            AgentBuilder::default().build(TestLocalAgent::default());

        let local_agent_1: DynLocalAgent = Arc::new(TestLocalAgent::default());
        let remote_agent_for_local_agent_1 =
            AgentBuilder::default().build(local_agent_1.clone());

        let builder =
            Arc::new(default_test_builder().with_default_config().unwrap());

        let blocks = builder
            .blocks
            .create(builder.clone(), TEST_SPACE_ID)
            .await
            .unwrap();
        let peer_store = builder
            .peer_store
            .create(builder.clone(), TEST_SPACE_ID, blocks)
            .await
            .unwrap();
        let local_agent_store = builder
            .local_agent_store
            .create(builder.clone())
            .await
            .unwrap();

        peer_store
            .insert(vec![
                remote_agent_1.clone(),
                remote_agent_2.clone(),
                remote_agent_for_local_agent_1.clone(),
            ])
            .await
            .unwrap();

        local_agent_store.add(local_agent_1).await.unwrap();

        let all_remote_agents =
            get_all_remote_agents(peer_store, local_agent_store)
                .await
                .unwrap();

        assert_eq!(all_remote_agents.len(), 2);
        assert!(all_remote_agents.contains(&remote_agent_1));
        assert!(all_remote_agents.contains(&remote_agent_2));
        assert!(!all_remote_agents.contains(&remote_agent_for_local_agent_1));
    }

    #[tokio::test]
    async fn get_remote_agents_by_overlapping_storage_arc_filters_local() {
        let remote_agent_1 = AgentBuilder::default()
            .with_storage_arc(DhtArc::FULL)
            .build(TestLocalAgent::default());
        let remote_agent_2 = AgentBuilder::default()
            .with_storage_arc(DhtArc::Empty)
            .build(TestLocalAgent::default());

        let local_agent_1: DynLocalAgent = Arc::new(TestLocalAgent::default());
        let remote_agent_for_local_agent_1 = AgentBuilder::default()
            .with_storage_arc(DhtArc::FULL)
            .build(local_agent_1.clone());

        let builder =
            Arc::new(default_test_builder().with_default_config().unwrap());

        let blocks = builder
            .blocks
            .create(builder.clone(), TEST_SPACE_ID)
            .await
            .unwrap();

        let peer_store = builder
            .peer_store
            .create(builder.clone(), TEST_SPACE_ID, blocks)
            .await
            .unwrap();
        let local_agent_store = builder
            .local_agent_store
            .create(builder.clone())
            .await
            .unwrap();

        peer_store
            .insert(vec![
                remote_agent_1.clone(),
                remote_agent_2.clone(),
                remote_agent_for_local_agent_1.clone(),
            ])
            .await
            .unwrap();

        local_agent_store.add(local_agent_1).await.unwrap();

        let remote_with_overlapping_arc =
            get_remote_agents_by_overlapping_storage_arc(
                peer_store,
                local_agent_store,
                DhtArc::Arc(0, 100),
            )
            .await
            .unwrap();

        assert_eq!(remote_with_overlapping_arc.len(), 1);
        assert!(remote_with_overlapping_arc.contains(&remote_agent_1));
        assert!(!remote_with_overlapping_arc.contains(&remote_agent_2));
        assert!(!remote_with_overlapping_arc
            .contains(&remote_agent_for_local_agent_1));
    }

    #[tokio::test]
    async fn get_remote_agents_near_location_filters_local() {
        let remote_agent_1 = AgentBuilder::default()
            .with_storage_arc(DhtArc::FULL)
            .build(TestLocalAgent::default());
        let remote_agent_2 = AgentBuilder::default()
            .with_storage_arc(DhtArc::Empty)
            .build(TestLocalAgent::default());

        let local_agent_1: DynLocalAgent = Arc::new(TestLocalAgent::default());
        let remote_agent_for_local_agent_1 = AgentBuilder::default()
            .with_storage_arc(DhtArc::FULL)
            .build(local_agent_1.clone());

        let builder =
            Arc::new(default_test_builder().with_default_config().unwrap());

        let blocks = builder
            .blocks
            .create(builder.clone(), TEST_SPACE_ID)
            .await
            .unwrap();

        let peer_store = builder
            .peer_store
            .create(builder.clone(), TEST_SPACE_ID, blocks)
            .await
            .unwrap();
        let local_agent_store = builder
            .local_agent_store
            .create(builder.clone())
            .await
            .unwrap();

        peer_store
            .insert(vec![
                remote_agent_1.clone(),
                remote_agent_2.clone(),
                remote_agent_for_local_agent_1.clone(),
            ])
            .await
            .unwrap();

        local_agent_store.add(local_agent_1).await.unwrap();

        let remote_near_location = get_remote_agents_near_location(
            peer_store,
            local_agent_store,
            10,
            5,
        )
        .await
        .unwrap();

        assert_eq!(remote_near_location.len(), 1);
        assert!(remote_near_location.contains(&remote_agent_1));
        assert!(!remote_near_location.contains(&remote_agent_2));
        assert!(!remote_near_location.contains(&remote_agent_for_local_agent_1));
    }

    #[tokio::test]
    async fn get_all_responsive_remote_agents_filters_unresponsive() {
        let remote_agent_1 = AgentBuilder::default()
            .with_url(Some(Url::from_str("ws://responsi.ve:80/1").unwrap()))
            .build(TestLocalAgent::default());
        // Agents without URL are filtered out.
        let remote_agent_2 =
            AgentBuilder::default().build(TestLocalAgent::default());
        let local_agent: DynLocalAgent = Arc::new(TestLocalAgent::default());
        let remote_agent_for_local_agent = AgentBuilder::default()
            .with_url(Some(Url::from_str("ws://responsi.ve:80/3").unwrap()))
            .build(local_agent.clone());
        let unresponsive_agent = AgentBuilder::default()
            .with_url(Some(Url::from_str("ws://unresponsi.ve:80").unwrap()))
            .build(TestLocalAgent::default());

        let builder =
            Arc::new(default_test_builder().with_default_config().unwrap());
        let blocks = builder
            .blocks
            .create(builder.clone(), TEST_SPACE_ID)
            .await
            .unwrap();

        let peer_store = builder
            .peer_store
            .create(builder.clone(), TEST_SPACE_ID, blocks)
            .await
            .unwrap();
        let local_agent_store = builder
            .local_agent_store
            .create(builder.clone())
            .await
            .unwrap();
        let peer_meta_store = builder
            .peer_meta_store
            .create(builder.clone(), TEST_SPACE_ID)
            .await
            .unwrap();

        peer_store
            .insert(vec![
                remote_agent_1.clone(),
                remote_agent_2.clone(),
                remote_agent_for_local_agent.clone(),
                unresponsive_agent.clone(),
            ])
            .await
            .unwrap();
        local_agent_store.add(local_agent).await.unwrap();
        peer_meta_store
            .set_unresponsive(
                unresponsive_agent.url.clone().unwrap(),
                Timestamp::now(),
                Timestamp::now(),
            )
            .await
            .unwrap();

        let all_responsive_remote_agents = get_all_responsive_remote_agents(
            peer_store,
            local_agent_store,
            peer_meta_store,
        )
        .await
        .unwrap();
        assert_eq!(all_responsive_remote_agents.len(), 1);
        assert!(all_responsive_remote_agents.contains(&remote_agent_1));
        assert!(!all_responsive_remote_agents.contains(&remote_agent_2));
        assert!(!all_responsive_remote_agents
            .contains(&remote_agent_for_local_agent));
        assert!(!all_responsive_remote_agents.contains(&unresponsive_agent));
    }

    #[tokio::test]
    async fn get_responsive_remote_agents_by_overlapping_storage_arc_filters_unresponsive(
    ) {
        let remote_agent_1 = AgentBuilder::default()
            .with_storage_arc(DhtArc::FULL)
            .with_url(Some(Url::from_str("ws://responsi.ve:80/1").unwrap()))
            .build(TestLocalAgent::default());
        let remote_agent_2 = AgentBuilder::default()
            .with_storage_arc(DhtArc::Empty)
            .with_url(Some(Url::from_str("ws://responsi.ve:80/2").unwrap()))
            .build(TestLocalAgent::default());
        let local_agent: DynLocalAgent = Arc::new(TestLocalAgent::default());
        let remote_agent_for_local_agent = AgentBuilder::default()
            .with_storage_arc(DhtArc::FULL)
            .with_url(Some(Url::from_str("ws://responsi.ve:80/3").unwrap()))
            .build(local_agent.clone());
        let unresponsive_agent = AgentBuilder::default()
            .with_storage_arc(DhtArc::FULL)
            .with_url(Some(Url::from_str("ws://unresponsi.ve:80").unwrap()))
            .build(TestLocalAgent::default());

        let builder =
            Arc::new(default_test_builder().with_default_config().unwrap());
        let blocks = builder
            .blocks
            .create(builder.clone(), TEST_SPACE_ID)
            .await
            .unwrap();

        let peer_store = builder
            .peer_store
            .create(builder.clone(), TEST_SPACE_ID, blocks)
            .await
            .unwrap();
        let local_agent_store = builder
            .local_agent_store
            .create(builder.clone())
            .await
            .unwrap();
        let peer_meta_store = builder
            .peer_meta_store
            .create(builder.clone(), TEST_SPACE_ID)
            .await
            .unwrap();

        peer_store
            .insert(vec![
                remote_agent_1.clone(),
                remote_agent_2.clone(),
                remote_agent_for_local_agent.clone(),
                unresponsive_agent.clone(),
            ])
            .await
            .unwrap();
        local_agent_store.add(local_agent).await.unwrap();
        peer_meta_store
            .set_unresponsive(
                unresponsive_agent.url.clone().unwrap(),
                Timestamp::now(),
                Timestamp::now(),
            )
            .await
            .unwrap();

        let responsive_remote_with_overlapping_arc =
            get_responsive_remote_agents_by_overlapping_storage_arc(
                peer_store,
                local_agent_store,
                peer_meta_store,
                DhtArc::Arc(0, 100),
            )
            .await
            .unwrap();

        assert_eq!(responsive_remote_with_overlapping_arc.len(), 1);
        assert!(
            responsive_remote_with_overlapping_arc.contains(&remote_agent_1)
        );
        assert!(
            !responsive_remote_with_overlapping_arc.contains(&remote_agent_2)
        );
        assert!(!responsive_remote_with_overlapping_arc
            .contains(&remote_agent_for_local_agent));
        assert!(!responsive_remote_with_overlapping_arc
            .contains(&unresponsive_agent));
    }

    #[tokio::test]
    async fn get_responsive_remote_agents_near_location_filters_unresponsive() {
        let remote_agent_1 = AgentBuilder::default()
            .with_storage_arc(DhtArc::FULL)
            .with_url(Some(Url::from_str("ws://responsi.ve:80/1").unwrap()))
            .build(TestLocalAgent::default());
        let remote_agent_2 = AgentBuilder::default()
            .with_storage_arc(DhtArc::Empty)
            .build(TestLocalAgent::default());
        let local_agent: DynLocalAgent = Arc::new(TestLocalAgent::default());
        let remote_agent_for_local_agent = AgentBuilder::default()
            .with_storage_arc(DhtArc::FULL)
            .build(local_agent.clone());
        let unresponsive_agent = AgentBuilder::default()
            .with_storage_arc(DhtArc::FULL)
            .with_url(Some(Url::from_str("ws://unresponsi.ve:80").unwrap()))
            .build(TestLocalAgent::default());

        let builder =
            Arc::new(default_test_builder().with_default_config().unwrap());
        let blocks = builder
            .blocks
            .create(builder.clone(), TEST_SPACE_ID)
            .await
            .unwrap();

        let peer_store = builder
            .peer_store
            .create(builder.clone(), TEST_SPACE_ID, blocks)
            .await
            .unwrap();
        let local_agent_store = builder
            .local_agent_store
            .create(builder.clone())
            .await
            .unwrap();
        let peer_meta_store = builder
            .peer_meta_store
            .create(builder.clone(), TEST_SPACE_ID)
            .await
            .unwrap();

        peer_store
            .insert(vec![
                remote_agent_1.clone(),
                remote_agent_2.clone(),
                remote_agent_for_local_agent.clone(),
                unresponsive_agent.clone(),
            ])
            .await
            .unwrap();
        local_agent_store.add(local_agent).await.unwrap();
        peer_meta_store
            .set_unresponsive(
                unresponsive_agent.url.clone().unwrap(),
                Timestamp::now(),
                Timestamp::now(),
            )
            .await
            .unwrap();

        let responsive_remote_near_location =
            get_responsive_remote_agents_near_location(
                peer_store,
                local_agent_store,
                peer_meta_store,
                10,
                5,
            )
            .await
            .unwrap();

        assert_eq!(responsive_remote_near_location.len(), 1);
        assert!(responsive_remote_near_location.contains(&remote_agent_1));
        assert!(!responsive_remote_near_location.contains(&remote_agent_2));
        assert!(!responsive_remote_near_location
            .contains(&remote_agent_for_local_agent));
        assert!(!responsive_remote_near_location.contains(&unresponsive_agent));
    }
}
