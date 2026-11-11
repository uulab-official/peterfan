//! Embedded, pre-normalized sprites. Missing or invalid art leaves the legacy runner intact.
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
    let cycles = std::array::from_fn::<_, LOAD_BANDS, _>(|_| cycle.clone());
    RunnerPack::from_color_cycles(character, cycles)
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn valid_strip_preserves_color_and_all_load_bands() {
        let bytes = png_strip(PIXEL_WIDTH * 8, PIXEL_HEIGHT, true, false);
        let pack = decode_strip(RunnerCharacter::Cat, &bytes).unwrap();
        assert!(pack.is_color());
        for pose in 0..8u8 {
            let frame = pack.frame_by_index(0, pose).unwrap();
            assert!(frame.chunks_exact(4).any(|p| p == [160, 90, 40, 255]));
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
