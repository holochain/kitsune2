use crate::gossip::DropAbortHandle;
use crate::K2GossipConfig;
use kitsune2_api::{Timestamp, Url, UNIX_TIMESTAMP};
use std::collections::HashMap;
use std::fmt::Debug;
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Clone)]
pub(crate) struct AcceptBurstTracker {
    config: Arc<K2GossipConfig>,
    state: Arc<Mutex<HashMap<Url, Vec<Timestamp>>>>,
    _task: Arc<DropAbortHandle>,
}

impl Debug for AcceptBurstTracker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AcceptBurstTracker")
            .field("state", &self.state)
            .finish()
    }
}

impl AcceptBurstTracker {
    pub(crate) fn new(config: Arc<K2GossipConfig>) -> Self {
        let state: Arc<Mutex<HashMap<Url, Vec<Timestamp>>>> =
            Default::default();

        // A task is needed to clear out old timestamps. If we only clear them when checking a
        // new accept, then we would never clear timestamps for peers that are no longer sending
        // us accepts.
        let task_config = config.clone();
        let state_weak = Arc::downgrade(&state);
        let handle = tokio::task::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(5 * 60)).await;

                if let Some(state) = state_weak.upgrade() {
                    Self::cleanup(&state, &task_config);
                } else {
                    tracing::debug!("AcceptBurstTracker task exiting");
                    break;
                }
            }
        });

        Self {
            config,
            state,
            _task: Arc::new(DropAbortHandle {
                name: "AcceptBurstTracker".to_string(),
                handle: handle.abort_handle(),
            }),
        }
    }

    pub(crate) fn check_accept(
        &self,
        peer_url: &Url,
        timestamp: Timestamp,
    ) -> bool {
        let max_burst = Self::max_burst(&self.config);
        let threshold = Self::threshold(&self.config);

        let mut lock = self.state.lock().expect("mutex poisoned");
        let entry = lock.entry(peer_url.clone()).or_default();

        // Remove timestamps that are older than the threshold
        entry.retain(|t| *t > threshold);

        if entry.len() >= max_burst {
            tracing::info!("Peer {peer_url} has reached the burst limit, this gossip request will be ignored");
            false
        } else {
            entry.push(timestamp);
            true
        }
    }

    pub(crate) fn max_burst(config: &K2GossipConfig) -> usize {
        (config.initiate_burst_factor * config.initiate_burst_window_count)
            as usize
    }

    fn threshold(config: &K2GossipConfig) -> Timestamp {
        (Timestamp::now()
            - Duration::from_millis(
                config.initiate_interval_ms as u64
                    * config.initiate_burst_window_count as u64,
            ))
        .unwrap_or(UNIX_TIMESTAMP)
    }

    fn cleanup(
        state: &Arc<Mutex<HashMap<Url, Vec<Timestamp>>>>,
        config: &K2GossipConfig,
    ) {
        let threshold = Self::threshold(config);
        let mut lock = state.lock().expect("mutex poisoned");
        lock.retain(|_, timestamps| {
            timestamps.retain(|&t| t > threshold);
            !timestamps.is_empty()
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Sanity test that we can accept a single request.
    #[tokio::test]
    async fn accept_one() {
        let config = Arc::new(K2GossipConfig::default());
        let tracker = AcceptBurstTracker::new(config.clone());

        let peer_url = Url::from_str("ws://localhost:1234").unwrap();
        let timestamp = Timestamp::now();

        assert!(tracker.check_accept(&peer_url, timestamp));
    }

    /// Should accept a burst of requests from the same peer.
    #[tokio::test]
    async fn accept_burst() {
        let config = Arc::new(K2GossipConfig::default());
        let tracker = AcceptBurstTracker::new(config.clone());

        let max_burst = AcceptBurstTracker::max_burst(&config);

        let peer_url = Url::from_str("ws://localhost:1234").unwrap();
        for _ in 0..max_burst {
            assert!(tracker.check_accept(&peer_url, Timestamp::now()));
        }

        assert!(!tracker.check_accept(&peer_url, Timestamp::now()));
    }

    /// When accepting over time, old requests should be removed from tracking,
    /// and we should be able to keep accepting requests.
    #[tokio::test]
    async fn accept_continuous() {
        let config = Arc::new(K2GossipConfig {
            initiate_interval_ms: 50,
            ..K2GossipConfig::default()
        });
        let tracker = AcceptBurstTracker::new(config.clone());

        let peer_url = Url::from_str("ws://localhost:1234").unwrap();
        for _ in 0..50 {
            assert!(tracker.check_accept(&peer_url, Timestamp::now()));
            tokio::time::sleep(config.initiate_interval()).await;
        }

        let open = tracker
            .state
            .lock()
            .unwrap()
            .get(&peer_url)
            .unwrap()
            .clone();
        // The content of `open` should be recent timestamps but the actual number is unpredictable
        // based on the timing of the sleeps above. Just check something is present.
        // The main logic of the test is to be able to keep on accepting so the assertion in the
        // loop above is the main check.
        assert!(!open.is_empty(), "There should be some open timestamps");
    }

    #[tokio::test]
    async fn accept_burst_for_multiple_peers() {
        let config = Arc::new(K2GossipConfig::default());
        let tracker = AcceptBurstTracker::new(config.clone());

        let peer_url1 = Url::from_str("ws://localhost:1234").unwrap();
        let peer_url2 = Url::from_str("ws://localhost:5678").unwrap();

        let max_burst = AcceptBurstTracker::max_burst(&config);
        for _ in 0..max_burst {
            assert!(tracker.check_accept(&peer_url1, Timestamp::now()));
            assert!(tracker.check_accept(&peer_url2, Timestamp::now()));
        }

        assert!(!tracker.check_accept(&peer_url1, Timestamp::now()));
        assert!(!tracker.check_accept(&peer_url2, Timestamp::now()));
    }

    #[tokio::test]
    async fn cleanup() {
        let config = Arc::new(K2GossipConfig {
            initiate_interval_ms: 5,
            ..K2GossipConfig::default()
        });
        let tracker = AcceptBurstTracker::new(config.clone());

        // Recent, should keep
        tracker.state.lock().unwrap().insert(
            Url::from_str("ws://localhost:1234").unwrap(),
            vec![Timestamp::now()],
        );

        // Old, should remove
        tracker.state.lock().unwrap().insert(
            Url::from_str("ws://localhost:5678").unwrap(),
            vec![(Timestamp::now() - Duration::from_secs(3000)).unwrap()],
        );

        AcceptBurstTracker::cleanup(&tracker.state, &config);

        let state = tracker.state.lock().unwrap();
        assert_eq!(1, state.len());
        assert_eq!(
            1,
            state
                .get(&Url::from_str("ws://localhost:1234").unwrap())
                .unwrap()
                .len()
        );
    }
}
