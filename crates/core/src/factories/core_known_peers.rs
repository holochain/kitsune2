//! The core known-peers index implementation.

use kitsune2_api::*;
use std::collections::{HashMap, HashSet};
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
    /// URL → agents whose newest advertisement has that URL. Kept in sync
    /// with `store`, so `get_by_url` does not scan every known agent.
    by_url: HashMap<Url, HashSet<AgentId>>,
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
            let previous_url = self
                .store
                .insert(
                    agent_info.agent.clone(),
                    (agent_info.created_at, agent_info.url.clone()),
                )
                .and_then(|(_, url)| url);
            if previous_url == agent_info.url {
                continue;
            }
            if let Some(url) = previous_url
                && let Some(agents) = self.by_url.get_mut(&url)
            {
                agents.remove(&agent_info.agent);
                if agents.is_empty() {
                    self.by_url.remove(&url);
                }
            }
            if let Some(url) = &agent_info.url {
                self.by_url
                    .entry(url.clone())
                    .or_default()
                    .insert(agent_info.agent.clone());
            }
        }
    }

    fn get_by_url(&self, url: &Url) -> Vec<AgentId> {
        self.by_url
            .get(url)
            .map(|agents| agents.iter().cloned().collect())
            .unwrap_or_default()
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

    #[tokio::test]
    async fn agents_sharing_a_url_are_indexed_independently() {
        let known = CoreKnownPeers::default();
        let now = Timestamp::now();
        let shared_url = Url::from_str("ws://a.b:80/shared").unwrap();
        let other_url = Url::from_str("ws://a.b:80/other").unwrap();
        let agent_1 = AgentId(Id(bytes::Bytes::from_static(b"shared-1")));
        let agent_2 = AgentId(Id(bytes::Bytes::from_static(b"shared-2")));
        let info = |agent: &AgentId, created_at, url: &Url| {
            AgentBuilder {
                agent: Some(agent.clone()),
                created_at: Some(created_at),
                url: Some(Some(url.clone())),
                ..Default::default()
            }
            .build(TestLocalAgent::default())
        };

        known
            .record(vec![
                info(&agent_1, now, &shared_url),
                info(&agent_2, now, &shared_url),
            ])
            .await
            .unwrap();
        let mut at_shared_url =
            known.get_by_url(shared_url.clone()).await.unwrap();
        at_shared_url.sort();
        let mut both = vec![agent_1.clone(), agent_2.clone()];
        both.sort();
        assert_eq!(at_shared_url, both);

        // Moving one agent leaves the other at the shared URL.
        known
            .record(vec![info(
                &agent_1,
                now + Duration::from_secs(1),
                &other_url,
            )])
            .await
            .unwrap();
        assert_eq!(known.get_by_url(shared_url).await.unwrap(), vec![agent_2]);
        assert_eq!(known.get_by_url(other_url).await.unwrap(), vec![agent_1]);
    }
}
