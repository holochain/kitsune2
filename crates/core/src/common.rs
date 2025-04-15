use kitsune2_api::{
    AgentInfoSigned, DhtArc, DynLocalAgentStore, DynPeerStore, K2Result,
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
    remove_local_agents(local_agent_store, all_agents).await
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
    remove_local_agents(local_agent_store, by_overlapping_storage_arc).await
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
    remove_local_agents(local_agent_store, near_location).await
}

async fn remove_local_agents(
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::default_test_builder;
    use kitsune2_api::DynLocalAgent;
    use kitsune2_test_utils::agent::{AgentBuilder, TestLocalAgent};
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

        let peer_store =
            builder.peer_store.create(builder.clone()).await.unwrap();
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

        let peer_store =
            builder.peer_store.create(builder.clone()).await.unwrap();
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

        let peer_store =
            builder.peer_store.create(builder.clone()).await.unwrap();
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
}
