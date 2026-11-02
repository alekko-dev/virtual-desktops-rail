//! Bounded, in-memory diagnostics. QPC has a common epoch across local processes.
use serde::Serialize;
use std::collections::VecDeque;

#[link(name = "kernel32")]
unsafe extern "system" {
    fn QueryPerformanceCounter(counter: *mut i64) -> i32;
    fn QueryPerformanceFrequency(frequency: *mut i64) -> i32;
}

pub fn clock_us() -> Option<u64> {
    let mut counter = 0;
    let mut frequency = 0;
    if unsafe { QueryPerformanceCounter(&mut counter) } == 0
        || unsafe { QueryPerformanceFrequency(&mut frequency) } == 0
        || counter < 0
        || frequency <= 0
    {
        return None;
    }
    u64::try_from(counter as u128 * 1_000_000 / frequency as u128).ok()
}

#[derive(Default, Serialize)]
pub struct DispatchLatency {
    pub count: u64,
    pub max_us: u64,
    pub over_50ms: u64,
    pub recent_us: VecDeque<u64>,
}

impl DispatchLatency {
    pub fn record(&mut self, us: u64) {
        self.count = self.count.saturating_add(1);
        self.max_us = self.max_us.max(us);
        self.over_50ms = self.over_50ms.saturating_add(u64::from(us > 50_000));
        if self.recent_us.len() == 256 {
            self.recent_us.pop_front();
        }
        self.recent_us.push_back(us);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latency_history_is_bounded_but_retains_lifetime_failures() {
        let mut stats = DispatchLatency::default();
        stats.record(50_001);
        for _ in 0..300 {
            stats.record(50_000);
        }
        assert_eq!(stats.count, 301);
        assert_eq!(stats.max_us, 50_001);
        assert_eq!(stats.over_50ms, 1);
        assert_eq!(stats.recent_us.len(), 256);
        assert!(stats.recent_us.iter().all(|us| *us == 50_000));
    }
}
