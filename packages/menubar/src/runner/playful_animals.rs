//! Original, silhouette-first eight-pose animals. Coordinates describe anatomy;
//! the common cycle normalizer alone controls final scale and placement.
use peterfan_core::config::RunnerCharacter;

use super::draw::{Pt, RunnerCanvas};
use super::RgbaFrame;

const INK: (u8, u8, u8, u8) = (255, 255, 255, 255);
const FAR: (u8, u8, u8, u8) = (255, 255, 255, 210);
const SINE: [f32; 8] = [0.0, 0.707, 1.0, 0.707, 0.0, -0.707, -1.0, -0.707];
const COSINE: [f32; 8] = [1.0, 0.707, 0.0, -0.707, -1.0, -0.707, 0.0, 0.707];

pub(crate) fn render(character: RunnerCharacter, cpu_pct: f32, frame: u8) -> RgbaFrame {
    let pose = usize::from(frame % 8);
    let load = if cpu_pct.is_finite() {
        cpu_pct.clamp(0.0, 100.0) / 100.0
    } else {
        0.0
    };
    let amplitude = 0.86 + load * 0.32;
    let mut c = RunnerCanvas::new();
    match character {
        RunnerCharacter::Bird => bird(&mut c, pose, amplitude),
        RunnerCharacter::Duck => duck(&mut c, pose, amplitude),
        RunnerCharacter::Sheep => sheep(&mut c, pose, amplitude),
        RunnerCharacter::Fish => fish(&mut c, pose, amplitude),
        RunnerCharacter::Slime => slime(&mut c, pose, amplitude),
        _ => unreachable!("non-playful character passed to animal renderer"),
    }
    c.finish()
}

fn bird(c: &mut RunnerCanvas, p: usize, a: f32) {
    let s = SINE[p];
    let k = COSINE[p];
    let bob = -s.abs() * 0.6 * a;
    let shoulder = Pt::new(23.0, 16.5 + bob);
    // The elbow leads the wrist through the beat. A broad folded fan remains
    // connected at both joints; three primary tips read as a wing, not a stick.
    let elbow = Pt::new(19.0 + k * 2.0, 16.0 - s * 5.5 * a);
    let wrist = Pt::new(12.0 + k * 3.0, 15.0 - s * 9.0 * a);
    c.triangle(shoulder, elbow, Pt::new(28.0, 18.0 + bob), FAR);
    c.triangle(elbow, wrist, Pt::new(25.0, 19.0 + bob), FAR);
    c.triangle(
        Pt::new(16.0, 18.0 + bob),
        Pt::new(5.5, 15.0 + bob),
        Pt::new(8.0, 21.0 + bob),
        INK,
    );
    c.ellipse(Pt::new(25.0, 18.0 + bob), 10.3, 4.6, INK);
    c.disc(Pt::new(35.0, 14.6 + bob), 4.1, INK);
    c.triangle(
        Pt::new(38.0, 13.8 + bob),
        Pt::new(43.0, 15.2 + bob),
        Pt::new(38.0, 16.6 + bob),
        INK,
    );
    let near_elbow = Pt::new(22.0 + k * 2.5, 17.0 - s * 5.3 * a);
    let near_wrist = Pt::new(17.0 + k * 4.0, 16.5 - s * 6.9 * a);
    c.triangle(shoulder, near_elbow, Pt::new(29.0, 19.0 + bob), INK);
    c.triangle(near_elbow, near_wrist, Pt::new(28.0, 19.0 + bob), INK);
    for finger in 0..3 {
        let f = finger as f32;
        c.line(
            near_wrist,
            Pt::new(
                near_wrist.x - 4.0 + f * 2.1,
                near_wrist.y - s.signum() * (2.7 - f * 0.5),
            ),
            2.4,
            INK,
        );
    }
    // Feet tuck against the belly instead of hanging like a third wing.
    c.line(
        Pt::new(27.0, 21.0 + bob),
        Pt::new(23.0 + k, 23.0 + bob),
        1.8,
        INK,
    );
    c.line(
        Pt::new(23.0 + k, 23.0 + bob),
        Pt::new(28.0, 23.0 + bob),
        1.8,
        INK,
    );
    c.erase_disc(Pt::new(36.1, 13.9 + bob), 1.1);
}

// A backward-bending knee keeps the leg continuous while the foot plants.
fn leg_joint(hip: Pt, foot: Pt, length: f32) -> Pt {
    let dx = foot.x - hip.x;
    let dy = foot.y - hip.y;
    let d = dx.hypot(dy).max(0.01);
    let h = (length * length - d * d * 0.25).max(0.0).sqrt();
    Pt::new(
        (hip.x + foot.x) * 0.5 - dy / d * h,
        (hip.y + foot.y) * 0.5 + dx / d * h,
    )
}

fn duck(c: &mut RunnerCanvas, p: usize, a: f32) {
    let s = SINE[p];
    let k = COSINE[p];
    let bob = -s.abs() * 1.5 * a;
    // Webbed feet alternate reach, flat contact, push-off, and tucked recovery.
    for (hip, phase, color) in [(20.0, (p + 4) % 8, FAR), (28.0, p, INK)] {
        let foot_x = hip + COSINE[phase] * 6.0 * a;
        let foot_y = 27.0 - SINE[phase].max(0.0) * 3.8 * a;
        let root = Pt::new(hip, 21.0 + bob);
        let ankle = Pt::new(foot_x - 1.0, foot_y - 1.0);
        let knee = leg_joint(root, ankle, 5.2);
        c.line(root, knee, 2.6, color);
        c.line(knee, ankle, 2.6, color);
        c.triangle(
            Pt::new(foot_x - 3.0, foot_y),
            Pt::new(foot_x + 4.0, foot_y),
            Pt::new(foot_x, foot_y - 2.0),
            color,
        );
    }
    c.triangle(
        Pt::new(14.0, 19.0 + bob),
        Pt::new(5.5, 13.5 + bob - k * a),
        Pt::new(9.0, 22.0 + bob),
        INK,
    );
    c.ellipse(Pt::new(23.0, 19.0 + bob), 12.7, 6.1, INK);
    c.ellipse(Pt::new(32.0 + s * 0.25 * a, 14.0 + bob), 4.3, 6.5, INK);
    c.disc(Pt::new(33.3 + s * 0.25 * a, 10.5 + bob), 4.6, INK);
    c.ellipse(Pt::new(39.3 + s * 0.25 * a, 12.0 + bob), 4.5, 1.8, INK);
    // One broad wing crease survives downsampling; no isolated motion marks.
    c.erase_disc(Pt::new(22.8 - k * a, 18.0 + bob), 3.5);
    c.ellipse(Pt::new(23.4 - k * a, 16.9 + bob), 4.4, 2.5, INK);
    c.erase_disc(Pt::new(35.0 + s * 0.25 * a, 9.6 + bob), 1.0);
}

fn sheep(c: &mut RunnerCanvas, p: usize, a: f32) {
    let s = SINE[p];
    let k = COSINE[p];
    let bob = -s.abs() * 2.0 * a;
    for (hip, phase, color) in [
        (15.0, (p + 2) % 8, FAR),
        (30.0, (p + 6) % 8, FAR),
        (17.0, p, INK),
        (29.0, (p + 4) % 8, INK),
    ] {
        let stretch = COSINE[phase] * 5.5 * a;
        let hoof = Pt::new(hip + stretch, 27.0 - SINE[phase].abs() * 2.0 * a);
        let knee = leg_joint(Pt::new(hip, 19.0 + bob), hoof, 6.0);
        c.line(Pt::new(hip, 19.0 + bob), knee, 2.8, color);
        c.line(knee, hoof, 2.5, color);
        c.line(hoof, Pt::new(hoof.x + 2.3, hoof.y), 2.8, color);
    }
    c.line(
        Pt::new(12.0, 16.0 + bob),
        Pt::new(6.3, 12.0 + bob + k * 0.9 * a),
        4.2,
        INK,
    );
    c.ellipse(Pt::new(23.0, 16.8 + bob), 12.8, 6.0, INK);
    // Large overlapping scallops form a continuous fleece, not floating dots.
    for (x, y, r) in [
        (12.0, 15.0, 4.0),
        (16.0, 11.7, 3.8),
        (23.0, 10.7, 4.0),
        (29.5, 12.0, 3.7),
        (32.0, 17.0, 4.0),
        (26.0, 20.0, 4.2),
        (18.0, 20.0, 4.1),
    ] {
        c.disc(Pt::new(x, y + bob), r, INK);
    }
    c.ellipse(Pt::new(36.0, 15.0 + bob + k * 0.35 * a), 4.6, 5.5, INK);
    c.ellipse(Pt::new(39.0, 17.5 + bob), 3.7, 2.7, INK);
    c.line(
        Pt::new(34.0, 12.0 + bob),
        Pt::new(30.5, 9.3 + bob - s * 0.5),
        3.3,
        INK,
    );
    c.erase_disc(Pt::new(37.4, 13.6 + bob), 1.0);
}

fn fish(c: &mut RunnerCanvas, p: usize, a: f32) {
    let s = SINE[p];
    let k = COSINE[p];
    let bob = s * 0.8 * a;
    let tail_y = 16.0 + s * 3.8 * a;
    // The narrow caudal peduncle bends first; the broad tail follows a quarter
    // beat later. Both triangular lobes remain joined to the body.
    let joint = Pt::new(15.0 + k * 0.8 * a, 16.0 + s * 1.2 * a);
    c.line(joint, Pt::new(20.0, 16.0 + bob), 5.0, INK);
    c.triangle(
        joint,
        Pt::new(5.5 + k * 1.8 * a, tail_y - 6.0),
        Pt::new(7.0 - k * a, tail_y + 6.0),
        INK,
    );
    c.triangle(
        Pt::new(22.0, 13.0 + bob),
        Pt::new(25.0 + k * 1.5 * a, 6.5 + bob),
        Pt::new(32.0, 12.0 + bob),
        INK,
    );
    c.triangle(
        Pt::new(25.0, 20.0 + bob),
        Pt::new(29.0 - k * 1.5 * a, 24.0),
        Pt::new(34.0, 19.0 + bob),
        INK,
    );
    c.ellipse(Pt::new(28.0, 16.0 + bob), 12.5, 7.0, INK);
    c.ellipse(Pt::new(37.0, 16.0 + bob), 5.7, 4.8, INK);
    c.erase_disc(Pt::new(37.7, 14.2 + bob), 1.1);
    // A curved gill and articulated pectoral fin give the swimming body a face.
    c.erase_disc(Pt::new(32.7, 16.3 + bob), 3.3);
    c.ellipse(Pt::new(31.8, 15.6 + bob), 2.8, 4.2, INK);
    c.triangle(
        Pt::new(26.0, 17.0 + bob),
        Pt::new(22.0 + k * 2.5 * a, 24.0 + bob),
        Pt::new(30.0, 18.5 + bob),
        INK,
    );
}

fn slime(c: &mut RunnerCanvas, p: usize, a: f32) {
    // Compression stores energy before the upright take-off; the broad belly
    // becomes a taller dome while its area remains approximately constant.
    const HEIGHT: [f32; 8] = [7.3, 9.0, 10.2, 8.9, 7.3, 6.1, 5.5, 6.0];
    const LIFT: [f32; 8] = [0.0, -0.6, -1.6, -1.0, 0.0, 0.0, 0.0, 0.0];
    let ry = 7.3 + (HEIGHT[p] - 7.3) * a;
    let rx = 105.0 / ry;
    let bottom = 27.0 + LIFT[p] * a;
    let lean = COSINE[p] * 0.9 * a;
    let center = Pt::new(24.0 + lean, bottom - ry);
    c.ellipse(center, rx, ry, INK);
    // Two attached soft lobes, slightly delayed relative to the dome.
    c.ellipse(
        Pt::new(24.0 - rx * 0.62 + lean * 0.3, bottom - 2.1),
        rx * 0.34,
        2.2,
        INK,
    );
    c.ellipse(
        Pt::new(24.0 + rx * 0.62 + lean * 0.3, bottom - 2.1),
        rx * 0.34,
        2.2,
        INK,
    );
    let face_y = center.y - 0.3;
    c.erase_disc(Pt::new(25.0 + lean, face_y), 1.15);
    c.erase_disc(Pt::new(32.0 + lean, face_y), 1.15);
    c.erase_disc(Pt::new(28.5 + lean, face_y + 2.5), 2.2);
    c.ellipse(Pt::new(28.5 + lean, face_y + 1.5), 2.7, 1.6, INK);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::{manifest, measure_cycle, normalize_cycle, validate_cycle};

    const ANIMALS: [RunnerCharacter; 5] = [
        RunnerCharacter::Bird,
        RunnerCharacter::Duck,
        RunnerCharacter::Sheep,
        RunnerCharacter::Fish,
        RunnerCharacter::Slime,
    ];

    #[test]
    fn animals_pass_shared_geometry_and_motion_gates_at_every_load() {
        for animal in ANIMALS {
            for load in [0.0, 30.0, 65.0, 90.0, 100.0] {
                let source = std::array::from_fn(|p| render(animal, load, p as u8));
                let cycle = normalize_cycle(manifest(animal), source).unwrap();
                let metrics = validate_cycle(manifest(animal), &cycle)
                    .unwrap_or_else(|e| panic!("{animal:?} {load}%: {e}"));
                assert!(
                    metrics
                        .frames
                        .iter()
                        .all(|f| f.opaque_pixels >= 150 && f.opaque_pixels <= 520),
                    "{animal:?} {load}% occupancy: {:?}",
                    metrics
                        .frames
                        .iter()
                        .map(|f| f.opaque_pixels)
                        .collect::<Vec<_>>()
                );
                for i in 0..8 {
                    for j in i + 1..8 {
                        assert_ne!(cycle[i], cycle[j], "{animal:?}: duplicate poses {i}/{j}");
                    }
                }
            }
        }
    }

    #[test]
    fn adjacent_load_bands_change_visible_anatomy_without_replacing_it() {
        for animal in ANIMALS {
            let bands: Vec<_> = [0.0, 30.0, 65.0, 90.0]
                .into_iter()
                .map(|load| {
                    normalize_cycle(
                        manifest(animal),
                        std::array::from_fn(|p| render(animal, load, p as u8)),
                    )
                    .unwrap()
                })
                .collect();
            for pair in bands.windows(2) {
                let deltas: Vec<_> = (0..8)
                    .map(|p| {
                        pair[0][p]
                            .as_chunks::<4>()
                            .0
                            .iter()
                            .zip(pair[1][p].as_chunks::<4>().0.iter())
                            .filter(|(l, r)| l[3].abs_diff(r[3]) > 48 && l[3].max(r[3]) > 96)
                            .count()
                    })
                    .collect();
                let total: usize = deltas.iter().sum();
                assert!(
                    (16..=3000).contains(&total),
                    "{animal:?}: cross-band delta {total}"
                );
                assert!(
                    deltas.iter().filter(|&&d| d > 0).count() >= 2,
                    "{animal:?}: static load bands"
                );
                assert!(
                    deltas.iter().all(|&d| d <= 400),
                    "{animal:?}: excessive band deformation"
                );
            }
        }
    }

    #[test]
    fn animals_have_clear_source_margins_and_stable_body_volume() {
        for animal in ANIMALS {
            let cycle = std::array::from_fn(|p| render(animal, 100.0, p as u8));
            let metrics = measure_cycle(&cycle).unwrap();
            assert!(
                metrics.frames.iter().all(|m| !m.clipped),
                "{animal:?}: cropped anatomy"
            );
            let smallest = metrics
                .frames
                .iter()
                .map(|m| m.opaque_pixels)
                .min()
                .unwrap();
            let largest = metrics
                .frames
                .iter()
                .map(|m| m.opaque_pixels)
                .max()
                .unwrap();
            assert!(
                largest as f32 / (smallest as f32) < 1.65,
                "{animal:?}: disappearing body volume"
            );
        }
    }
}
