use std::time::Duration;

use kitsune2_api::fetch::{DynFetchQueue, FetchTaskConfig, FetchTaskT};

pub struct FetchTask {
    config: FetchTaskConfig,
}

impl FetchTaskT for FetchTask {
    fn spawn(&self, fetch_queue: DynFetchQueue) {
        let pause_between_runs = self.config.pause_between_runs;
        tokio::spawn(async move {
            loop {
                let ops = fetch_queue.get_ops_to_fetch();
                // Do not attempt to fetch if there are no ops to be fetched.
                if !ops.is_empty() {
                    if let Some(source) = fetch_queue.get_random_source() {
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

impl FetchTask {
    pub fn new() -> Self {
        Self {
            config: FetchTaskConfig::default(),
        }
    }
}
