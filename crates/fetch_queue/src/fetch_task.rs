use std::time::Duration;

use kitsune2_api::fetch::{DynFetchQueue, FetchTaskT};

/// Configuration for [`FetchTask`].
pub struct FetchTaskConfig {
    /// How long to pause after sending a fetch request, before the next attempt.
    pub pause_between_runs: u64, // in ms
    /// Maximum number of ops to request from a remote.
    pub max_ops_to_request: usize,
}

impl Default for FetchTaskConfig {
    /// Default fetch task config.
    fn default() -> Self {
        Self {
            pause_between_runs: 1000 * 5, // 5 seconds
            max_ops_to_request: 100,
        }
    }
}

#[derive(Default)]
pub struct FetchTask {
    config: FetchTaskConfig,
}

impl FetchTaskT for FetchTask {
    fn spawn(&self, fetch_queue: DynFetchQueue) {
        let pause_between_runs = self.config.pause_between_runs;
        let max_ops_to_request = self.config.max_ops_to_request;
        tokio::spawn(async move {
            loop {
                let ops = fetch_queue.get_ops_to_fetch();
                // Do not attempt to fetch if there are no ops to be fetched.
                if !ops.is_empty() {
                    let _batch_to_fetch = ops
                        .into_iter()
                        .take(max_ops_to_request)
                        .collect::<Vec<_>>();
                    if let Some(_source) = fetch_queue.get_random_source() {
                        todo!()
                    } else {
                        eprintln!("no sources to fetch from in fetch queue");
                    }
                }
                tokio::time::sleep(Duration::from_millis(pause_between_runs))
                    .await;
            }
        });
    }
}
