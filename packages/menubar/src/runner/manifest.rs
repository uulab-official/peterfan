use peterfan_core::config::RunnerCharacter;

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Direction {
    Right,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct RunnerManifest {
    pub character: RunnerCharacter,
    #[cfg(test)]
    pub output_direction: Direction,
    pub mirror_source: bool,
    pub target_width: u32,
    pub target_height: u32,
    pub baseline_y: u32,
    pub max_centroid_drift: f32,
    pub contact_frames: [u8; 2],
    pub flight_frames: [u8; 2],
}

const CONTACT_FRAMES: [u8; 2] = [0, 4];
const FLIGHT_FRAMES: [u8; 2] = [2, 6];

const MANIFESTS: [RunnerManifest; 20] = [
    runner_manifest(RunnerCharacter::Cat, true, 42, 22, 28, 2.5),
    runner_manifest(RunnerCharacter::Dog, true, 42, 22, 28, 2.5),
    runner_manifest(RunnerCharacter::Rabbit, true, 40, 26, 28, 2.5),
    runner_manifest(RunnerCharacter::Fox, true, 44, 20, 28, 2.5),
    runner_manifest(RunnerCharacter::Penguin, false, 40, 28, 29, 2.5),
    runner_manifest(RunnerCharacter::Dinosaur, false, 42, 27, 29, 3.0),
    runner_manifest(RunnerCharacter::Robot, false, 38, 27, 29, 2.5),
    runner_manifest(RunnerCharacter::Ghost, false, 32, 28, 28, 3.0),
    runner_manifest(RunnerCharacter::Bird, false, 42, 26, 26, 2.5),
    runner_manifest(RunnerCharacter::Duck, false, 40, 22, 28, 2.5),
    runner_manifest(RunnerCharacter::Sheep, false, 40, 22, 28, 2.5),
    runner_manifest(RunnerCharacter::Fish, false, 40, 21, 25, 2.5),
    runner_manifest(RunnerCharacter::Slime, false, 36, 18, 28, 2.5),
    runner_manifest(RunnerCharacter::Human, false, 24, 29, 29, 2.5),
    runner_manifest(RunnerCharacter::Pushup, false, 40, 25, 29, 2.5),
    runner_manifest(RunnerCharacter::Orbit, false, 40, 22, 27, 2.5),
    runner_manifest(RunnerCharacter::Sushi, false, 40, 20, 28, 2.5),
    runner_manifest(RunnerCharacter::City, false, 42, 22, 28, 2.5),
    runner_manifest(RunnerCharacter::Sausage, false, 42, 23, 28, 2.5),
    runner_manifest(RunnerCharacter::Party, false, 40, 26, 28, 3.0),
];

const fn runner_manifest(
    character: RunnerCharacter,
    mirror_source: bool,
    target_width: u32,
    target_height: u32,
    baseline_y: u32,
    max_centroid_drift: f32,
) -> RunnerManifest {
    RunnerManifest {
        character,
        #[cfg(test)]
        output_direction: Direction::Right,
        mirror_source,
        target_width,
        target_height,
        baseline_y,
        max_centroid_drift,
        contact_frames: CONTACT_FRAMES,
        flight_frames: FLIGHT_FRAMES,
    }
}

pub(crate) fn manifest(character: RunnerCharacter) -> &'static RunnerManifest {
    match character {
        RunnerCharacter::Cat => &MANIFESTS[0],
        RunnerCharacter::Dog => &MANIFESTS[1],
        RunnerCharacter::Rabbit => &MANIFESTS[2],
        RunnerCharacter::Fox => &MANIFESTS[3],
        RunnerCharacter::Penguin => &MANIFESTS[4],
        RunnerCharacter::Dinosaur => &MANIFESTS[5],
        RunnerCharacter::Robot => &MANIFESTS[6],
        RunnerCharacter::Ghost => &MANIFESTS[7],
        RunnerCharacter::Bird => &MANIFESTS[8],
        RunnerCharacter::Duck => &MANIFESTS[9],
        RunnerCharacter::Sheep => &MANIFESTS[10],
        RunnerCharacter::Fish => &MANIFESTS[11],
        RunnerCharacter::Slime => &MANIFESTS[12],
        RunnerCharacter::Human => &MANIFESTS[13],
        RunnerCharacter::Pushup => &MANIFESTS[14],
        RunnerCharacter::Orbit => &MANIFESTS[15],
        RunnerCharacter::Sushi => &MANIFESTS[16],
        RunnerCharacter::City => &MANIFESTS[17],
        RunnerCharacter::Sausage => &MANIFESTS[18],
        RunnerCharacter::Party => &MANIFESTS[19],
    }
}

#[cfg(test)]
mod tests {
    use peterfan_core::config::RunnerCharacter;

    use super::*;
    use crate::runner::PIXEL_HEIGHT;

    #[test]
    fn every_character_has_a_right_facing_manifest() {
        for character in RunnerCharacter::ALL {
            let manifest = manifest(character);
            assert_eq!(manifest.character, character);
            assert_eq!(manifest.output_direction, Direction::Right);
            assert!((14..=44).contains(&manifest.target_width));
            assert!((18..=29).contains(&manifest.target_height));
            assert!(manifest.baseline_y < PIXEL_HEIGHT);
            assert!(manifest.max_centroid_drift <= 3.5);
            assert_eq!(manifest.contact_frames, [0, 4]);
            assert_eq!(manifest.flight_frames, [2, 6]);
        }
    }

    #[test]
    fn manifest_table_matches_expected_geometry_and_mirroring() {
        let expected = [
            (RunnerCharacter::Cat, true, 42, 22, 28, 2.5),
            (RunnerCharacter::Dog, true, 42, 22, 28, 2.5),
            (RunnerCharacter::Rabbit, true, 40, 26, 28, 2.5),
            (RunnerCharacter::Fox, true, 44, 20, 28, 2.5),
            (RunnerCharacter::Penguin, false, 40, 28, 29, 2.5),
            (RunnerCharacter::Dinosaur, false, 42, 27, 29, 3.0),
            (RunnerCharacter::Robot, false, 38, 27, 29, 2.5),
            (RunnerCharacter::Ghost, false, 32, 28, 28, 3.0),
            (RunnerCharacter::Bird, false, 42, 26, 26, 2.5),
            (RunnerCharacter::Duck, false, 40, 22, 28, 2.5),
            (RunnerCharacter::Sheep, false, 40, 22, 28, 2.5),
            (RunnerCharacter::Fish, false, 40, 21, 25, 2.5),
            (RunnerCharacter::Slime, false, 36, 18, 28, 2.5),
            (RunnerCharacter::Human, false, 24, 29, 29, 2.5),
            (RunnerCharacter::Pushup, false, 40, 25, 29, 2.5),
            (RunnerCharacter::Orbit, false, 40, 22, 27, 2.5),
            (RunnerCharacter::Sushi, false, 40, 20, 28, 2.5),
            (RunnerCharacter::City, false, 42, 22, 28, 2.5),
            (RunnerCharacter::Sausage, false, 42, 23, 28, 2.5),
            (RunnerCharacter::Party, false, 40, 26, 28, 3.0),
        ];

        for (character, mirror_source, width, height, baseline_y, drift) in expected {
            let manifest = manifest(character);
            assert_eq!(manifest.mirror_source, mirror_source, "{character:?}");
            assert_eq!(manifest.target_width, width, "{character:?}");
            assert_eq!(manifest.target_height, height, "{character:?}");
            assert_eq!(manifest.baseline_y, baseline_y, "{character:?}");
            assert!(
                (manifest.max_centroid_drift - drift).abs() < f32::EPSILON,
                "{character:?}"
            );
        }
    }
}
