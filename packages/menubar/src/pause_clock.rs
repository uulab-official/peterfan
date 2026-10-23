//! Detect event-loop pauses even on platforms whose Instant stops during sleep.
use std::time::{Duration, Instant};

pub(crate) struct PauseClock {
    gap: Duration,
    last: Option<(Instant, Option<Duration>)>,
}

impl PauseClock {
    pub(crate) fn new(gap: Duration) -> Self {
        Self { gap, last: None }
    }

    pub(crate) fn observe(&mut self, now: Instant, continuous: Option<Duration>) -> bool {
        let previous = self.last.replace((now, continuous));
        previous.is_some_and(|(last, last_continuous)| {
            now.saturating_duration_since(last) >= self.gap
                || last_continuous
                    .zip(continuous)
                    .and_then(|(last, now)| now.checked_sub(last))
                    .is_some_and(|elapsed| elapsed >= self.gap)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suspension_is_detected_when_instant_barely_advances() {
        let awake = Instant::now();
        let mut clock = PauseClock::new(Duration::from_secs(8));
        assert!(!clock.observe(awake, Some(Duration::from_secs(100))));
        assert!(clock.observe(
            awake + Duration::from_millis(10),
            Some(Duration::from_secs(3700))
        ));
        assert!(!clock.observe(
            awake + Duration::from_millis(20),
            Some(Duration::from_millis(3_700_010))
        ));
    }

    #[test]
    fn ordinary_blocking_pause_still_recovers_without_a_native_counter() {
        let awake = Instant::now();
        let mut clock = PauseClock::new(Duration::from_secs(8));
        assert!(!clock.observe(awake, None));
        assert!(!clock.observe(awake + Duration::from_secs(7), None));
        assert!(clock.observe(awake + Duration::from_secs(15), None));
        assert!(!clock.observe(awake + Duration::from_secs(15), None));
    }

    #[test]
    fn backwards_or_temporarily_unavailable_native_counter_does_not_panic() {
        let awake = Instant::now();
        let mut clock = PauseClock::new(Duration::from_secs(8));
        assert!(!clock.observe(awake, Some(Duration::from_secs(100))));
        assert!(!clock.observe(
            awake + Duration::from_millis(10),
            Some(Duration::from_secs(1))
        ));
        assert!(!clock.observe(awake + Duration::from_millis(20), None));
        assert!(!clock.observe(
            awake + Duration::from_millis(30),
            Some(Duration::from_secs(200))
        ));
        assert!(clock.observe(
            awake + Duration::from_secs(9),
            Some(Duration::from_secs(201))
        ));
    }
}
