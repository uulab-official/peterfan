use peterfan_core::config::RunnerCharacter;
use serde::{Deserialize, Serialize};

use super::{manifest, RgbaFrame, RunnerBuildError, RunnerPack, LOAD_BANDS, PIXEL_HEIGHT, PIXEL_WIDTH};

pub(crate) const CHANNELS_PER_PIXEL: usize = 4;
pub(crate) const VISIBLE_ALPHA_THRESHOLD: u8 = 96;
const DELTA_ALPHA_THRESHOLD: u8 = 48;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Bounds {
    pub min_x: u32,
    pub min_y: u32,
    pub max_x: u32,
    pub max_y: u32,
}

impl Bounds {
    pub fn width(&self) -> u32 {
        self.max_x.saturating_sub(self.min_x) + 1
    }

    pub fn height(&self) -> u32 {
        self.max_y.saturating_sub(self.min_y) + 1
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct FrameMetrics {
    pub bounds: Bounds,
    pub centroid_x: f32,
    pub baseline_y: u32,
    pub opaque_pixels: usize,
    pub clipped: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct CycleMetrics {
    pub frames: [FrameMetrics; 8],
    pub max_centroid_drift: f32,
    pub baseline_variance: u32,
    pub minimum_frame_delta: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct RunnerBandReport {
    pub cpu_pct: f32,
    pub max_centroid_drift: f32,
    pub allowed_centroid_drift: f32,
    pub baseline_variance: u32,
    pub minimum_frame_delta: usize,
    pub clipped: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct RunnerCharacterReport {
    pub name: String,
    pub bands: Vec<RunnerBandReport>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct RunnerQualityReport {
    pub characters: Vec<RunnerCharacterReport>,
}

pub(crate) const LOAD_PRESET_CPUS: [f32; LOAD_BANDS] = [0.0, 30.0, 65.0, 90.0];

pub(crate) fn measure_frame(frame: &RgbaFrame) -> Result<FrameMetrics, RunnerBuildError> {
    validate_frame_len(frame)?;

    let mut min_x = PIXEL_WIDTH;
    let mut min_y = PIXEL_HEIGHT;
    let mut max_x = 0;
    let mut max_y = 0;
    let mut opaque_pixels = 0usize;
    let mut centroid_sum_x = 0.0_f32;

    for (index, pixel) in frame.chunks_exact(CHANNELS_PER_PIXEL).enumerate() {
        if pixel[3] <= VISIBLE_ALPHA_THRESHOLD {
            continue;
        }

        let pixel_index = index as u32;
        let x = pixel_index % PIXEL_WIDTH;
        let y = pixel_index / PIXEL_WIDTH;
        min_x = min_x.min(x);
        min_y = min_y.min(y);
        max_x = max_x.max(x);
        max_y = max_y.max(y);
        opaque_pixels += 1;
        centroid_sum_x += x as f32;
    }

    if opaque_pixels == 0 {
        return Err(invalid_frame_error("empty frame"));
    }

    let bounds = Bounds {
        min_x,
        min_y,
        max_x,
        max_y,
    };
    Ok(FrameMetrics {
        bounds,
        centroid_x: centroid_sum_x / opaque_pixels as f32,
        baseline_y: max_y,
        opaque_pixels,
        clipped: min_x == 0 || min_y == 0 || max_x + 1 == PIXEL_WIDTH || max_y + 1 == PIXEL_HEIGHT,
    })
}

pub(crate) fn measure_cycle(cycle: &[RgbaFrame; 8]) -> Result<CycleMetrics, RunnerBuildError> {
    let frame_metrics: [Result<FrameMetrics, RunnerBuildError>; 8] =
        std::array::from_fn(|index| measure_frame(&cycle[index]));
    let frames: [FrameMetrics; 8] = frame_metrics
        .into_iter()
        .collect::<Result<Vec<_>, RunnerBuildError>>()?
        .try_into()
        .expect("array length matches cycle length");

    let min_centroid = frames
        .iter()
        .map(|frame| frame.centroid_x)
        .fold(f32::INFINITY, f32::min);
    let max_centroid = frames
        .iter()
        .map(|frame| frame.centroid_x)
        .fold(f32::NEG_INFINITY, f32::max);
    let min_baseline = frames
        .iter()
        .map(|frame| frame.baseline_y)
        .fold(u32::MAX, u32::min);
    let max_baseline = frames
        .iter()
        .map(|frame| frame.baseline_y)
        .fold(0, u32::max);

    let minimum_frame_delta = frames
        .iter()
        .enumerate()
        .map(|(index, _)| frame_delta(&cycle[index], &cycle[(index + 1) % cycle.len()]))
        .min()
        .unwrap_or(0);

    Ok(CycleMetrics {
        frames,
        max_centroid_drift: max_centroid - min_centroid,
        baseline_variance: max_baseline.saturating_sub(min_baseline),
        minimum_frame_delta,
    })
}

pub(crate) fn build_quality_report() -> Result<RunnerQualityReport, RunnerBuildError> {
    let characters = RunnerCharacter::ALL
        .into_iter()
        .map(build_character_report)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(RunnerQualityReport { characters })
}

pub(crate) fn build_quality_report_json() -> Result<String, RunnerBuildError> {
    let report = build_quality_report()?;
    serde_json::to_string_pretty(&report).map_err(|error| {
        RunnerBuildError::InvalidFrame {
            character: RunnerCharacter::default(),
            detail: format!("quality report JSON failed: {error}"),
        }
    })
}

fn build_character_report(character: RunnerCharacter) -> Result<RunnerCharacterReport, RunnerBuildError> {
    let pack = RunnerPack::build(character)?;
    let manifest = manifest(character);
    let bands = LOAD_PRESET_CPUS
        .into_iter()
        .enumerate()
        .map(|(band, cpu_pct)| {
            let cycle: [RgbaFrame; 8] = std::array::from_fn(|frame| {
                pack.frame_by_index(band, frame as u8)
                    .expect("pack band frame")
                    .to_vec()
            });
            let metrics = measure_cycle(&cycle)?;
            Ok(RunnerBandReport {
                cpu_pct,
                max_centroid_drift: metrics.max_centroid_drift,
                allowed_centroid_drift: manifest.max_centroid_drift,
                baseline_variance: metrics.baseline_variance,
                minimum_frame_delta: metrics.minimum_frame_delta,
                clipped: metrics.frames.iter().any(|frame| frame.clipped),
            })
        })
        .collect::<Result<Vec<_>, RunnerBuildError>>()?;

    Ok(RunnerCharacterReport {
        name: character.as_str().to_string(),
        bands,
    })
}

pub(crate) fn validate_frame_len(frame: &RgbaFrame) -> Result<(), RunnerBuildError> {
    let expected_len = (PIXEL_WIDTH * PIXEL_HEIGHT) as usize * CHANNELS_PER_PIXEL;
    if frame.len() != expected_len {
        return Err(invalid_frame_error(format!(
            "buffer length {} does not match {}",
            frame.len(),
            expected_len
        )));
    }
    Ok(())
}

fn invalid_frame_error(detail: impl Into<String>) -> RunnerBuildError {
    RunnerBuildError::InvalidFrame {
        character: RunnerCharacter::default(),
        detail: detail.into(),
    }
}

fn frame_delta(left: &RgbaFrame, right: &RgbaFrame) -> usize {
    left.chunks_exact(CHANNELS_PER_PIXEL)
        .zip(right.chunks_exact(CHANNELS_PER_PIXEL))
        .filter(|(left, right)| left[3].abs_diff(right[3]) > DELTA_ALPHA_THRESHOLD)
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::{RgbaFrame, RunnerBuildError, PIXEL_HEIGHT, PIXEL_WIDTH};

    fn empty_frame() -> RgbaFrame {
        vec![0; (PIXEL_WIDTH * PIXEL_HEIGHT * 4) as usize]
    }

    fn paint_rect(
        frame: &mut RgbaFrame,
        min_x: u32,
        min_y: u32,
        max_x: u32,
        max_y: u32,
        alpha: u8,
    ) {
        for y in min_y..=max_y {
            for x in min_x..=max_x {
                frame[((y * PIXEL_WIDTH + x) * 4 + 3) as usize] = alpha;
            }
        }
    }

    #[test]
    fn frame_metrics_report_bounds_centroid_and_baseline() {
        let mut frame = empty_frame();
        paint_rect(&mut frame, 8, 10, 17, 19, 255);

        let metrics = measure_frame(&frame).expect("metrics");
        assert_eq!(
            metrics.bounds,
            Bounds {
                min_x: 8,
                min_y: 10,
                max_x: 17,
                max_y: 19,
            }
        );
        assert!((metrics.centroid_x - 12.5).abs() < 0.01);
        assert_eq!(metrics.baseline_y, 19);
        assert_eq!(metrics.opaque_pixels, 100);
        assert!(!metrics.clipped);
    }

    #[test]
    fn frame_metrics_reject_invalid_lengths_and_empty_frames() {
        let invalid = vec![0; (PIXEL_WIDTH * PIXEL_HEIGHT * 4 - 1) as usize];
        let invalid_error = measure_frame(&invalid).expect_err("invalid frame");
        assert!(matches!(
            invalid_error,
            RunnerBuildError::InvalidFrame { detail, .. }
            if detail.contains("buffer length")
        ));

        let empty_error = measure_frame(&empty_frame()).expect_err("empty frame");
        assert!(matches!(
            empty_error,
            RunnerBuildError::InvalidFrame { detail, .. }
            if detail.contains("empty")
        ));
    }

    #[test]
    fn frame_metrics_use_visible_alpha_threshold_and_edge_clipping() {
        let mut frame = empty_frame();
        paint_rect(&mut frame, 0, 5, 0, 6, 97);
        paint_rect(&mut frame, 2, 5, 2, 6, 96);
        paint_rect(
            &mut frame,
            PIXEL_WIDTH - 1,
            PIXEL_HEIGHT - 2,
            PIXEL_WIDTH - 1,
            PIXEL_HEIGHT - 1,
            255,
        );

        let metrics = measure_frame(&frame).expect("metrics");
        assert_eq!(metrics.bounds.min_x, 0);
        assert_eq!(metrics.bounds.max_x, PIXEL_WIDTH - 1);
        assert_eq!(metrics.bounds.max_y, PIXEL_HEIGHT - 1);
        assert_eq!(metrics.opaque_pixels, 4);
        assert!(metrics.clipped);
    }

    #[test]
    fn cycle_metrics_capture_drift_baseline_variance_and_minimum_delta() {
        let frames = std::array::from_fn(|index| {
            let mut frame = empty_frame();
            let x = 8 + index.min(3) as u32;
            let y = 18 + (index % 2) as u32;
            paint_rect(&mut frame, x, y, x + 7, y + 3, 255);
            frame
        });

        let metrics = measure_cycle(&frames).expect("cycle metrics");
        assert_eq!(metrics.frames.len(), 8);
        assert!((metrics.max_centroid_drift - 3.0).abs() < 0.01);
        assert_eq!(metrics.baseline_variance, 1);
        assert_eq!(metrics.minimum_frame_delta, 16);
    }

    #[test]
    fn cycle_metrics_ignore_small_alpha_differences_for_delta() {
        let frames = std::array::from_fn(|index| {
            let mut frame = empty_frame();
            let alpha = if index % 2 == 0 { 120 } else { 160 };
            paint_rect(&mut frame, 10, 12, 13, 15, alpha);
            frame
        });

        let metrics = measure_cycle(&frames).expect("cycle metrics");
        assert_eq!(metrics.minimum_frame_delta, 0);
    }

    #[test]
    fn quality_report_covers_every_character_and_load_band() {
        let report = build_quality_report().expect("quality report");
        assert_eq!(report.characters.len(), RunnerCharacter::ALL.len());
        assert!(
            report
                .characters
                .iter()
                .all(|entry| entry.bands.len() == LOAD_BANDS)
        );
        assert!(report.characters.iter().flat_map(|entry| &entry.bands).all(|band| {
            band.max_centroid_drift <= band.allowed_centroid_drift
                && band.minimum_frame_delta >= 24
                && !band.clipped
        }));
    }

    #[test]
    fn quality_report_json_is_pretty_and_sorted_by_character_order() {
        let json = build_quality_report_json().expect("quality report JSON");
        assert!(json.starts_with("{\n  \"characters\": [\n"));
        assert!(!json.contains(env!("CARGO_MANIFEST_DIR")));
        assert!(!json.contains("/Users/"));
        assert!(!json.contains("2026-"));

        let report: RunnerQualityReport =
            serde_json::from_str(&json).expect("quality report must parse");
        let names = report
            .characters
            .iter()
            .map(|character| character.name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            names,
            RunnerCharacter::ALL
                .map(|character| character.as_str())
                .to_vec()
        );
        for (character, entry) in RunnerCharacter::ALL.into_iter().zip(report.characters.iter()) {
            assert_eq!(entry.name, character.as_str());
            assert_eq!(entry.bands.len(), LOAD_BANDS);
            let band_cpus = entry.bands.iter().map(|band| band.cpu_pct).collect::<Vec<_>>();
            assert_eq!(band_cpus, LOAD_PRESET_CPUS.to_vec());
        }
    }
}
