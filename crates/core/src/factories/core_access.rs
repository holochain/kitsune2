use kitsune2_api::{
    AccessDecision, BlockTarget, DynBlocks, DynPeerStore, K2Result, PeerAccess,
    PeerAccessState, Timestamp, Url,
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
    pub fn new(peer_store: DynPeerStore, blocks: DynBlocks) -> K2Result<Self> {
        let decisions = Arc::new(RwLock::new(HashMap::new()));
        peer_store.register_peer_update_listener(Arc::new({
            let peer_store = Arc::downgrade(&peer_store);
            let blocks = Arc::downgrade(&blocks);
            let decisions = decisions.clone();

            move |agent_info| {
                let peer_store = peer_store.clone();
                let blocks = blocks.clone();
                let decisions = decisions.clone();

                Box::pin(async move {
                    let Some(peer_store) = peer_store.upgrade() else {
                        tracing::info!("PeerStore dropped, cannot make access decision");
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

                    // fetch peers by url
                    let agents_by_url: Vec<_> = match peer_store
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
                            Ok(all_blocked) => all_blocked,
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
