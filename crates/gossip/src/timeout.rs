use crate::gossip::K2Gossip;
use crate::state::GossipRoundState;
use crate::K2GossipConfig;
use kitsune2_api::Url;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::task::AbortHandle;

/// Spawns a task that checks for timed-out gossip rounds and drops their state.
pub(crate) fn spawn_timeout_task(
    config: Arc<K2GossipConfig>,
    gossip: K2Gossip,
) -> AbortHandle {
    tracing::info!("Starting timeout task");

    let gossip_timeout = config.gossip_timeout();
    tokio::spawn(async move {
        loop {
            // Check for timed out rounds every 5s
            tokio::time::sleep(Duration::from_secs(5)).await;

            remove_timed_out_rounds(
                gossip_timeout,
                gossip.initiated_round_state.clone(),
                gossip.accepted_round_states.clone(),
            )
            .await;
        }
    })
    .abort_handle()
}

async fn remove_timed_out_rounds(
    gossip_timeout: Duration,
    initiated_round_state: Arc<Mutex<Option<GossipRoundState>>>,
    accepted_round_states: Arc<Mutex<HashMap<Url, GossipRoundState>>>,
) {
    {
        let mut initiated_state = initiated_round_state.lock().await;
        match initiated_state.as_ref() {
            Some(state) if state.started_at.elapsed() > gossip_timeout => {
                tracing::warn!("Initiated round timed out: {:?}", state);
                *initiated_state = None;
            }
            _ => (),
        }
    }

    accepted_round_states.lock().await.retain(|_, state| {
        match state.started_at.elapsed() > gossip_timeout {
            true => {
                tracing::warn!("Accepted round timed out: {:?}", state);
                false
            }
            false => true,
        }
    });
}

#[cfg(test)]
mod tests {
    use crate::state::GossipRoundState;
    use crate::timeout::remove_timed_out_rounds;
    use kitsune2_api::Url;
    use std::collections::HashMap;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    #[tokio::test(start_paused = true)]
    async fn expire_initiated_round() {
        let initiated = Arc::new(Mutex::new(Some(GossipRoundState::new(
            Url::from_str("ws://test:80/1").unwrap(),
            vec![],
        ))));
        remove_timed_out_rounds(
            std::time::Duration::from_secs(60),
            initiated.clone(),
            Default::default(),
        )
        .await;

        assert!(initiated.lock().await.is_some());

        tokio::time::advance(std::time::Duration::from_secs(61)).await;

        remove_timed_out_rounds(
            std::time::Duration::from_secs(60),
            initiated.clone(),
            Arc::new(Mutex::new(Default::default())),
        )
        .await;

        assert!(initiated.lock().await.is_none());
    }

    #[tokio::test(start_paused = true)]
    async fn expire_accepted_rounds() {
        let accepted = Arc::new(Mutex::new(HashMap::new()));

        accepted.lock().await.insert(
            Url::from_str("ws://test:80/1").unwrap(),
            GossipRoundState::new(
                Url::from_str("ws://test:80/1").unwrap(),
                vec![],
            ),
        );

        tokio::time::advance(std::time::Duration::from_secs(30)).await;

        accepted.lock().await.insert(
            Url::from_str("ws://test:80/2").unwrap(),
            GossipRoundState::new(
                Url::from_str("ws://test:80/2").unwrap(),
                vec![],
            ),
        );

        remove_timed_out_rounds(
            std::time::Duration::from_secs(60),
            Default::default(),
            accepted.clone(),
        )
        .await;

        assert_eq!(2, accepted.lock().await.len());

        tokio::time::advance(std::time::Duration::from_secs(31)).await;

        remove_timed_out_rounds(
            std::time::Duration::from_secs(60),
            Default::default(),
            accepted.clone(),
        )
        .await;

        assert_eq!(1, accepted.lock().await.len());
        assert_eq!(
            Url::from_str("ws://test:80/2").unwrap(),
            accepted.lock().await.keys().next().unwrap().clone()
        );

        tokio::time::advance(std::time::Duration::from_secs(31)).await;

        remove_timed_out_rounds(
            std::time::Duration::from_secs(60),
            Default::default(),
            accepted.clone(),
        )
        .await;

        assert_eq!(0, accepted.lock().await.len());
    }
}
