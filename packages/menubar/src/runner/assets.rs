use std::io::Cursor;
use std::sync::OnceLock;

use peterfan_core::config::RunnerCharacter;

use super::{RgbaFrame, RunnerBuildError, FRAME_COUNT, PIXEL_HEIGHT, PIXEL_WIDTH};

const HUMAN_RUN_FRAME_WIDTH: u32 = 32;
const HUMAN_RUN_FRAME_HEIGHT: u32 = 32;
const HUMAN_RUN_DRAW_WIDTH: u32 = 30;
const HUMAN_RUN_SHEET: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/runner/caveman.png"
));
const CAT_RUN_FRAME_WIDTH: u32 = 66;
const CAT_RUN_FRAME_HEIGHT: u32 = 66;
const CAT_RUN_FRAME_COUNT: u32 = 5;
const CAT_RUN_SHEET: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/runner/cat-run-cc0.png"
));
const FOX_RUN_FRAME_WIDTH: u32 = 64;
const FOX_RUN_FRAME_HEIGHT: u32 = 36;
const FOX_RUN_FRAME_COUNT: u32 = 6;
const FOX_RUN_SHEET: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/runner/fox-run-cc0.png"
));
const DOG_RUN_FRAME_WIDTH: u32 = 60;
const DOG_RUN_FRAME_HEIGHT: u32 = 38;
const DOG_RUN_FRAME_COUNT: u32 = 6;
const DOG_RUN_SHEET_ROW: u32 = 2;
const DOG_RUN_SHEET: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/runner/dog-run-cc0.png"
));
const RABBIT_RUN_FRAME_WIDTH: u32 = 32;
const RABBIT_RUN_FRAME_HEIGHT: u32 = 26;
const RABBIT_RUN_FRAME_COUNT: u32 = 6;
const RABBIT_RUN_SHEET: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/runner/rabbit-run-cc0.png"
));
const GHOST_RUN_FRAME_WIDTH: u32 = 32;
const GHOST_RUN_FRAME_HEIGHT: u32 = 32;
const GHOST_RUN_FRAME_COUNT: u32 = 6;
const GHOST_RUN_SHEET: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/runner/ghost-run-cc0.png"
));
const DINO_RUN_FRAMES: &[&[u8]] = &[
    include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../assets/runner/dino-run-1-cc0.png"
    )),
    include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../assets/runner/dino-run-2-cc0.png"
    )),
    include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../assets/runner/dino-run-3-cc0.png"
    )),
    include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../assets/runner/dino-run-4-cc0.png"
    )),
    include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../assets/runner/dino-run-5-cc0.png"
    )),
    include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../assets/runner/dino-run-6-cc0.png"
    )),
    include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../assets/runner/dino-run-7-cc0.png"
    )),
    include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../assets/runner/dino-run-8-cc0.png"
    )),
];

pub(crate) fn render(
    character: RunnerCharacter,
    cpu_pct: f32,
    frame: u8,
) -> Result<RgbaFrame, RunnerBuildError> {
    match character {
        RunnerCharacter::Cat => make_cat_asset_runner_rgba(cpu_pct, frame),
        RunnerCharacter::Dog => make_dog_asset_runner_rgba(cpu_pct, frame),
        RunnerCharacter::Rabbit => make_rabbit_asset_runner_rgba(cpu_pct, frame),
        RunnerCharacter::Fox => make_fox_asset_runner_rgba(cpu_pct, frame),
        RunnerCharacter::Dinosaur => make_dinosaur_asset_runner_rgba(cpu_pct, frame),
        RunnerCharacter::Ghost => make_ghost_asset_runner_rgba(cpu_pct, frame),
        RunnerCharacter::Human => make_human_runner_rgba(frame),
        _ => unreachable!("non-asset runner reached asset renderer"),
    }
}

fn make_cat_asset_runner_rgba(cpu_pct: f32, frame: u8) -> Result<RgbaFrame, RunnerBuildError> {
    static FRAMES: OnceLock<Result<Vec<RgbaFrame>, RunnerBuildError>> = OnceLock::new();
    let frames = FRAMES
        .get_or_init(decode_cat_asset_frames)
        .as_ref()
        .map_err(Clone::clone)?;
    apply_asset_runner_motion(frames, RunnerCharacter::Cat, cpu_pct, frame)
}

fn decode_cat_asset_frames() -> Result<Vec<RgbaFrame>, RunnerBuildError> {
    let character = RunnerCharacter::Cat;
    let decoder = png::Decoder::new(Cursor::new(CAT_RUN_SHEET));
    let mut reader = decoder
        .read_info()
        .map_err(|error| decode_error(character, format!("metadata: {error}")))?;
    let mut source = vec![
        0;
        reader.output_buffer_size().ok_or_else(|| {
            decode_error(
                character,
                "buffer size missing from PNG metadata".to_string(),
            )
        })?
    ];
    let info = reader
        .next_frame(&mut source)
        .map_err(|error| decode_error(character, format!("frame decode: {error}")))?;
    if info.width != CAT_RUN_FRAME_WIDTH * CAT_RUN_FRAME_COUNT {
        return Err(decode_error(
            character,
            format!(
                "sheet width mismatch: expected {}, got {}",
                CAT_RUN_FRAME_WIDTH * CAT_RUN_FRAME_COUNT,
                info.width
            ),
        ));
    }
    if info.height != CAT_RUN_FRAME_HEIGHT {
        return Err(decode_error(
            character,
            format!(
                "sheet height mismatch: expected {}, got {}",
                CAT_RUN_FRAME_HEIGHT, info.height
            ),
        ));
    }
    if info.color_type != png::ColorType::Rgba {
        return Err(decode_error(
            character,
            format!("expected RGBA sheet, got {:?}", info.color_type),
        ));
    }

    let source = &source[..info.buffer_size()];
    let mut frames = (0..FRAME_COUNT)
        .map(|frame| {
            let mut rgba = vec![0u8; (PIXEL_WIDTH * PIXEL_HEIGHT * 4) as usize];
            let frame_position =
                f32::from(frame) * CAT_RUN_FRAME_COUNT as f32 / f32::from(FRAME_COUNT);
            let frame_a = frame_position.floor() as u32 % CAT_RUN_FRAME_COUNT;
            let frame_b = (frame_a + 1) % CAT_RUN_FRAME_COUNT;
            let frame_mix = frame_position.fract();
            let draw_width = PIXEL_WIDTH;
            let draw_height = PIXEL_HEIGHT;

            let sample_alpha = |source_frame: u32, source_x: u32, source_y: u32| {
                let frame_x = source_frame * CAT_RUN_FRAME_WIDTH;
                let source_index = ((source_y * info.width + frame_x + source_x) as usize) * 4;
                let red = source[source_index];
                let green = source[source_index + 1];
                let blue = source[source_index + 2];
                let source_alpha = source[source_index + 3];
                let is_blue_background =
                    blue > red.saturating_add(35) && blue > green.saturating_add(10) && blue > 100;
                if is_blue_background {
                    0
                } else {
                    source_alpha
                }
            };

            for draw_y in 0..draw_height {
                for draw_x in 0..draw_width {
                    let source_x = 1 + draw_x * (CAT_RUN_FRAME_WIDTH - 2) / draw_width;
                    let source_y = 4 + draw_y * (CAT_RUN_FRAME_HEIGHT - 8) / draw_height;
                    let alpha_a = sample_alpha(frame_a, source_x, source_y);
                    let alpha_b = sample_alpha(frame_b, source_x, source_y);
                    let target_index = ((draw_y * PIXEL_WIDTH + draw_x) as usize) * 4;
                    rgba[target_index + 3] = (f32::from(alpha_a) * (1.0 - frame_mix)
                        + f32::from(alpha_b) * frame_mix)
                        .round() as u8;
                }
            }

            rgba
        })
        .collect::<Vec<_>>();
    normalize_runner_asset_frames(&mut frames);
    Ok(frames)
}

struct RunnerAssetLayout {
    frame_width: u32,
    frame_height: u32,
    frame_count: u32,
    sheet_row: u32,
    crop_left: u32,
    crop_top: u32,
    crop_width: u32,
    crop_height: u32,
}

fn decode_runner_asset_frames(
    character: RunnerCharacter,
    sheet: &[u8],
    layout: RunnerAssetLayout,
) -> Result<Vec<RgbaFrame>, RunnerBuildError> {
    let RunnerAssetLayout {
        frame_width,
        frame_height,
        frame_count,
        sheet_row,
        crop_left,
        crop_top,
        crop_width,
        crop_height,
    } = layout;
    let mut decoder = png::Decoder::new(Cursor::new(sheet));
    decoder.set_transformations(png::Transformations::EXPAND);
    let mut reader = decoder
        .read_info()
        .map_err(|error| decode_error(character, format!("metadata: {error}")))?;
    let mut source = vec![
        0;
        reader.output_buffer_size().ok_or_else(|| {
            decode_error(
                character,
                "buffer size missing from PNG metadata".to_string(),
            )
        })?
    ];
    let info = reader
        .next_frame(&mut source)
        .map_err(|error| decode_error(character, format!("frame decode: {error}")))?;
    if info.width != frame_width * frame_count {
        return Err(decode_error(
            character,
            format!(
                "sheet width mismatch: expected {}, got {}",
                frame_width * frame_count,
                info.width
            ),
        ));
    }
    if info.height < frame_height * (sheet_row + 1) {
        return Err(decode_error(
            character,
            format!(
                "sheet height mismatch: expected at least {}, got {}",
                frame_height * (sheet_row + 1),
                info.height
            ),
        ));
    }

    let channels = match info.color_type {
        png::ColorType::Rgb => 3,
        png::ColorType::Rgba => 4,
        png::ColorType::Grayscale => 1,
        png::ColorType::GrayscaleAlpha => 2,
        color_type => {
            return Err(decode_error(
                character,
                format!("unsupported color type: {color_type:?}"),
            ));
        }
    };
    let has_alpha = matches!(
        info.color_type,
        png::ColorType::Rgba | png::ColorType::GrayscaleAlpha
    );
    let source = &source[..info.buffer_size()];

    let mut frames = (0..FRAME_COUNT)
        .map(|frame| {
            let mut rgba = vec![0u8; (PIXEL_WIDTH * PIXEL_HEIGHT * 4) as usize];
            let frame_position = f32::from(frame) * frame_count as f32 / f32::from(FRAME_COUNT);
            let frame_a = frame_position.floor() as u32 % frame_count;
            let frame_b = (frame_a + 1) % frame_count;
            let frame_mix = frame_position.fract();

            let sample_alpha = |source_frame: u32, source_x: u32, source_y: u32| {
                let source_index = (((sheet_row * frame_height + source_y) * info.width
                    + source_frame * frame_width
                    + source_x) as usize)
                    * channels;
                let alpha = if has_alpha {
                    if channels == 4 {
                        source[source_index + 3]
                    } else {
                        source[source_index + 1]
                    }
                } else {
                    255
                };
                let is_matte = match info.color_type {
                    png::ColorType::Rgb => {
                        let red = source[source_index];
                        let green = source[source_index + 1];
                        let blue = source[source_index + 2];
                        red < 35 && green < 35 && blue < 35
                    }
                    png::ColorType::Grayscale => source[source_index] < 35,
                    png::ColorType::Rgba | png::ColorType::GrayscaleAlpha => false,
                    _ => false,
                };
                if is_matte {
                    0
                } else {
                    alpha
                }
            };

            for draw_y in 0..PIXEL_HEIGHT {
                for draw_x in 0..PIXEL_WIDTH {
                    let source_x = crop_left + draw_x.min(47) * crop_width.saturating_sub(1) / 47;
                    let source_y = crop_top + draw_y.min(31) * crop_height.saturating_sub(1) / 31;
                    let alpha_a = sample_alpha(frame_a, source_x, source_y);
                    let alpha_b = sample_alpha(frame_b, source_x, source_y);
                    let target_index = ((draw_y * PIXEL_WIDTH + draw_x) as usize) * 4;
                    rgba[target_index + 3] = (f32::from(alpha_a) * (1.0 - frame_mix)
                        + f32::from(alpha_b) * frame_mix)
                        .round() as u8;
                }
            }

            rgba
        })
        .collect::<Vec<_>>();
    normalize_runner_asset_frames(&mut frames);
    Ok(frames)
}

fn apply_asset_runner_motion(
    frames: &[RgbaFrame],
    character: RunnerCharacter,
    cpu_pct: f32,
    frame: u8,
) -> Result<RgbaFrame, RunnerBuildError> {
    const BOUNCE: [f32; 8] = [1.0, 0.2, -1.0, -2.2, -1.3, 0.2, 0.9, -0.8];
    let load = cpu_pct.clamp(0.0, 100.0) / 100.0;
    let motion = 0.55 + load.sqrt() * 0.70;
    let vertical_shift = (0.5 + BOUNCE[usize::from(frame % FRAME_COUNT)] * motion).round() as i32;
    let base = frame_at(frames, character, frame)?;
    let mut rgba = vec![0u8; base.len()];

    for y in 0..PIXEL_HEIGHT as i32 {
        let target_y = y + vertical_shift;
        if !(0..PIXEL_HEIGHT as i32).contains(&target_y) {
            continue;
        }
        for x in 0..PIXEL_WIDTH as i32 {
            let source_index = ((y as u32 * PIXEL_WIDTH + x as u32) as usize) * 4;
            let target_index = ((target_y as u32 * PIXEL_WIDTH + x as u32) as usize) * 4;
            rgba[target_index..target_index + 4]
                .copy_from_slice(&base[source_index..source_index + 4]);
        }
    }
    Ok(rgba)
}

fn make_fox_asset_runner_rgba(cpu_pct: f32, frame: u8) -> Result<RgbaFrame, RunnerBuildError> {
    static FRAMES: OnceLock<Result<Vec<RgbaFrame>, RunnerBuildError>> = OnceLock::new();
    let frames = FRAMES
        .get_or_init(|| {
            decode_runner_asset_frames(
                RunnerCharacter::Fox,
                FOX_RUN_SHEET,
                RunnerAssetLayout {
                    frame_width: FOX_RUN_FRAME_WIDTH,
                    frame_height: FOX_RUN_FRAME_HEIGHT,
                    frame_count: FOX_RUN_FRAME_COUNT,
                    sheet_row: 0,
                    crop_left: 1,
                    crop_top: 0,
                    crop_width: FOX_RUN_FRAME_WIDTH - 2,
                    crop_height: FOX_RUN_FRAME_HEIGHT,
                },
            )
        })
        .as_ref()
        .map_err(Clone::clone)?;
    apply_asset_runner_motion(frames, RunnerCharacter::Fox, cpu_pct, frame)
}

fn make_rabbit_asset_runner_rgba(cpu_pct: f32, frame: u8) -> Result<RgbaFrame, RunnerBuildError> {
    static FRAMES: OnceLock<Result<Vec<RgbaFrame>, RunnerBuildError>> = OnceLock::new();
    let frames = FRAMES
        .get_or_init(|| {
            decode_runner_asset_frames(
                RunnerCharacter::Rabbit,
                RABBIT_RUN_SHEET,
                RunnerAssetLayout {
                    frame_width: RABBIT_RUN_FRAME_WIDTH,
                    frame_height: RABBIT_RUN_FRAME_HEIGHT,
                    frame_count: RABBIT_RUN_FRAME_COUNT,
                    sheet_row: 0,
                    crop_left: 2,
                    crop_top: 1,
                    crop_width: RABBIT_RUN_FRAME_WIDTH - 4,
                    crop_height: RABBIT_RUN_FRAME_HEIGHT - 2,
                },
            )
        })
        .as_ref()
        .map_err(Clone::clone)?;
    apply_asset_runner_motion(frames, RunnerCharacter::Rabbit, cpu_pct, frame)
}

fn make_dog_asset_runner_rgba(cpu_pct: f32, frame: u8) -> Result<RgbaFrame, RunnerBuildError> {
    static FRAMES: OnceLock<Result<Vec<RgbaFrame>, RunnerBuildError>> = OnceLock::new();
    let frames = FRAMES
        .get_or_init(|| {
            decode_runner_asset_frames(
                RunnerCharacter::Dog,
                DOG_RUN_SHEET,
                RunnerAssetLayout {
                    frame_width: DOG_RUN_FRAME_WIDTH,
                    frame_height: DOG_RUN_FRAME_HEIGHT,
                    frame_count: DOG_RUN_FRAME_COUNT,
                    sheet_row: DOG_RUN_SHEET_ROW,
                    crop_left: 0,
                    crop_top: 0,
                    crop_width: DOG_RUN_FRAME_WIDTH,
                    crop_height: DOG_RUN_FRAME_HEIGHT,
                },
            )
        })
        .as_ref()
        .map_err(Clone::clone)?;
    apply_asset_runner_motion(frames, RunnerCharacter::Dog, cpu_pct, frame)
}

fn make_ghost_asset_runner_rgba(cpu_pct: f32, frame: u8) -> Result<RgbaFrame, RunnerBuildError> {
    static FRAMES: OnceLock<Result<Vec<RgbaFrame>, RunnerBuildError>> = OnceLock::new();
    let frames = FRAMES
        .get_or_init(|| {
            decode_runner_asset_frames(
                RunnerCharacter::Ghost,
                GHOST_RUN_SHEET,
                RunnerAssetLayout {
                    frame_width: GHOST_RUN_FRAME_WIDTH,
                    frame_height: GHOST_RUN_FRAME_HEIGHT,
                    frame_count: GHOST_RUN_FRAME_COUNT,
                    sheet_row: 0,
                    crop_left: 1,
                    crop_top: 0,
                    crop_width: GHOST_RUN_FRAME_WIDTH - 2,
                    crop_height: GHOST_RUN_FRAME_HEIGHT,
                },
            )
        })
        .as_ref()
        .map_err(Clone::clone)?;
    apply_asset_runner_motion(frames, RunnerCharacter::Ghost, cpu_pct, frame)
}

fn decode_runner_asset_sequence_frames(
    character: RunnerCharacter,
    sheets: &[&[u8]],
    source_width: u32,
    source_height: u32,
    crop_left: u32,
    crop_top: u32,
    crop_width: u32,
    crop_height: u32,
) -> Result<Vec<RgbaFrame>, RunnerBuildError> {
    let mut frames = sheets
        .iter()
        .map(|sheet| {
            let decoder = png::Decoder::new(Cursor::new(*sheet));
            let mut reader = decoder
                .read_info()
                .map_err(|error| decode_error(character, format!("metadata: {error}")))?;
            let mut source = vec![
                0;
                reader.output_buffer_size().ok_or_else(|| {
                    decode_error(
                        character,
                        "buffer size missing from PNG metadata".to_string(),
                    )
                })?
            ];
            let info = reader
                .next_frame(&mut source)
                .map_err(|error| decode_error(character, format!("frame decode: {error}")))?;
            if info.width != source_width {
                return Err(decode_error(
                    character,
                    format!(
                        "frame width mismatch: expected {source_width}, got {}",
                        info.width
                    ),
                ));
            }
            if info.height != source_height {
                return Err(decode_error(
                    character,
                    format!(
                        "frame height mismatch: expected {source_height}, got {}",
                        info.height
                    ),
                ));
            }
            if info.color_type != png::ColorType::Rgba {
                return Err(decode_error(
                    character,
                    format!("expected RGBA sequence frame, got {:?}", info.color_type),
                ));
            }
            let source = &source[..info.buffer_size()];
            let mut rgba = vec![0u8; (PIXEL_WIDTH * PIXEL_HEIGHT * 4) as usize];

            for draw_y in 0..PIXEL_HEIGHT {
                for draw_x in 0..PIXEL_WIDTH {
                    let source_x = crop_left + draw_x * crop_width.saturating_sub(1) / 47;
                    let source_y = crop_top + draw_y * crop_height.saturating_sub(1) / 31;
                    let source_index = ((source_y * source_width + source_x) as usize) * 4;
                    let target_index = ((draw_y * PIXEL_WIDTH + draw_x) as usize) * 4;
                    rgba[target_index + 3] = source[source_index + 3];
                }
            }
            Ok(rgba)
        })
        .collect::<Result<Vec<_>, _>>()?;
    normalize_runner_asset_frames(&mut frames);
    Ok(frames)
}

fn normalize_runner_asset_frames(frames: &mut [RgbaFrame]) {
    #[derive(Clone, Copy)]
    struct Bounds {
        min_x: u32,
        max_x: u32,
        min_y: u32,
        max_y: u32,
        weighted_x: f32,
        alpha_sum: f32,
    }

    let bounds_for = |rgba: &[u8]| -> Option<Bounds> {
        let mut bounds = Bounds {
            min_x: PIXEL_WIDTH,
            max_x: 0,
            min_y: PIXEL_HEIGHT,
            max_y: 0,
            weighted_x: 0.0,
            alpha_sum: 0.0,
        };
        let mut found = false;
        for (index, pixel) in rgba.chunks_exact(4).enumerate() {
            let alpha = f32::from(pixel[3]);
            if alpha < 8.0 {
                continue;
            }
            let x = (index as u32) % PIXEL_WIDTH;
            let y = (index as u32) / PIXEL_WIDTH;
            bounds.min_x = bounds.min_x.min(x);
            bounds.max_x = bounds.max_x.max(x);
            bounds.min_y = bounds.min_y.min(y);
            bounds.max_y = bounds.max_y.max(y);
            bounds.weighted_x += x as f32 * alpha;
            bounds.alpha_sum += alpha;
            found = true;
        }
        found.then_some(bounds)
    };

    let all_bounds = frames
        .iter()
        .filter_map(|frame| bounds_for(frame))
        .collect::<Vec<_>>();
    if all_bounds.is_empty() {
        return;
    }

    let target_center = all_bounds
        .iter()
        .map(|frame| frame.weighted_x / frame.alpha_sum.max(1.0))
        .sum::<f32>()
        / all_bounds.len() as f32;
    let target_bottom = all_bounds
        .iter()
        .map(|frame| frame.max_y)
        .max()
        .unwrap_or(0);

    for frame in frames.iter_mut() {
        let Some(frame_bounds) = bounds_for(frame) else {
            continue;
        };
        let center = frame_bounds.weighted_x / frame_bounds.alpha_sum.max(1.0);
        let shift_x = (target_center - center).round() as i32;
        let shift_y = (i32::try_from(target_bottom).unwrap_or(0)
            - i32::try_from(frame_bounds.max_y).unwrap_or(0))
        .clamp(-3, 3);
        if shift_x == 0 && shift_y == 0 {
            continue;
        }

        let source = frame.clone();
        frame.fill(0);
        for y in 0..PIXEL_HEIGHT as i32 {
            for x in 0..PIXEL_WIDTH as i32 {
                let target_x = x + shift_x;
                let target_y = y + shift_y;
                if !(0..PIXEL_WIDTH as i32).contains(&target_x)
                    || !(0..PIXEL_HEIGHT as i32).contains(&target_y)
                {
                    continue;
                }
                let source_index = ((y as u32 * PIXEL_WIDTH + x as u32) as usize) * 4;
                let target_index = ((target_y as u32 * PIXEL_WIDTH + target_x as u32) as usize) * 4;
                frame[target_index..target_index + 4]
                    .copy_from_slice(&source[source_index..source_index + 4]);
            }
        }
    }
}

fn make_dinosaur_asset_runner_rgba(cpu_pct: f32, frame: u8) -> Result<RgbaFrame, RunnerBuildError> {
    static FRAMES: OnceLock<Result<Vec<RgbaFrame>, RunnerBuildError>> = OnceLock::new();
    let frames = FRAMES
        .get_or_init(|| {
            decode_runner_asset_sequence_frames(
                RunnerCharacter::Dinosaur,
                DINO_RUN_FRAMES,
                680,
                472,
                20,
                45,
                640,
                400,
            )
        })
        .as_ref()
        .map_err(Clone::clone)?;
    apply_asset_runner_motion(frames, RunnerCharacter::Dinosaur, cpu_pct, frame)
}

fn make_human_runner_rgba(frame: u8) -> Result<RgbaFrame, RunnerBuildError> {
    static FRAMES: OnceLock<Result<Vec<RgbaFrame>, RunnerBuildError>> = OnceLock::new();
    let frames = FRAMES
        .get_or_init(decode_human_runner_frames)
        .as_ref()
        .map_err(Clone::clone)?;
    frame_at(frames, RunnerCharacter::Human, frame)
}

fn decode_human_runner_frames() -> Result<Vec<RgbaFrame>, RunnerBuildError> {
    let character = RunnerCharacter::Human;
    let decoder = png::Decoder::new(Cursor::new(HUMAN_RUN_SHEET));
    let mut reader = decoder
        .read_info()
        .map_err(|error| decode_error(character, format!("metadata: {error}")))?;
    let mut source = vec![
        0;
        reader.output_buffer_size().ok_or_else(|| {
            decode_error(
                character,
                "buffer size missing from PNG metadata".to_string(),
            )
        })?
    ];
    let info = reader
        .next_frame(&mut source)
        .map_err(|error| decode_error(character, format!("frame decode: {error}")))?;
    if info.width != 128 {
        return Err(decode_error(
            character,
            format!("sheet width changed: expected 128, got {}", info.width),
        ));
    }
    if info.height != 128 {
        return Err(decode_error(
            character,
            format!("sheet height changed: expected 128, got {}", info.height),
        ));
    }
    if info.color_type != png::ColorType::Rgba {
        return Err(decode_error(
            character,
            format!("expected RGBA sheet, got {:?}", info.color_type),
        ));
    }

    let source = &source[..info.buffer_size()];
    Ok((0..FRAME_COUNT)
        .map(|frame| {
            let mut rgba = vec![0u8; (PIXEL_WIDTH * PIXEL_HEIGHT * 4) as usize];
            let frame_x = u32::from(frame % 4) * HUMAN_RUN_FRAME_WIDTH;
            let frame_y = u32::from(frame / 4) * HUMAN_RUN_FRAME_HEIGHT;
            let draw_left = (PIXEL_WIDTH - HUMAN_RUN_DRAW_WIDTH) / 2;

            for y in 0..PIXEL_HEIGHT {
                for x in 0..PIXEL_WIDTH {
                    if x < draw_left || x >= draw_left + HUMAN_RUN_DRAW_WIDTH {
                        continue;
                    }
                    let draw_x = x - draw_left;
                    let source_x = draw_x * HUMAN_RUN_FRAME_WIDTH / HUMAN_RUN_DRAW_WIDTH;
                    let source_index =
                        ((frame_y + y) * info.width + frame_x + source_x) as usize * 4;
                    let target_index = ((y * PIXEL_WIDTH + x) * 4) as usize;
                    rgba[target_index] = source[source_index];
                    rgba[target_index + 1] = source[source_index + 1];
                    rgba[target_index + 2] = source[source_index + 2];
                    let source_alpha = source[source_index + 3];
                    let is_dark_outline = source[source_index] < 48
                        && source[source_index + 1] < 48
                        && source[source_index + 2] < 48;
                    rgba[target_index + 3] = if is_dark_outline {
                        source_alpha / 3
                    } else {
                        source_alpha
                    };
                }
            }

            let (weighted_x, alpha_sum) = rgba.chunks_exact(4).enumerate().fold(
                (0.0_f32, 0.0_f32),
                |(weighted_x, alpha_sum), (index, pixel)| {
                    let alpha = f32::from(pixel[3]);
                    (
                        weighted_x + (index as u32 % PIXEL_WIDTH) as f32 * alpha,
                        alpha_sum + alpha,
                    )
                },
            );
            let center = weighted_x / alpha_sum.max(1.0);
            let shift = (((PIXEL_WIDTH - 1) as f32 / 2.0) - center).round() as i32;
            if shift != 0 {
                let original = rgba.clone();
                rgba.fill(0);
                for y in 0..PIXEL_HEIGHT {
                    for x in 0..PIXEL_WIDTH {
                        let target_x = x as i32 + shift;
                        if !(0..PIXEL_WIDTH as i32).contains(&target_x) {
                            continue;
                        }
                        let source_index = ((y * PIXEL_WIDTH + x) * 4) as usize;
                        let target_index = ((y * PIXEL_WIDTH + target_x as u32) * 4) as usize;
                        rgba[target_index..target_index + 4]
                            .copy_from_slice(&original[source_index..source_index + 4]);
                    }
                }
            }

            rgba
        })
        .collect())
}

fn frame_at(
    frames: &[RgbaFrame],
    character: RunnerCharacter,
    frame: u8,
) -> Result<RgbaFrame, RunnerBuildError> {
    let index = usize::from(frame % FRAME_COUNT);
    frames
        .get(index)
        .cloned()
        .ok_or_else(|| RunnerBuildError::InvalidFrame {
            character,
            detail: format!(
                "requested pose {index}, but only {} cached frame(s) exist",
                frames.len()
            ),
        })
}

fn decode_error(character: RunnerCharacter, detail: String) -> RunnerBuildError {
    RunnerBuildError::Decode { character, detail }
}
