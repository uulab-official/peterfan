use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use peterfan_core::config::RunnerCharacter;

pub(crate) const ROTATION_INTERVAL: Duration = Duration::from_secs(60);

/// A shuffled bag visits every character before starting another round.
/// Its local PRNG only selects artwork; it is never used for security.
pub(crate) struct RandomRunner {
    enabled: bool,
    next_change: Instant,
    bag: Vec<RunnerCharacter>,
    seed: u64,
}

impl RandomRunner {
    pub(crate) fn new(enabled: bool, now: Instant) -> Self {
        Self::with_seed(
            enabled,
            now,
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos() as u64
                ^ u64::from(std::process::id()),
        )
    }

    fn with_seed(enabled: bool, now: Instant, seed: u64) -> Self {
        Self {
            enabled,
            next_change: now,
            bag: Vec::new(),
            seed: seed.max(1),
        }
    }

    pub(crate) fn enabled(&self) -> bool {
        self.enabled
    }

    pub(crate) fn set_enabled(&mut self, enabled: bool, now: Instant) {
        if self.enabled != enabled {
            self.enabled = enabled;
            self.bag.clear();
            self.next_change = now;
        }
    }

    pub(crate) fn deadline(&self, visible_and_animated: bool) -> Option<Instant> {
        (self.enabled && visible_and_animated).then_some(self.next_change)
    }

    pub(crate) fn next_due(
        &mut self,
        now: Instant,
        current: RunnerCharacter,
        visible_and_animated: bool,
    ) -> Option<RunnerCharacter> {
        if !self.enabled || !visible_and_animated || now < self.next_change {
            return None;
        }
        // A long sleep advances once, never a burst of catch-up swaps.
        self.next_change = now + ROTATION_INTERVAL;
        if self.bag.is_empty() {
            self.bag.extend(RunnerCharacter::FEATURED);
            for i in (1..self.bag.len()).rev() {
                self.seed ^= self.seed << 13;
                self.seed ^= self.seed >> 7;
                self.seed ^= self.seed << 17;
                let j = (self.seed % (i as u64 + 1)) as usize;
                self.bag.swap(i, j);
            }
            let last = self.bag.len() - 1;
            if self.bag[last] == current {
                self.bag.swap(0, last);
            }
        }
        // A failed artwork build may leave `current` unchanged. Skip it too.
        while let Some(next) = self.bag.pop() {
            if next != current {
                return Some(next);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visits_every_character_once_without_repeating_at_round_boundaries() {
        let mut now = Instant::now();
        let mut rotation = RandomRunner::with_seed(true, now, 42);
        let mut current = RunnerCharacter::Cat;
        for _ in 0..4 {
            let mut round = Vec::new();
            for _ in RunnerCharacter::FEATURED {
                let next = rotation.next_due(now, current, true).unwrap();
                assert_ne!(next, current);
                assert!(!round.contains(&next));
                round.push(next);
                current = next;
                now += ROTATION_INTERVAL;
            }
            assert!(RunnerCharacter::FEATURED.iter().all(|c| round.contains(c)));
        }
    }

    #[test]
    fn waits_a_minute_and_pauses_for_number_only_or_reduced_motion() {
        let now = Instant::now();
        let mut rotation = RandomRunner::with_seed(true, now, 1);
        assert_eq!(rotation.deadline(false), None);
        assert_eq!(rotation.next_due(now, RunnerCharacter::Cat, false), None);
        let first = rotation.next_due(now, RunnerCharacter::Cat, true).unwrap();
        assert_eq!(rotation.deadline(true), Some(now + ROTATION_INTERVAL));
        assert_eq!(
            rotation.next_due(now + Duration::from_secs(59), first, true),
            None
        );
        let awake = now + Duration::from_secs(3600);
        assert!(rotation.next_due(awake, first, true).is_some());
        assert_eq!(rotation.next_due(awake, first, true), None);
    }

    #[test]
    fn fixed_selection_disables_rotation_and_reenable_starts_fresh() {
        let now = Instant::now();
        let mut rotation = RandomRunner::with_seed(true, now, 3);
        rotation.set_enabled(false, now);
        assert!(!rotation.enabled());
        assert_eq!(rotation.deadline(true), None);
        assert_eq!(
            rotation.next_due(now + ROTATION_INTERVAL, RunnerCharacter::Fox, true),
            None
        );
        rotation.set_enabled(true, now);
        let next = rotation.next_due(now, RunnerCharacter::Fox, true).unwrap();
        assert_ne!(next, RunnerCharacter::Fox);
        assert!(rotation.enabled());
    }
}
