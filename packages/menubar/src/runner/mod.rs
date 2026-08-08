mod assets;
mod draw;
mod procedural;
pub(crate) mod timing;

use std::fmt;

use peterfan_core::config::RunnerCharacter;

pub(crate) use timing::*;

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
    match character {
        RunnerCharacter::Cat
        | RunnerCharacter::Dog
        | RunnerCharacter::Rabbit
        | RunnerCharacter::Fox
        | RunnerCharacter::Dinosaur
        | RunnerCharacter::Ghost
        | RunnerCharacter::Human => assets::render(character, cpu_pct, frame),
        _ => Ok(procedural::render(character, cpu_pct, frame)),
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
}
