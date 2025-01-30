use kitsune2_api::Timestamp;
use kitsune2_dht::DhtApi;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::task::AbortHandle;
use tokio::time::Instant;

pub(crate) fn spawn_dht_update_task(
    dht: Arc<RwLock<dyn DhtApi>>,
) -> AbortHandle {
    tracing::info!("Starting DHT update task");

    tokio::spawn(async move {
        loop {
            let current_time = Timestamp::now();
            let next_update = dht.read().await.next_update_at();
            tracing::trace!("Next DHT update at: {:?}", next_update);

            let wake = Instant::now()
                + (next_update - current_time)
                    .unwrap_or(Duration::from_secs(30));
            tokio::time::sleep_until(wake).await;

            tracing::trace!("Doing DHT update");
            if let Err(e) = dht.write().await.update(Timestamp::now()).await {
                tracing::error!(?e, "Failed to update DHT");
            }
        }
    })
    .abort_handle()
}

#[cfg(test)]
mod tests {
    use super::*;
    use kitsune2_dht::{DhtApi, MockDhtApi};
    use kitsune2_test_utils::enable_tracing;
    use std::sync::atomic::AtomicU32;

    #[tokio::test(start_paused = true)]
    async fn periodic_dht_update() {
        enable_tracing();

        let mut dht = MockDhtApi::default();

        dht.expect_next_update_at().returning(|| {
            let now = Timestamp::now();
            now + Duration::from_secs(5)
        });

        let update_count = Arc::new(AtomicU32::new(0));
        dht.expect_update().returning({
            let update_count = update_count.clone();
            move |_| {
                update_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                Box::pin(async move { Ok(()) })
            }
        });

        let dht: Arc<RwLock<dyn DhtApi>> = Arc::new(RwLock::new(dht));
        let handle = spawn_dht_update_task(dht.clone());

        for _ in 0..3 {
            // Let the task go to sleep
            tokio::time::sleep(Duration::from_millis(10)).await;
            // Advance time so that the update will run
            tokio::time::advance(Duration::from_secs(5)).await;
        }

        // Let the task run the update and go back to sleep
        tokio::time::sleep(Duration::from_millis(10)).await;

        handle.abort();

        assert_eq!(3, update_count.load(std::sync::atomic::Ordering::Relaxed));
    }
}
