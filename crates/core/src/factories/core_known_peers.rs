//! The core known-peers index implementation.

use kitsune2_api::*;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

/// The core known-peers factory.
///
/// This stores agent ID → URL mappings in memory.  Entries are never
/// removed – the whole point of this store is to retain the URL → agent
/// mapping even after an agent has been blocked and removed from the
/// peer store.
#[derive(Debug)]
pub struct CoreKnownPeersFactory {}

impl CoreKnownPeersFactory {
    /// Construct a new `CoreKnownPeersFactory`.
    pub fn create() -> DynKnownPeersFactory {
        let out: DynKnownPeersFactory = Arc::new(Self {});
        out
    }
}

impl KnownPeersFactory for CoreKnownPeersFactory {
    fn default_config(&self, _config: &mut Config) -> K2Result<()> {
        Ok(())
    }

    fn validate_config(&self, _config: &Config) -> K2Result<()> {
        Ok(())
    }

    fn create(
        &self,
        _builder: Arc<Builder>,
        _space_id: SpaceId,
    ) -> BoxFut<'static, K2Result<DynKnownPeers>> {
        Box::pin(async move {
            let out: DynKnownPeers = Arc::new(CoreKnownPeers::default());
            Ok(out)
        })
    }
}

/// The core implementation of the [`KnownPeers`] trait.
#[derive(Default)]
pub struct CoreKnownPeers(Mutex<Inner>);

impl std::fmt::Debug for CoreKnownPeers {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CoreKnownPeers").finish()
    }
}

impl KnownPeers for CoreKnownPeers {
    fn record(
        &self,
        agent_infos: Vec<Arc<AgentInfoSigned>>,
    ) -> BoxFut<'_, K2Result<()>> {
        Box::pin(async move {
            self.0.lock().await.record(agent_infos);
            Ok(())
        })
    }

    fn get_by_url(&self, url: Url) -> BoxFut<'_, K2Result<Vec<AgentId>>> {
        Box::pin(async move { Ok(self.0.lock().await.get_by_url(&url)) })
    }
}

#[derive(Default)]
struct Inner {
    /// Agent ID → newest advertisement timestamp and URL for that agent.
    store: HashMap<AgentId, (Timestamp, Option<Url>)>,
}

impl Inner {
    fn record(&mut self, agent_infos: Vec<Arc<AgentInfoSigned>>) {
        for agent_info in agent_infos {
            // Discovery batches may contain cached advertisements. Match the
            // peer store's ordering so a stale URL cannot erase the current
            // endpoint's agent mapping before access listeners run.
            if self.store.get(&agent_info.agent).is_some_and(
                |(created_at, _)| *created_at >= agent_info.created_at,
            ) {
                continue;
            }
            self.store.insert(
                agent_info.agent.clone(),
                (agent_info.created_at, agent_info.url.clone()),
            );
        }
    }

    fn get_by_url(&self, url: &Url) -> Vec<AgentId> {
        self.store
            .iter()
            .filter(|(_, (_, stored_url))| stored_url.as_ref() == Some(url))
            .map(|(agent_id, _)| agent_id.clone())
            .collect()
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use kitsune2_test_utils::agent::{AgentBuilder, TestLocalAgent};
    use std::time::Duration;

    #[tokio::test]
    async fn endpoint_ordering_preserves_equal_time_and_accepts_newer_info() {
        let known = CoreKnownPeers::default();
        let agent =
            AgentId(Id(bytes::Bytes::from_static(b"endpoint-ordering")));
        let now = Timestamp::now();
        let first_url = Url::from_str("ws://a.b:80/first").unwrap();
        let next_url = Url::from_str("ws://a.b:80/next").unwrap();
        let info = |created_at, url| {
            AgentBuilder {
                agent: Some(agent.clone()),
                created_at: Some(created_at),
                url: Some(Some(url)),
                ..Default::default()
            }
            .build(TestLocalAgent::default())
        };
        let first = info(now, first_url.clone());
        let equal = info(now, next_url.clone());
        known.record(vec![first.clone(), equal]).await.unwrap();
        assert_eq!(
            known.get_by_url(first_url.clone()).await.unwrap(),
            vec![agent.clone()]
        );
        assert!(known.get_by_url(next_url.clone()).await.unwrap().is_empty());
        let newer = info(now + Duration::from_secs(1), next_url.clone());
        known.record(vec![newer, first]).await.unwrap();
        assert_eq!(known.get_by_url(next_url).await.unwrap(), vec![agent]);
        assert!(known.get_by_url(first_url).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn stale_info_cannot_replace_a_newer_tombstone() {
        let known = CoreKnownPeers::default();
        let agent =
            AgentId(Id(bytes::Bytes::from_static(b"endpoint-tombstone")));
        let now = Timestamp::now();
        let url = Url::from_str("ws://a.b:80/old").unwrap();
        let live = AgentBuilder {
            agent: Some(agent.clone()),
            created_at: Some(now),
            url: Some(Some(url.clone())),
            ..Default::default()
        }
        .build(TestLocalAgent::default());
        let tombstone = AgentBuilder {
            agent: Some(agent),
            created_at: Some(now + Duration::from_secs(1)),
            url: Some(None),
            is_tombstone: Some(true),
            ..Default::default()
        }
        .build(TestLocalAgent::default());
        known.record(vec![tombstone, live]).await.unwrap();
        assert!(known.get_by_url(url).await.unwrap().is_empty());
    }
}
