mod assets;
mod draw;
mod manifest;
mod normalize;
mod procedural;
mod quality;
pub(crate) mod timing;

use std::fmt;

use peterfan_core::config::RunnerCharacter;

#[allow(unused_imports)]
pub(crate) use {
    manifest::{manifest, Direction, RunnerManifest},
    normalize::{normalize_cycle, validate_cycle},
    quality::{measure_cycle, measure_frame, Bounds, CycleMetrics, FrameMetrics},
    timing::*,
};

pub(crate) const PIXEL_WIDTH: u32 = 48;
pub(crate) const PIXEL_HEIGHT: u32 = 32;
pub(crate) const LOAD_BANDS: usize = 4;
pub(crate) type RgbaFrame = Vec<u8>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RunnerBuildError {
    Decode {
        character: RunnerCharacter,
        detail: String,
    },
    InvalidFrame {
        character: RunnerCharacter,
        detail: String,
    },
}

impl fmt::Display for RunnerBuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Decode { character, detail } => {
                write!(f, "runner '{}' decode failed: {detail}", character.as_str())
            }
            Self::InvalidFrame { character, detail } => {
                write!(f, "runner '{}' invalid frame: {detail}", character.as_str())
            }
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct RunnerPack {
    character: RunnerCharacter,
    frames: Vec<RgbaFrame>,
}

impl RunnerPack {
    pub(crate) fn build(character: RunnerCharacter) -> Result<Self, RunnerBuildError> {
        Self::build_with(character, build_normalized_cycle)
    }

    fn build_with<F>(character: RunnerCharacter, mut cycle_builder: F) -> Result<Self, RunnerBuildError>
    where
        F: FnMut(RunnerCharacter, f32) -> Result<[RgbaFrame; 8], RunnerBuildError>,
    {
        let frames = [0.0, 30.0, 65.0, 90.0]
            .into_iter()
            .map(|cpu_pct| cycle_builder(character, cpu_pct))
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .flatten()
            .map(style_template_frame)
            .collect::<Vec<_>>();
        if frames.len() != LOAD_BANDS * FRAME_COUNT as usize {
            return Err(RunnerBuildError::InvalidFrame {
                character,
                detail: format!("expected 32 cached frames, got {}", frames.len()),
            });
        }
        Ok(Self { character, frames })
    }

    pub(crate) fn frame(&self, cpu_pct: f32, pose: u8) -> &[u8] {
        &self.frames[load_band(cpu_pct) * FRAME_COUNT as usize + usize::from(pose % FRAME_COUNT)]
    }

    pub(crate) fn character(&self) -> RunnerCharacter {
        self.character
    }

    pub(crate) fn len(&self) -> usize {
        self.frames.len()
    }

    pub(crate) fn frames(&self) -> &[RgbaFrame] {
        &self.frames
    }

    pub(crate) fn frame_by_index(&self, band: usize, pose: u8) -> Option<&[u8]> {
        (band < LOAD_BANDS && pose < FRAME_COUNT)
            .then(|| self.frames[band * FRAME_COUNT as usize + pose as usize].as_slice())
    }

    pub(crate) fn preview_alpha_hex(&self, band: usize, pose: u8) -> Option<String> {
        self.frame_by_index(band, pose).map(alpha_hex)
    }
}

pub(crate) fn build_or_fallback(character: RunnerCharacter) -> (RunnerPack, Option<RunnerBuildError>) {
    build_or_fallback_with(character, RunnerPack::build)
}

pub(crate) fn build_or_fallback_with<F>(
    character: RunnerCharacter,
    builder: F,
) -> (RunnerPack, Option<RunnerBuildError>)
where
    F: Fn(RunnerCharacter) -> Result<RunnerPack, RunnerBuildError>,
{
    match builder(character) {
        Ok(pack) => (pack, None),
        Err(error) => (
            builder(RunnerCharacter::Cat).expect("embedded cat runner pack must validate"),
            Some(error),
        ),
    }
}

pub(crate) fn alpha_hex(rgba: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut alpha = String::with_capacity((PIXEL_WIDTH * PIXEL_HEIGHT * 2) as usize);
    for pixel in rgba.chunks_exact(4) {
        let value = pixel[3];
        alpha.push(HEX[usize::from(value >> 4)] as char);
        alpha.push(HEX[usize::from(value & 0x0f)] as char);
    }
    alpha
}

fn style_template_frame(mut rgba: RgbaFrame) -> RgbaFrame {
    for pixel in rgba.chunks_exact_mut(4) {
        if pixel[3] != 0 {
            pixel[0] = 255;
            pixel[1] = 255;
            pixel[2] = 255;
        }
    }
    rgba
}

pub(crate) fn render_raw_frame(
    character: RunnerCharacter,
    cpu_pct: f32,
    frame: u8,
) -> Result<RgbaFrame, RunnerBuildError> {
    let cycle = build_normalized_cycle(character, cpu_pct)?;
    Ok(cycle[usize::from(frame % FRAME_COUNT)].clone())
}

pub(crate) fn build_normalized_cycle(
    character: RunnerCharacter,
    cpu_pct: f32,
) -> Result<[RgbaFrame; 8], RunnerBuildError> {
    match character {
        RunnerCharacter::Cat => assets::build_cycle(character, cpu_pct),
        RunnerCharacter::Dog => assets::build_cycle(character, cpu_pct),
        RunnerCharacter::Rabbit => assets::build_cycle(character, cpu_pct),
        RunnerCharacter::Fox => assets::build_cycle(character, cpu_pct),
        RunnerCharacter::Dinosaur => assets::build_cycle(character, cpu_pct),
        RunnerCharacter::Ghost => assets::build_cycle(character, cpu_pct),
        RunnerCharacter::Human => assets::build_cycle(character, cpu_pct),
        RunnerCharacter::Penguin => procedural::build_cycle(character, cpu_pct),
        RunnerCharacter::Robot => procedural::build_cycle(character, cpu_pct),
        RunnerCharacter::Bird => procedural::build_cycle(character, cpu_pct),
        RunnerCharacter::Duck => procedural::build_cycle(character, cpu_pct),
        RunnerCharacter::Sheep => procedural::build_cycle(character, cpu_pct),
        RunnerCharacter::Fish => procedural::build_cycle(character, cpu_pct),
        RunnerCharacter::Slime => procedural::build_cycle(character, cpu_pct),
        RunnerCharacter::Pushup => procedural::build_cycle(character, cpu_pct),
        RunnerCharacter::Orbit => procedural::build_cycle(character, cpu_pct),
        RunnerCharacter::Sushi => procedural::build_cycle(character, cpu_pct),
        RunnerCharacter::City => procedural::build_cycle(character, cpu_pct),
        RunnerCharacter::Sausage => procedural::build_cycle(character, cpu_pct),
        RunnerCharacter::Party => procedural::build_cycle(character, cpu_pct),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pack_has_four_bands_and_preview_uses_identical_alpha() {
        for character in RunnerCharacter::ALL {
            let pack = RunnerPack::build(character).expect("runner pack");
            assert_eq!(pack.character(), character);
            assert_eq!(pack.len(), LOAD_BANDS * FRAME_COUNT as usize);
            assert_eq!(pack.frames().len(), LOAD_BANDS * FRAME_COUNT as usize);
            for band in 0..LOAD_BANDS {
                for pose in 0..FRAME_COUNT {
                    let rgba = pack.frame_by_index(band, pose).expect("cached frame");
                    assert_eq!(pack.frame([0.0, 30.0, 65.0, 90.0][band], pose), rgba);
                    assert_eq!(pack.preview_alpha_hex(band, pose), Some(alpha_hex(rgba)));
                }
            }
        }
    }

    #[test]
    fn build_failure_falls_back_to_cat_without_emptying_the_cache() {
        let (pack, error) = build_or_fallback_with(RunnerCharacter::Fox, |character| {
            if character == RunnerCharacter::Fox {
                Err(RunnerBuildError::InvalidFrame {
                    character,
                    detail: "forced failure".to_string(),
                })
            } else {
                RunnerPack::build(character)
            }
        });

        assert_eq!(pack.character(), RunnerCharacter::Cat);
        assert!(error.is_some());
        assert_eq!(pack.len(), LOAD_BANDS * FRAME_COUNT as usize);
    }

    #[test]
    fn runner_pack_builds_exactly_four_cycles_per_character() {
        let mut builds = Vec::new();
        let pack = RunnerPack::build_with(RunnerCharacter::Cat, |character, cpu_pct| {
            builds.push((character, cpu_pct));
            build_normalized_cycle(character, cpu_pct)
        })
        .expect("runner pack");

        assert_eq!(pack.len(), LOAD_BANDS * FRAME_COUNT as usize);
        assert_eq!(builds.len(), LOAD_BANDS);
        assert_eq!(
            builds,
            vec![
                (RunnerCharacter::Cat, 0.0),
                (RunnerCharacter::Cat, 30.0),
                (RunnerCharacter::Cat, 65.0),
                (RunnerCharacter::Cat, 90.0),
            ]
        );
    }

    #[test]
    fn runner_pack_bounds_check_invalid_band_and_pose() {
        let pack = RunnerPack::build(RunnerCharacter::Cat).expect("runner pack");
        assert!(pack.frame_by_index(LOAD_BANDS, 0).is_none());
        assert!(pack.frame_by_index(0, FRAME_COUNT).is_none());
        assert!(pack.preview_alpha_hex(LOAD_BANDS, 0).is_none());
        assert!(pack.preview_alpha_hex(0, FRAME_COUNT).is_none());
    }

    #[test]
    fn every_character_renders_a_complete_raw_frame() {
        for character in RunnerCharacter::ALL {
            for frame in 0..FRAME_COUNT {
                let rgba = render_raw_frame(character, 55.0, frame).expect("raw runner frame");
                assert_eq!(rgba.len(), (PIXEL_WIDTH * PIXEL_HEIGHT * 4) as usize);
                assert!(rgba.chunks_exact(4).any(|pixel| pixel[3] > 0));
            }
        }
    }

    #[test]
    fn build_normalized_cycle_dispatches_all_runner_characters() {
        for character in RunnerCharacter::ALL {
            for cpu_pct in [0.0, 30.0, 65.0, 90.0] {
                let cycle = build_normalized_cycle(character, cpu_pct)
                    .unwrap_or_else(|error| panic!("{character:?} at {cpu_pct}%: {error}"));
                assert_eq!(cycle.len(), usize::from(FRAME_COUNT));
                assert!(cycle.iter().all(|frame| frame.len() == (PIXEL_WIDTH * PIXEL_HEIGHT * 4) as usize));
            }
        }
    }
}
