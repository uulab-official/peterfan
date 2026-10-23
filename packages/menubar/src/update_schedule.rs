//! Wall-clock update policy. A bounded poll also notices overdue work after sleep.
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

pub const STARTUP_DELAY: u64 = 20;
pub const CHECK_INTERVAL: u64 = 6 * 60 * 60;
const RETRY_DELAYS: [u64; 3] = [5 * 60, 15 * 60, 60 * 60];
const MAX_POLL: u64 = 30;

#[derive(Clone, Copy, Debug, Default)]
pub struct UpdateSchedule {
    pub last_attempt_unix: Option<u64>,
    pub last_success_unix: Option<u64>,
    pub next_check_unix: Option<u64>,
    failures: usize,
}

impl UpdateSchedule {
    pub const fn new() -> Self {
        Self {
            last_attempt_unix: None,
            last_success_unix: None,
            next_check_unix: None,
            failures: 0,
        }
    }

    pub fn start(&mut self, now: u64) {
        // A manual check may have already completed before the worker starts.
        if self.last_attempt_unix.is_none() && self.next_check_unix.is_none() {
            self.next_check_unix = Some(now.saturating_add(STARTUP_DELAY));
        }
    }

    pub fn due(&self, now: u64) -> bool {
        self.next_check_unix.is_some_and(|next| now >= next)
    }

    pub fn begin(&mut self, now: u64) {
        self.last_attempt_unix = Some(now);
        self.next_check_unix = None;
    }

    pub fn finish(&mut self, now: u64, success: bool) {
        let delay = if success {
            self.last_success_unix = Some(now);
            self.failures = 0;
            CHECK_INTERVAL
        } else {
            self.retry_delay()
        };
        self.next_check_unix = Some(now.saturating_add(delay));
    }

    fn retry_delay(&mut self) -> u64 {
        let delay = RETRY_DELAYS[self.failures.min(RETRY_DELAYS.len() - 1)];
        self.failures = self.failures.saturating_add(1);
        delay
    }

    pub fn finish_pending_assets(&mut self, now: u64) {
        self.last_success_unix = Some(now);
        self.next_check_unix = Some(now.saturating_add(self.retry_delay()));
    }

    pub fn poll_delay(&self, now: u64) -> Duration {
        Duration::from_secs(
            self.next_check_unix
                .map_or(MAX_POLL, |next| next.saturating_sub(now).clamp(1, MAX_POLL)),
        )
    }
}

/// Every exit path, including unwinding, releases the native update pipeline.
pub struct UpdateLease<'a>(&'a AtomicBool);
impl<'a> UpdateLease<'a> {
    pub fn acquire(flag: &'a AtomicBool) -> Option<Self> {
        flag.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .ok()
            .map(|_| Self(flag))
    }
}
impl Drop for UpdateLease<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Barrier};

    #[test]
    fn startup_and_six_hour_checks_run_once_when_due_or_overdue_after_sleep() {
        let mut s = UpdateSchedule::new();
        s.start(100);
        assert!(!s.due(119));
        assert!(s.due(120));
        s.begin(120);
        assert!(!s.due(120));
        s.finish(123, true);
        assert_eq!(s.next_check_unix, Some(123 + CHECK_INTERVAL));
        assert!(!s.due(123 + CHECK_INTERVAL - 1));
        assert!(s.due(123 + 3 * CHECK_INTERVAL));
        s.begin(123 + 3 * CHECK_INTERVAL);
        assert!(!s.due(123 + 3 * CHECK_INTERVAL));
    }

    #[test]
    fn failed_checks_back_off_and_success_resets_retries() {
        let mut s = UpdateSchedule::new();
        for delay in [300, 900, 3600, 3600] {
            s.begin(100);
            s.finish(101, false);
            assert_eq!(s.next_check_unix, Some(101 + delay));
            assert_eq!(s.last_success_unix, None);
        }
        s.begin(200);
        s.finish(201, true);
        assert_eq!(s.last_attempt_unix, Some(200));
        assert_eq!(s.last_success_unix, Some(201));
        s.begin(300);
        s.finish(301, false);
        assert_eq!(s.next_check_unix, Some(601));
        assert_eq!(s.last_success_unix, Some(201));
    }

    #[test]
    fn release_asset_uploads_retry_without_claiming_network_failure() {
        let mut s = UpdateSchedule::new();
        for (now, delay) in [(100, 300), (400, 900), (1300, 3600)] {
            s.begin(now);
            s.finish_pending_assets(now + 1);
            assert_eq!(s.last_success_unix, Some(now + 1));
            assert_eq!(s.next_check_unix, Some(now + 1 + delay));
        }
        s.finish(5000, true);
        assert_eq!(s.next_check_unix, Some(5000 + CHECK_INTERVAL));
    }

    #[test]
    fn manual_check_replaces_startup_or_retry_deadline() {
        let mut s = UpdateSchedule::new();
        s.start(100);
        s.begin(105);
        s.finish(106, true);
        s.start(107);
        assert_eq!(s.next_check_unix, Some(106 + CHECK_INTERVAL));
        s.begin(110);
        s.finish(111, false);
        s.begin(112);
        s.finish(113, true);
        assert!(!s.due(411));
        assert_eq!(s.next_check_unix, Some(113 + CHECK_INTERVAL));
    }

    #[test]
    fn bounded_wall_clock_poll_handles_overdue_and_backward_clock() {
        let mut s = UpdateSchedule::new();
        s.start(100);
        assert_eq!(s.poll_delay(100), Duration::from_secs(20));
        assert_eq!(s.poll_delay(0), Duration::from_secs(30));
        assert_eq!(s.poll_delay(1000), Duration::from_secs(1));
    }

    #[test]
    fn lease_serializes_auto_manual_install_and_channel_changes() {
        let flag = Arc::new(AtomicBool::new(false));
        let barrier = Arc::new(Barrier::new(9));
        let workers: Vec<_> = (0..8)
            .map(|_| {
                let flag = Arc::clone(&flag);
                let barrier = Arc::clone(&barrier);
                std::thread::spawn(move || {
                    let lease = UpdateLease::acquire(&flag);
                    barrier.wait();
                    lease.is_some()
                })
            })
            .collect();
        barrier.wait();
        assert_eq!(
            workers
                .into_iter()
                .map(|w| w.join().unwrap())
                .filter(|won| *won)
                .count(),
            1
        );
        assert!(UpdateLease::acquire(&flag).is_some());
    }

    #[test]
    fn lease_is_released_during_unwinding() {
        let flag = AtomicBool::new(false);
        let _ = std::panic::catch_unwind(|| {
            let _lease = UpdateLease::acquire(&flag).unwrap();
            panic!("simulated updater failure");
        });
        assert!(UpdateLease::acquire(&flag).is_some());
    }
}
