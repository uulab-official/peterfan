use std::time::Duration;

use peterfan_core::config::{MenubarDisplay, RunnerCharacter};

pub(crate) const FRAME_COUNT: u8 = 8;
pub(crate) const MIN_INTERVAL: Duration = Duration::from_millis(20);
pub(crate) const MAX_INTERVAL: Duration = Duration::from_millis(180);
pub(crate) const NATIVE_TICK: Duration = MIN_INTERVAL;
#[cfg(target_os = "macos")]
pub(crate) const NATIVE_TOLERANCE: Duration = Duration::from_millis(3);

fn finite_cpu(cpu: f32) -> f32 {
    if cpu.is_finite() {
        cpu.clamp(0.0, 100.0)
    } else {
        0.0
    }
}

pub(crate) fn frame_interval(cpu_pct: f32) -> Duration {
    let load = finite_cpu(cpu_pct) / 100.0;
    let min_ms = MIN_INTERVAL.as_millis() as f32;
    let max_ms = MAX_INTERVAL.as_millis() as f32;
    let ms = min_ms + (max_ms - min_ms) * (1.0 - load).powf(1.6);
    Duration::from_millis(ms.round() as u64)
}

// All eight-pose characters share one CPU clock. Per-character caps and
// contact-pose holds made exercise and dance look slower at the same load.
pub(crate) fn frame_interval_for_character(_character: RunnerCharacter, cpu_pct: f32) -> Duration {
    frame_interval(cpu_pct)
}

pub(crate) fn frame_interval_for_pose(
    _character: RunnerCharacter,
    cpu_pct: f32,
    _frame: u8,
) -> Duration {
    frame_interval(cpu_pct)
}

pub(crate) fn smooth_cpu(previous: f32, sample: f32, has_sample: bool) -> f32 {
    let sample = finite_cpu(sample);
    let previous = finite_cpu(previous);
    if !has_sample {
        return sample;
    }
    let alpha = if sample >= previous { 1.0 } else { 0.22 };
    (previous + (sample - previous) * alpha).clamp(0.0, 100.0)
}

pub(crate) fn enabled(display: MenubarDisplay) -> bool {
    matches!(display, MenubarDisplay::Graph | MenubarDisplay::Both)
}

pub(crate) fn should_animate(display: MenubarDisplay, reduce_motion: bool) -> bool {
    enabled(display) && !reduce_motion
}

pub(crate) fn uses_event_loop_timer(display: MenubarDisplay, reduce_motion: bool) -> bool {
    #[cfg(target_os = "macos")]
    {
        let _ = (display, reduce_motion);
        false
    }
    #[cfg(not(target_os = "macos"))]
    {
        should_animate(display, reduce_motion)
    }
}

pub(crate) fn load_band(cpu_pct: f32) -> usize {
    match finite_cpu(cpu_pct) {
        x if x < 20.0 => 0,
        x if x < 55.0 => 1,
        x if x < 80.0 => 2,
        _ => 3,
    }
}

pub(crate) fn load_band_with_hysteresis(cpu_pct: f32, current: usize) -> usize {
    let load = finite_cpu(cpu_pct);
    match current {
        0 if load >= 24.0 => 1,
        0 => 0,
        1 if load < 16.0 => 0,
        1 if load >= 59.0 => 2,
        1 => 1,
        2 if load < 51.0 => 1,
        2 if load >= 84.0 => 3,
        2 => 2,
        3 if load < 76.0 => 2,
        3 => 3,
        _ => load_band(load),
    }
}

#[cfg(test)]
pub(crate) fn phase_step(phase_us: u64, tick_us: u64, cpu_pct: f32, frame: u8) -> (u64, bool) {
    let interval_us =
        frame_interval_for_pose(RunnerCharacter::Cat, cpu_pct, frame).as_micros() as u64;
    let next = phase_us.saturating_add(tick_us);
    if next >= interval_us {
        (next % interval_us, true)
    } else {
        (next, false)
    }
}

/// Consume real elapsed time, including late callbacks, in chronological pose order.
/// A suspended app resumes its current pose instead of replaying stale animation.
pub(crate) fn advance_phase(
    character: RunnerCharacter,
    phase_us: u64,
    elapsed_us: u64,
    cpu_pct: f32,
    frame: u8,
) -> (u64, u8) {
    if elapsed_us > 1_000_000 {
        return (0, frame % FRAME_COUNT);
    }
    let mut remaining = phase_us
        .min((MAX_INTERVAL * 2).as_micros() as u64)
        .saturating_add(elapsed_us);
    let mut pose = frame % FRAME_COUNT;
    while remaining >= frame_interval_for_pose(character, cpu_pct, pose).as_micros() as u64 {
        remaining -= frame_interval_for_pose(character, cpu_pct, pose).as_micros() as u64;
        pose = (pose + 1) % FRAME_COUNT;
    }
    (remaining, pose)
}

#[cfg(target_os = "macos")]
pub(crate) fn native_next_delay(
    character: RunnerCharacter,
    phase_us: u64,
    motion_cpu_pct: f32,
    target_cpu_pct: f32,
    frame: u8,
) -> Duration {
    if (target_cpu_pct - motion_cpu_pct).abs() >= 0.1 {
        return NATIVE_TICK;
    }
    let interval_us = frame_interval_for_pose(character, motion_cpu_pct, frame).as_micros() as u64;
    let remaining_us = interval_us.saturating_sub(phase_us.min(interval_us));
    Duration::from_micros(remaining_us.max(1_000))
}

pub(crate) fn ease_motion_cpu(current: f32, target: f32, tick: Duration) -> f32 {
    let current = finite_cpu(current);
    let target = finite_cpu(target);
    let delta = target - current;
    if delta.abs() < 0.1 {
        return target;
    }

    let response_seconds = if delta > 0.0 { 0.075 } else { 0.30 };
    let alpha = 1.0 - (-tick.as_secs_f32() / response_seconds).exp();
    (current + delta * alpha).clamp(0.0, 100.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_character_and_pose_shares_the_same_cpu_cadence() {
        for cpu in 0..=100 {
            for character in RunnerCharacter::ALL {
                assert_eq!(
                    frame_interval_for_character(character, cpu as f32),
                    frame_interval(cpu as f32),
                    "{character:?} at {cpu}% must use the common CPU clock"
                );
                for pose in 0..FRAME_COUNT {
                    assert_eq!(
                        frame_interval_for_pose(character, cpu as f32, pose),
                        frame_interval(cpu as f32),
                        "{character:?} pose {pose} must not have a separate speed cap"
                    );
                }
            }
        }
    }

    #[test]
    fn exercise_dance_and_ghost_keep_the_same_elapsed_pose_and_remainder() {
        for character in [
            RunnerCharacter::Cat,
            RunnerCharacter::Pushup,
            RunnerCharacter::Ghost,
            RunnerCharacter::Party,
        ] {
            assert_eq!(
                frame_interval_for_character(character, 100.0) * 8,
                Duration::from_millis(160)
            );
            assert_eq!(advance_phase(character, 0, 95_000, 100.0, 0), (15_000, 4));
            assert_eq!(
                advance_phase(character, 15_000, 30_000, 100.0, 4),
                (5_000, 6)
            );
        }
    }

    #[test]
    fn idle_gait_is_readable_and_busy_gait_keeps_accelerating() {
        assert!(frame_interval(5.0) <= Duration::from_millis(180));
        assert!(frame_interval(95.0) <= Duration::from_millis(35));
        assert!(frame_interval(60.0) > frame_interval(80.0));
        assert!(frame_interval(80.0) > frame_interval(100.0));
        for cpu in 0..100 {
            assert!(frame_interval(cpu as f32) >= frame_interval(cpu as f32 + 1.0));
        }
    }

    #[test]
    fn acceleration_reaches_a_spike_within_a_quarter_second() {
        let mut cpu = 5.0;
        for _ in 0..8 {
            cpu = ease_motion_cpu(cpu, 95.0, Duration::from_millis(30));
        }
        assert!(cpu > 90.0, "animation still lagging at {cpu}%");
    }

    #[test]
    fn delayed_tick_catches_up_all_elapsed_poses_without_losing_remainder() {
        let (phase, pose) = advance_phase(RunnerCharacter::Cat, 0, 95_000, 100.0, 0);
        assert_eq!(pose, 4);
        assert_eq!(phase, 15_000);
        let (phase, pose) = advance_phase(RunnerCharacter::Cat, phase, 25_000, 100.0, pose);
        assert_eq!((phase, pose), (0, 6));
    }

    #[test]
    fn invalid_cpu_samples_keep_a_finite_nonzero_cadence() {
        for sample in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert!(frame_interval(sample) >= MIN_INTERVAL);
            let (phase, pose) = advance_phase(RunnerCharacter::Cat, 0, 95_000, sample, 0);
            assert_eq!(pose, 0);
            assert_eq!(phase, 95_000);
        }
    }

    #[test]
    fn long_idle_pose_preserves_its_accumulated_progress() {
        let (phase, pose) = advance_phase(RunnerCharacter::Cat, 170_000, 1_000, 0.0, 0);
        assert_eq!((phase, pose), (171_000, 0));
    }

    #[test]
    fn workload_spikes_reach_the_motion_target_in_one_sample() {
        assert_eq!(smooth_cpu(5.0, 95.0, true), 95.0);
        assert!(smooth_cpu(95.0, 5.0, true) > 70.0);
    }

    #[test]
    fn long_pause_never_replays_a_backlog_of_animation_frames() {
        let (phase, pose) = advance_phase(RunnerCharacter::Cat, 0, 30_000_000, 100.0, 2);
        assert_eq!((phase, pose), (0, 2));
    }

    #[test]
    fn cpu_load_maps_monotonically_to_runner_speed() {
        let idle = frame_interval(5.0);
        let normal = frame_interval(45.0);
        let busy = frame_interval(95.0);
        assert!(idle > normal && normal > busy);
        assert!((167..=169).contains(&idle.as_millis()));
        assert!((80..=82).contains(&normal.as_millis()));
        assert!((20..=22).contains(&busy.as_millis()));
    }

    #[test]
    fn motion_attack_is_faster_than_decay_without_overshoot() {
        let tick = Duration::from_millis(30);
        let attack = ease_motion_cpu(10.0, 90.0, tick);
        let decay = ease_motion_cpu(90.0, 10.0, tick);
        assert!(attack > 10.0 && attack < 90.0);
        assert!(decay > 10.0 && decay < 90.0);
        assert!(attack - 10.0 > 90.0 - decay);
    }

    #[test]
    fn phase_keeps_remainder_when_cadence_changes() {
        let (phase, advanced) = phase_step(10_000, 15_000, 100.0, 0);
        assert!(advanced);
        assert_eq!(phase, 5_000);
        let (phase, advanced) = phase_step(phase, 10_000, 100.0, 1);
        assert!(!advanced);
        assert_eq!(phase, 15_000);
        let (phase, advanced) = phase_step(phase, 10_000, 100.0, 1);
        assert!(advanced);
        assert_eq!(phase, 5_000);
    }
}
