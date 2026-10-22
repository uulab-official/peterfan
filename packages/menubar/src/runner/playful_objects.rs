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
            16.0 + 6.5 * angle.sin() + 2.5 * angle.cos(),
        )
    };
    let points: Vec<_> = (0..=48).map(|i| ring(i as f32 * TAU / 48.0)).collect();
    path(canvas, &points, 1.7);
    canvas.disc(Pt::new(24.0, 16.0), 7.0 + load * 1.5, INK);
    // A single generous crescent survives the actual menu-bar downsample.
    canvas.erase_disc(Pt::new(26.6, 13.0), 3.4 + load * 1.1);
    for (angle, radius) in [(phase + 0.2, 3.5), (phase + PI + 0.2, 2.6)] {
        let moon = ring(angle);
        canvas.disc(moon, radius + load * 0.5, INK);
    }
}

// Alternating reach / plant / fold / recovery with a low, double-frequency bob.
// The two sides share the cycle but have a half-cycle offset.
fn jog_leg(canvas: &mut RunnerCanvas, hip: Pt, phase: f32, reach: f32) {
    let swing = phase.cos();
    let lift = phase.sin().max(0.0);
    let knee = Pt::new(hip.x + swing * reach * 0.45, hip.y + 3.0 - lift);
    let foot = Pt::new(hip.x + swing * reach, 28.0 - lift * 4.0);
    path(
        canvas,
        &[hip, knee, foot, Pt::new(foot.x + 2.6, foot.y)],
        2.5,
    );
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
    canvas.erase_rect(13.0, 16.0 + bob, 35.0, 17.3 + bob);
    for x in [17.0, 24.0, 31.0] {
        canvas.erase_rect(x, 11.5 + bob, x + 1.5 + load * 0.5, 14.7 + bob);
    }
    // One eye on the forward edge of the rice.
    canvas.erase_disc(Pt::new(32.0, 19.0 + bob), 1.1);
}

fn city(canvas: &mut RunnerCanvas, frame: u8, load: f32) {
    // Buildings and street remain perfectly still. Only windows and vehicles
    // animate, so the scene reads as a city rather than a bouncing block.
    for (left, top, right) in [(4.0, 12.0, 15.0), (18.0, 5.0, 29.0), (32.0, 10.0, 43.0)] {
        canvas.rect(left, top, right, 23.0, INK);
    }
    canvas.line(Pt::new(23.0, 5.0), Pt::new(23.0, 2.5), 1.6, INK);
    // Scrolling bands of dark windows suggest offices switching lights.
    for building in 0..3 {
        let left = [4.0, 18.0, 32.0][building];
        let top = [12.0, 6.0, 10.0][building];
        for row in 0..3 {
            for col in 0..2 {
                let clock = (usize::from(frame) + building + row) % 4;
                if (clock < 2) == (col == 0) {
                    let x = left + 2.0 + col as f32 * 4.5;
                    let y = top + 2.0 + row as f32 * 4.2;
                    if y + 2.2 < 22.5 {
                        canvas.erase_rect(x, y, x + 2.1 + load * 2.5, y + 2.4 + load * 0.5);
                    }
                }
            }
        }
    }
    canvas.line(Pt::new(3.5, 29.0), Pt::new(44.0, 29.0), 1.5, INK);
    // Two evenly spaced cars wrap around the same lane. Both endpoints are
    // fully within the canvas; no vehicle is clipped when the cycle loops.
    for offset in [0_u8, 4] {
        let x = 7.0 + f32::from((frame + offset) % 8) * 4.6;
        let van = if offset == 4 { 0.9 } else { 0.0 };
        canvas.line(Pt::new(x - 2.8, 26.5), Pt::new(x + 2.8, 26.5), 3.0, INK);
        canvas.rect(x - 1.6, 23.5 - load * 1.4 - van, x + 1.8, 26.0, INK);
        canvas.disc(Pt::new(x - 1.9, 28.0), 1.0, INK);
        canvas.disc(Pt::new(x + 1.9, 28.0), 1.0, INK);
    }
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
        canvas.erase_rect(x, 12.8 + bob, x + 1.6 + load * 0.6, 17.2 + bob);
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

fn party(canvas: &mut RunnerCanvas, phase: f32, load: f32) {
    // A single dancer, with a continuous shoulder roll and alternating kicks.
    let sway = phase.sin() * (1.0 + load * 0.8);
    let bob = -(phase * 2.0).sin().powi(2) * 0.8;
    let neck = Pt::new(24.0 + sway, 12.0 + bob);
    let hip = Pt::new(24.0 - sway * 0.4, 20.0 + bob);
    for (side, angle) in [(-1.0, phase), (1.0, phase + PI)] {
        let elbow = Pt::new(neck.x + side * 6.0, 13.0 + angle.sin() * (3.5 + load));
        let hand = Pt::new(
            neck.x + side * (10.5 + load),
            8.5 + angle.sin() * (4.0 + load),
        );
        path(canvas, &[neck, elbow, hand], 3.3);
        canvas.disc(hand, 1.65, INK);
        let knee = Pt::new(hip.x + side * 3.8, 23.0 - angle.sin().max(0.0) * 2.0);
        let foot = Pt::new(
            hip.x + side * (6.5 + angle.cos() * (2.4 + load)),
            28.0 - angle.sin().max(0.0) * 4.5,
        );
        path(
            canvas,
            &[hip, knee, foot, Pt::new(foot.x + 2.0, foot.y)],
            3.3,
        );
    }
    canvas.line(neck, hip, 6.4, INK);
    canvas.disc(Pt::new(24.0 + sway, 7.5 + bob), 3.6, INK);
    // A tilted party hat is attached to the dancer, never floating confetti.
    canvas.triangle(
        Pt::new(21.0 + sway, 5.0 + bob),
        Pt::new(26.5 + sway, 5.0 + bob),
        Pt::new(22.0 + sway, 1.8 + bob),
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

    #[test]
    fn playful_cycles_have_eight_distinct_unclipped_normalized_poses() {
        for character in CHARACTERS {
            for cpu in [0.0, 30.0, 65.0, 90.0] {
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
                assert!(metrics.minimum_frame_delta >= 24);
            }
        }
    }
}
