use super::{
    measure_cycle, measure_frame, Bounds, CycleMetrics, RgbaFrame, RunnerBuildError,
    RunnerManifest, PIXEL_HEIGHT, PIXEL_WIDTH,
};
use crate::runner::quality::{validate_frame_len, CHANNELS_PER_PIXEL};

#[derive(Debug, Clone, Copy)]
struct CycleTransform {
    scale: f32,
    translate_x: f32,
    translate_y: f32,
}

pub(crate) fn normalize_cycle(
    manifest: &RunnerManifest,
    source: [RgbaFrame; 8],
) -> Result<[RgbaFrame; 8], RunnerBuildError> {
    validate_manifest_pose_indices(manifest, "normalize_cycle")?;

    let source = source
        .into_iter()
        .enumerate()
        .map(|(pose, frame)| {
            validate_frame_len(&frame).map_err(|error| {
                stage_error(
                    manifest.character,
                    "normalize_cycle[buffer_length]",
                    format!("frame {pose}: {}", error_detail(&error)),
                )
            })?;
            if manifest.mirror_source {
                mirror_alpha(&frame).map_err(|error| {
                    stage_error(
                        manifest.character,
                        "normalize_cycle[mirror]",
                        format!("frame {pose}: {}", error_detail(&error)),
                    )
                })
            } else {
                Ok(frame)
            }
        })
        .collect::<Result<Vec<_>, _>>()?;

    let frame_bounds = source
        .iter()
        .enumerate()
        .map(|(pose, frame)| {
            alpha_bounds(frame).map_err(|error| {
                stage_error(
                    manifest.character,
                    "normalize_cycle[source_bounds]",
                    format!("frame {pose}: {}", error_detail(&error)),
                )
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let union = union_bounds(&frame_bounds).ok_or_else(|| {
        stage_error(
            manifest.character,
            "normalize_cycle[source_bounds]",
            "cycle did not contain visible alpha".to_string(),
        )
    })?;
    let union_width = union.width() as f32;
    let union_height = union.height() as f32;
    let scale = (manifest.target_width as f32 / union_width)
        .min(manifest.target_height as f32 / union_height);
    if !scale.is_finite() || scale <= 0.0 {
        return Err(stage_error(
            manifest.character,
            "normalize_cycle[scale]",
            format!(
                "invalid scale from union {}x{} into target {}x{}",
                union.width(),
                union.height(),
                manifest.target_width,
                manifest.target_height
            ),
        ));
    }

    let transform = cycle_transform(manifest, &frame_bounds, union, scale)?;
    let normalized = source
        .iter()
        .enumerate()
        .map(|(pose, frame)| {
            resample_place_white(manifest, pose, frame, frame_bounds[pose], transform)
        })
        .collect::<Result<Vec<_>, _>>()?;

    normalized.try_into().map_err(|frames: Vec<RgbaFrame>| {
        stage_error(
            manifest.character,
            "normalize_cycle[array_length]",
            format!("expected 8 normalized frames, got {}", frames.len()),
        )
    })
}

pub(crate) fn validate_cycle(
    manifest: &RunnerManifest,
    cycle: &[RgbaFrame; 8],
) -> Result<CycleMetrics, RunnerBuildError> {
    validate_manifest_pose_indices(manifest, "validate_cycle")?;

    for (index, frame) in cycle.iter().enumerate() {
        validate_frame_len(frame).map_err(|error| {
            stage_error(
                manifest.character,
                "validate_cycle[buffer_length]",
                format!("frame {index}: {}", error_detail(&error)),
            )
        })?;
        validate_white_pixels(manifest, index, frame)?;

        let metrics = measure_frame(frame).map_err(|error| {
            let detail = error_detail(&error);
            if detail.contains("empty frame") {
                stage_error(
                    manifest.character,
                    "validate_cycle[empty]",
                    format!("frame {index}: {detail}"),
                )
            } else {
                stage_error(
                    manifest.character,
                    "validate_cycle[frame_metrics]",
                    format!("frame {index}: {detail}"),
                )
            }
        })?;
        if metrics.clipped {
            return Err(stage_error(
                manifest.character,
                "validate_cycle[clipped]",
                format!("frame {index}: visible alpha touches the canvas edge"),
            ));
        }
    }

    let metrics = measure_cycle(cycle).map_err(|error| {
        stage_error(
            manifest.character,
            "validate_cycle[measure_cycle]",
            error_detail(&error).to_string(),
        )
    })?;

    let union = union_bounds(
        &metrics
            .frames
            .iter()
            .map(|frame| frame.bounds)
            .collect::<Vec<_>>(),
    )
    .ok_or_else(|| {
        stage_error(
            manifest.character,
            "validate_cycle[empty]",
            "cycle did not contain visible alpha".to_string(),
        )
    })?;
    let union_width = union.width();
    let union_height = union.height();
    if union_width > manifest.target_width || union_height > manifest.target_height {
        return Err(stage_error(
            manifest.character,
            "validate_cycle[union_max]",
            format!(
                "union {}x{} exceeds target {}x{}",
                union_width, union_height, manifest.target_width, manifest.target_height
            ),
        ));
    }
    if (union_width as f32) < manifest.target_width as f32 * 0.6
        && (union_height as f32) < manifest.target_height as f32 * 0.6
    {
        return Err(stage_error(
            manifest.character,
            "validate_cycle[union_min]",
            format!(
                "union {}x{} is below 60% of target {}x{}",
                union_width, union_height, manifest.target_width, manifest.target_height
            ),
        ));
    }
    if union_width.abs_diff(manifest.target_width) > 2
        && union_height.abs_diff(manifest.target_height) > 2
    {
        return Err(stage_error(
            manifest.character,
            "validate_cycle[union_fit]",
            format!(
                "union {}x{} misses both target dimensions {}x{} by more than 2px",
                union_width, union_height, manifest.target_width, manifest.target_height
            ),
        ));
    }

    if metrics.max_centroid_drift > manifest.max_centroid_drift {
        return Err(stage_error(
            manifest.character,
            "validate_cycle[centroid_drift]",
            format!(
                "centroid drift {:.2}px exceeds {:.2}px",
                metrics.max_centroid_drift, manifest.max_centroid_drift
            ),
        ));
    }

    let contact_baselines = manifest
        .contact_frames
        .map(|index| metrics.frames[usize::from(index)].baseline_y);
    let contact_min = contact_baselines.into_iter().fold(u32::MAX, u32::min);
    let contact_max = contact_baselines.into_iter().fold(0, u32::max);
    let contact_delta = contact_max.saturating_sub(contact_min);
    if contact_delta > 1 {
        return Err(stage_error(
            manifest.character,
            "validate_cycle[contact_baseline]",
            format!("contact frames differ by {}px (allowed 1px)", contact_delta),
        ));
    }

    for index in 0..cycle.len() {
        let next = (index + 1) % cycle.len();
        if cycle[index] == cycle[next] {
            return Err(stage_error(
                manifest.character,
                "validate_cycle[duplicate]",
                format!("frames {index} and {next} are identical"),
            ));
        }
    }

    if metrics.minimum_frame_delta < 24 {
        return Err(stage_error(
            manifest.character,
            "validate_cycle[minimum_frame_delta]",
            format!(
                "minimum frame delta {}px is below 24px",
                metrics.minimum_frame_delta
            ),
        ));
    }

    Ok(metrics)
}

fn mirror_alpha(frame: &RgbaFrame) -> Result<RgbaFrame, RunnerBuildError> {
    validate_frame_len(frame)?;

    let mut mirrored = vec![0; frame.len()];
    for y in 0..PIXEL_HEIGHT {
        for x in 0..PIXEL_WIDTH {
            let source_index = ((y * PIXEL_WIDTH + x) * CHANNELS_PER_PIXEL as u32) as usize;
            let target_index =
                ((y * PIXEL_WIDTH + (PIXEL_WIDTH - 1 - x)) * CHANNELS_PER_PIXEL as u32) as usize;
            let alpha = frame[source_index + 3];
            if alpha == 0 {
                continue;
            }
            mirrored[target_index..target_index + 3].copy_from_slice(&[255, 255, 255]);
            mirrored[target_index + 3] = alpha;
        }
    }
    Ok(mirrored)
}

fn alpha_bounds(frame: &RgbaFrame) -> Result<Bounds, RunnerBuildError> {
    validate_frame_len(frame)?;

    let mut min_x = PIXEL_WIDTH;
    let mut min_y = PIXEL_HEIGHT;
    let mut max_x = 0;
    let mut max_y = 0;
    let mut found = false;

    for (index, pixel) in frame.chunks_exact(CHANNELS_PER_PIXEL).enumerate() {
        if pixel[3] == 0 {
            continue;
        }
        let x = index as u32 % PIXEL_WIDTH;
        let y = index as u32 / PIXEL_WIDTH;
        min_x = min_x.min(x);
        min_y = min_y.min(y);
        max_x = max_x.max(x);
        max_y = max_y.max(y);
        found = true;
    }

    found
        .then_some(Bounds {
            min_x,
            min_y,
            max_x,
            max_y,
        })
        .ok_or_else(|| RunnerBuildError::InvalidFrame {
            character: manifest_character_placeholder(),
            detail: "empty frame".to_string(),
        })
}

fn union_bounds(bounds: &[Bounds]) -> Option<Bounds> {
    bounds.iter().copied().reduce(|left, right| Bounds {
        min_x: left.min_x.min(right.min_x),
        min_y: left.min_y.min(right.min_y),
        max_x: left.max_x.max(right.max_x),
        max_y: left.max_y.max(right.max_y),
    })
}

fn resample_place_white(
    manifest: &RunnerManifest,
    pose: usize,
    frame: &RgbaFrame,
    bounds: Bounds,
    transform: CycleTransform,
) -> Result<RgbaFrame, RunnerBuildError> {
    let destination_min_x = bounds.min_x as f32 * transform.scale + transform.translate_x;
    let destination_max_x = bounds.max_x as f32 * transform.scale + transform.translate_x;
    let destination_min_y = bounds.min_y as f32 * transform.scale + transform.translate_y;
    let destination_max_y = bounds.max_y as f32 * transform.scale + transform.translate_y;
    if destination_min_x < 0.0
        || destination_max_x > (PIXEL_WIDTH - 1) as f32
        || destination_min_y < 0.0
        || destination_max_y > (PIXEL_HEIGHT - 1) as f32
    {
        return Err(stage_error(
            manifest.character,
            "normalize_cycle[clip]",
            format!(
                "frame {pose}: transformed bounds x={destination_min_x:.2}..{destination_max_x:.2}, y={destination_min_y:.2}..{destination_max_y:.2} exceed canvas {}x{}",
                PIXEL_WIDTH, PIXEL_HEIGHT
            ),
        ));
    }

    let mut normalized = vec![0; frame.len()];
    for y in 0..PIXEL_HEIGHT {
        let source_y = (y as f32 - transform.translate_y) / transform.scale;
        for x in 0..PIXEL_WIDTH {
            let source_x = (x as f32 - transform.translate_x) / transform.scale;
            let alpha = bilinear_alpha(frame, source_x, source_y);
            if alpha == 0 {
                continue;
            }
            let index = ((y * PIXEL_WIDTH + x) * CHANNELS_PER_PIXEL as u32) as usize;
            normalized[index..index + 3].copy_from_slice(&[255, 255, 255]);
            normalized[index + 3] = alpha;
        }
    }

    let metrics = measure_frame(&normalized).map_err(|error| {
        stage_error(
            manifest.character,
            "normalize_cycle[measure_frame]",
            format!("frame {pose}: {}", error_detail(&error)),
        )
    })?;
    if metrics.clipped {
        return Err(stage_error(
            manifest.character,
            "normalize_cycle[clip]",
            format!("frame {pose}: visible alpha touches the canvas edge after resampling"),
        ));
    }

    Ok(normalized)
}

fn cycle_transform(
    manifest: &RunnerManifest,
    frame_bounds: &[Bounds],
    union: Bounds,
    scale: f32,
) -> Result<CycleTransform, RunnerBuildError> {
    let source_center_x = (union.min_x + union.max_x) as f32 * 0.5;
    let source_contact_baseline = manifest
        .contact_frames
        .iter()
        .map(|&index| frame_bounds[usize::from(index)].max_y)
        .max()
        .ok_or_else(|| {
            stage_error(
                manifest.character,
                "normalize_cycle[contact_frames]",
                "contact frame list was empty".to_string(),
            )
        })?;

    Ok(CycleTransform {
        scale,
        translate_x: (PIXEL_WIDTH as f32 - 1.0) * 0.5 - source_center_x * scale,
        translate_y: manifest.baseline_y as f32 - source_contact_baseline as f32 * scale,
    })
}

fn bilinear_alpha(frame: &RgbaFrame, source_x: f32, source_y: f32) -> u8 {
    if !source_x.is_finite()
        || !source_y.is_finite()
        || source_x < 0.0
        || source_y < 0.0
        || source_x > (PIXEL_WIDTH - 1) as f32
        || source_y > (PIXEL_HEIGHT - 1) as f32
    {
        return 0;
    }

    let x0 = source_x.floor() as u32;
    let y0 = source_y.floor() as u32;
    let x1 = (x0 + 1).min(PIXEL_WIDTH - 1);
    let y1 = (y0 + 1).min(PIXEL_HEIGHT - 1);
    let tx = source_x - x0 as f32;
    let ty = source_y - y0 as f32;

    let a00 = alpha_at(frame, x0, y0) as f32;
    let a10 = alpha_at(frame, x1, y0) as f32;
    let a01 = alpha_at(frame, x0, y1) as f32;
    let a11 = alpha_at(frame, x1, y1) as f32;

    let top = a00 * (1.0 - tx) + a10 * tx;
    let bottom = a01 * (1.0 - tx) + a11 * tx;
    (top * (1.0 - ty) + bottom * ty).round().clamp(0.0, 255.0) as u8
}

fn alpha_at(frame: &RgbaFrame, x: u32, y: u32) -> u8 {
    frame[((y * PIXEL_WIDTH + x) * CHANNELS_PER_PIXEL as u32 + 3) as usize]
}

fn validate_white_pixels(
    manifest: &RunnerManifest,
    index: usize,
    frame: &RgbaFrame,
) -> Result<(), RunnerBuildError> {
    for (pixel_index, pixel) in frame.chunks_exact(CHANNELS_PER_PIXEL).enumerate() {
        if pixel[3] == 0 {
            continue;
        }
        if pixel[..3] != [255, 255, 255] {
            let x = pixel_index as u32 % PIXEL_WIDTH;
            let y = pixel_index as u32 / PIXEL_WIDTH;
            return Err(stage_error(
                manifest.character,
                "validate_cycle[non_white]",
                format!(
                    "frame {index}: pixel at ({x}, {y}) has RGB [{}, {}, {}] with alpha {}",
                    pixel[0], pixel[1], pixel[2], pixel[3]
                ),
            ));
        }
    }
    Ok(())
}

fn stage_error(
    character: peterfan_core::config::RunnerCharacter,
    stage: &str,
    detail: String,
) -> RunnerBuildError {
    RunnerBuildError::InvalidFrame {
        character,
        detail: format!("{stage}: {detail}"),
    }
}

fn validate_manifest_pose_indices(
    manifest: &RunnerManifest,
    stage_prefix: &str,
) -> Result<(), RunnerBuildError> {
    validate_pose_index_array(
        manifest.character,
        stage_prefix,
        "contact_frames",
        &manifest.contact_frames,
    )?;
    validate_pose_index_array(
        manifest.character,
        stage_prefix,
        "flight_frames",
        &manifest.flight_frames,
    )?;
    Ok(())
}

fn validate_pose_index_array(
    character: peterfan_core::config::RunnerCharacter,
    stage_prefix: &str,
    label: &str,
    indices: &[u8],
) -> Result<(), RunnerBuildError> {
    for (slot, &index) in indices.iter().enumerate() {
        if usize::from(index) >= 8 {
            return Err(stage_error(
                character,
                &format!("{stage_prefix}[{label}]"),
                format!("{}[{}]={} exceeds last frame index 7", label, slot, index),
            ));
        }
    }
    Ok(())
}

fn error_detail(error: &RunnerBuildError) -> &str {
    match error {
        RunnerBuildError::Cache { detail, .. }
        | RunnerBuildError::Decode { detail, .. }
        | RunnerBuildError::InvalidFrame { detail, .. } => detail,
    }
}

fn manifest_character_placeholder() -> peterfan_core::config::RunnerCharacter {
    Default::default()
}

#[cfg(test)]
mod tests {
    use peterfan_core::config::RunnerCharacter;

    use super::{mirror_alpha, normalize_cycle, validate_cycle};
    use crate::runner::{
        measure_cycle, Bounds, Direction, RgbaFrame, RunnerManifest, PIXEL_HEIGHT, PIXEL_WIDTH,
    };

    fn alpha_at(frame: &[u8], x: u32, y: u32) -> u8 {
        frame[((y * PIXEL_WIDTH + x) * 4 + 3) as usize]
    }

    fn set_pixel(frame: &mut [u8], x: u32, y: u32, rgba: [u8; 4]) {
        let offset = ((y * PIXEL_WIDTH + x) * 4) as usize;
        frame[offset..offset + 4].copy_from_slice(&rgba);
    }

    fn set_alpha(frame: &mut [u8], x: u32, y: u32, alpha: u8) {
        set_pixel(frame, x, y, [255, 255, 255, alpha]);
    }

    fn synthetic_pose(left: u32, top: u32, width: u32, height: u32) -> RgbaFrame {
        let mut frame = vec![0; (PIXEL_WIDTH * PIXEL_HEIGHT * 4) as usize];
        for y in top..top + height {
            for x in left..left + width {
                set_alpha(&mut frame, x, y, 255);
            }
        }
        frame
    }

    fn asymmetric_arrow_frame() -> RgbaFrame {
        let mut frame = synthetic_pose(8, 12, 14, 5);
        for offset in 0..5 {
            set_alpha(&mut frame, 22 + offset, 10 + offset, 255);
        }
        frame
    }

    fn jogging_cycle(width: u32, height: u32, top: u32, shift: u32) -> [RgbaFrame; 8] {
        std::array::from_fn(|pose| synthetic_pose(8 + pose as u32 * shift, top, width, height))
    }

    fn contact_cycle(width: u32, height: u32, top: u32, contact_offset: u32) -> [RgbaFrame; 8] {
        std::array::from_fn(|pose| {
            let pose_top = if pose == 4 { top + contact_offset } else { top };
            synthetic_pose(8 + pose as u32 * 2, pose_top, width, height)
        })
    }

    fn drifting_bouncing_cycle() -> [RgbaFrame; 8] {
        let lefts = [8, 10, 12, 10, 8, 9, 11, 9];
        let tops = [12, 11, 8, 9, 12, 11, 8, 9];
        std::array::from_fn(|pose| synthetic_pose(lefts[pose], tops[pose], 12, 8))
    }

    fn union_from_cycle(cycle: &[RgbaFrame; 8]) -> Bounds {
        let metrics = measure_cycle(cycle).expect("cycle metrics");
        metrics
            .frames
            .iter()
            .map(|frame| frame.bounds)
            .reduce(|left, right| Bounds {
                min_x: left.min_x.min(right.min_x),
                min_y: left.min_y.min(right.min_y),
                max_x: left.max_x.max(right.max_x),
                max_y: left.max_y.max(right.max_y),
            })
            .expect("union bounds")
    }

    fn test_manifest(
        width: u32,
        height: u32,
        baseline_y: u32,
        mirror_source: bool,
    ) -> RunnerManifest {
        RunnerManifest {
            character: RunnerCharacter::Cat,
            output_direction: Direction::Right,
            mirror_source,
            target_width: width,
            target_height: height,
            baseline_y,
            max_centroid_drift: 2.5,
            contact_frames: [0, 4],
            flight_frames: [2, 6],
        }
    }

    #[test]
    fn normalization_places_white_frames_on_the_manifest_baseline() {
        let manifest = test_manifest(20, 12, 27, false);
        let source = std::array::from_fn(|pose| synthetic_pose(4 + pose as u32 % 2, 6, 10, 8));

        let normalized = normalize_cycle(&manifest, source).expect("normalized cycle");
        let metrics = measure_cycle(&normalized).expect("measured cycle");

        assert!(metrics
            .frames
            .iter()
            .all(|frame| frame.baseline_y <= manifest.baseline_y));
        assert!(metrics.max_centroid_drift <= manifest.max_centroid_drift);
        for pixel in normalized.iter().flat_map(|frame| frame.chunks_exact(4)) {
            if pixel[3] != 0 {
                assert_eq!(&pixel[..3], &[255, 255, 255]);
            }
        }
    }

    #[test]
    fn normalization_preserves_relative_centroid_and_baseline_motion_across_poses() {
        let manifest = test_manifest(24, 18, 27, false);
        let source = drifting_bouncing_cycle();
        let source_metrics = measure_cycle(&source).expect("source metrics");
        let source_union = union_from_cycle(&source);
        let scale = (manifest.target_width as f32 / source_union.width() as f32)
            .min(manifest.target_height as f32 / source_union.height() as f32);

        let normalized = normalize_cycle(&manifest, source).expect("normalized cycle");
        let normalized_metrics = measure_cycle(&normalized).expect("normalized metrics");

        for pose in 0..normalized_metrics.frames.len() {
            let expected_centroid_delta = (source_metrics.frames[pose].centroid_x
                - source_metrics.frames[0].centroid_x)
                * scale;
            let actual_centroid_delta = normalized_metrics.frames[pose].centroid_x
                - normalized_metrics.frames[0].centroid_x;
            assert!(
                (actual_centroid_delta - expected_centroid_delta).abs() <= 0.75,
                "pose {pose} centroid drift changed: expected {expected_centroid_delta:.2}, got {actual_centroid_delta:.2}"
            );

            let expected_baseline_delta = (source_metrics.frames[pose].baseline_y as f32
                - source_metrics.frames[0].baseline_y as f32)
                * scale;
            let actual_baseline_delta = normalized_metrics.frames[pose].baseline_y as f32
                - normalized_metrics.frames[0].baseline_y as f32;
            assert!(
                (actual_baseline_delta - expected_baseline_delta).abs() <= 1.0,
                "pose {pose} baseline bounce changed: expected {expected_baseline_delta:.2}, got {actual_baseline_delta:.2}"
            );
        }
    }

    #[test]
    fn normalize_cycle_rejects_geometry_that_would_clip_on_the_canvas() {
        let manifest = test_manifest(20, 12, 5, false);
        let source = std::array::from_fn(|_| synthetic_pose(8, 10, 10, 12));

        let error = normalize_cycle(&manifest, source).expect_err("clipping must fail");

        assert!(error.to_string().contains("normalize_cycle[clip]"));
    }

    #[test]
    fn mirror_source_reverses_horizontal_alpha_coverage() {
        let original = asymmetric_arrow_frame();
        let mirrored = mirror_alpha(&original).expect("mirrored frame");

        for y in 0..PIXEL_HEIGHT {
            for x in 0..PIXEL_WIDTH {
                let a = alpha_at(&original, x, y);
                let b = alpha_at(&mirrored, PIXEL_WIDTH - 1 - x, y);
                assert_eq!(a, b);
            }
        }
    }

    #[test]
    fn validation_rejects_a_duplicate_cycle() {
        let manifest = test_manifest(10, 8, 27, false);
        let frame = synthetic_pose(8, 8, 10, 8);
        let cycle = std::array::from_fn(|_| frame.clone());

        let error = validate_cycle(&manifest, &cycle).expect_err("duplicate cycle must fail");

        assert!(error.to_string().contains("validate_cycle[duplicate]"));
    }

    #[test]
    fn validation_rejects_non_white_visible_pixels() {
        let manifest = test_manifest(20, 12, 27, false);
        let mut frame = synthetic_pose(10, 10, 14, 10);
        set_pixel(&mut frame, 12, 12, [32, 64, 128, 255]);
        let cycle = std::array::from_fn(|_| frame.clone());

        let error = validate_cycle(&manifest, &cycle).expect_err("non-white pixels must fail");

        assert!(error.to_string().contains("validate_cycle[non_white]"));
    }

    #[test]
    fn validation_rejects_frames_with_invalid_buffer_lengths() {
        let manifest = test_manifest(20, 12, 27, false);
        let mut cycle = std::array::from_fn(|pose| synthetic_pose(8 + pose as u32 * 2, 10, 14, 10));
        cycle[3].truncate(cycle[3].len() - 4);

        let error = validate_cycle(&manifest, &cycle).expect_err("invalid buffer must fail");

        assert!(error.to_string().contains("validate_cycle[buffer_length]"));
    }

    #[test]
    fn validation_rejects_clipped_frames() {
        let manifest = test_manifest(20, 12, 27, false);
        let cycle = std::array::from_fn(|pose| synthetic_pose(pose as u32, 10, 14, 10));

        let error = validate_cycle(&manifest, &cycle).expect_err("clipped frame must fail");

        assert!(error.to_string().contains("validate_cycle[clipped]"));
    }

    #[test]
    fn validation_rejects_union_geometry_that_exceeds_manifest_targets() {
        let manifest = test_manifest(20, 12, 27, false);
        let cycle = jogging_cycle(24, 12, 10, 1);

        let error = validate_cycle(&manifest, &cycle).expect_err("oversize geometry must fail");

        assert!(error.to_string().contains("validate_cycle[union_max]"));
    }

    #[test]
    fn validation_rejects_union_geometry_that_is_too_small_in_both_dimensions() {
        let manifest = test_manifest(20, 12, 27, false);
        let frame = synthetic_pose(12, 14, 11, 7);
        let cycle = std::array::from_fn(|_| frame.clone());

        let error = validate_cycle(&manifest, &cycle).expect_err("undersize geometry must fail");

        assert!(error.to_string().contains("validate_cycle[union_min]"));
    }

    #[test]
    fn validation_rejects_union_geometry_that_misses_both_target_dimensions() {
        let manifest = test_manifest(20, 16, 27, false);
        let frame = synthetic_pose(12, 12, 14, 10);
        let cycle = std::array::from_fn(|_| frame.clone());

        let error = validate_cycle(&manifest, &cycle).expect_err("off-target geometry must fail");

        assert!(error.to_string().contains("validate_cycle[union_fit]"));
    }

    #[test]
    fn validation_rejects_excessive_centroid_drift() {
        let manifest = test_manifest(35, 10, 27, false);
        let cycle = jogging_cycle(14, 10, 10, 3);

        let error = validate_cycle(&manifest, &cycle).expect_err("drift must fail");

        assert!(error.to_string().contains("validate_cycle[centroid_drift]"));
    }

    #[test]
    fn validation_rejects_contact_frames_with_mismatched_baselines() {
        let mut manifest = test_manifest(28, 12, 27, false);
        manifest.max_centroid_drift = 20.0;
        let cycle = contact_cycle(14, 10, 10, 2);

        let error = validate_cycle(&manifest, &cycle).expect_err("contact mismatch must fail");

        assert!(error
            .to_string()
            .contains("validate_cycle[contact_baseline]"));
    }

    #[test]
    fn validation_rejects_out_of_range_contact_frame_indices() {
        let valid_manifest = test_manifest(20, 12, 27, false);
        let source = std::array::from_fn(|pose| synthetic_pose(4 + pose as u32 % 2, 6, 10, 8));
        let cycle = normalize_cycle(&valid_manifest, source).expect("normalized cycle");
        let mut manifest = valid_manifest;
        manifest.contact_frames = [0, 8];

        let error =
            validate_cycle(&manifest, &cycle).expect_err("out-of-range contact frame must fail");

        assert!(error.to_string().contains("validate_cycle[contact_frames]"));
    }

    #[test]
    fn validation_rejects_out_of_range_flight_frame_indices() {
        let valid_manifest = test_manifest(20, 12, 27, false);
        let source = std::array::from_fn(|pose| synthetic_pose(4 + pose as u32 % 2, 6, 10, 8));
        let cycle = normalize_cycle(&valid_manifest, source).expect("normalized cycle");
        let mut manifest = valid_manifest;
        manifest.flight_frames = [2, 8];

        let error =
            validate_cycle(&manifest, &cycle).expect_err("out-of-range flight frame must fail");

        assert!(error.to_string().contains("validate_cycle[flight_frames]"));
    }

    #[test]
    fn validation_rejects_adjacent_frames_with_too_little_delta() {
        let mut manifest = test_manifest(21, 10, 27, false);
        manifest.max_centroid_drift = 20.0;
        let cycle = jogging_cycle(14, 10, 10, 1);

        let error = validate_cycle(&manifest, &cycle).expect_err("low-delta cycle must fail");

        assert!(error
            .to_string()
            .contains("validate_cycle[minimum_frame_delta]"));
    }
}
