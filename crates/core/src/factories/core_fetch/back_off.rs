use std::{
    collections::{hash_map::Entry, HashMap},
    time::{Duration, Instant},
};

use backon::BackoffBuilder;
use kitsune2_api::AgentId;

#[derive(Debug)]
pub struct BackOffList {
    state: HashMap<AgentId, BackOff>,
    first_back_off_interval: Duration,
    last_back_off_interval: Duration,
    num_back_off_intervals: usize,
}

impl BackOffList {
    pub fn new(
        first_back_off_interval: Duration,
        last_back_off_interval: Duration,
        num_back_off_intervals: usize,
    ) -> Self {
        Self {
            state: HashMap::new(),
            first_back_off_interval,
            last_back_off_interval,
            num_back_off_intervals,
        }
    }

    pub fn back_off_agent(&mut self, agent_id: &AgentId) {
        match self.state.entry(agent_id.clone()) {
            Entry::Occupied(mut o) => {
                o.get_mut().back_off();
            }
            Entry::Vacant(v) => {
                v.insert(BackOff::new(
                    self.first_back_off_interval,
                    self.last_back_off_interval,
                    self.num_back_off_intervals,
                ));
            }
        }
    }

    pub fn is_agent_on_back_off(&mut self, agent_id: &AgentId) -> bool {
        match self.state.get(agent_id) {
            Some(back_off) => back_off.is_on_back_off(),
            None => false,
        }
    }

    pub fn has_last_back_off_expired(&self, agent_id: &AgentId) -> bool {
        match self.state.get(agent_id) {
            Some(back_off) => back_off.has_last_interval_expired(),
            None => false,
        }
    }

    pub fn remove_agent(&mut self, agent_id: &AgentId) {
        self.state.remove(agent_id);
    }
}

#[derive(Debug)]
struct BackOff {
    back_off: backon::ExponentialBackoff,
    current_interval: Duration,
    interval_start: Instant,
    is_last_interval: bool,
}

impl BackOff {
    pub fn new(
        first_back_off_interval: Duration,
        last_back_off_interval: Duration,
        num_back_off_intervals: usize,
    ) -> Self {
        let mut back_off = backon::ExponentialBuilder::default()
            .with_factor(2.0)
            .with_min_delay(first_back_off_interval)
            .with_max_delay(last_back_off_interval)
            .with_max_times(num_back_off_intervals)
            .build();
        let current_interval = back_off
            .next()
            .expect("back off must have at least one interval");
        Self {
            back_off,
            current_interval,
            interval_start: Instant::now(),
            is_last_interval: false,
        }
    }

    pub fn back_off(&mut self) {
        match self.back_off.next() {
            None => self.is_last_interval = true,
            Some(interval) => {
                self.interval_start = Instant::now();
                self.current_interval = interval;
            }
        }
    }

    pub fn is_on_back_off(&self) -> bool {
        self.interval_start.elapsed() < self.current_interval
    }

    pub fn has_last_interval_expired(&self) -> bool {
        self.is_last_interval
            && self.interval_start.elapsed() > self.current_interval
    }
}

#[cfg(test)]
mod test {
    use crate::{
        default_builder,
        factories::core_fetch::{
            back_off::BackOffList,
            test::{create_op_list, random_agent_id, MockTransport},
            CoreFetch, CoreFetchConfig,
        },
    };
    use kitsune2_api::{fetch::Fetch, SpaceId, Url};
    use kitsune2_test_utils::agent::*;
    use std::{sync::Arc, time::Duration};

    #[test]
    fn back_off() {
        let mut back_off_list = BackOffList::new(
            Duration::from_millis(10),
            Duration::from_millis(10),
            2,
        );
        let agent_id = random_agent_id();
        back_off_list.back_off_agent(&agent_id);
        assert!(back_off_list.is_agent_on_back_off(&agent_id));

        std::thread::sleep(
            back_off_list.state.get(&agent_id).unwrap().current_interval,
        );

        assert!(!back_off_list.is_agent_on_back_off(&agent_id));

        back_off_list.back_off_agent(&agent_id);
        assert!(back_off_list.is_agent_on_back_off(&agent_id));

        std::thread::sleep(
            back_off_list.state.get(&agent_id).unwrap().current_interval,
        );

        assert!(!back_off_list.is_agent_on_back_off(&agent_id));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn agent_on_back_off_is_removed_from_list_after_successful_send() {
        let builder =
            Arc::new(default_builder().with_default_config().unwrap());
        let peer_store =
            builder.peer_store.create(builder.clone()).await.unwrap();
        let config = CoreFetchConfig {
            first_back_off_interval: Duration::from_millis(10),
            ..Default::default()
        };
        let mock_transport = MockTransport::new(false);

        let op_list = create_op_list(1);
        let agent_id = random_agent_id();
        let agent_info = AgentBuilder {
            agent: Some(agent_id.clone()),
            url: Some(Some(Url::from_str("wss://127.0.0.1:1").unwrap())),
            ..Default::default()
        }
        .build(TestLocalAgent::default());
        peer_store.insert(vec![agent_info.clone()]).await.unwrap();

        let fetch = CoreFetch::new(
            config.clone(),
            agent_info.space.clone(),
            peer_store,
            mock_transport.clone(),
        );

        let first_back_off_interval = {
            let mut lock = fetch.state.lock().unwrap();
            lock.back_off_list.back_off_agent(&agent_id);

            assert!(lock.back_off_list.is_agent_on_back_off(&agent_id));

            lock.back_off_list.first_back_off_interval
        };

        tokio::time::sleep(first_back_off_interval).await;

        fetch.add_ops(op_list, agent_id.clone()).await.unwrap();

        tokio::time::timeout(Duration::from_millis(10), async {
            loop {
                tokio::time::sleep(Duration::from_millis(1)).await;
                if !mock_transport.requests_sent.lock().unwrap().is_empty() {
                    break;
                }
            }
        })
        .await
        .unwrap();

        assert!(fetch.state.lock().unwrap().back_off_list.state.is_empty());
    }

    #[tokio::test(flavor = "multi_thread")]
    #[cfg_attr(target_os = "windows", ignore = "flaky: back_off.rs:333:10: called `Result::unwrap()` on an `Err` value: Elapsed(())")]
    async fn requests_are_dropped_when_max_back_off_expired() {
        let builder =
            Arc::new(default_builder().with_default_config().unwrap());
        let peer_store =
            builder.peer_store.create(builder.clone()).await.unwrap();
        let config = CoreFetchConfig {
            first_back_off_interval: Duration::from_millis(10),
            last_back_off_interval: Duration::from_millis(10),
            ..Default::default()
        };
        let mock_transport = MockTransport::new(true);
        let space_id = SpaceId::from(bytes::Bytes::from_static(b"space_1"));

        let op_list_1 = create_op_list(2);
        let agent_id_1 = random_agent_id();
        let agent_info_1 = AgentBuilder {
            agent: Some(agent_id_1.clone()),
            url: Some(Some(Url::from_str("wss://127.0.0.1:1").unwrap())),
            ..Default::default()
        }
        .build(TestLocalAgent::default());
        let agent_url_1 = agent_info_1.url.clone().unwrap();
        peer_store.insert(vec![agent_info_1.clone()]).await.unwrap();

        // Create a second agent to later check that their ops have not been removed.
        let op_list_2 = create_op_list(2);
        let agent_id_2 = random_agent_id();
        let agent_info_2 = AgentBuilder {
            agent: Some(agent_id_2.clone()),
            url: Some(Some(Url::from_str("wss://127.0.0.1:2").unwrap())),
            ..Default::default()
        }
        .build(TestLocalAgent::default());
        peer_store.insert(vec![agent_info_2.clone()]).await.unwrap();

        let fetch = CoreFetch::new(
            config.clone(),
            space_id.clone(),
            peer_store,
            mock_transport.clone(),
        );

        fetch
            .add_ops(op_list_1.clone(), agent_id_1.clone())
            .await
            .unwrap();

        // Wait for one request to fail, so agent is put on back off list.
        tokio::time::timeout(Duration::from_millis(10), async {
            loop {
                tokio::time::sleep(Duration::from_millis(1)).await;
                if !mock_transport
                    .requests_sent
                    .lock()
                    .unwrap()
                    .clone()
                    .is_empty()
                {
                    break;
                }
            }
        })
        .await
        .unwrap();

        let current_number_of_requests_to_agent_1 = mock_transport
            .requests_sent
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, a)| *a != agent_url_1)
            .count();

        // Back off agent the maximum possible number of times.
        let last_back_off_interval = {
            let mut lock = fetch.state.lock().unwrap();
            assert!(op_list_1.iter().all(|op_id| lock
                .requests
                .contains(&(op_id.clone(), agent_id_1.clone()))));
            for _ in 0..config.num_back_off_intervals {
                lock.back_off_list.back_off_agent(&agent_id_1);
            }

            lock.back_off_list.last_back_off_interval
        };

        // Wait for back off interval to expire. Afterwards the request should fail again and all
        // of the agent's requests should be removed from the set.
        tokio::time::sleep(last_back_off_interval).await;

        assert!(fetch
            .state
            .lock()
            .unwrap()
            .back_off_list
            .has_last_back_off_expired(&agent_id_1));

        // Add control agent's ops to set.
        fetch.add_ops(op_list_2, agent_id_2.clone()).await.unwrap();

        // Wait for another request attempt to agent 1, which should remove all of their requests
        // from the set.
        tokio::time::timeout(Duration::from_millis(10), async {
            loop {
                if mock_transport
                    .requests_sent
                    .lock()
                    .unwrap()
                    .iter()
                    .filter(|(_, a)| *a != agent_url_1)
                    .count()
                    > current_number_of_requests_to_agent_1
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
        })
        .await
        .unwrap();

        assert!(fetch
            .state
            .lock()
            .unwrap()
            .requests
            .iter()
            .all(|(_, agent_id)| *agent_id != agent_id_1),);
    }
}
