use std::time::Duration;

use peterfan_core::config::MenubarDisplay;

pub(crate) const FRAME_COUNT: u8 = 8;
pub(crate) const MIN_INTERVAL: Duration = Duration::from_millis(60);
pub(crate) const MAX_INTERVAL: Duration = Duration::from_millis(480);
pub(crate) const NATIVE_TICK: Duration = MIN_INTERVAL;
#[cfg(target_os = "macos")]
pub(crate) const NATIVE_TOLERANCE: Duration = Duration::from_millis(8);

const GAIT_WEIGHTS: [f32; 8] = [1.14, 1.00, 0.84, 1.02, 1.14, 1.00, 0.84, 1.02];

pub(crate) fn frame_interval(cpu_pct: f32) -> Duration {
    let load = cpu_pct.clamp(0.0, 100.0) / 100.0;
    let min_ms = MIN_INTERVAL.as_millis() as f32;
    let max_ms = MAX_INTERVAL.as_millis() as f32;
    let ms = min_ms + (max_ms - min_ms) * (1.0 - load).powi(3);
    Duration::from_millis(ms.round() as u64)
}

pub(crate) fn frame_interval_for_pose(cpu_pct: f32, frame: u8) -> Duration {
    let load = cpu_pct.clamp(0.0, 100.0) / 100.0;
    let influence = (1.0 - load / 0.8).clamp(0.0, 1.0);
    let weight = 1.0 + (GAIT_WEIGHTS[usize::from(frame % FRAME_COUNT)] - 1.0) * influence;
    let millis = frame_interval(cpu_pct).as_secs_f32() * 1000.0 * weight;
    Duration::from_millis((millis.round() as u64).max(MIN_INTERVAL.as_millis() as u64))
}

pub(crate) fn smooth_cpu(previous: f32, sample: f32, has_sample: bool) -> f32 {
    let sample = sample.clamp(0.0, 100.0);
    if !has_sample {
        return sample;
    }
    let alpha = if sample >= previous { 0.82 } else { 0.22 };
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
    match cpu_pct.clamp(0.0, 100.0) {
        x if x < 20.0 => 0,
        x if x < 55.0 => 1,
        x if x < 80.0 => 2,
        _ => 3,
    }
}

pub(crate) fn load_band_with_hysteresis(cpu_pct: f32, current: usize) -> usize {
    let load = cpu_pct.clamp(0.0, 100.0);
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

pub(crate) fn phase_step(phase_us: u64, tick_us: u64, cpu_pct: f32, frame: u8) -> (u64, bool) {
    let interval_us = frame_interval_for_pose(cpu_pct, frame).as_micros() as u64;
    let next = phase_us.saturating_add(tick_us);
    if next >= interval_us {
        (next % interval_us, true)
    } else {
        (next, false)
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn native_next_delay(
    phase_us: u64,
    motion_cpu_pct: f32,
    target_cpu_pct: f32,
    frame: u8,
) -> Duration {
    if (target_cpu_pct - motion_cpu_pct).abs() >= 0.1 {
        return NATIVE_TICK;
    }
    let interval_us = frame_interval_for_pose(motion_cpu_pct, frame).as_micros() as u64;
    let remaining_us = interval_us.saturating_sub(phase_us.min(interval_us));
    Duration::from_micros(remaining_us.max(NATIVE_TICK.as_micros() as u64))
}

pub(crate) fn ease_motion_cpu(current: f32, target: f32, tick: Duration) -> f32 {
    let current = current.clamp(0.0, 100.0);
    let target = target.clamp(0.0, 100.0);
    let delta = target - current;
    if delta.abs() < 0.1 {
        return target;
    }

    let response_seconds = if delta > 0.0 { 0.18 } else { 0.42 };
    let alpha = 1.0 - (-tick.as_secs_f32() / response_seconds).exp();
    (current + delta * alpha).clamp(0.0, 100.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_load_maps_monotonically_to_runner_speed() {
        let idle = frame_interval(5.0);
        let normal = frame_interval(45.0);
        let busy = frame_interval(95.0);
        assert!(idle > normal && normal > busy);
        assert!((418..=422).contains(&idle.as_millis()));
        assert!((128..=132).contains(&normal.as_millis()));
        assert!((59..=61).contains(&busy.as_millis()));
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
        let (phase, advanced) = phase_step(450_000, 30_000, 5.0, 0);
        assert!(advanced);
        let (phase, advanced) = phase_step(phase, 30_000, 95.0, 1);
        assert!(!advanced);
        let (phase, advanced) = phase_step(phase, 30_000, 95.0, 1);
        assert!(advanced);
        assert!(phase < frame_interval_for_pose(95.0, 1).as_micros() as u64);
    }
}
