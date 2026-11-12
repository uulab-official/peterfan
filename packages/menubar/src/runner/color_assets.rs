//! Embedded, pre-normalized sprites. Missing or invalid art leaves the legacy runner intact.
use std::collections::VecDeque;
use std::io::Cursor;

use peterfan_core::config::RunnerCharacter;

use super::{
    RgbaFrame, RunnerBuildError, RunnerPack, FRAME_COUNT, LOAD_BANDS, PIXEL_HEIGHT, PIXEL_WIDTH,
};

include!(concat!(env!("OUT_DIR"), "/color_runner_registry.rs"));

pub(super) fn color_pack(
    character: RunnerCharacter,
) -> Option<Result<RunnerPack, RunnerBuildError>> {
    embedded_strip(character.as_str()).map(|bytes| decode_strip(character, bytes))
}

fn decode_strip(character: RunnerCharacter, bytes: &[u8]) -> Result<RunnerPack, RunnerBuildError> {
    let fail = |detail: String| RunnerBuildError::Decode { character, detail };
    let mut decoder = png::Decoder::new(Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND);
    let mut reader = decoder
        .read_info()
        .map_err(|e| fail(format!("color strip metadata: {e}")))?;
    let expected_width = PIXEL_WIDTH * u32::from(FRAME_COUNT);
    if reader.info().width != expected_width || reader.info().height != PIXEL_HEIGHT {
        return Err(fail(format!(
            "color strip must be {expected_width}x{PIXEL_HEIGHT}"
        )));
    }
    let size = reader
        .output_buffer_size()
        .ok_or_else(|| fail("color strip missing buffer size".into()))?;
    // An exact 8-bit RGBA strip fits this bound. Reject oversized or higher-depth sources.
    let expected_bytes = (expected_width * PIXEL_HEIGHT * 4) as usize;
    if size != expected_bytes {
        return Err(fail("color strip must be 8-bit RGBA".into()));
    }
    let mut pixels = vec![0; size];
    let info = reader
        .next_frame(&mut pixels)
        .map_err(|e| fail(format!("color strip decode: {e}")))?;
    if info.color_type != png::ColorType::Rgba
        || info.bit_depth != png::BitDepth::Eight
        || info.buffer_size() != expected_bytes
    {
        return Err(fail("color strip must be 8-bit RGBA".into()));
    }
    let mut cycle: [RgbaFrame; FRAME_COUNT as usize] =
        std::array::from_fn(|_| Vec::with_capacity((PIXEL_WIDTH * PIXEL_HEIGHT * 4) as usize));
    let stride = (expected_width * 4) as usize;
    let frame_stride = (PIXEL_WIDTH * 4) as usize;
    for row in pixels.chunks_exact(stride) {
        for (index, frame) in cycle.iter_mut().enumerate() {
            frame.extend_from_slice(&row[index * frame_stride..(index + 1) * frame_stride]);
        }
    }
    if cycle.iter().skip(1).all(|frame| frame == &cycle[0]) {
        return Err(fail("color strip contains no animation".into()));
    }
    // The orange cat already contrasts against dark menus. Adding a bright
    // contour to its small detailed sprite creates distracting white fragments.
    // Keep the contrast treatment for the other, potentially darker runners.
    if character != RunnerCharacter::Cat {
        for frame in &mut cycle {
            add_dark_edge_contrast(frame);
        }
    }
    let cycles = std::array::from_fn::<_, LOAD_BANDS, _>(|_| cycle.clone());
    RunnerPack::from_color_cycles(character, cycles)
}

/// Add a subtle light contour outside dark silhouettes so they stay visible
/// against dark menu bars. Existing art and enclosed transparent holes are
/// left intact; the original asset bytes are never modified.
fn add_dark_edge_contrast(frame: &mut RgbaFrame) {
    let width = PIXEL_WIDTH as usize;
    let height = PIXEL_HEIGHT as usize;
    let original = frame.clone();
    let mut outside = vec![false; width * height];
    let mut queue = VecDeque::new();
    for y in 0..height {
        for x in 0..width {
            if (x == 0 || y == 0 || x + 1 == width || y + 1 == height)
                && original[(y * width + x) * 4 + 3] == 0
            {
                outside[y * width + x] = true;
                queue.push_back((x, y));
            }
        }
    }
    while let Some((x, y)) = queue.pop_front() {
        for dy in -1_isize..=1 {
            for dx in -1_isize..=1 {
                let Some(nx) = x.checked_add_signed(dx).filter(|&v| v < width) else {
                    continue;
                };
                let Some(ny) = y.checked_add_signed(dy).filter(|&v| v < height) else {
                    continue;
                };
                let index = ny * width + nx;
                if !outside[index] && original[index * 4 + 3] == 0 {
                    outside[index] = true;
                    queue.push_back((nx, ny));
                }
            }
        }
    }
    for y in 0..height {
        for x in 0..width {
            let index = y * width + x;
            // A generated contour touching the image boundary is clipped by
            // the frame, which can look like a stray horizontal/vertical line.
            // Original artwork remains untouched even when it reaches an edge.
            if x == 0 || y == 0 || x + 1 == width || y + 1 == height || !outside[index] {
                continue;
            }
            let dark_neighbor = (-1_isize..=1).any(|dy| {
                (-1_isize..=1).any(|dx| {
                    let Some(nx) = x.checked_add_signed(dx).filter(|&v| v < width) else {
                        return false;
                    };
                    let Some(ny) = y.checked_add_signed(dy).filter(|&v| v < height) else {
                        return false;
                    };
                    let i = (ny * width + nx) * 4;
                    let p = &original[i..i + 4];
                    p[3] >= 64
                        && 299 * u32::from(p[0]) + 587 * u32::from(p[1]) + 114 * u32::from(p[2])
                            < 145_000
                })
            });
            if dark_neighbor {
                frame[index * 4..index * 4 + 4].copy_from_slice(&[245, 245, 245, 208]);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dark_outline_does_not_create_clipped_lines_at_frame_edges() {
        let width = PIXEL_WIDTH as usize;
        let height = PIXEL_HEIGHT as usize;
        let mut frame = vec![0; width * height * 4];
        let offset = |x: usize, y: usize| (y * width + x) * 4;
        for (x, y) in [(12, 1), (12, height - 2), (1, 12), (width - 2, 12)] {
            frame[offset(x, y)..offset(x, y) + 4].copy_from_slice(&[20, 30, 40, 255]);
        }
        frame[offset(0, 25)..offset(0, 25) + 4].copy_from_slice(&[20, 30, 40, 255]);
        let original = frame.clone();
        add_dark_edge_contrast(&mut frame);
        for y in 0..height {
            for x in 0..width {
                if x == 0 || y == 0 || x + 1 == width || y + 1 == height {
                    assert_eq!(
                        &frame[offset(x, y)..offset(x, y) + 4],
                        &original[offset(x, y)..offset(x, y) + 4],
                        "halo must not introduce an edge pixel at ({x},{y})"
                    );
                }
            }
        }
        assert!(
            frame[offset(12, 2) + 3] > 0,
            "interior halo remains visible"
        );
    }

    #[test]
    fn cat_color_frames_keep_every_original_pixel_without_a_halo() {
        let bytes = embedded_strip(RunnerCharacter::Cat.as_str()).expect("embedded cat strip");
        let mut decoder = png::Decoder::new(Cursor::new(bytes));
        decoder.set_transformations(png::Transformations::EXPAND);
        let mut reader = decoder.read_info().expect("cat PNG header");
        let mut strip = vec![0; reader.output_buffer_size().expect("cat PNG size")];
        let info = reader.next_frame(&mut strip).expect("cat PNG pixels");
        assert_eq!(info.color_type, png::ColorType::Rgba);
        assert_eq!(info.bit_depth, png::BitDepth::Eight);
        let pack = decode_strip(RunnerCharacter::Cat, bytes).expect("cat runner frames");
        let width = PIXEL_WIDTH as usize;
        let height = PIXEL_HEIGHT as usize;
        let strip_width = width * FRAME_COUNT as usize;
        for pose in 0..FRAME_COUNT {
            let frame = pack.frame_by_index(0, pose).expect("cat frame");
            for y in 0..height {
                for x in 0..width {
                    let source = (y * strip_width + pose as usize * width + x) * 4;
                    let target = (y * width + x) * 4;
                    let before = &strip[source..source + 4];
                    let after = &frame[target..target + 4];
                    assert_eq!(before, after, "changed pixel in pose {pose} at ({x},{y})");
                }
            }
        }
    }

    #[test]
    fn dark_outline_preserves_art_and_only_marks_external_dark_edges() {
        let mut frame = vec![0; (PIXEL_WIDTH * PIXEL_HEIGHT * 4) as usize];
        let offset = |x: usize, y: usize| (y * PIXEL_WIDTH as usize + x) * 4;
        // A dark shape with an enclosed transparent hole, and a separate
        // light-colored pixel. Neither the artwork nor the hole may change.
        for y in 10..13 {
            for x in 10..13 {
                if (x, y) != (11, 11) {
                    frame[offset(x, y)..offset(x, y) + 4].copy_from_slice(&[20, 30, 40, 255]);
                }
            }
        }
        frame[offset(30, 10)..offset(30, 10) + 4].copy_from_slice(&[245, 245, 245, 255]);
        let original = frame.clone();
        add_dark_edge_contrast(&mut frame);
        for (before, after) in original
            .as_chunks::<4>()
            .0
            .iter()
            .zip(frame.as_chunks::<4>().0)
        {
            if before[3] != 0 {
                assert_eq!(before, after, "artwork pixels must be unchanged");
            }
        }
        assert!(frame[offset(9, 10) + 3] > 0, "dark silhouette needs a halo");
        let halo = &frame[offset(9, 10)..offset(9, 10) + 4];
        let dark_art = &frame[offset(10, 10)..offset(10, 10) + 4];
        let over = |p: &[u8], background: u32| {
            (u32::from(p[0]) * u32::from(p[3]) + background * (255 - u32::from(p[3]))) / 255
        };
        assert!(over(halo, 0) > 180, "halo must be visible on dark menus");
        assert!(
            over(dark_art, 255) < 50,
            "dark art must remain visible on light menus"
        );
        assert_eq!(
            frame[offset(11, 11) + 3],
            0,
            "interior hole must stay clear"
        );
        assert_eq!(frame[offset(29, 10) + 3], 0, "light edge needs no halo");
        assert_eq!(frame[offset(0, 0) + 3], 0, "background must stay clear");
        let once = frame.clone();
        add_dark_edge_contrast(&mut frame);
        assert_eq!(frame, once, "contrast must not grow when reapplied");
    }

    fn png_strip(width: u32, height: u32, moving: bool, empty: bool) -> Vec<u8> {
        let mut pixels = vec![0; (width * height * 4) as usize];
        for pose in 0..8usize {
            if pose == 4 && empty {
                continue;
            }
            let x = pose * PIXEL_WIDTH as usize + if moving { pose } else { 0 };
            if x >= width as usize {
                continue;
            }
            let i = x * 4;
            pixels[i..i + 4].copy_from_slice(&[160, 90, 40, 255]);
        }
        let mut bytes = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut bytes, width, height);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().unwrap();
            writer.write_image_data(&pixels).unwrap();
        }
        bytes
    }

    #[test]
    fn other_characters_keep_the_dark_edge_contrast() {
        let bytes = png_strip(PIXEL_WIDTH * 8, PIXEL_HEIGHT, true, false);
        let cat = decode_strip(RunnerCharacter::Cat, &bytes).unwrap();
        let dog = decode_strip(RunnerCharacter::Dog, &bytes).unwrap();
        let cat_frame = cat.frame_by_index(0, 1).unwrap();
        let dog_frame = dog.frame_by_index(0, 1).unwrap();
        assert!(
            dog_frame
                .as_chunks::<4>()
                .0
                .iter()
                .zip(cat_frame.as_chunks::<4>().0.iter())
                .any(|(dog, cat)| { *dog == [245, 245, 245, 208] && cat[3] == 0 }),
            "other characters should retain contrast on dark menus"
        );
    }

    #[test]
    fn valid_strip_preserves_color_and_all_load_bands() {
        let bytes = png_strip(PIXEL_WIDTH * 8, PIXEL_HEIGHT, true, false);
        let pack = decode_strip(RunnerCharacter::Cat, &bytes).unwrap();
        assert!(pack.is_color());
        for pose in 0..8u8 {
            let frame = pack.frame_by_index(0, pose).unwrap();
            assert!(frame.as_chunks::<4>().0.contains(&[160, 90, 40, 255]));
            for band in 1..LOAD_BANDS {
                assert_eq!(Some(frame), pack.frame_by_index(band, pose));
            }
        }
        assert_ne!(pack.frame_by_index(0, 0), pack.frame_by_index(0, 1));
    }

    #[test]
    fn every_shipped_character_loads_a_color_cycle_at_each_cpu_band() {
        for character in RunnerCharacter::ALL {
            let pack = RunnerPack::build(character).expect("runner must load");
            assert!(pack.is_color(), "{character:?} fell back to monochrome");
            for cpu in [0.0, 30.0, 65.0, 95.0] {
                let first = pack.frame(cpu, 0);
                assert!(
                    (1..8).any(|pose| pack.frame(cpu, pose) != first),
                    "{character:?} is static at {cpu}%"
                );
            }
            assert!(
                super::super::timing::frame_interval_for_character(character, 5.0)
                    > super::super::timing::frame_interval_for_character(character, 95.0)
            );
        }
    }

    #[test]
    fn bird_color_cycle_has_distinct_raised_and_lowered_wings() {
        let pack = RunnerPack::build(RunnerCharacter::Bird).expect("bird runner");
        assert!(pack.is_color());
        let raised_wing_pixels = |pose| {
            let frame = pack.frame_by_index(0, pose).unwrap();
            (1..13)
                .flat_map(|y| (3..24).map(move |x| (y * PIXEL_WIDTH as usize + x) * 4 + 3))
                .filter(|&alpha| frame[alpha] > 128)
                .count()
        };
        let counts: Vec<_> = (0..FRAME_COUNT).map(raised_wing_pixels).collect();
        assert!(counts.iter().max().unwrap() - counts.iter().min().unwrap() >= 50);
    }

    #[test]
    fn bird_flight_cycle_preserves_wing_phases_and_repeated_order() {
        let pack = RunnerPack::build(RunnerCharacter::Bird).expect("bird runner");
        let frames: Vec<_> = (0..FRAME_COUNT)
            .map(|pose| pack.frame_by_index(0, pose).unwrap())
            .collect();
        let has_flight_order = |frames: &[&[u8]]| {
            // The hand-reviewed bird artwork has raised wings at phases 0 and 4
            // and downstrokes at 2 and 6. Compare the actual wing region rather
            // than total image occupancy, which also changes when feet move.
            let wing_occupancy = |frame: &[u8]| {
                (1..13)
                    .flat_map(|y| (3..24).map(move |x| (y * PIXEL_WIDTH as usize + x) * 4 + 3))
                    .filter(|&alpha| frame[alpha] > 128)
                    .count()
            };
            let paired_phases_match = (0..4).all(|i| {
                let distance = |other: usize| {
                    frames[i]
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .zip(frames[other].as_chunks::<4>().0.iter())
                        .filter(|(a, b)| a != b)
                        .count()
                };
                let paired = distance(i + 4);
                (4..8).filter(|&j| j != i + 4).all(|j| paired < distance(j))
            });
            paired_phases_match
                && wing_occupancy(frames[0]) >= wing_occupancy(frames[2]) + 50
                && wing_occupancy(frames[4]) >= wing_occupancy(frames[6]) + 50
        };
        assert!(has_flight_order(&frames), "bird flight phases changed");
        let mut scrambled = frames.clone();
        scrambled.swap(4, 6);
        assert!(
            !has_flight_order(&scrambled),
            "out-of-order wing phases must fail"
        );
        // Structural regression only: these pixels cannot prove the motion looks
        // natural or that a viewer perceives the sprite as flying.
    }

    #[test]
    fn rejects_invalid_dimensions_blank_pose_and_static_strip() {
        let c = RunnerCharacter::Cat;
        assert!(decode_strip(c, &png_strip(32, 32, true, false)).is_err());
        assert!(decode_strip(c, &png_strip(PIXEL_WIDTH * 8, PIXEL_HEIGHT, true, true)).is_err());
        assert!(decode_strip(c, &png_strip(PIXEL_WIDTH * 8, PIXEL_HEIGHT, false, false)).is_err());
        assert!(decode_strip(c, b"not a PNG").is_err());
    }

    #[test]
    fn missing_and_invalid_color_keep_the_same_legacy_character() {
        let character = RunnerCharacter::Dog;
        for candidate in [None, Some(decode_strip(character, b"invalid"))] {
            let pack = RunnerPack::build_with_optional_color(character, candidate, || {
                RunnerPack::build_with(character, |_, _| {
                    Ok(std::array::from_fn(|_| {
                        vec![255; (PIXEL_WIDTH * PIXEL_HEIGHT * 4) as usize]
                    }))
                })
            })
            .unwrap();
            assert_eq!(pack.character(), character);
            assert!(!pack.is_color());
        }
    }
}
