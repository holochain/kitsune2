use std::{
    collections::{hash_map::Entry, HashMap},
    time::{Duration, Instant},
};

use backon::BackoffBuilder;
use kitsune2_api::{Timestamp, Url};

#[derive(Debug)]
pub struct BackOffList {
    pub(crate) state: HashMap<Url, BackOff>,
    pub(crate) first_back_off_interval_ms: u32,
    pub(crate) last_back_off_interval_ms: u32,
    num_back_off_intervals_ms: usize,
}

impl BackOffList {
    pub fn new(
        first_back_off_interval_ms: u32,
        last_back_off_interval_ms: u32,
        num_back_off_intervals_ms: usize,
    ) -> Self {
        Self {
            state: HashMap::new(),
            first_back_off_interval_ms,
            last_back_off_interval_ms,
            num_back_off_intervals_ms,
        }
    }

    pub fn back_off_peer(&mut self, peer_url: &Url) {
        match self.state.entry(peer_url.clone()) {
            Entry::Occupied(mut o) => {
                o.get_mut().back_off();
            }
            Entry::Vacant(v) => {
                v.insert(BackOff::new(
                    self.first_back_off_interval_ms,
                    self.last_back_off_interval_ms,
                    self.num_back_off_intervals_ms,
                ));
            }
        }
    }

    pub fn is_peer_on_back_off(&mut self, peer_url: &Url) -> bool {
        match self.state.get(peer_url) {
            Some(back_off) => back_off.is_on_back_off(),
            None => false,
        }
    }

    pub fn has_last_back_off_expired(&self, peer_url: &Url) -> bool {
        match self.state.get(peer_url) {
            Some(back_off) => back_off.has_last_interval_expired(),
            None => false,
        }
    }

    pub fn remove_peer(&mut self, peer_url: &Url) {
        self.state.remove(peer_url);
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
        first_back_off_interval: u32,
        last_back_off_interval: u32,
        num_back_off_intervals: usize,
    ) -> Self {
        let mut back_off = backon::ExponentialBuilder::default()
            .with_factor(2.0)
            .with_min_delay(Duration::from_millis(
                first_back_off_interval as u64,
            ))
            .with_max_delay(Duration::from_millis(
                last_back_off_interval as u64,
            ))
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

    /// Get the timestamp when the current back off will expire.
    pub(crate) fn current_backoff_expiry(&self) -> Timestamp {
        let remaining_duration = self
            .current_interval
            .saturating_sub(self.interval_start.elapsed());
        Timestamp::now() + remaining_duration
    }
}

#[cfg(test)]
mod test {
    use crate::factories::core_fetch::back_off::BackOffList;
    use crate::factories::core_fetch::test::test_utils::random_peer_url;

    #[test]
    fn back_off() {
        let mut back_off_list = BackOffList::new(10, 10, 2);
        let agent_id = random_peer_url();
        back_off_list.back_off_peer(&agent_id);
        assert!(back_off_list.is_peer_on_back_off(&agent_id));

        std::thread::sleep(
            back_off_list.state.get(&agent_id).unwrap().current_interval,
        );

        assert!(!back_off_list.is_peer_on_back_off(&agent_id));

        back_off_list.back_off_peer(&agent_id);
        assert!(back_off_list.is_peer_on_back_off(&agent_id));

        std::thread::sleep(
            back_off_list.state.get(&agent_id).unwrap().current_interval,
        );

        assert!(!back_off_list.is_peer_on_back_off(&agent_id));
    }
}
