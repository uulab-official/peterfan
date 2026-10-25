#![cfg_attr(not(any(target_os = "macos", test)), allow(dead_code))]
//! Bounded CPU-widget data and original monochrome template rasters.
use peterfan_core::config::{CpuWidgetConfig, CpuWidgetStyle};
use std::collections::VecDeque;
use std::time::{Duration, Instant};

pub(crate) const WIDGET_PIXEL_WIDTH: u32 = 64;
pub(crate) const WIDGET_PIXEL_HEIGHT: u32 = 36;
const HISTORY_CAP: usize = 60;

#[derive(Default)]
pub(crate) struct CpuWidgetHistory {
    samples: VecDeque<Option<f32>>,
    sampled_at: Option<Instant>,
    cores: Vec<Option<f32>>,
}

impl CpuWidgetHistory {
    #[cfg(test)]
    pub(crate) fn sample_at(&mut self, value: f32, now: Instant) {
        self.sample_cpu_at(value, &[], now);
    }

    pub(crate) fn sample_cpu_at(&mut self, value: f32, cores: &[f32], now: Instant) {
        if let Some(previous) = self.sampled_at {
            let elapsed = now.saturating_duration_since(previous);
            if elapsed < Duration::from_secs(1) {
                return;
            }
            if elapsed > Duration::from_secs(8) {
                self.samples.clear();
            }
        }
        self.sampled_at = Some(now);
        self.push(value);
        if cores.is_empty() {
            self.cores.fill(None);
        } else {
            self.cores = cores.iter().copied().map(valid).collect();
        }
    }
    pub(crate) fn push(&mut self, value: f32) {
        if self.samples.len() == HISTORY_CAP {
            self.samples.pop_front();
        }
        self.samples.push_back(valid(value));
    }

    pub(crate) fn values(&self) -> Vec<Option<f32>> {
        self.samples.iter().copied().collect()
    }

    pub(crate) fn current(&self) -> Option<f32> {
        self.samples.back().copied().flatten()
    }

    pub(crate) fn core_values(&self) -> &[Option<f32>] {
        &self.cores
    }

    pub(crate) fn pixel_width(&self, style: CpuWidgetStyle) -> u32 {
        if style == CpuWidgetStyle::Cores && !self.cores.is_empty() {
            4 + 8 * self.cores.len() as u32
        } else {
            WIDGET_PIXEL_WIDTH
        }
    }

    pub(crate) fn point_width(&self, config: CpuWidgetConfig) -> f64 {
        if config.style == CpuWidgetStyle::Cores {
            f64::from(self.pixel_width(config.style)) / 2.
                + 12.
                + if config.show_percentage { 38. } else { 0. }
        } else {
            point_width(config)
        }
    }

    pub(crate) fn render_rgba(&self, style: CpuWidgetStyle) -> Vec<u8> {
        let width = self.pixel_width(style);
        let mut pixels = vec![0; (width * WIDGET_PIXEL_HEIGHT * 4) as usize];
        let mut rect = |left, top, right, bottom, alpha| {
            rectangle(&mut pixels, width, left, top, right, bottom, alpha);
        };
        match style {
            CpuWidgetStyle::Number => {}
            CpuWidgetStyle::Cores => {
                if self.cores.is_empty() {
                    rect(28, 17, 36, 19, 180);
                }
                for (index, value) in self.cores.iter().enumerate() {
                    let left = 2 + 8 * index;
                    rect(left, 3, left + 6, 33, 48);
                    if let Some(value) = value {
                        let height = (30. * value / 100.).round() as usize;
                        rect(left, 33 - height, left + 6, 33, 255);
                    } else {
                        rect(left + 1, 17, left + 5, 19, 180);
                    }
                }
            }
            CpuWidgetStyle::Bar => {
                rect(4, 9, 60, 27, 100);
                rect(6, 11, 58, 25, 0);
                if let Some(value) = self.current() {
                    let end = 6 + (52. * value / 100.).round() as usize;
                    rect(6, 11, end, 25, 255);
                } else {
                    rect(28, 17, 36, 19, 180);
                }
            }
            CpuWidgetStyle::History => {
                for y in [3, 18, 32] {
                    rect(2, y, 62, y + 1, 48);
                }
                // Newest samples always meet the right edge. Missing samples
                // occupy a column without inventing a zero or joining a gap.
                let offset = HISTORY_CAP - self.samples.len();
                for (index, value) in self.samples.iter().enumerate() {
                    if let Some(value) = value {
                        let height = (29. * value / 100.).round() as usize;
                        rect(2 + offset + index, 32 - height, 3 + offset + index, 33, 255);
                    }
                }
            }
        }
        pixels
    }
}

#[cfg(test)]
mod cadence_tests {
    use super::*;
    #[test]
    fn cpu_widget_history_samples_at_one_hz_and_discards_suspend_gap() {
        let start = Instant::now();
        let mut history = CpuWidgetHistory::default();
        history.sample_at(10., start);
        for millis in [1, 200, 400, 600, 999] {
            history.sample_at(90., start + Duration::from_millis(millis));
        }
        assert_eq!(history.values(), vec![Some(10.)]);
        history.sample_at(20., start + Duration::from_secs(1));
        assert_eq!(history.values(), vec![Some(10.), Some(20.)]);
        history.sample_at(30., start + Duration::from_secs(10));
        assert_eq!(history.values(), vec![Some(30.)]);
    }
}

fn valid(value: f32) -> Option<f32> {
    (value.is_finite() && (0.0..=100.0).contains(&value)).then_some(value)
}

fn rectangle(
    pixels: &mut [u8],
    width: u32,
    left: usize,
    top: usize,
    right: usize,
    bottom: usize,
    alpha: u8,
) {
    for y in top..bottom.min(WIDGET_PIXEL_HEIGHT as usize) {
        for x in left..right.min(width as usize) {
            let start = (y * width as usize + x) * 4;
            pixels[start..start + 4].copy_from_slice(&[0, 0, 0, alpha]);
        }
    }
}

pub(crate) fn title(config: CpuWidgetConfig, value: Option<f32>) -> String {
    if config.style != CpuWidgetStyle::Number && !config.show_percentage {
        return String::new();
    }
    let number = value
        .and_then(valid)
        .map_or_else(|| " --%".to_string(), |v| format!("{v:>3.0}%"));
    if config.style == CpuWidgetStyle::Number {
        format!("CPU {number}")
    } else {
        number
    }
}

pub(crate) fn point_width(config: CpuWidgetConfig) -> f64 {
    match (config.style, config.show_percentage) {
        (CpuWidgetStyle::Number, _) => 68.,
        (_, true) => 82.,
        (_, false) => 44.,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn core_bars_use_individual_fixed_scale_and_keep_every_core() {
        let start = Instant::now();
        let mut h = CpuWidgetHistory::default();
        for count in [1, 8, 10, 18, 32, 64] {
            let values: Vec<_> = (0..count).map(|i| [0., 50., 100.][i % 3]).collect();
            h.sample_cpu_at(90., &values, start + Duration::from_secs(count as u64));
            let width = h.pixel_width(CpuWidgetStyle::Cores);
            assert_eq!(width, 4 + 8 * count as u32);
            let image = h.render_rgba(CpuWidgetStyle::Cores);
            assert_eq!(image.len(), (width * WIDGET_PIXEL_HEIGHT * 4) as usize);
            for (i, value) in values.iter().enumerate() {
                let x = 2 + 8 * i;
                let opaque = (0..36)
                    .filter(|y| image[(y * width as usize + x) * 4 + 3] == 255)
                    .count();
                assert_eq!(opaque, (30.0 * value / 100.0_f32).round() as usize);
                assert!((0..36).all(|y| image[(y * width as usize + x + 6) * 4 + 3] == 0));
            }
        }
    }

    #[test]
    fn core_samples_share_cadence_and_missing_values_never_keep_old_load() {
        let start = Instant::now();
        let mut h = CpuWidgetHistory::default();
        h.sample_cpu_at(50., &[0., 100.], start);
        h.sample_cpu_at(20., &[80.; 8], start + Duration::from_millis(500));
        assert_eq!(h.core_values(), &[Some(0.), Some(100.)]);
        let config = CpuWidgetConfig {
            style: CpuWidgetStyle::Cores,
            show_percentage: false,
            ..CpuWidgetConfig::default()
        };
        let width = h.point_width(config);
        h.sample_cpu_at(50., &[f32::NAN, 101.], start + Duration::from_secs(1));
        assert_eq!(h.core_values(), &[None, None]);
        assert_eq!(h.point_width(config), width);
        h.sample_cpu_at(30., &[], start + Duration::from_secs(2));
        assert_eq!(h.core_values(), &[None, None]);
        h.sample_cpu_at(50., &[50.; 8], start + Duration::from_secs(11));
        assert_eq!(h.values(), vec![Some(50.)]);
        assert!(h.point_width(config) > width);
        assert_eq!(h.core_values().len(), 8);
    }

    fn filled(rgba: &[u8]) -> usize {
        rgba.as_chunks::<4>()
            .0
            .iter()
            .filter(|p| p[3] == 255)
            .count()
    }

    #[test]
    fn bar_uses_fixed_0_to_100_scale_and_owned_template_pixels() {
        let mut history = CpuWidgetHistory::default();
        let counts: Vec<_> = [0., 50., 100.]
            .into_iter()
            .map(|value| {
                history.push(value);
                let pixels = history.render_rgba(CpuWidgetStyle::Bar);
                assert_eq!(
                    pixels.len(),
                    (WIDGET_PIXEL_WIDTH * WIDGET_PIXEL_HEIGHT * 4) as usize
                );
                assert!(pixels
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .all(|p| p[0] == p[1] && p[1] == p[2]));
                filled(&pixels)
            })
            .collect();
        assert!(counts[0] < counts[1] && counts[1] < counts[2]);
    }

    #[test]
    fn history_is_bounded_and_keeps_invalid_samples_as_visible_gaps() {
        let mut history = CpuWidgetHistory::default();
        for _ in 0..1000 {
            history.push(50.);
        }
        assert_eq!(history.samples.len(), HISTORY_CAP);
        assert_eq!(history.current(), Some(50.));
        let valid = history.render_rgba(CpuWidgetStyle::History);
        history.push(f32::NAN);
        assert_eq!(history.current(), None);
        let invalid = history.render_rgba(CpuWidgetStyle::History);
        assert!(filled(&invalid) < filled(&valid));
        for value in [f32::INFINITY, -1., 101.] {
            history.push(value);
            assert_eq!(history.current(), None);
        }
        assert_eq!(history.samples.len(), HISTORY_CAP);
    }

    #[test]
    fn number_mode_and_missing_data_never_invent_zero_or_shift_width() {
        let mut config = CpuWidgetConfig {
            enabled: true,
            style: CpuWidgetStyle::Number,
            show_percentage: false,
        };
        assert_eq!(title(config, Some(9.)), "CPU   9%");
        assert_eq!(title(config, Some(100.)), "CPU 100%");
        assert_eq!(title(config, None), "CPU  --%");
        assert_eq!(point_width(config), 68.);
        config.style = CpuWidgetStyle::Bar;
        assert_eq!(title(config, Some(42.)), "");
        assert_eq!(point_width(config), 44.);
        config.show_percentage = true;
        assert_eq!(title(config, Some(42.)), " 42%");
        assert_eq!(point_width(config), 82.);
        config.style = CpuWidgetStyle::History;
        assert_eq!(point_width(config), 82.);
        assert!(CpuWidgetHistory::default()
            .render_rgba(CpuWidgetStyle::Number)
            .iter()
            .all(|v| *v == 0));
    }
}
