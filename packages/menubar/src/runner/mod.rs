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
