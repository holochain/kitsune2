use crate::K2GossipConfig;
use crate::gossip::K2Gossip;
use crate::peer_meta_store::K2PeerMetaStore;
use crate::state::GossipRoundState;
use kitsune2_api::Url;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, RwLock};
use tokio::task::AbortHandle;

/// Spawns a task that checks for timed-out gossip rounds and drops their state.
pub(crate) fn spawn_timeout_task(
    config: Arc<K2GossipConfig>,
    gossip: K2Gossip,
) -> AbortHandle {
    tracing::info!("Starting timeout task");

    let round_timeout = config.round_timeout();
    tokio::spawn(async move {
        loop {
            // Check for timed out rounds every 5s
            tokio::time::sleep(Duration::from_secs(5)).await;

            remove_timed_out_rounds(
                round_timeout,
                gossip.initiated_round_state.clone(),
                gossip.accepted_round_states.clone(),
                gossip.peer_meta_store.clone(),
            )
            .await;
        }
    })
    .abort_handle()
}

async fn remove_timed_out_rounds(
    round_timeout: Duration,
    initiated_round_state: Arc<Mutex<Option<GossipRoundState>>>,
    accepted_round_states: Arc<
        RwLock<HashMap<Url, Arc<Mutex<GossipRoundState>>>>,
    >,
    peer_meta_store: Arc<K2PeerMetaStore>,
) {
    {
        let mut initiated_state = initiated_round_state.lock().await;
        match initiated_state.as_ref() {
            Some(state) if state.started_at.elapsed() > round_timeout => {
                tracing::warn!("Initiated round timed out: {:?}", state);

                if let Err(e) = peer_meta_store
                    .incr_peer_timeout(state.session_with_peer.clone())
                    .await
                {
                    tracing::error!(
                        "Failed to increment peer timeout: {:?}",
                        e
                    );
                }

                *initiated_state = None;
            }
            _ => (),
        }
    }
    let mut accepted_round_states = accepted_round_states.write().await;
    let mut remove = HashSet::new();
    {
        for (url, state) in accepted_round_states.iter() {
            if state.lock().await.started_at.elapsed() > round_timeout {
                tracing::warn!("Accepted round timed out: {:?}", state);

                if let Err(e) =
                    peer_meta_store.incr_peer_timeout(url.clone()).await
                {
                    tracing::error!(
                        "Failed to increment peer timeout: {:?}",
                        e
                    );
                }

                remove.insert(url.clone());
            }
        }
    }
    accepted_round_states.retain(|k, _| !remove.contains(k));
}

#[cfg(test)]
mod tests {
    use crate::peer_meta_store::K2PeerMetaStore;
    use crate::state::GossipRoundState;
    use crate::timeout::remove_timed_out_rounds;
    use kitsune2_api::DhtArc;
    use kitsune2_api::Url;
    use kitsune2_core::factories::MemPeerMetaStore;
    use kitsune2_dht::ArcSet;
    use std::collections::HashMap;
    use std::sync::Arc;
    use tokio::sync::{Mutex, RwLock};

    fn test_peer_meta_store() -> Arc<K2PeerMetaStore> {
        Arc::new(K2PeerMetaStore::new(MemPeerMetaStore::create()))
    }

    #[tokio::test(start_paused = true)]
    async fn expire_initiated_round() {
        let url = Url::from_str("ws://test:80/1").unwrap();
        let store = test_peer_meta_store();
        let initiated = Arc::new(Mutex::new(Some(GossipRoundState::new(
            url.clone(),
            vec![],
            ArcSet::new(vec![DhtArc::FULL]).unwrap(),
        ))));
        remove_timed_out_rounds(
            std::time::Duration::from_secs(60),
            initiated.clone(),
            Default::default(),
            store.clone(),
        )
        .await;

        assert!(initiated.lock().await.is_some());
        assert!(store.peer_timeouts(url.clone()).await.unwrap().is_none());

        tokio::time::advance(std::time::Duration::from_secs(61)).await;

        remove_timed_out_rounds(
            std::time::Duration::from_secs(60),
            initiated.clone(),
            Default::default(),
            store.clone(),
        )
        .await;

        assert!(initiated.lock().await.is_none());
        assert_eq!(Some(1), store.peer_timeouts(url.clone()).await.unwrap());
    }

    #[tokio::test(start_paused = true)]
    async fn expire_accepted_rounds() {
        let store = test_peer_meta_store();
        let accepted = Arc::new(RwLock::new(HashMap::new()));

        let url_1 = Url::from_str("ws://test:80/1").unwrap();
        accepted.write().await.insert(
            url_1.clone(),
            Arc::new(Mutex::new(GossipRoundState::new(
                url_1.clone(),
                vec![],
                ArcSet::new(vec![DhtArc::FULL]).unwrap(),
            ))),
        );

        tokio::time::advance(std::time::Duration::from_secs(30)).await;

        let expected_url = Url::from_str("ws://test:80/2").unwrap();
        accepted.write().await.insert(
            expected_url.clone(),
            Arc::new(Mutex::new(GossipRoundState::new(
                expected_url.clone(),
                vec![],
                ArcSet::new(vec![DhtArc::FULL]).unwrap(),
            ))),
        );

        remove_timed_out_rounds(
            std::time::Duration::from_secs(60),
            Default::default(),
            accepted.clone(),
            store.clone(),
        )
        .await;

        assert_eq!(2, accepted.read().await.len());

        tokio::time::advance(std::time::Duration::from_secs(31)).await;

        remove_timed_out_rounds(
            std::time::Duration::from_secs(60),
            Default::default(),
            accepted.clone(),
            store.clone(),
        )
        .await;

        assert_eq!(1, accepted.read().await.len());
        assert_eq!(
            expected_url,
            accepted.read().await.keys().next().unwrap().clone()
        );
        assert_eq!(Some(1), store.peer_timeouts(url_1.clone()).await.unwrap());

        tokio::time::advance(std::time::Duration::from_secs(31)).await;

        remove_timed_out_rounds(
            std::time::Duration::from_secs(60),
            Default::default(),
            accepted.clone(),
            store.clone(),
        )
        .await;

        assert_eq!(0, accepted.read().await.len());
        assert_eq!(
            Some(1),
            store.peer_timeouts(expected_url.clone()).await.unwrap()
        );
    }
}
