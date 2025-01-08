use std::{
    collections::{hash_map::Entry, HashMap},
    time::{Duration, Instant},
};

use backon::BackoffBuilder;
use kitsune2_api::AgentId;

#[derive(Debug)]
pub struct BackOffList {
    pub(crate) state: HashMap<AgentId, BackOff>,
    pub(crate) first_back_off_interval: Duration,
    pub(crate) last_back_off_interval: Duration,
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
pub(crate) struct BackOff {
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
    use crate::factories::core_fetch::{
        back_off::BackOffList, test::utils::random_agent_id,
    };
    use std::time::Duration;

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
}
