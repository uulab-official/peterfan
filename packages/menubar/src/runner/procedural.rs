use peterfan_core::config::RunnerCharacter;

use super::draw::{Pt, RunnerCanvas};
use super::{
    manifest, normalize_cycle, RgbaFrame, RunnerBuildError, FRAME_COUNT, PIXEL_HEIGHT, PIXEL_WIDTH,
};

#[cfg(test)]
pub(crate) fn render(
    character: RunnerCharacter,
    cpu_pct: f32,
    frame: u8,
) -> Result<RgbaFrame, RunnerBuildError> {
    let cycle = build_cycle(character, cpu_pct)?;
    Ok(cycle[usize::from(frame % FRAME_COUNT)].clone())
}

pub(crate) fn source_cycle(
    character: RunnerCharacter,
    cpu_pct: f32,
) -> Result<[RgbaFrame; 8], RunnerBuildError> {
    if is_asset_backed(character) {
        return Err(RunnerBuildError::InvalidFrame {
            character,
            detail: "not a procedural runner".to_string(),
        });
    }

    Ok(std::array::from_fn(|pose| {
        render_pose(character, cpu_pct, pose as u8)
    }))
}

pub(crate) fn build_cycle(
    character: RunnerCharacter,
    cpu_pct: f32,
) -> Result<[RgbaFrame; 8], RunnerBuildError> {
    normalize_cycle(manifest(character), source_cycle(character, cpu_pct)?)
}

fn is_asset_backed(character: RunnerCharacter) -> bool {
    matches!(
        character,
        RunnerCharacter::Cat
            | RunnerCharacter::Dog
            | RunnerCharacter::Rabbit
            | RunnerCharacter::Fox
            | RunnerCharacter::Dinosaur
            | RunnerCharacter::Ghost
            | RunnerCharacter::Human
    )
}

fn render_pose(character: RunnerCharacter, cpu_pct: f32, frame: u8) -> RgbaFrame {
    match character {
        RunnerCharacter::Penguin => make_penguin_runner_rgba(cpu_pct, frame),
        RunnerCharacter::Robot => make_robot_runner_rgba(cpu_pct, frame),
        RunnerCharacter::Pushup => make_pushup_runner_rgba(cpu_pct, frame),
        RunnerCharacter::Bird
        | RunnerCharacter::Duck
        | RunnerCharacter::Sheep
        | RunnerCharacter::Fish
        | RunnerCharacter::Slime => super::playful_animals::render(character, cpu_pct, frame),
        RunnerCharacter::Orbit
        | RunnerCharacter::Sushi
        | RunnerCharacter::City
        | RunnerCharacter::Sausage
        | RunnerCharacter::Party => super::playful_objects::render(character, cpu_pct, frame),
        _ => unreachable!("asset-backed runner reached procedural pose renderer"),
    }
}

// Original side-view gallop. Each entry is an authored joint/foot pair, rather
// than a translated duplicate. The head and ribcage retain their volume while
// the limbs progress through reach, contact, push-off, tuck and suspension.
pub(crate) fn animal_source_cycle(character: RunnerCharacter, cpu_pct: f32) -> [RgbaFrame; 8] {
    std::array::from_fn(|pose| {
        let mut canvas = RunnerCanvas::new();
        let solid = (255, 255, 255, 255);
        let far = (255, 255, 255, 220);
        let load = cpu_pct.clamp(0.0, 100.0) / 100.0;
        let reach = 0.88 + load * 0.32;
        let bob = [0.0, -0.7, -1.7, -0.8, 0.0, -0.5, -1.4, -0.6][pose];
        // Knee X/Y and paw X/Y relative to the hip.
        const HIND: [[f32; 4]; 8] = [
            [-4.0, 3.0, -7.0, 8.0],
            [-5.0, 2.5, -9.0, 5.0],
            [-2.0, 3.0, 1.0, 5.0],
            [3.0, 4.0, 6.0, 7.0],
            [4.0, 3.0, 5.0, 8.0],
            [2.0, 4.0, 0.0, 8.0],
            [-1.0, 3.0, -4.0, 5.0],
            [-4.0, 2.0, -7.0, 5.0],
        ];
        const FRONT: [[f32; 4]; 8] = [
            [3.0, 3.0, 6.0, 9.0],
            [1.0, 4.0, -1.0, 9.0],
            [-3.0, 3.0, -5.0, 5.0],
            [-4.0, 2.0, -1.0, 4.0],
            [-2.0, 3.0, 2.0, 9.0],
            [2.0, 3.0, 6.0, 8.0],
            [4.0, 2.0, 8.0, 4.0],
            [4.0, 2.0, 8.0, 6.0],
        ];
        let limb = |canvas: &mut RunnerCanvas, hip: Pt, joints: [f32; 4], color| {
            let knee = Pt::new(hip.x + joints[0] * reach, hip.y + joints[1]);
            let paw = Pt::new(hip.x + joints[2] * reach, hip.y + joints[3]);
            canvas.line(hip, knee, 3.1, color);
            canvas.line(knee, paw, 2.5, color);
            canvas.line(paw, Pt::new(paw.x + 1.8, paw.y), 2.5, color);
        };
        limb(
            &mut canvas,
            Pt::new(18.0, 19.0 + bob),
            HIND[(pose + 2) % 8],
            far,
        );
        limb(
            &mut canvas,
            Pt::new(29.0, 18.0 + bob),
            FRONT[(pose + 2) % 8],
            far,
        );
        // The tail follows the spine with a small delayed wave, never a detached dash.
        let tail = [0.0, -0.8, -1.4, -1.0, 0.0, 0.8, 1.1, 0.7][pose];
        match character {
            RunnerCharacter::Rabbit => {
                canvas.disc(Pt::new(12.5, 17.0 + bob), 3.0, solid);
                canvas.ellipse(Pt::new(22.0, 18.0 + bob), 9.0, 5.8, solid);
                canvas.disc(Pt::new(33.0, 13.5 + bob), 5.0, solid);
                canvas.line(
                    Pt::new(30.5, 11.0 + bob),
                    Pt::new(25.5 - tail * 0.4, 6.0 + bob),
                    4.1,
                    solid,
                );
                canvas.line(
                    Pt::new(34.0, 10.5 + bob),
                    Pt::new(31.0 - tail * 0.4, 5.3 + bob),
                    4.0,
                    solid,
                );
                canvas.ellipse(Pt::new(37.0, 15.0 + bob), 3.2, 2.3, solid);
            }
            RunnerCharacter::Dog => {
                canvas.line(
                    Pt::new(15.0, 17.0 + bob),
                    Pt::new(9.0, 12.0 + bob),
                    4.5,
                    solid,
                );
                canvas.line(
                    Pt::new(9.0, 12.0 + bob),
                    Pt::new(7.0, 8.0 + bob + tail),
                    3.8,
                    solid,
                );
                canvas.ellipse(Pt::new(23.0, 17.5 + bob), 10.0, 5.2, solid);
                canvas.ellipse(Pt::new(31.5, 15.0 + bob), 5.5, 5.1, solid);
                canvas.disc(Pt::new(35.0, 12.0 + bob), 4.5, solid);
                canvas.ellipse(Pt::new(39.5, 14.0 + bob), 4.1, 2.6, solid);
                // Long drooping ear, deliberately distinct from the cat's spikes.
                canvas.line(
                    Pt::new(32.0, 9.3 + bob),
                    Pt::new(29.5 - tail * 0.4, 15.0 + bob),
                    4.6,
                    solid,
                );
            }
            RunnerCharacter::Fox => {
                // A tapered brush tail and narrow muzzle keep the fox distinct.
                canvas.triangle(
                    Pt::new(17.0, 18.0 + bob),
                    Pt::new(6.0, 9.0 + bob + tail),
                    Pt::new(3.5, 16.0 + bob + tail),
                    solid,
                );
                canvas.line(
                    Pt::new(7.0, 15.0 + bob + tail),
                    Pt::new(16.0, 17.0 + bob),
                    5.0,
                    solid,
                );
                canvas.ellipse(Pt::new(23.0, 17.0 + bob), 10.0, 4.8, solid);
                canvas.ellipse(Pt::new(33.0, 13.5 + bob), 5.7, 4.0, solid);
                canvas.triangle(
                    Pt::new(34.0, 12.0 + bob),
                    Pt::new(43.0, 15.0 + bob),
                    Pt::new(35.0, 17.0 + bob),
                    solid,
                );
                canvas.triangle(
                    Pt::new(29.5, 12.0 + bob),
                    Pt::new(30.0, 5.5 + bob),
                    Pt::new(34.0, 10.0 + bob),
                    solid,
                );
                canvas.triangle(
                    Pt::new(34.0, 10.0 + bob),
                    Pt::new(37.0, 6.0 + bob),
                    Pt::new(38.0, 13.0 + bob),
                    solid,
                );
            }
            RunnerCharacter::Cat => {
                canvas.line(
                    Pt::new(16.0, 16.5 + bob),
                    Pt::new(9.0, 13.0 + bob),
                    3.6,
                    solid,
                );
                canvas.line(
                    Pt::new(9.0, 13.0 + bob),
                    Pt::new(5.0, 9.5 + bob + tail),
                    3.0,
                    solid,
                );
                canvas.ellipse(Pt::new(23.0, 17.0 + bob), 10.0, 5.1, solid);
                canvas.ellipse(Pt::new(31.0, 15.0 + bob), 5.2, 5.2, solid);
                canvas.disc(Pt::new(35.0, 12.5 + bob), 5.0, solid);
                canvas.triangle(
                    Pt::new(30.7, 10.0 + bob),
                    Pt::new(31.0, 5.8 + bob),
                    Pt::new(35.0, 8.5 + bob),
                    solid,
                );
                canvas.triangle(
                    Pt::new(35.0, 8.5 + bob),
                    Pt::new(38.0, 6.0 + bob),
                    Pt::new(39.0, 12.0 + bob),
                    solid,
                );
                canvas.ellipse(Pt::new(39.0, 14.0 + bob), 3.2, 2.1, solid);
            }
            _ => unreachable!("non-quadruped in authored animal renderer"),
        }
        limb(&mut canvas, Pt::new(17.0, 19.0 + bob), HIND[pose], solid);
        limb(&mut canvas, Pt::new(30.0, 18.0 + bob), FRONT[pose], solid);
        let eye = match character {
            RunnerCharacter::Rabbit => Pt::new(34.5, 12.5 + bob),
            RunnerCharacter::Fox => Pt::new(35.3, 12.0 + bob),
            _ => Pt::new(36.8, 11.8 + bob),
        };
        canvas.erase_disc(eye, 0.95);
        // Assets retain their historical mirror_source contract.
        let frame = canvas.finish();
        let mut mirrored = vec![0; frame.len()];
        for y in 0..PIXEL_HEIGHT {
            for x in 0..PIXEL_WIDTH {
                let src = ((y * PIXEL_WIDTH + x) * 4) as usize;
                let dst = ((y * PIXEL_WIDTH + PIXEL_WIDTH - 1 - x) * 4) as usize;
                mirrored[dst..dst + 4].copy_from_slice(&frame[src..src + 4]);
            }
        }
        mirrored
    })
}

// A rounded, readable ghost with a travelling cloth hem. Speed changes the
// cloth/arm reach while the face and dome retain the same proportions.
pub(crate) fn ghost_source_cycle(cpu_pct: f32) -> [RgbaFrame; 8] {
    std::array::from_fn(|pose| {
        let solid = (255, 255, 255, 255);
        let load = cpu_pct.clamp(0.0, 100.0) / 100.0;
        let amplitude = 1.0 + load * 1.2;
        let wave = [0.0, 0.7, 1.0, 0.7, 0.0, -0.7, -1.0, -0.7];
        let bob = [0.0, -0.6, -1.2, -0.6, 0.0, -0.6, -1.2, -0.6][pose];
        let mut canvas = RunnerCanvas::new();
        canvas.disc(Pt::new(25.0, 12.0 + bob), 8.0, solid);
        canvas.rect(17.0, 12.0 + bob, 33.0, 22.0 + bob, solid);
        for segment in 0..4 {
            let x = 17.0 + segment as f32 * 4.0;
            let flutter = wave[(pose + segment * 2) % 8] * amplitude;
            canvas.triangle(
                Pt::new(x, 21.0 + bob),
                Pt::new(x + 4.0, 21.0 + bob),
                Pt::new(x + 2.0 + flutter * 0.5, 26.0 + bob + flutter),
                solid,
            );
        }
        let hand_wave = wave[pose] * amplitude;
        canvas.line(
            Pt::new(18.0, 15.0 + bob),
            Pt::new(12.5, 18.0 + bob + hand_wave),
            4.0,
            solid,
        );
        canvas.line(
            Pt::new(32.0, 15.0 + bob),
            Pt::new(37.5, 17.0 + bob - hand_wave),
            4.0,
            solid,
        );
        canvas.erase_disc(Pt::new(24.5, 11.5 + bob), 1.2);
        canvas.erase_disc(Pt::new(29.5, 11.5 + bob), 1.2);
        canvas.erase_disc(Pt::new(27.5, 16.0 + bob), 1.4);
        canvas.finish()
    })
}

fn make_penguin_runner_rgba(cpu_pct: f32, frame: u8) -> RgbaFrame {
    let pose = usize::from(frame % FRAME_COUNT);
    let load = cpu_pct.clamp(0.0, 100.0) / 100.0;
    let reach = 0.9 + load * 0.22;
    let bob = [0.0, -0.8, -1.8, -0.8, 0.0, -0.8, -1.8, -0.8][pose];
    // Alternating planted, trailing and tucked feet; the four-frame offset
    // gives a full left/right stride without changing the body proportions.
    const FEET: [[f32; 4]; 8] = [
        [4.0, 3.0, 6.0, 6.0],
        [0.0, 4.0, 1.0, 6.0],
        [-4.0, 2.0, -5.0, 3.0],
        [-4.0, 0.0, -1.0, 2.0],
        [-2.0, 2.0, 3.0, 6.0],
        [2.0, 2.0, 6.0, 5.0],
        [4.0, 0.0, 7.0, 2.0],
        [4.0, 1.0, 7.0, 4.0],
    ];
    let solid = (255, 255, 255, 255);
    let far = (255, 255, 255, 220);
    let mut canvas = RunnerCanvas::new();
    let foot = |canvas: &mut RunnerCanvas, phase: usize, x: f32, color| {
        let joints = FEET[phase];
        let hip = Pt::new(x, 22.0 + bob);
        let ankle = Pt::new(x + joints[0] * reach, hip.y + joints[1]);
        let toe = Pt::new(x + joints[2] * reach, hip.y + joints[3]);
        canvas.line(hip, ankle, 3.0, color);
        canvas.line(ankle, toe, 3.0, color);
        canvas.line(
            Pt::new(toe.x - 1.0, toe.y),
            Pt::new(toe.x + 3.0, toe.y),
            2.8,
            color,
        );
    };
    foot(&mut canvas, (pose + 4) % 8, 24.0, far);
    let swing = [-3.0, -1.0, 2.0, 4.0, 3.0, 1.0, -2.0, -4.0][pose] * reach;
    canvas.line(
        Pt::new(24.0, 12.0 + bob),
        Pt::new(18.0 - swing * 0.4, 19.0 + bob + swing * 0.4),
        4.2,
        far,
    );
    canvas.ellipse(Pt::new(27.0, 17.0 + bob), 7.0, 9.1, solid);
    canvas.disc(Pt::new(29.0, 8.0 + bob), 5.5, solid);
    canvas.triangle(
        Pt::new(33.0, 8.0 + bob),
        Pt::new(39.0, 10.0 + bob),
        Pt::new(33.0, 11.5 + bob),
        solid,
    );
    canvas.line(
        Pt::new(29.0, 13.5 + bob),
        Pt::new(32.0 + swing * 0.75, 19.0 + bob - swing * 0.45),
        3.7,
        solid,
    );
    foot(&mut canvas, pose, 28.0, solid);
    // A single visible eye and a narrow breast crescent read at menu-bar scale.
    canvas.erase_disc(Pt::new(30.7, 7.0 + bob), 0.95);
    canvas.erase_disc(Pt::new(33.5, 17.0 + bob), 2.8);
    canvas.finish()
}

fn make_robot_runner_rgba(cpu_pct: f32, frame: u8) -> RgbaFrame {
    let pose = usize::from(frame % FRAME_COUNT);
    let load = cpu_pct.clamp(0.0, 100.0) / 100.0;
    let reach = 0.9 + load * 0.2;
    let bob = [0.0, -0.8, -1.7, -0.8, 0.0, -0.8, -1.7, -0.8][pose];
    // Hip/knee/ankle chains follow an actual run, with the rear knee folding
    // under the pelvis rather than stretching opposite corners of a box.
    const LEG: [[f32; 4]; 8] = [
        [4.0, 3.5, 7.0, 9.0],
        [0.0, 5.0, -2.0, 9.0],
        [-4.0, 2.0, -7.0, 4.0],
        [-4.0, 0.0, -2.0, 1.0],
        [-1.0, 3.0, 5.0, 9.0],
        [3.0, 3.5, 7.0, 7.0],
        [5.0, 1.0, 7.0, 3.0],
        [5.0, 1.5, 8.0, 6.0],
    ];
    const ARM: [[f32; 4]; 8] = [
        [-5.0, 3.0, -3.0, 6.0],
        [-3.0, 4.0, 0.0, 6.0],
        [0.0, 4.0, 4.0, 3.0],
        [4.0, 3.0, 7.0, 0.0],
        [5.0, 2.0, 6.0, -1.0],
        [3.0, 4.0, 5.0, 1.0],
        [0.0, 4.0, 2.0, 6.0],
        [-4.0, 3.0, -2.0, 6.0],
    ];
    let solid = (255, 255, 255, 255);
    let far = (255, 255, 255, 215);
    let mut canvas = RunnerCanvas::new();
    let joint_chain = |canvas: &mut RunnerCanvas, root: Pt, offsets: [f32; 4], width, color| {
        let joint = Pt::new(root.x + offsets[0] * reach, root.y + offsets[1]);
        let end = Pt::new(root.x + offsets[2] * reach, root.y + offsets[3]);
        canvas.line(root, joint, width, color);
        canvas.line(joint, end, width, color);
        canvas.disc(joint, width * 0.55, color);
        end
    };
    let rear_foot = joint_chain(
        &mut canvas,
        Pt::new(24.0, 19.0 + bob),
        LEG[(pose + 4) % 8],
        3.0,
        far,
    );
    canvas.line(rear_foot, Pt::new(rear_foot.x + 2.5, rear_foot.y), 2.8, far);
    joint_chain(
        &mut canvas,
        Pt::new(26.0, 12.0 + bob),
        ARM[(pose + 4) % 8],
        2.8,
        far,
    );
    canvas.rect(22.0, 11.0 + bob, 31.0, 20.0 + bob, solid);
    canvas.line(
        Pt::new(27.0, 10.0 + bob),
        Pt::new(27.0, 12.0 + bob),
        3.0,
        solid,
    );
    canvas.rect(23.0, 4.7 + bob, 34.0, 12.5 + bob, solid);
    canvas.line(
        Pt::new(27.0, 5.2 + bob),
        Pt::new(26.0, 3.5 + bob),
        1.8,
        solid,
    );
    canvas.disc(Pt::new(26.0, 3.5 + bob), 1.1, solid);
    let near_foot = joint_chain(
        &mut canvas,
        Pt::new(27.0, 19.0 + bob),
        LEG[pose],
        3.3,
        solid,
    );
    canvas.line(
        near_foot,
        Pt::new(near_foot.x + 2.5, near_foot.y),
        3.0,
        solid,
    );
    let hand = joint_chain(
        &mut canvas,
        Pt::new(29.0, 12.5 + bob),
        ARM[pose],
        3.0,
        solid,
    );
    canvas.disc(hand, 1.6, solid);
    canvas.erase_rect(27.5, 7.0 + bob, 32.5, 10.0 + bob);
    canvas.erase_disc(Pt::new(26.0, 15.0 + bob), 1.1);
    canvas.finish()
}

#[derive(Clone, Copy)]
struct PushupPose {
    toe: Pt,
    knee: Pt,
    hip: Pt,
    shoulder: Pt,
    elbow: Pt,
    hand: Pt,
    neck: Pt,
    head: Pt,
}

fn pushup_pose(cpu_pct: f32, frame: u8) -> PushupPose {
    // Rotate a rigid toe-to-shoulder chain. Independent Y curves made the old
    // character sag and stretch instead of lowering a straight plank.
    const ANGLES: [f32; 8] = [33.4, 27.0, 21.0, 15.0, 12.0, 18.0, 24.0, 28.0];
    let angle = ANGLES[usize::from(frame % FRAME_COUNT)];
    let effort = cpu_pct.clamp(0.0, 100.0) / 100.0;
    let depth = [0.0, -0.8, -2.0, -3.5, -4.5, -2.8, -1.5, -0.3][usize::from(frame % FRAME_COUNT)];
    let angle = (angle + depth * effort).to_radians();
    let direction = Pt::new(angle.cos(), -angle.sin());
    let toe = Pt::new(3.8, 27.7);
    let on_spine = |distance: f32| {
        Pt::new(
            toe.x + direction.x * distance,
            toe.y + direction.y * distance,
        )
    };
    let shoulder = on_spine(32.0);
    let hand = Pt::new(36.0, 27.7);

    // Two-bone inverse kinematics keeps both arm lengths fixed. The elbow
    // bends toward the feet rather than sliding through or ahead of the head.
    let dx = hand.x - shoulder.x;
    let dy = hand.y - shoulder.y;
    let distance = dx.hypot(dy);
    let along = (9.15_f32.powi(2) - 9.35_f32.powi(2) + distance.powi(2)) / (2.0 * distance);
    let bend = (9.15_f32.powi(2) - along.powi(2)).max(0.0).sqrt();
    let elbow = Pt::new(
        shoulder.x + (dx * along - dy * bend) / distance,
        shoulder.y + (dy * along + dx * bend) / distance,
    );
    let neck = Pt::new(
        shoulder.x + direction.x * 2.0 - angle.sin() * 0.8,
        shoulder.y + direction.y * 2.0 - angle.cos() * 0.8,
    );
    let head = Pt::new(neck.x + direction.x * 2.9, neck.y + direction.y * 2.9);
    PushupPose {
        toe,
        knee: on_spine(7.5),
        hip: on_spine(15.0),
        shoulder,
        elbow,
        hand,
        neck,
        head,
    }
}

fn make_pushup_runner_rgba(cpu_pct: f32, frame: u8) -> RgbaFrame {
    let pose = pushup_pose(cpu_pct, frame);
    let mut canvas = RunnerCanvas::new();
    let solid = (255, 255, 255, 255);
    // One clean side-view contour leaves the elbow triangle open at native
    // size; overlapping pairs of arms previously filled it into a prone bar.
    canvas.line(pose.toe, pose.knee, 2.7, solid);
    canvas.line(pose.knee, pose.hip, 3.0, solid);
    canvas.line(pose.hip, pose.shoulder, 4.6, solid);
    canvas.line(pose.shoulder, pose.neck, 2.7, solid);
    canvas.line(pose.neck, pose.head, 2.5, solid);
    canvas.disc(pose.head, 3.0, solid);
    canvas.line(pose.shoulder, pose.elbow, 2.7, solid);
    canvas.line(pose.elbow, pose.hand, 2.6, solid);
    canvas.disc(pose.elbow, 1.2, solid);
    canvas.line(
        Pt::new(pose.hand.x - 1.2, pose.hand.y),
        Pt::new(pose.hand.x + 2.0, pose.hand.y),
        2.3,
        solid,
    );
    canvas.line(
        Pt::new(pose.toe.x - 0.7, pose.toe.y),
        Pt::new(pose.toe.x + 2.0, pose.toe.y),
        2.3,
        solid,
    );
    canvas.finish()
}

#[cfg(test)]
mod tests {
    use peterfan_core::config::RunnerCharacter;

    use super::{build_cycle, render, source_cycle};
    use crate::runner::{manifest, validate_cycle};

    fn procedural_characters() -> [RunnerCharacter; 13] {
        [
            RunnerCharacter::Penguin,
            RunnerCharacter::Robot,
            RunnerCharacter::Bird,
            RunnerCharacter::Duck,
            RunnerCharacter::Sheep,
            RunnerCharacter::Fish,
            RunnerCharacter::Slime,
            RunnerCharacter::Pushup,
            RunnerCharacter::Orbit,
            RunnerCharacter::Sushi,
            RunnerCharacter::City,
            RunnerCharacter::Sausage,
            RunnerCharacter::Party,
        ]
    }

    #[test]
    fn pushup_keeps_contacts_spine_and_limb_lengths_throughout_each_rep() {
        let distance = |a: super::Pt, b: super::Pt| (a.x - b.x).hypot(a.y - b.y);
        for cpu in [0.0, 30.0, 65.0, 90.0] {
            for frame in 0..8 {
                let p = super::pushup_pose(cpu, frame);
                assert!((p.toe.x - 3.8).abs() < 0.01 && (p.toe.y - 27.7).abs() < 0.01);
                assert!((p.hand.x - 36.0).abs() < 0.01 && (p.hand.y - 27.7).abs() < 0.01);
                assert!((distance(p.toe, p.shoulder) - 32.0).abs() < 0.01);
                assert!((distance(p.shoulder, p.elbow) - 9.15).abs() < 0.01);
                assert!((distance(p.elbow, p.hand) - 9.35).abs() < 0.01);
                for joint in [p.knee, p.hip] {
                    let area = (joint.x - p.toe.x) * (p.shoulder.y - p.toe.y)
                        - (joint.y - p.toe.y) * (p.shoulder.x - p.toe.x);
                    assert!(area.abs() < 0.01, "bent spine in pose {frame}");
                }
                let bend = (p.hand.x - p.shoulder.x) * (p.elbow.y - p.shoulder.y)
                    - (p.hand.y - p.shoulder.y) * (p.elbow.x - p.shoulder.x);
                assert!(
                    bend > 0.0,
                    "elbow must bend toward feet relative to arm axis"
                );
            }
        }
    }

    #[test]
    fn pushup_elbow_and_forearm_stay_on_the_ground_side_of_the_spine() {
        for cpu in [0.0, 30.0, 65.0, 90.0, 100.0] {
            for frame in 0..8 {
                let p = super::pushup_pose(cpu, frame);
                let spine_y = p.toe.y
                    + (p.shoulder.y - p.toe.y) * (p.elbow.x - p.toe.x) / (p.shoulder.x - p.toe.x);
                assert!(
                    p.elbow.y - spine_y > 1.0,
                    "elbow crosses back in pose {frame} at {cpu}%: clearance={}",
                    p.elbow.y - spine_y
                );
                for t in [0.0, 0.25, 0.5, 0.75, 1.0] {
                    let x = p.elbow.x + (p.hand.x - p.elbow.x) * t;
                    let y = p.elbow.y + (p.hand.y - p.elbow.y) * t;
                    let spine_y = p.toe.y
                        + (p.shoulder.y - p.toe.y) * (x - p.toe.x) / (p.shoulder.x - p.toe.x);
                    assert!(y > spine_y, "forearm crosses torso");
                }
            }
        }
    }

    #[test]
    fn pushup_top_pose_extends_the_arm_instead_of_holding_half_a_rep() {
        let p = super::pushup_pose(0.0, 0);
        let upper = super::Pt::new(p.elbow.x - p.shoulder.x, p.elbow.y - p.shoulder.y);
        let lower = super::Pt::new(p.hand.x - p.elbow.x, p.hand.y - p.elbow.y);
        let alignment = (upper.x * lower.x + upper.y * lower.y) / (9.15 * 9.35);
        assert!(
            alignment > 0.98,
            "top pose arm is still bent: alignment={alignment}"
        );
    }

    #[test]
    fn pushup_has_a_raised_plank_and_a_visibly_lower_bottom() {
        let top = super::make_pushup_runner_rgba(0.0, 0);
        let bottom = super::make_pushup_runner_rgba(0.0, 4);
        let top_bounds = crate::runner::measure_frame(&top).expect("top").bounds;
        let low_bounds = crate::runner::measure_frame(&bottom)
            .expect("bottom")
            .bounds;
        assert!(
            top_bounds.min_y <= 8,
            "plank is flattened at y={}",
            top_bounds.min_y
        );
        assert!(
            low_bounds.min_y >= top_bounds.min_y + 9,
            "full body must visibly descend"
        );
    }

    #[test]
    fn robot_authored_frames_keep_a_clear_canvas_margin() {
        for pose in 0..8 {
            let frame = super::make_robot_runner_rgba(90.0, pose);
            let w = crate::runner::PIXEL_WIDTH;
            let h = crate::runner::PIXEL_HEIGHT;
            for y in 0..h {
                for x in 0..w {
                    if x == 0 || y == 0 || x == w - 1 || y == h - 1 {
                        assert_eq!(
                            frame[((y * w + x) * 4 + 3) as usize],
                            0,
                            "robot pose {pose} has cropped raw artwork at {x},{y}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn normalized_robot_keeps_a_visible_visor_in_every_pose() {
        for cpu_pct in [0.0, 30.0, 65.0, 90.0] {
            for (pose, frame) in build_cycle(RunnerCharacter::Robot, cpu_pct)
                .expect("robot cycle")
                .iter()
                .enumerate()
            {
                let w = crate::runner::PIXEL_WIDTH;
                let alpha = |x: u32, y: u32| frame[((y * w + x) * 4 + 3) as usize];
                // The visor is an enclosed dark horizontal cavity in the head,
                // above the chest button. Require two visibly dark pixels and
                // solid side walls, so a faint antialiasing dent cannot pass.
                let mut visor_pixels = 0;
                for y in 2..=12 {
                    let row = (1..w - 1)
                        .filter(|x| alpha(*x, y) >= 180)
                        .collect::<Vec<_>>();
                    if let (Some(&left), Some(&right)) = (row.first(), row.last()) {
                        for x in left + 1..right {
                            if alpha(x, y) < 96 && alpha(x, y - 2) >= 180 && alpha(x, y + 2) >= 180
                            {
                                visor_pixels += 1;
                            }
                        }
                    }
                }
                assert!(visor_pixels >= 2, "robot pose {pose} at {cpu_pct}% loses its visible visor ({visor_pixels} dark pixels)");
            }
        }
    }

    #[test]
    fn ghost_face_cavities_are_offset_toward_the_running_direction() {
        let frame = super::ghost_source_cycle(0.0)[0].clone();
        let alpha = |x: u32, y: u32| frame[((y * crate::runner::PIXEL_WIDTH + x) * 4 + 3) as usize];
        let dome_x = (0..crate::runner::PIXEL_WIDTH)
            .filter(|x| alpha(*x, 12) > 200)
            .collect::<Vec<_>>();
        let left = *dome_x.first().expect("ghost dome");
        let right = *dome_x.last().expect("ghost dome");
        let dome_center = (left + right) as f32 / 2.0;
        let mut eye_x = Vec::new();
        for y in 10..=13 {
            for x in left + 2..right - 1 {
                if alpha(x, y) < 180 {
                    eye_x.push(x as f32);
                }
            }
        }
        assert!(!eye_x.is_empty(), "ghost must retain visible eye cavities");
        let face_center = eye_x.iter().sum::<f32>() / eye_x.len() as f32;
        assert!(
            face_center > dome_center + 1.0,
            "ghost face at {face_center} must lead the dome at {dome_center} to the right"
        );
        assert!(
            !crate::runner::manifest(RunnerCharacter::Ghost).mirror_source,
            "the authored right-facing ghost must not be mirrored during normalization"
        );
    }

    #[test]
    fn penguin_animation_uses_anatomy_instead_of_detached_speed_lines() {
        for frame in source_cycle(RunnerCharacter::Penguin, 90.0).expect("penguin source") {
            for y in 0..crate::runner::PIXEL_HEIGHT {
                for x in 0..12 {
                    assert_eq!(
                        frame[((y * crate::runner::PIXEL_WIDTH + x) * 4 + 3) as usize],
                        0,
                        "detached marks at {x},{y} shrink the penguin during normalization"
                    );
                }
            }
        }
    }

    #[test]
    fn procedural_characters_pass_the_shared_quality_gate() {
        let mut failures = Vec::new();

        for character in procedural_characters() {
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
            "procedural quality failures:\n{}",
            failures.join("\n")
        );
    }

    #[test]
    fn build_cycle_matches_rendered_frames_for_all_procedural_characters_and_bands() {
        let mut mismatches = Vec::new();
        for character in procedural_characters() {
            for cpu_pct in [0.0, 30.0, 65.0, 90.0] {
                let cycle = build_cycle(character, cpu_pct).expect("procedural build cycle");
                for frame in 0..8 {
                    let rendered = render(character, cpu_pct, frame).expect("procedural render");
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
            "procedural render/build mismatches:\n{}",
            mismatches.join("\n")
        );
    }

    #[test]
    fn asset_backed_characters_are_rejected_by_procedural_builders() {
        for character in [
            RunnerCharacter::Cat,
            RunnerCharacter::Dog,
            RunnerCharacter::Rabbit,
            RunnerCharacter::Fox,
            RunnerCharacter::Dinosaur,
            RunnerCharacter::Ghost,
            RunnerCharacter::Human,
        ] {
            let source_error =
                source_cycle(character, 30.0).expect_err("asset-backed source cycle rejected");
            assert!(
                source_error.to_string().contains("not a procedural runner"),
                "{character:?}: {source_error}"
            );

            let build_error =
                build_cycle(character, 30.0).expect_err("asset-backed build cycle rejected");
            assert!(
                build_error.to_string().contains("not a procedural runner"),
                "{character:?}: {build_error}"
            );
        }
    }
}
