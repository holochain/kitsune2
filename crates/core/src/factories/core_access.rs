use kitsune2_api::{
    AccessDecision, BlockTarget, DynBlocks, DynKnownPeers, K2Result,
    PeerAccess, PeerAccessState, Timestamp, Url,
};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::Duration;

/// Core implementation of the [`PeerAccessState`] trait.
pub struct CorePeerAccessState {
    decisions: Arc<RwLock<HashMap<Url, PeerAccess>>>,
    abort_handle: tokio::task::AbortHandle,
}

impl Drop for CorePeerAccessState {
    fn drop(&mut self) {
        tracing::info!(
            "CorePeerAccessState is being dropped, aborting background task"
        );
        self.abort_handle.abort();
    }
}

impl CorePeerAccessState {
    /// Create a new instance of the [`CorePeerAccessState`].
    ///
    /// `known_peers` is used to resolve peer URLs to agent IDs regardless of
    /// block status.  `peer_store` is used only to register a listener so
    /// that access decisions are updated whenever the peer store is updated.
    pub fn new(
        known_peers: DynKnownPeers,
        blocks: DynBlocks,
        peer_store: &kitsune2_api::DynPeerStore,
    ) -> K2Result<Self> {
        let decisions = Arc::new(RwLock::new(HashMap::new()));
        peer_store.register_peer_update_listener(Arc::new({
            let known_peers = Arc::downgrade(&known_peers);
            let blocks = Arc::downgrade(&blocks);
            let decisions = decisions.clone();

            move |agent_info| {
                let known_peers = known_peers.clone();
                let blocks = blocks.clone();
                let decisions = decisions.clone();

                Box::pin(async move {
                    let Some(known_peers) = known_peers.upgrade() else {
                        tracing::info!("KnownPeers dropped, cannot make access decision");
                        return;
                    };
                    let Some(blocks) = blocks.upgrade() else {
                        tracing::info!("Blocks dropped, cannot make access decision");
                        return;
                    };

                    let peer_url = match agent_info.url.clone() {
                        Some(url) => url,
                        None => {
                            if !agent_info.is_tombstone {
                                tracing::warn!("AgentInfo has no URL: {:?}", agent_info);
                            }
                            return;
                        }
                    };

                    tracing::debug!("Making access decision for peer URL: {:?}", peer_url);

                    // Use known_peers (not peer_store) so we find blocked agents too.
                    let agents_by_url: Vec<_> = match known_peers
                        .get_by_url(peer_url.clone())
                        .await {
                        Ok(peers) => peers.into_iter()
                        .map(|agent| BlockTarget::Agent(agent.agent.clone()))
                        .collect(),
                        Err(e) => {
                            tracing::error!(
                                "Failed to get agents by url {:?}: {:?}",
                                peer_url,
                                e
                            );
                            return;
                        }
                    };

                    if agents_by_url.is_empty() {
                        tracing::debug!("No agents found for url, clearing decision because they will be treated as blocked anyway: {:?}", peer_url);

                        // Any existing decision can be removed
                        decisions
                            .write()
                            .expect("poisoned")
                            .remove(&peer_url);
                    } else {
                        let any_blocked = match blocks.is_any_blocked(agents_by_url).await {
                            Ok(any_blocked) => any_blocked,
                            Err(e) => {
                                tracing::error!(
                                    "Failed to check block status for url {:?}: {:?}",
                                    peer_url,
                                    e
                                );
                                return;
                            }
                        };

                        let access = if any_blocked {
                            PeerAccess {
                                decision: AccessDecision::Blocked,
                                decided_at: Timestamp::now(),
                            }
                        } else {
                            PeerAccess {
                                decision: AccessDecision::Granted,
                                decided_at: Timestamp::now(),
                            }
                        };

                        tracing::debug!("Access decision for peer URL {peer_url:?}: {:?}", access.decision);

                        decisions
                            .write()
                            .expect("poisoned")
                            .insert(peer_url, access.clone());
                    }
                })
            }
        }))?;

        let abort_handle = tokio::task::spawn({
            let decisions = decisions.clone();
            async move {
                loop {
                    // Agent information is expected to be updated regularly. If updates aren't
                    // received then the access decisions will become stale and can be pruned.

                    tokio::time::sleep(Duration::from_secs(60 * 60)).await;

                    let result = Timestamp::now() - Duration::from_secs(60 * 60);
                    let Ok(old) = result else {
                        tracing::warn!("Failed to compute old timestamp for pruning access decisions");
                        continue;
                    };

                    decisions.write().expect("poisoned").retain(|_, v| {
                        v.decided_at > old
                    });
                }
            }
        }).abort_handle();

        Ok(Self {
            decisions,
            abort_handle,
        })
    }
}

impl std::fmt::Debug for CorePeerAccessState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CorePeerAccessState").finish()
    }
}

impl PeerAccessState for CorePeerAccessState {
    fn get_access_decision(
        &self,
        peer_url: Url,
    ) -> K2Result<Option<PeerAccess>> {
        let decision = self
            .decisions
            .read()
            .expect("poisoned")
            .get(&peer_url)
            .cloned();
        Ok(decision)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::factories::{
        MemBlocks, MemKnownPeers, MemPeerStore, MemPeerStoreConfig,
    };
    use kitsune2_api::{AccessDecision, AgentId, BlockTarget, Blocks, Id};
    use kitsune2_test_utils::agent::{AgentBuilder, TestLocalAgent};
    use std::sync::Arc;

    const AGENT_1: AgentId = AgentId(Id(bytes::Bytes::from_static(b"agent1")));
    const AGENT_2: AgentId = AgentId(Id(bytes::Bytes::from_static(b"agent2")));

    fn make_url(s: &str) -> Url {
        Url::from_str(format!("ws://a.b:80/{s}")).unwrap()
    }

    fn make_peer_store(
        blocks: Arc<MemBlocks>,
        known_peers: Arc<MemKnownPeers>,
    ) -> kitsune2_api::DynPeerStore {
        Arc::new(MemPeerStore::new(
            MemPeerStoreConfig {
                prune_interval_s: 10,
            },
            blocks,
            known_peers,
        ))
    }

    /// Regression test: blocking one agent at a URL must block the URL even
    /// when a non-blocked agent also resides at that URL.
    ///
    /// Before the `KnownPeers` fix the blocked agent was removed from the
    /// `PeerStore`, so `get_by_url` only saw the non-blocked agent and
    /// returned `Granted` instead of `Blocked`.
    #[tokio::test]
    async fn shared_url_blocked_agent_blocks_url() {
        let url = make_url("shared");
        let blocks = Arc::new(MemBlocks::default());
        let known_peers = Arc::new(MemKnownPeers::default());
        let peer_store = make_peer_store(blocks.clone(), known_peers.clone());

        let access_state = CorePeerAccessState::new(
            known_peers.clone(),
            blocks.clone(),
            &peer_store,
        )
        .unwrap();

        // Insert both agents at the same URL; this records them in KnownPeers
        // and triggers the listener which makes access decisions.
        let info1 = AgentBuilder {
            agent: Some(AGENT_1),
            url: Some(Some(url.clone())),
            ..Default::default()
        }
        .build(TestLocalAgent::default());
        let info2 = AgentBuilder {
            agent: Some(AGENT_2),
            url: Some(Some(url.clone())),
            ..Default::default()
        }
        .build(TestLocalAgent::default());
        peer_store.insert(vec![info1, info2]).await.unwrap();

        // Allow async listener tasks to complete.
        tokio::task::yield_now().await;

        // Both agents present, neither blocked → Granted.
        let decision = access_state.get_access_decision(url.clone()).unwrap();
        assert_eq!(
            decision.map(|d| d.decision),
            Some(AccessDecision::Granted),
            "expected Granted before any block"
        );

        // Block agent_1 and remove it from the peer store (as the Host must).
        blocks.block(BlockTarget::Agent(AGENT_1)).await.unwrap();
        peer_store.remove(AGENT_1).await.unwrap();

        // Allow async listener tasks to complete.
        tokio::task::yield_now().await;

        // agent_1 is now blocked; even though agent_2 (non-blocked) is still
        // at the same URL, the URL must be Blocked.
        let decision = access_state.get_access_decision(url.clone()).unwrap();
        assert_eq!(
            decision.map(|d| d.decision),
            Some(AccessDecision::Blocked),
            "expected Blocked after agent_1 at the shared URL was blocked"
        );
    }

    /// Sanity test: a URL with only non-blocked agents remains Granted.
    #[tokio::test]
    async fn url_with_no_blocked_agents_stays_granted() {
        let url = make_url("clean");
        let blocks = Arc::new(MemBlocks::default());
        let known_peers = Arc::new(MemKnownPeers::default());
        let peer_store = make_peer_store(blocks.clone(), known_peers.clone());

        let access_state = CorePeerAccessState::new(
            known_peers.clone(),
            blocks.clone(),
            &peer_store,
        )
        .unwrap();

        let info = AgentBuilder {
            agent: Some(AGENT_1),
            url: Some(Some(url.clone())),
            ..Default::default()
        }
        .build(TestLocalAgent::default());
        peer_store.insert(vec![info]).await.unwrap();
        tokio::task::yield_now().await;

        let decision = access_state.get_access_decision(url.clone()).unwrap();
        assert_eq!(decision.map(|d| d.decision), Some(AccessDecision::Granted));
    }
}
