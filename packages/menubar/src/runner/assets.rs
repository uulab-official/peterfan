use std::io::Cursor;
use std::sync::OnceLock;

use peterfan_core::config::RunnerCharacter;

use super::{
    manifest, normalize_cycle, validate_cycle,
    timing::load_band,
    RgbaFrame, RunnerBuildError, FRAME_COUNT, LOAD_BANDS, PIXEL_HEIGHT, PIXEL_WIDTH,
};

const HUMAN_RUN_FRAME_WIDTH: u32 = 32;
const HUMAN_RUN_FRAME_HEIGHT: u32 = 32;
const HUMAN_RUN_DRAW_WIDTH: u32 = 30;
const HUMAN_RUN_DRAW_HEIGHT: u32 = 28;
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
const DOG_RUN_SHEET_ROW: u32 = 1;
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
const DINO_RUN_FRAME_WIDTH: u32 = 680;
const DINO_RUN_FRAME_HEIGHT: u32 = 472;
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
const LOAD_PRESET_CPUS: [f32; 4] = [0.0, 30.0, 65.0, 90.0];
const CHANNELS_PER_PIXEL: usize = 4;
const MATTE_DARK_THRESHOLD: u8 = 35;
const CAT_BLUE_MATTE_MIN: u8 = 100;
const VISIBLE_SOURCE_ALPHA_THRESHOLD: u8 = 1;
const ALPHA_DELTA_THRESHOLD: u8 = 48;
const NEAREST_POSE_VISIBLE_DELTA_RATIO: f32 = 0.35;

#[derive(Clone, Copy)]
struct Crop {
    left: u32,
    top: u32,
    width: u32,
    height: u32,
}

#[derive(Clone, Copy)]
struct SheetLayout {
    frame_width: u32,
    frame_height: u32,
    sheet_columns: u32,
    source_pose_count: u32,
    sheet_row: u32,
    phase_offset: f32,
    draw_height: u32,
    crop: Crop,
    matte: MatteKey,
}

#[derive(Clone, Copy)]
struct SequenceLayout {
    source_width: u32,
    source_height: u32,
    draw_width: u32,
    draw_height: u32,
    crop: Crop,
}

#[derive(Clone, Copy, Debug)]
enum MatteKey {
    None,
    BlueBackground,
    DarkBackground,
}

struct DecodedPng {
    width: u32,
    height: u32,
    color_type: png::ColorType,
    channels: usize,
    pixels: Vec<u8>,
}

pub(crate) fn render(
    character: RunnerCharacter,
    cpu_pct: f32,
    frame: u8,
) -> Result<RgbaFrame, RunnerBuildError> {
    let cycle = build_cycle(character, cpu_pct)?;
    frame_at(&cycle, character, frame)
}

pub(crate) fn build_cycle(
    character: RunnerCharacter,
    cpu_pct: f32,
) -> Result<[RgbaFrame; 8], RunnerBuildError> {
    cached_final_cycle(character, load_band(cpu_pct)).map(clone_cycle)
}

pub(crate) fn source_cycle(character: RunnerCharacter) -> Result<[RgbaFrame; 8], RunnerBuildError> {
    cached_source_cycle(character).map(clone_cycle)
}

fn cached_final_cycle(
    character: RunnerCharacter,
    band_index: usize,
) -> Result<&'static [RgbaFrame; 8], RunnerBuildError> {
    let band_index = band_index.min(LOAD_BANDS.saturating_sub(1));
    match character {
        RunnerCharacter::Cat => {
            static CYCLES: OnceLock<[Result<[RgbaFrame; 8], RunnerBuildError>; LOAD_BANDS]> =
                OnceLock::new();
            CYCLES.get_or_init(|| build_final_cycles(RunnerCharacter::Cat))[band_index]
                .as_ref()
                .map_err(Clone::clone)
        }
        RunnerCharacter::Dog => {
            static CYCLES: OnceLock<[Result<[RgbaFrame; 8], RunnerBuildError>; LOAD_BANDS]> =
                OnceLock::new();
            CYCLES.get_or_init(|| build_final_cycles(RunnerCharacter::Dog))[band_index]
                .as_ref()
                .map_err(Clone::clone)
        }
        RunnerCharacter::Rabbit => {
            static CYCLES: OnceLock<[Result<[RgbaFrame; 8], RunnerBuildError>; LOAD_BANDS]> =
                OnceLock::new();
            CYCLES.get_or_init(|| build_final_cycles(RunnerCharacter::Rabbit))[band_index]
                .as_ref()
                .map_err(Clone::clone)
        }
        RunnerCharacter::Fox => {
            static CYCLES: OnceLock<[Result<[RgbaFrame; 8], RunnerBuildError>; LOAD_BANDS]> =
                OnceLock::new();
            CYCLES.get_or_init(|| build_final_cycles(RunnerCharacter::Fox))[band_index]
                .as_ref()
                .map_err(Clone::clone)
        }
        RunnerCharacter::Dinosaur => {
            static CYCLES: OnceLock<[Result<[RgbaFrame; 8], RunnerBuildError>; LOAD_BANDS]> =
                OnceLock::new();
            CYCLES.get_or_init(|| build_final_cycles(RunnerCharacter::Dinosaur))[band_index]
                .as_ref()
                .map_err(Clone::clone)
        }
        RunnerCharacter::Ghost => {
            static CYCLES: OnceLock<[Result<[RgbaFrame; 8], RunnerBuildError>; LOAD_BANDS]> =
                OnceLock::new();
            CYCLES.get_or_init(|| build_final_cycles(RunnerCharacter::Ghost))[band_index]
                .as_ref()
                .map_err(Clone::clone)
        }
        RunnerCharacter::Human => {
            static CYCLES: OnceLock<[Result<[RgbaFrame; 8], RunnerBuildError>; LOAD_BANDS]> =
                OnceLock::new();
            CYCLES.get_or_init(|| build_final_cycles(RunnerCharacter::Human))[band_index]
                .as_ref()
                .map_err(Clone::clone)
        }
        _ => Err(decode_error(
            character,
            "cached_final_cycle[unsupported]: character does not use asset-backed runner"
                .to_string(),
        )),
    }
}

fn cached_source_cycle(
    character: RunnerCharacter,
) -> Result<&'static [RgbaFrame; 8], RunnerBuildError> {
    match character {
        RunnerCharacter::Cat => {
            static FRAMES: OnceLock<Result<[RgbaFrame; 8], RunnerBuildError>> = OnceLock::new();
            FRAMES.get_or_init(decode_cat_source_cycle).as_ref().map_err(Clone::clone)
        }
        RunnerCharacter::Dog => {
            static FRAMES: OnceLock<Result<[RgbaFrame; 8], RunnerBuildError>> = OnceLock::new();
            FRAMES.get_or_init(decode_dog_source_cycle).as_ref().map_err(Clone::clone)
        }
        RunnerCharacter::Rabbit => {
            static FRAMES: OnceLock<Result<[RgbaFrame; 8], RunnerBuildError>> = OnceLock::new();
            FRAMES
                .get_or_init(decode_rabbit_source_cycle)
                .as_ref()
                .map_err(Clone::clone)
        }
        RunnerCharacter::Fox => {
            static FRAMES: OnceLock<Result<[RgbaFrame; 8], RunnerBuildError>> = OnceLock::new();
            FRAMES.get_or_init(decode_fox_source_cycle).as_ref().map_err(Clone::clone)
        }
        RunnerCharacter::Dinosaur => {
            static FRAMES: OnceLock<Result<[RgbaFrame; 8], RunnerBuildError>> = OnceLock::new();
            FRAMES
                .get_or_init(decode_dinosaur_source_cycle)
                .as_ref()
                .map_err(Clone::clone)
        }
        RunnerCharacter::Ghost => {
            static FRAMES: OnceLock<Result<[RgbaFrame; 8], RunnerBuildError>> = OnceLock::new();
            FRAMES
                .get_or_init(decode_ghost_source_cycle)
                .as_ref()
                .map_err(Clone::clone)
        }
        RunnerCharacter::Human => {
            static FRAMES: OnceLock<Result<[RgbaFrame; 8], RunnerBuildError>> = OnceLock::new();
            FRAMES
                .get_or_init(decode_human_source_cycle)
                .as_ref()
                .map_err(Clone::clone)
        }
        _ => Err(decode_error(
            character,
            "cached_source_cycle[unsupported]: character does not use asset-backed runner"
                .to_string(),
        )),
    }
}

fn build_final_cycles(
    character: RunnerCharacter,
) -> [Result<[RgbaFrame; 8], RunnerBuildError>; LOAD_BANDS] {
    std::array::from_fn(|band_index| finalize_cycle(character, LOAD_PRESET_CPUS[band_index]))
}

fn finalize_cycle(
    character: RunnerCharacter,
    cpu_pct: f32,
) -> Result<[RgbaFrame; 8], RunnerBuildError> {
    let raw = source_cycle(character)?;
    let motion_input = apply_motion_cycle(&raw, cpu_pct);
    let normalized = normalize_cycle(manifest(character), motion_input)?;
    validate_cycle(manifest(character), &normalized)?;
    Ok(normalized)
}

fn decode_cat_source_cycle() -> Result<[RgbaFrame; 8], RunnerBuildError> {
    decode_sheet_cycle(
        RunnerCharacter::Cat,
        CAT_RUN_SHEET,
        SheetLayout {
            frame_width: CAT_RUN_FRAME_WIDTH,
            frame_height: CAT_RUN_FRAME_HEIGHT,
            sheet_columns: CAT_RUN_FRAME_COUNT,
            source_pose_count: CAT_RUN_FRAME_COUNT,
            sheet_row: 0,
            phase_offset: 0.0,
            draw_height: PIXEL_HEIGHT,
            crop: Crop {
                left: 1,
                top: 4,
                width: 64,
                height: 58,
            },
            matte: MatteKey::BlueBackground,
        },
    )
}

fn decode_dog_source_cycle() -> Result<[RgbaFrame; 8], RunnerBuildError> {
    decode_sheet_cycle(
        RunnerCharacter::Dog,
        DOG_RUN_SHEET,
        SheetLayout {
            frame_width: DOG_RUN_FRAME_WIDTH,
            frame_height: DOG_RUN_FRAME_HEIGHT,
            sheet_columns: DOG_RUN_FRAME_COUNT,
            source_pose_count: DOG_RUN_FRAME_COUNT,
            sheet_row: DOG_RUN_SHEET_ROW,
            phase_offset: 0.0,
            draw_height: 18,
            crop: Crop {
                left: 0,
                top: 0,
                width: 60,
                height: 38,
            },
            matte: MatteKey::DarkBackground,
        },
    )
}

fn decode_rabbit_source_cycle() -> Result<[RgbaFrame; 8], RunnerBuildError> {
    decode_sheet_cycle(
        RunnerCharacter::Rabbit,
        RABBIT_RUN_SHEET,
        SheetLayout {
            frame_width: RABBIT_RUN_FRAME_WIDTH,
            frame_height: RABBIT_RUN_FRAME_HEIGHT,
            sheet_columns: RABBIT_RUN_FRAME_COUNT,
            source_pose_count: RABBIT_RUN_FRAME_COUNT,
            sheet_row: 0,
            phase_offset: 1.0,
            draw_height: 14,
            crop: Crop {
                left: 2,
                top: 1,
                width: 28,
                height: 24,
            },
            matte: MatteKey::DarkBackground,
        },
    )
}

fn decode_fox_source_cycle() -> Result<[RgbaFrame; 8], RunnerBuildError> {
    decode_sheet_cycle(
        RunnerCharacter::Fox,
        FOX_RUN_SHEET,
        SheetLayout {
            frame_width: FOX_RUN_FRAME_WIDTH,
            frame_height: FOX_RUN_FRAME_HEIGHT,
            sheet_columns: FOX_RUN_FRAME_COUNT,
            source_pose_count: FOX_RUN_FRAME_COUNT,
            sheet_row: 0,
            phase_offset: 0.0,
            draw_height: 18,
            crop: Crop {
                left: 1,
                top: 0,
                width: 62,
                height: 36,
            },
            matte: MatteKey::DarkBackground,
        },
    )
}

fn decode_ghost_source_cycle() -> Result<[RgbaFrame; 8], RunnerBuildError> {
    decode_sheet_cycle(
        RunnerCharacter::Ghost,
        GHOST_RUN_SHEET,
        SheetLayout {
            frame_width: GHOST_RUN_FRAME_WIDTH,
            frame_height: GHOST_RUN_FRAME_HEIGHT,
            sheet_columns: GHOST_RUN_FRAME_COUNT,
            source_pose_count: GHOST_RUN_FRAME_COUNT,
            sheet_row: 0,
            phase_offset: 0.0,
            draw_height: 18,
            crop: Crop {
                left: 1,
                top: 0,
                width: 30,
                height: 32,
            },
            matte: MatteKey::DarkBackground,
        },
    )
}

fn decode_dinosaur_source_cycle() -> Result<[RgbaFrame; 8], RunnerBuildError> {
    decode_sequence_cycle(
        RunnerCharacter::Dinosaur,
        DINO_RUN_FRAMES,
        SequenceLayout {
            source_width: DINO_RUN_FRAME_WIDTH,
            source_height: DINO_RUN_FRAME_HEIGHT,
            draw_width: 45,
            draw_height: 19,
            crop: Crop {
                left: 20,
                top: 45,
                width: 640,
                height: 400,
            },
        },
    )
}

fn decode_human_source_cycle() -> Result<[RgbaFrame; 8], RunnerBuildError> {
    let decoded = decode_png(RunnerCharacter::Human, HUMAN_RUN_SHEET, "decode_human_source_cycle")?;
    if decoded.width != 128 {
        return Err(decode_error(
            RunnerCharacter::Human,
            format!(
                "decode_human_source_cycle[width]: expected 128px sheet width, got {}",
                decoded.width
            ),
        ));
    }
    if decoded.height != 128 {
        return Err(decode_error(
            RunnerCharacter::Human,
            format!(
                "decode_human_source_cycle[height]: expected 128px sheet height, got {}",
                decoded.height
            ),
        ));
    }

    Ok(std::array::from_fn(|pose| {
        let frame_x = u32::try_from(pose % 4).unwrap_or(0) * HUMAN_RUN_FRAME_WIDTH;
        let frame_y = u32::try_from(pose / 4).unwrap_or(0) * HUMAN_RUN_FRAME_HEIGHT;
        extract_human_frame(
            &decoded,
            RunnerCharacter::Human,
            frame_x,
            frame_y,
        )
    }))
}

fn decode_sheet_cycle(
    character: RunnerCharacter,
    sheet: &[u8],
    layout: SheetLayout,
) -> Result<[RgbaFrame; 8], RunnerBuildError> {
    validate_crop(character, "decode_sheet_cycle[crop]", layout.frame_width, layout.frame_height, layout.crop)?;
    let decoded = decode_png(character, sheet, "decode_sheet_cycle[png]")?;
    let expected_width = layout.frame_width * layout.sheet_columns;
    if decoded.width != expected_width {
        return Err(decode_error(
            character,
            format!(
                "decode_sheet_cycle[width]: expected {}px ({} columns × {}px), got {}",
                expected_width, layout.sheet_columns, layout.frame_width, decoded.width
            ),
        ));
    }
    let minimum_height = layout.frame_height * (layout.sheet_row + 1);
    if decoded.height < minimum_height {
        return Err(decode_error(
            character,
            format!(
                "decode_sheet_cycle[height]: expected at least {}px for row {}, got {}",
                minimum_height, layout.sheet_row, decoded.height
            ),
        ));
    }

    let source_frames = (0..layout.source_pose_count)
        .map(|source_frame| extract_sheet_frame(&decoded, character, layout, source_frame))
        .collect::<Result<Vec<_>, _>>()?;
    resample_source_cycle(character, &source_frames, layout.phase_offset)
}

fn decode_sequence_cycle(
    character: RunnerCharacter,
    sheets: &[&[u8]],
    layout: SequenceLayout,
) -> Result<[RgbaFrame; 8], RunnerBuildError> {
    validate_crop(
        character,
        "decode_sequence_cycle[crop]",
        layout.source_width,
        layout.source_height,
        layout.crop,
    )?;

    let source_frames = sheets
        .iter()
        .enumerate()
        .map(|(index, sheet)| {
            let decoded = decode_png(character, sheet, &format!("decode_sequence_cycle[png:{index}]"))?;
            if decoded.width != layout.source_width {
                return Err(decode_error(
                    character,
                    format!(
                        "decode_sequence_cycle[width]: frame {index} expected {}px, got {}",
                        layout.source_width, decoded.width
                    ),
                ));
            }
            if decoded.height != layout.source_height {
                return Err(decode_error(
                    character,
                    format!(
                        "decode_sequence_cycle[height]: frame {index} expected {}px, got {}",
                        layout.source_height, decoded.height
                    ),
                ));
            }
            extract_sequence_frame(
                &decoded,
                character,
                layout.crop,
                layout.draw_width,
                layout.draw_height,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    source_frames.try_into().map_err(|frames: Vec<RgbaFrame>| {
        decode_error(
            character,
            format!(
                "decode_sequence_cycle[array_length]: expected 8 source frames, got {}",
                frames.len()
            ),
        )
    })
}

fn resample_source_cycle(
    character: RunnerCharacter,
    source_frames: &[RgbaFrame],
    phase_offset: f32,
) -> Result<[RgbaFrame; 8], RunnerBuildError> {
    if source_frames.is_empty() {
        return Err(decode_error(
            character,
            "resample_source_cycle[empty]: no source frames were decoded".to_string(),
        ));
    }
    if source_frames.len() == usize::from(FRAME_COUNT) {
        return source_frames
            .to_vec()
            .try_into()
            .map_err(|frames: Vec<RgbaFrame>| {
                decode_error(
                    character,
                    format!(
                        "resample_source_cycle[array_length]: expected 8 source frames, got {}",
                        frames.len()
                    ),
                )
            });
    }

    let frames = (0..usize::from(FRAME_COUNT))
        .map(|pose| {
            let position =
                phase_offset + pose as f32 * source_frames.len() as f32 / f32::from(FRAME_COUNT);
            let left_index = position.floor() as usize % source_frames.len();
            let right_index = (left_index + 1) % source_frames.len();
            let mix = position.fract();
            if mix == 0.0 {
                return source_frames[left_index].clone();
            }

            if alpha_delta_ratio(&source_frames[left_index], &source_frames[right_index])
                > NEAREST_POSE_VISIBLE_DELTA_RATIO
            {
                let source_index = if mix < 0.5 {
                    left_index
                } else {
                    right_index
                };
                source_frames[source_index].clone()
            } else {
                interpolate_frames(&source_frames[left_index], &source_frames[right_index], mix)
            }
        })
        .collect::<Vec<_>>();
    frames.try_into().map_err(|frames: Vec<RgbaFrame>| {
        decode_error(
            character,
            format!(
                "resample_source_cycle[array_length]: expected 8 output frames, got {}",
                frames.len()
            ),
        )
    })
}

fn apply_motion_cycle(base: &[RgbaFrame; 8], cpu_pct: f32) -> [RgbaFrame; 8] {
    std::array::from_fn(|pose| {
        let shift = motion_shift(cpu_pct, u8::try_from(pose).unwrap_or(0));
        shift_frame(&base[pose], shift)
    })
}

fn motion_shift(cpu_pct: f32, frame: u8) -> i32 {
    const BAND_SHIFTS: [[i32; 8]; LOAD_BANDS] = [
        [1, 0, -1, -2, 1, 0, -1, 0],
        [2, 0, -1, -2, 2, 0, -1, 0],
        [2, 0, -1, -2, 2, 0, -1, 0],
        [2, 0, -1, -3, 2, 0, -1, 0],
    ];

    BAND_SHIFTS[load_band(cpu_pct)][usize::from(frame % FRAME_COUNT)]
}

fn clone_cycle(base: &[RgbaFrame; 8]) -> [RgbaFrame; 8] {
    std::array::from_fn(|pose| base[pose].clone())
}

fn shift_frame(frame: &RgbaFrame, vertical_shift: i32) -> RgbaFrame {
    if vertical_shift == 0 {
        return frame.clone();
    }

    let mut shifted = vec![0; frame.len()];
    for y in 0..PIXEL_HEIGHT as i32 {
        let target_y = y + vertical_shift;
        if !(0..PIXEL_HEIGHT as i32).contains(&target_y) {
            continue;
        }
        for x in 0..PIXEL_WIDTH {
            let source_index = ((y as u32 * PIXEL_WIDTH + x) as usize) * CHANNELS_PER_PIXEL;
            let target_index =
                ((target_y as u32 * PIXEL_WIDTH + x) as usize) * CHANNELS_PER_PIXEL;
            shifted[target_index..target_index + CHANNELS_PER_PIXEL]
                .copy_from_slice(&frame[source_index..source_index + CHANNELS_PER_PIXEL]);
        }
    }
    shifted
}

fn decode_png(
    character: RunnerCharacter,
    sheet: &[u8],
    stage: &str,
) -> Result<DecodedPng, RunnerBuildError> {
    let mut decoder = png::Decoder::new(Cursor::new(sheet));
    decoder.set_transformations(png::Transformations::EXPAND);
    let mut reader = decoder
        .read_info()
        .map_err(|error| decode_error(character, format!("{stage}: metadata: {error}")))?;
    let mut pixels = vec![
        0;
        reader.output_buffer_size().ok_or_else(|| {
            decode_error(character, format!("{stage}: PNG metadata omitted output buffer size"))
        })?
    ];
    let info = reader
        .next_frame(&mut pixels)
        .map_err(|error| decode_error(character, format!("{stage}: frame decode: {error}")))?;
    let channels = match info.color_type {
        png::ColorType::Rgb => 3,
        png::ColorType::Rgba => 4,
        png::ColorType::Grayscale => 1,
        png::ColorType::GrayscaleAlpha => 2,
        color_type => {
            return Err(decode_error(
                character,
                format!("{stage}: unsupported color type {color_type:?}"),
            ));
        }
    };

    pixels.truncate(info.buffer_size());
    Ok(DecodedPng {
        width: info.width,
        height: info.height,
        color_type: info.color_type,
        channels,
        pixels,
    })
}

fn validate_crop(
    character: RunnerCharacter,
    stage: &str,
    frame_width: u32,
    frame_height: u32,
    crop: Crop,
) -> Result<(), RunnerBuildError> {
    if crop.left + crop.width > frame_width || crop.top + crop.height > frame_height {
        return Err(decode_error(
            character,
            format!(
                "{stage}: crop ({}, {}, {}, {}) exceeds source frame {}x{}",
                crop.left, crop.top, crop.width, crop.height, frame_width, frame_height
            ),
        ));
    }
    Ok(())
}

fn extract_sheet_frame(
    decoded: &DecodedPng,
    character: RunnerCharacter,
    layout: SheetLayout,
    source_frame: u32,
) -> Result<RgbaFrame, RunnerBuildError> {
    extract_cropped_frame(
        decoded,
        character,
        SheetFrameOrigin {
            x: source_frame * layout.frame_width,
            y: layout.sheet_row * layout.frame_height,
        },
        layout.crop,
        layout.matte,
        PIXEL_WIDTH,
        layout.draw_height,
    )
}

fn extract_sequence_frame(
    decoded: &DecodedPng,
    character: RunnerCharacter,
    crop: Crop,
    draw_width: u32,
    draw_height: u32,
) -> Result<RgbaFrame, RunnerBuildError> {
    extract_cropped_frame(
        decoded,
        character,
        SheetFrameOrigin { x: 0, y: 0 },
        crop,
        MatteKey::None,
        draw_width,
        draw_height,
    )
}

#[derive(Clone, Copy)]
struct SheetFrameOrigin {
    x: u32,
    y: u32,
}

fn extract_cropped_frame(
    decoded: &DecodedPng,
    character: RunnerCharacter,
    origin: SheetFrameOrigin,
    crop: Crop,
    matte: MatteKey,
    draw_width: u32,
    draw_height: u32,
) -> Result<RgbaFrame, RunnerBuildError> {
    if draw_width == 0 || draw_width > PIXEL_WIDTH || draw_height == 0 || draw_height > PIXEL_HEIGHT {
        return Err(decode_error(
            character,
            format!(
                "extract_cropped_frame[draw_size]: draw size {}x{} is outside 1..={} by 1..={}",
                draw_width, draw_height, PIXEL_WIDTH, PIXEL_HEIGHT
            ),
        ));
    }

    let mut frame = vec![0u8; (PIXEL_WIDTH * PIXEL_HEIGHT * CHANNELS_PER_PIXEL as u32) as usize];
    let draw_left = (PIXEL_WIDTH - draw_width) / 2;
    let draw_top = (PIXEL_HEIGHT - draw_height) / 2;
    for draw_y in 0..draw_height {
        let source_y = crop.top + sample_axis(draw_y, draw_height, crop.height);
        for draw_x in 0..draw_width {
            let source_x = crop.left + sample_axis(draw_x, draw_width, crop.width);
            let alpha = sample_alpha(
                decoded,
                character,
                origin.x + source_x,
                origin.y + source_y,
                matte,
            )?;
            if alpha == 0 {
                continue;
            }
            let target_index =
                (((draw_top + draw_y) * PIXEL_WIDTH + draw_left + draw_x) as usize)
                    * CHANNELS_PER_PIXEL;
            frame[target_index + 3] = alpha;
        }
    }
    Ok(frame)
}

fn extract_human_frame(
    decoded: &DecodedPng,
    character: RunnerCharacter,
    frame_x: u32,
    frame_y: u32,
) -> RgbaFrame {
    let mut frame = vec![0u8; (PIXEL_WIDTH * PIXEL_HEIGHT * CHANNELS_PER_PIXEL as u32) as usize];
    let draw_left = (PIXEL_WIDTH - HUMAN_RUN_DRAW_WIDTH) / 2;
    let draw_top = (PIXEL_HEIGHT - HUMAN_RUN_DRAW_HEIGHT) / 2;

    for draw_y in 0..HUMAN_RUN_DRAW_HEIGHT {
        let source_y = draw_y * HUMAN_RUN_FRAME_HEIGHT / HUMAN_RUN_DRAW_HEIGHT;
        for draw_x in 0..HUMAN_RUN_DRAW_WIDTH {
            let source_x = draw_x * HUMAN_RUN_FRAME_WIDTH / HUMAN_RUN_DRAW_WIDTH;
            let pixel = read_pixel(decoded, frame_x + source_x, frame_y + source_y);
            let alpha = if pixel.alpha == 0 {
                0
            } else if pixel.red < 48 && pixel.green < 48 && pixel.blue < 48 {
                pixel.alpha / 3
            } else {
                pixel.alpha
            };
            if alpha == 0 {
                continue;
            }
            let target_index =
                (((draw_top + draw_y) * PIXEL_WIDTH + draw_left + draw_x) as usize)
                    * CHANNELS_PER_PIXEL;
            frame[target_index + 3] = alpha;
        }
    }

    let _ = character;
    frame
}

fn sample_alpha(
    decoded: &DecodedPng,
    character: RunnerCharacter,
    x: u32,
    y: u32,
    matte: MatteKey,
) -> Result<u8, RunnerBuildError> {
    if x >= decoded.width || y >= decoded.height {
        return Err(RunnerBuildError::InvalidFrame {
            character,
            detail: format!(
                "sample_alpha[out_of_bounds]: requested source pixel ({x}, {y}) outside {}x{}",
                decoded.width, decoded.height
            ),
        });
    }

    let pixel = read_pixel(decoded, x, y);
    let masked = match matte {
        MatteKey::None => false,
        MatteKey::BlueBackground => {
            pixel.blue > pixel.red.saturating_add(35)
                && pixel.blue > pixel.green.saturating_add(10)
                && pixel.blue > CAT_BLUE_MATTE_MIN
        }
        MatteKey::DarkBackground => matches!(decoded.color_type, png::ColorType::Rgb | png::ColorType::Grayscale)
            && pixel.red < MATTE_DARK_THRESHOLD
            && pixel.green < MATTE_DARK_THRESHOLD
            && pixel.blue < MATTE_DARK_THRESHOLD,
    };

    Ok(if masked { 0 } else { pixel.alpha })
}

#[derive(Clone, Copy)]
struct PixelSample {
    red: u8,
    green: u8,
    blue: u8,
    alpha: u8,
}

fn read_pixel(decoded: &DecodedPng, x: u32, y: u32) -> PixelSample {
    let index = ((y * decoded.width + x) as usize) * decoded.channels;
    match decoded.color_type {
        png::ColorType::Rgb => PixelSample {
            red: decoded.pixels[index],
            green: decoded.pixels[index + 1],
            blue: decoded.pixels[index + 2],
            alpha: 255,
        },
        png::ColorType::Rgba => PixelSample {
            red: decoded.pixels[index],
            green: decoded.pixels[index + 1],
            blue: decoded.pixels[index + 2],
            alpha: decoded.pixels[index + 3],
        },
        png::ColorType::Grayscale => PixelSample {
            red: decoded.pixels[index],
            green: decoded.pixels[index],
            blue: decoded.pixels[index],
            alpha: 255,
        },
        png::ColorType::GrayscaleAlpha => PixelSample {
            red: decoded.pixels[index],
            green: decoded.pixels[index],
            blue: decoded.pixels[index],
            alpha: decoded.pixels[index + 1],
        },
        _ => unreachable!("unsupported color type already rejected"),
    }
}

fn sample_axis(draw_index: u32, draw_span: u32, source_span: u32) -> u32 {
    if draw_span <= 1 || source_span <= 1 {
        return 0;
    }
    draw_index.min(draw_span - 1) * source_span.saturating_sub(1) / (draw_span - 1)
}

fn alpha_delta_ratio(left: &RgbaFrame, right: &RgbaFrame) -> f32 {
    let mut visible = 0usize;
    let mut delta = 0usize;
    for (left, right) in left
        .chunks_exact(CHANNELS_PER_PIXEL)
        .zip(right.chunks_exact(CHANNELS_PER_PIXEL))
    {
        let left_alpha = left[3];
        let right_alpha = right[3];
        if left_alpha >= VISIBLE_SOURCE_ALPHA_THRESHOLD || right_alpha >= VISIBLE_SOURCE_ALPHA_THRESHOLD
        {
            visible += 1;
            if left_alpha.abs_diff(right_alpha) > ALPHA_DELTA_THRESHOLD {
                delta += 1;
            }
        }
    }

    if visible == 0 {
        0.0
    } else {
        delta as f32 / visible as f32
    }
}

fn interpolate_frames(left: &RgbaFrame, right: &RgbaFrame, mix: f32) -> RgbaFrame {
    let mut frame = vec![0; left.len()];
    for (index, (left, right)) in left
        .chunks_exact(CHANNELS_PER_PIXEL)
        .zip(right.chunks_exact(CHANNELS_PER_PIXEL))
        .enumerate()
    {
        let alpha = (f32::from(left[3]) * (1.0 - mix) + f32::from(right[3]) * mix).round() as u8;
        frame[index * CHANNELS_PER_PIXEL + 3] = alpha;
    }
    frame
}

fn frame_at(
    frames: &[RgbaFrame; 8],
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

#[cfg(test)]
mod tests {
    use peterfan_core::config::RunnerCharacter;

    use super::{
        build_cycle, render, resample_source_cycle, sample_alpha, DecodedPng, MatteKey,
    };
    use crate::runner::{manifest, validate_cycle, FRAME_COUNT};

    fn asset_characters() -> [RunnerCharacter; 7] {
        [
            RunnerCharacter::Cat,
            RunnerCharacter::Dog,
            RunnerCharacter::Rabbit,
            RunnerCharacter::Fox,
            RunnerCharacter::Dinosaur,
            RunnerCharacter::Ghost,
            RunnerCharacter::Human,
        ]
    }

    fn constant_alpha_frame(alpha: u8) -> Vec<u8> {
        let mut frame = vec![0; (crate::runner::PIXEL_WIDTH * crate::runner::PIXEL_HEIGHT * 4) as usize];
        for pixel in frame.chunks_exact_mut(4) {
            pixel[3] = alpha;
        }
        frame
    }

    #[test]
    fn asset_backed_characters_pass_the_shared_quality_gate() {
        assert_eq!(FRAME_COUNT, 8);

        let mut failures = Vec::new();
        for character in asset_characters() {
            for cpu_pct in [0.0, 30.0, 65.0, 90.0] {
                match build_cycle(character, cpu_pct) {
                    Ok(cycle) => {
                        if let Err(error) = validate_cycle(manifest(character), &cycle) {
                            failures.push(format!("{character:?} at {cpu_pct}%: {error}"));
                        }
                    }
                    Err(error) => failures.push(format!("{character:?} at {cpu_pct}%: {error}")),
                }
            }
        }

        assert!(
            failures.is_empty(),
            "asset-backed quality failures:\n{}",
            failures.join("\n")
        );
    }

    #[test]
    fn render_matches_build_cycle_frames_for_all_asset_characters_and_bands() {
        let mut mismatches = Vec::new();
        for character in asset_characters() {
            for cpu_pct in [0.0, 30.0, 65.0, 90.0] {
                let cycle = build_cycle(character, cpu_pct).expect("build cycle");
                for frame in 0..FRAME_COUNT {
                    let rendered = render(character, cpu_pct, frame).expect("rendered frame");
                    if rendered != cycle[usize::from(frame)] {
                        mismatches.push(format!(
                            "{character:?} at {cpu_pct}% frame {frame}: render output differed from build_cycle"
                        ));
                    }
                }
            }
        }

        assert!(
            mismatches.is_empty(),
            "render/build mismatches:\n{}",
            mismatches.join("\n")
        );
    }

    #[test]
    fn five_pose_resampling_uses_nearest_pose_when_alpha_delta_is_large() {
        let source_frames = [10_u8, 70, 130, 190, 250].map(constant_alpha_frame);
        let resampled = resample_source_cycle(RunnerCharacter::Cat, &source_frames, 0.0)
            .expect("resampled cycle");

        let expected_order = [0_usize, 1, 1, 2, 3, 3, 4, 4];
        for (pose, expected_source) in expected_order.into_iter().enumerate() {
            assert_eq!(
                resampled[pose],
                source_frames[expected_source],
                "pose {pose} should select source frame {expected_source}"
            );
        }
    }

    #[test]
    fn unsupported_asset_characters_return_runner_build_error() {
        let error = build_cycle(RunnerCharacter::Robot, 30.0).expect_err("unsupported asset");
        let detail = error.to_string();
        assert!(
            detail.contains("robot"),
            "expected character in error, got: {detail}"
        );
    }

    #[test]
    fn sample_alpha_out_of_bounds_reports_the_actual_character() {
        let decoded = DecodedPng {
            width: 1,
            height: 1,
            color_type: png::ColorType::Grayscale,
            channels: 1,
            pixels: vec![255],
        };

        let error = sample_alpha(&decoded, RunnerCharacter::Dog, 1, 0, MatteKey::None)
            .expect_err("out of bounds");
        let detail = error.to_string();
        assert!(
            detail.contains("runner 'dog'"),
            "expected dog diagnostic, got: {detail}"
        );
    }
}
