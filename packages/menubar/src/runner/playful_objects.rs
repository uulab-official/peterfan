//! Original monochrome scenes, authored at the shared 48 × 32 source size.
//! Pose geometry is periodic; load changes articulation rather than translating
//! the complete image. The shared normalizer owns output size and baseline.
use std::f32::consts::{PI, TAU};

use peterfan_core::config::RunnerCharacter;

use super::draw::{Pt, RunnerCanvas};
use super::RgbaFrame;

const INK: (u8, u8, u8, u8) = (255, 255, 255, 255);

pub(crate) fn render(character: RunnerCharacter, cpu_pct: f32, frame: u8) -> RgbaFrame {
    let mut canvas = RunnerCanvas::new();
    let phase = f32::from(frame % 8) * TAU / 8.0;
    let load = if cpu_pct.is_finite() {
        cpu_pct.clamp(0.0, 100.0) / 100.0
    } else {
        0.0
    };
    match character {
        RunnerCharacter::Orbit => orbit(&mut canvas, phase, load),
        RunnerCharacter::Sushi => sushi(&mut canvas, phase, load),
        RunnerCharacter::City => city(&mut canvas, frame % 8, load),
        RunnerCharacter::Sausage => sausage(&mut canvas, phase, load),
        RunnerCharacter::Party => party(&mut canvas, phase, load),
        _ => unreachable!("character is not a playful object"),
    }
    canvas.finish()
}

fn path(canvas: &mut RunnerCanvas, points: &[Pt], width: f32) {
    for points in points.windows(2) {
        canvas.line(points[0], points[1], width, INK);
    }
}

fn orbit(canvas: &mut RunnerCanvas, phase: f32, load: f32) {
    // A tilted orbital plane, with two balanced moons. The inner ring is
    // erased before drawing the globe, so the globe cleanly occludes it.
    let ring = |angle: f32| {
        Pt::new(
            24.0 + 17.0 * angle.cos(),
            16.0 + (6.0 + load * 2.5) * angle.sin() + (2.0 + load * 1.2) * angle.cos(),
        )
    };
    let points: Vec<_> = (0..=48).map(|i| ring(i as f32 * TAU / 48.0)).collect();
    path(canvas, &points, 2.0);
    let moons = [(phase + 0.2, 3.4), (phase + PI + 0.2, 2.9)];
    for (angle, radius) in moons {
        if angle.sin() < 0.0 {
            canvas.disc(ring(angle), radius, INK);
        }
    }
    canvas.erase_disc(Pt::new(24.0, 16.0), 9.2);
    canvas.disc(Pt::new(24.0, 16.0), 7.6 + load * 0.5, INK);
    canvas.erase_disc(Pt::new(27.0, 13.4), 4.1);
    // Redraw only the foreground arc: Saturn's ring passes over its globe.
    let front: Vec<_> = (0..=24).map(|i| ring(i as f32 * PI / 24.0)).collect();
    path(canvas, &front, 2.0);
    for (angle, radius) in moons {
        if angle.sin() >= 0.0 {
            let moon = ring(angle);
            canvas.erase_disc(moon, radius + 1.1);
            canvas.disc(moon, radius + load * 0.25, INK);
        }
    }
}

// Solve the knee from fixed thigh/shin lengths. Feet travel backwards during
// stance and fold up during recovery, rather than stretching the whole leg.
fn knee_between(hip: Pt, foot: Pt, length: f32, bend: f32) -> Pt {
    let dx = foot.x - hip.x;
    let dy = foot.y - hip.y;
    let distance = dx.hypot(dy).max(0.01);
    let height = (length * length - distance * distance * 0.25)
        .max(0.0)
        .sqrt();
    Pt::new(
        (hip.x + foot.x) * 0.5 + bend * dy / distance * height,
        (hip.y + foot.y) * 0.5 - bend * dx / distance * height,
    )
}

fn jog_leg(canvas: &mut RunnerCanvas, hip: Pt, phase: f32, reach: f32) {
    let lift = phase.sin().max(0.0);
    let foot = Pt::new(hip.x + phase.cos() * reach, 28.0 - lift * 5.2);
    let knee = knee_between(hip, foot, 6.7, 1.0);
    path(canvas, &[hip, knee, foot], 2.8);
    canvas.line(foot, Pt::new(foot.x + 2.6, foot.y - lift * 0.7), 2.8, INK);
}

fn sushi(canvas: &mut RunnerCanvas, phase: f32, load: f32) {
    let bob = -0.85 * (phase * 2.0).sin().powi(2);
    let center = Pt::new(24.0, 17.5 + bob);
    let reach = 5.0 + load * 2.2;
    jog_leg(canvas, Pt::new(20.0, 21.0 + bob), phase + PI, reach);
    jog_leg(canvas, Pt::new(28.0, 21.0 + bob), phase, reach);
    // Bent arms emerge from the rice, keeping the topping silhouette legible.
    for (side, angle) in [(-1.0, phase), (1.0, phase + PI)] {
        path(
            canvas,
            &[
                Pt::new(24.0 + side * 10.0, 17.5 + bob),
                Pt::new(24.0 + side * 14.5, 18.0 + angle.sin() * (1.7 + load)),
                Pt::new(24.0 + side * 16.0, 15.0 + angle.sin() * (2.0 + load)),
            ],
            2.4,
        );
    }
    canvas.ellipse(center, 12.5, 5.5, INK);
    // Nigiri: broad fish cap, separated rice seam, and diagonal fish cuts.
    canvas.line(
        Pt::new(11.5, 13.8 + bob),
        Pt::new(36.0, 13.8 + bob),
        7.0,
        INK,
    );
    canvas.erase_rect(15.0, 16.0 + bob, 33.0, 17.3 + bob);
    for x in [17.0, 24.0, 31.0] {
        canvas.erase_rect(x, 11.5 + bob, x + 1.6, 14.7 + bob);
    }
    // One eye on the forward edge of the rice.
    canvas.erase_disc(Pt::new(32.0, 19.0 + bob), 1.1);
}

fn city(canvas: &mut RunnerCanvas, frame: u8, load: f32) {
    // A tiny wheeled city: architecture stays rigid while the crank turns.
    // A stepped roof and one spire read at menu-bar size; large window openings
    // are constant, avoiding the old flashing checkerboard texture.
    canvas.rect(4.5, 13.0, 18.0, 23.0, INK);
    canvas.rect(18.0, 7.0, 28.0, 23.0, INK);
    canvas.rect(28.0, 11.0, 43.0, 23.0, INK);
    canvas.rect(20.0, 4.0, 26.0, 8.0, INK);
    canvas.line(Pt::new(23.0, 4.0), Pt::new(23.0, 2.4), 1.8, INK);
    for (x, y) in [
        (8.0, 16.0),
        (21.0, 10.0),
        (21.0, 16.0),
        (34.0, 14.0),
        (34.0, 19.0),
    ] {
        canvas.erase_rect(x, y, x + 4.0, y + 2.7);
    }
    canvas.line(Pt::new(4.0, 23.5), Pt::new(44.0, 23.5), 2.8, INK);
    let phase = f32::from(frame) * TAU / 8.0;
    canvas.triangle(
        Pt::new(23.0, 4.0),
        Pt::new(
            35.0 + phase.cos() * (1.0 + load * 4.0),
            6.9 + phase.sin() * (1.2 + load * 3.0),
        ),
        Pt::new(24.5, 8.0),
        INK,
    );
    for x in [12.0, 36.0] {
        let hub = Pt::new(x, 26.0);
        canvas.disc(hub, 4.1, INK);
        canvas.erase_disc(
            Pt::new(x + phase.cos() * 1.6, 26.0 + phase.sin() * 1.6),
            2.4,
        );
        let crank = Pt::new(x + phase.cos() * 2.0, 26.0 + phase.sin() * 2.0);
        canvas.line(hub, crank, 2.7, INK);
    }
    // The locomotive's connected piston makes every eighth turn distinct.
    let crank = Pt::new(12.0 + phase.cos() * 2.0, 26.0 + phase.sin() * 2.0);
    let slide = Pt::new(23.0 + phase.cos() * (1.5 + load), 25.0);
    canvas.line(crank, slide, 2.8, INK);
    canvas.line(
        slide,
        Pt::new(36.0 + phase.cos() * 2.0, 26.0 + phase.sin() * 2.0),
        1.8,
        INK,
    );
}

fn sausage(canvas: &mut RunnerCanvas, phase: f32, load: f32) {
    let bob = -0.9 * (2.0 * phase).sin().powi(2);
    let reach = 5.0 + load * 2.5;
    jog_leg(canvas, Pt::new(18.5, 20.0 + bob), phase + PI, reach);
    jog_leg(canvas, Pt::new(28.0, 20.0 + bob), phase, reach);
    let wag = phase.sin() * (1.0 + load);
    path(
        canvas,
        &[
            Pt::new(11.0, 15.5 + bob),
            Pt::new(7.0, 12.0 + wag),
            Pt::new(5.0, 15.5 + wag),
        ],
        2.8,
    );
    // Long capsule body with pinched casing ends instead of a generic oval.
    canvas.line(
        Pt::new(14.0, 16.0 + bob),
        Pt::new(34.0, 16.0 + bob),
        11.2,
        INK,
    );
    canvas.triangle(
        Pt::new(8.5, 16.0 + bob),
        Pt::new(5.0, 13.5 + bob),
        Pt::new(5.0, 18.5 + bob),
        INK,
    );
    canvas.triangle(
        Pt::new(39.0, 16.0 + bob),
        Pt::new(43.0, 13.5 + bob),
        Pt::new(43.0, 18.5 + bob),
        INK,
    );
    // Two generous grill slits and an eye distinguish the sausage at 27×18.
    for x in [18.0, 25.0] {
        canvas.erase_rect(x, 12.8 + bob, x + 1.8, 17.2 + bob);
    }
    canvas.erase_disc(Pt::new(34.0, 14.9 + bob), 1.1);
    path(
        canvas,
        &[
            Pt::new(32.0, 19.0 + bob),
            Pt::new(36.0, 21.0 + phase.cos() * 1.8),
            Pt::new(39.0, 18.0 + phase.cos() * (2.0 + load)),
        ],
        2.6,
    );
}

// Keep the authored skeleton available separately from rasterization: support
// contacts and limb lengths are properties of the dance, not of antialiasing.
struct PartyPose {
    neck: Pt,
    hip: Pt,
    shoulders: [Pt; 2],
    elbows: [Pt; 2],
    hands: [Pt; 2],
    leg_hips: [Pt; 2],
    knees: [Pt; 2],
    ankles: [Pt; 2],
}

fn party_pose(phase: f32, load: f32) -> PartyPose {
    let beat = phase.sin();
    let accent = phase.cos();
    let energy = 0.65 + load * 0.35;
    // Counts 1–4 load the left foot; counts 5–8 load the right. The
    // supporting ankle stays fixed while the other foot lifts into a side tap.
    // Both feet touch at the transfer, including the 7 -> 0 cycle seam.
    let bounce = 0.55 * (phase * 2.0).cos();
    let hip = Pt::new(24.0 - 2.4 * beat * energy, 19.6 + bounce);
    let neck = Pt::new(24.0 - 0.8 * beat * energy + 0.5 * accent, 11.5 + bounce);
    let shoulders = [
        Pt::new(neck.x - 2.1, neck.y + 1.1),
        Pt::new(neck.x + 2.1, neck.y + 1.1),
    ];
    // A bent low hand and an overhead disco point are deliberately offset.
    // They never become the mirrored jumping-jack pose of the old cycle.
    let hands = [
        Pt::new(
            neck.x - 6.8 - 1.3 * beat - 2.8 * load,
            15.2 + 2.3 * beat + 1.3 * accent,
        ),
        Pt::new(
            neck.x + 7.3 + 0.9 * accent + 2.8 * load,
            7.5 - 3.8 * beat * energy,
        ),
    ];
    let elbows = std::array::from_fn(|i| {
        knee_between(shoulders[i], hands[i], 6.0, if i == 0 { -1.0 } else { 1.0 })
    });
    let leg_hips = [Pt::new(hip.x - 1.1, hip.y), Pt::new(hip.x + 1.1, hip.y)];
    let ankles = [
        Pt::new(
            18.0 - 1.8 * (-beat).max(0.0),
            28.0 - 3.4 * (-beat).max(0.0) * energy,
        ),
        Pt::new(
            30.0 + 1.8 * beat.max(0.0),
            28.0 - 3.4 * beat.max(0.0) * energy,
        ),
    ];
    let knees = std::array::from_fn(|i| {
        knee_between(leg_hips[i], ankles[i], 7.0, if i == 0 { -1.0 } else { 1.0 })
    });
    PartyPose {
        neck,
        hip,
        shoulders,
        elbows,
        hands,
        leg_hips,
        knees,
        ankles,
    }
}

fn party(canvas: &mut RunnerCanvas, phase: f32, load: f32) {
    let pose = party_pose(phase, load);
    for i in 0..2 {
        path(
            canvas,
            &[pose.shoulders[i], pose.elbows[i], pose.hands[i]],
            2.9,
        );
        canvas.disc(pose.hands[i], 1.5, INK);
        path(
            canvas,
            &[pose.leg_hips[i], pose.knees[i], pose.ankles[i]],
            3.0,
        );
        let side = if i == 0 { -1.0 } else { 1.0 };
        canvas.line(
            pose.ankles[i],
            Pt::new(pose.ankles[i].x + side * 2.1, pose.ankles[i].y),
            3.0,
            INK,
        );
    }
    canvas.line(pose.neck, pose.hip, 4.8, INK);
    let head = Pt::new(pose.neck.x + 0.35 * phase.sin(), pose.neck.y - 4.8);
    canvas.line(pose.neck, head, 2.8, INK);
    canvas.disc(head, 3.3, INK);
    canvas.triangle(
        Pt::new(head.x - 2.6, head.y - 2.3),
        Pt::new(head.x + 2.6, head.y - 2.3),
        Pt::new(head.x - 1.7, head.y - 4.6),
        INK,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::{manifest, normalize_cycle, validate_cycle};

    const CHARACTERS: [RunnerCharacter; 5] = [
        RunnerCharacter::Orbit,
        RunnerCharacter::Sushi,
        RunnerCharacter::City,
        RunnerCharacter::Sausage,
        RunnerCharacter::Party,
    ];

    fn distance(a: Pt, b: Pt) -> f32 {
        (a.x - b.x).hypot(a.y - b.y)
    }

    fn pose_points(pose: &PartyPose) -> Vec<Pt> {
        let mut points = vec![pose.hip, pose.neck];
        for i in 0..2 {
            points.extend([
                pose.shoulders[i],
                pose.elbows[i],
                pose.hands[i],
                pose.leg_hips[i],
                pose.knees[i],
                pose.ankles[i],
            ]);
        }
        points
    }

    #[test]
    fn party_keeps_support_foot_planted_and_transfers_weight() {
        for load in [0.0, 0.3, 0.65, 1.0] {
            for sample in 0..128 {
                let phase = sample as f32 * TAU / 128.0;
                let pose = party_pose(phase, load);
                let support = usize::from(phase.sin() < 0.0);
                let planted = pose.ankles[support];
                assert!((planted.x - if support == 0 { 18.0 } else { 30.0 }).abs() < 0.001);
                assert!((planted.y - 28.0).abs() < 0.001);
                assert!(pose.ankles[1 - support].y <= 28.001);
            }
            let left = party_pose(PI * 0.5, load);
            let right = party_pose(PI * 1.5, load);
            assert!(
                right.hip.x - left.hip.x >= 3.1,
                "dance must visibly transfer its weight"
            );
            assert!(
                left.neck.x > left.hip.x && right.neck.x < right.hip.x,
                "torso counterbalances the loaded hip"
            );
            assert!(left.ankles[1].y < 26.0 && right.ankles[0].y < 26.0);
        }
    }

    #[test]
    fn party_preserves_limb_lengths_and_asymmetric_hand_accents() {
        for load in [0.0, 0.5, 1.0] {
            for sample in 0..128 {
                let pose = party_pose(sample as f32 * TAU / 128.0, load);
                for i in 0..2 {
                    for length in [
                        distance(pose.shoulders[i], pose.elbows[i]),
                        distance(pose.elbows[i], pose.hands[i]),
                    ] {
                        assert!((length - 6.0).abs() < 0.001, "arm must not stretch");
                    }
                    for length in [
                        distance(pose.leg_hips[i], pose.knees[i]),
                        distance(pose.knees[i], pose.ankles[i]),
                    ] {
                        assert!((length - 7.0).abs() < 0.001, "leg must not stretch");
                    }
                }
                assert!(
                    pose.hands[0].y - pose.hands[1].y > 1.0,
                    "hands must retain distinct high and low accents"
                );
            }
        }
    }

    #[test]
    fn party_wraps_without_a_pose_jump() {
        for load in [0.0, 0.5, 1.0] {
            let start = pose_points(&party_pose(0.0, load));
            let wrap = pose_points(&party_pose(TAU, load));
            assert!(start
                .iter()
                .zip(&wrap)
                .all(|(&a, &b)| distance(a, b) < 0.001));
            let step = TAU / 8.0;
            let deltas: Vec<_> = (0..8)
                .map(|i| {
                    let from = pose_points(&party_pose(i as f32 * step, load));
                    let to = pose_points(&party_pose((i + 1) as f32 * step, load));
                    from.iter()
                        .zip(&to)
                        .map(|(&a, &b)| distance(a, b))
                        .fold(0.0_f32, f32::max)
                })
                .collect();
            let ordinary = deltas[..7].iter().copied().fold(0.0_f32, f32::max);
            assert!(
                deltas[7] <= ordinary * 1.05,
                "last-to-first step must not exceed the ordinary movement envelope"
            );
        }
    }

    #[test]
    fn dancer_and_jogger_joint_solver_preserves_both_segments() {
        for hip in [Pt::new(20.0, 20.0), Pt::new(26.0, 19.0)] {
            for pose in 0..8 {
                let phase = pose as f32 * TAU / 8.0;
                let foot = Pt::new(hip.x + phase.cos() * 7.2, 28.0 - phase.sin().max(0.0) * 5.2);
                for bend in [-1.0, 1.0] {
                    let knee = knee_between(hip, foot, 6.7, bend);
                    assert!(((knee.x - hip.x).hypot(knee.y - hip.y) - 6.7).abs() < 0.001);
                    assert!(((knee.x - foot.x).hypot(knee.y - foot.y) - 6.7).abs() < 0.001);
                }
            }
        }
    }

    #[test]
    fn playful_cycles_have_eight_distinct_unclipped_normalized_poses() {
        for character in CHARACTERS {
            for cpu in [0.0, 30.0, 65.0, 90.0, 100.0] {
                let source = std::array::from_fn(|pose| render(character, cpu, pose as u8));
                let cycle = normalize_cycle(manifest(character), source).unwrap();
                let metrics = validate_cycle(manifest(character), &cycle)
                    .unwrap_or_else(|error| panic!("{character:?} {cpu}: {error}"));
                for first in 0..8 {
                    for second in first + 1..8 {
                        assert!(
                            cycle[first] != cycle[second],
                            "{character:?} {cpu} poses {first}/{second}"
                        );
                    }
                }
                if character == RunnerCharacter::City {
                    assert!(
                        metrics
                            .frames
                            .iter()
                            .all(|frame| frame.bounds.width() >= 30),
                        "city carriage must fill the optical slot at {cpu}%"
                    );
                }
                assert!(metrics.minimum_frame_delta >= 24);
            }
        }
    }
}
