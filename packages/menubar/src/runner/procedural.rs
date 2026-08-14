use std::f32::consts::TAU;

use peterfan_core::config::RunnerCharacter;

use super::draw::{
    draw_disc, draw_line, draw_rect, draw_rotated_ellipse, draw_runner_leg, draw_triangle,
    erase_disc, Pt, RunnerCanvas,
};
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
        | RunnerCharacter::Slime
        | RunnerCharacter::Orbit
        | RunnerCharacter::Sushi
        | RunnerCharacter::City
        | RunnerCharacter::Sausage
        | RunnerCharacter::Party => make_novelty_runner_rgba(character, cpu_pct, frame),
        _ => unreachable!("asset-backed runner reached procedural pose renderer"),
    }
}

fn make_penguin_runner_rgba(cpu_pct: f32, frame: u8) -> RgbaFrame {
    const W: u32 = PIXEL_WIDTH;
    const H: u32 = PIXEL_HEIGHT;
    const BOB: [f32; 8] = [0.4, -0.4, -1.8, -0.8, 0.5, -0.3, -2.0, -0.7];
    const LEAN: [f32; 8] = [-0.12, -0.06, 0.04, 0.13, 0.10, 0.02, -0.08, -0.14];
    const STEP: [f32; 8] = [-7.0, -4.0, 0.0, 5.0, 7.0, 4.0, 0.0, -5.0];
    const FLAP: [f32; 8] = [-4.0, -1.0, 3.0, 6.0, 3.0, -1.0, -5.0, -7.0];

    let mut rgba = vec![0u8; (W * H * 4) as usize];
    let pose = usize::from(frame % FRAME_COUNT);
    let load = cpu_pct.clamp(0.0, 100.0) / 100.0;
    let motion = 0.58 + load.sqrt() * 0.72;
    let bob = BOB[pose] * motion;
    let step = STEP[pose] * motion;
    let flap = FLAP[pose] * motion;
    let solid = (255, 255, 255, 248);
    let detail = (255, 255, 255, 224);

    draw_line(
        &mut rgba,
        W,
        H,
        Pt::new(2.0, 19.5 + bob + flap * 0.12),
        Pt::new(14.0, 19.5 + bob + flap * 0.12),
        2.2,
        detail,
    );
    draw_line(
        &mut rgba,
        W,
        H,
        Pt::new(6.0, 23.5 + bob - flap * 0.08),
        Pt::new(16.0, 23.5 + bob - flap * 0.08),
        1.7,
        detail,
    );
    draw_line(
        &mut rgba,
        W,
        H,
        Pt::new(23.0, 22.0 + bob),
        Pt::new(20.0 + step * 0.62, 27.0),
        3.0,
        detail,
    );
    draw_line(
        &mut rgba,
        W,
        H,
        Pt::new(30.0, 22.0 + bob),
        Pt::new(33.0 - step * 0.62, 27.0),
        3.0,
        solid,
    );
    draw_line(
        &mut rgba,
        W,
        H,
        Pt::new(21.0, 13.0 + bob),
        Pt::new(15.0, 17.0 + bob + flap * 0.52),
        4.5,
        detail,
    );
    draw_line(
        &mut rgba,
        W,
        H,
        Pt::new(33.0, 13.0 + bob),
        Pt::new(39.0, 17.0 + bob - flap * 0.38),
        4.0,
        detail,
    );
    draw_rotated_ellipse(
        &mut rgba,
        W,
        H,
        Pt::new(27.0, 16.2 + bob),
        8.3 + load * 0.45,
        10.2,
        LEAN[pose] * (0.65 + load * 0.55),
        solid,
    );
    draw_disc(&mut rgba, W, H, Pt::new(31.5, 7.8 + bob), 5.5, solid);
    draw_triangle(
        &mut rgba,
        W,
        H,
        Pt::new(35.0, 7.4 + bob),
        Pt::new(42.5, 9.4 + bob),
        Pt::new(35.0, 11.0 + bob),
        detail,
    );
    erase_disc(&mut rgba, W, H, Pt::new(32.8, 6.7 + bob), 1.0);
    rgba
}

fn make_robot_runner_rgba(cpu_pct: f32, frame: u8) -> RgbaFrame {
    const W: u32 = PIXEL_WIDTH;
    const H: u32 = PIXEL_HEIGHT;
    const BOB: [f32; 8] = [0.5, -0.2, -1.5, -2.7, -1.2, 0.0, 0.6, -1.4];
    const STEP: [f32; 8] = [-7.0, -4.0, 0.0, 6.0, 8.0, 4.0, 0.0, -6.0];
    const ARM: [f32; 8] = [6.0, 3.0, -1.0, -6.0, -7.0, -3.0, 1.0, 6.0];

    let mut rgba = vec![0u8; (W * H * 4) as usize];
    let pose = usize::from(frame % FRAME_COUNT);
    let load = cpu_pct.clamp(0.0, 100.0) / 100.0;
    let motion = 0.58 + load.sqrt() * 0.72;
    let bob = BOB[pose] * motion;
    let step = STEP[pose] * motion;
    let arm = ARM[pose] * motion;
    let solid = (255, 255, 255, 248);
    let detail = (255, 255, 255, 220);

    draw_runner_leg(
        &mut rgba,
        W,
        H,
        Pt::new(23.0, 20.0 + bob),
        Pt::new(18.0 + step, 27.0),
        -2.5,
        solid,
    );
    draw_runner_leg(
        &mut rgba,
        W,
        H,
        Pt::new(32.0, 20.0 + bob),
        Pt::new(35.0 - step, 27.0),
        2.5,
        detail,
    );
    draw_line(
        &mut rgba,
        W,
        H,
        Pt::new(20.0, 13.5 + bob),
        Pt::new(9.0, 13.5 + bob + arm),
        3.0,
        detail,
    );
    draw_disc(&mut rgba, W, H, Pt::new(8.0, 13.5 + bob + arm), 2.1, solid);
    draw_rect(&mut rgba, W, H, 19.0, 11.0 + bob, 36.0, 21.0 + bob, solid);
    draw_rect(&mut rgba, W, H, 23.0, 3.5 + bob, 39.0, 13.0 + bob, solid);
    draw_line(
        &mut rgba,
        W,
        H,
        Pt::new(31.0, 3.5 + bob),
        Pt::new(31.0, 0.8 + bob),
        1.7,
        detail,
    );
    draw_disc(&mut rgba, W, H, Pt::new(31.0, 1.0 + bob), 1.7, solid);
    draw_line(
        &mut rgba,
        W,
        H,
        Pt::new(36.0, 13.5 + bob),
        Pt::new(47.0, 13.5 + bob - arm),
        3.0,
        detail,
    );
    draw_disc(&mut rgba, W, H, Pt::new(47.0, 13.5 + bob - arm), 2.1, solid);
    erase_disc(&mut rgba, W, H, Pt::new(27.5, 7.8 + bob), 1.05);
    erase_disc(&mut rgba, W, H, Pt::new(34.5, 7.8 + bob), 1.05);
    erase_disc(&mut rgba, W, H, Pt::new(27.5, 16.0 + bob), 1.35);
    rgba
}

fn make_pushup_runner_rgba(cpu_pct: f32, frame: u8) -> RgbaFrame {
    const FAR_HAND: Pt = Pt::new(38.0, 27.0);
    const NEAR_HAND: Pt = Pt::new(41.0, 26.0);
    const FAR_TOE: Pt = Pt::new(3.0, 26.0);
    const NEAR_TOE: Pt = Pt::new(6.5, 26.0);
    const HIP_Y: [f32; 8] = [21.8, 22.4, 23.2, 24.0, 23.7, 23.0, 22.3, 22.2];
    const SHOULDER_Y: [f32; 8] = [19.7, 20.7, 22.0, 23.6, 22.8, 21.7, 20.9, 20.5];
    const ELBOW_X: [f32; 8] = [32.7, 32.0, 31.0, 30.0, 28.5, 31.5, 32.5, 33.5];
    const ELBOW_Y: [f32; 8] = [23.0, 24.0, 25.2, 26.0, 25.0, 24.8, 23.5, 23.8];
    const KNEE_Y: [f32; 8] = [23.0, 23.8, 24.7, 25.4, 24.4, 24.4, 23.4, 23.6];
    const HEAD_X: [f32; 8] = [34.2, 34.2, 34.2, 34.2, 34.2, 34.2, 34.2, 35.3];

    let pose = usize::from(frame % FRAME_COUNT);
    let effort = match cpu_pct.clamp(0.0, 100.0) {
        load if load < 15.0 => 0.0,
        load if load < 50.0 => 0.4,
        load if load < 80.0 => 0.85,
        _ => 1.35,
    };
    let hip = Pt::new(16.0, HIP_Y[pose] + effort);
    let shoulder = Pt::new(29.0, SHOULDER_Y[pose] + effort);
    let elbow = Pt::new(ELBOW_X[pose], ELBOW_Y[pose] + effort);
    let knee = Pt::new(10.0, KNEE_Y[pose] + effort * 0.7);
    let head = Pt::new(HEAD_X[pose], SHOULDER_Y[pose] - 2.3 + effort);
    let solid = (255, 255, 255, 248);
    let detail = (255, 255, 255, 222);
    let mut canvas = RunnerCanvas::new();

    // Far limbs establish the fixed toe and hand contacts first.
    canvas.leg(hip, FAR_TOE, -1.5, detail);
    canvas.line(shoulder, elbow, 2.8, detail);
    canvas.line(elbow, FAR_HAND, 2.6, detail);

    // The torso and near limbs move as one connected chain through the cycle.
    canvas.line(hip, shoulder, 4.8, solid);
    canvas.disc(Pt::new(22.0, (hip.y + shoulder.y) * 0.5), 1.2, solid);
    canvas.line(hip, knee, 2.8, solid);
    canvas.line(knee, NEAR_TOE, 2.8, solid);
    canvas.line(shoulder, elbow, 3.2, solid);
    canvas.line(elbow, NEAR_HAND, 3.0, solid);
    canvas.line(
        shoulder,
        Pt::new(31.0, SHOULDER_Y[pose] - 1.0 + effort),
        2.0,
        solid,
    );
    canvas.disc(head, 3.1, solid);
    canvas.erase_disc(Pt::new(head.x + 1.0, head.y - 0.6), 0.55);
    canvas.disc(Pt::new(16.0, HIP_Y[pose] + effort), 1.25, solid);
    canvas.disc(FAR_TOE, 1.0, detail);
    canvas.disc(NEAR_TOE, 1.1, solid);
    canvas.disc(FAR_HAND, 1.0, detail);
    canvas.disc(NEAR_HAND, 1.1, solid);
    canvas.line(Pt::new(2.0, 26.0), Pt::new(7.5, 26.0), 1.8, detail);
    canvas.line(Pt::new(36.5, 26.0), Pt::new(42.0, 26.0), 1.8, detail);

    canvas.finish()
}

fn make_novelty_runner_rgba(character: RunnerCharacter, cpu_pct: f32, frame: u8) -> RgbaFrame {
    const BOB: [f32; 8] = [0.8, 0.0, -1.5, -2.8, -1.5, 0.1, 0.9, -1.2];
    const SWING: [f32; 8] = [-7.0, -4.0, 0.0, 6.0, 8.0, 4.0, 0.5, -6.0];
    const WAVE: [f32; 8] = [4.0, 1.0, -3.0, -6.0, -3.5, 0.5, 5.5, 7.0];

    let pose = usize::from(frame % FRAME_COUNT);
    let load = cpu_pct.clamp(0.0, 100.0) / 100.0;
    let motion = 0.55 + load.sqrt() * 0.76;
    let bob = BOB[pose] * motion;
    let swing = SWING[pose] * motion;
    let wave = WAVE[pose] * motion;
    let solid = (255, 255, 255, 248);
    let detail = (255, 255, 255, 222);
    let mut canvas = RunnerCanvas::new();

    match character {
        RunnerCharacter::Bird => {
            canvas.triangle(
                Pt::new(17.0, 14.0 + bob),
                Pt::new(2.0, 9.0 + bob + wave * 0.4),
                Pt::new(15.0, 20.0 + bob),
                detail,
            );
            canvas.line(
                Pt::new(24.0, 15.0 + bob),
                Pt::new(16.0, 7.0 + bob + wave),
                4.2,
                detail,
            );
            canvas.ellipse(Pt::new(27.0, 15.0 + bob), 11.5, 5.7, solid);
            canvas.disc(Pt::new(38.0, 11.8 + bob), 4.2, solid);
            canvas.triangle(
                Pt::new(41.0, 11.0 + bob),
                Pt::new(47.5, 13.0 + bob),
                Pt::new(41.0, 14.0 + bob),
                detail,
            );
            canvas.line(
                Pt::new(24.0, 19.0 + bob),
                Pt::new(22.0 + swing * 0.65, 27.0),
                2.0,
                solid,
            );
            canvas.line(
                Pt::new(30.0, 19.0 + bob),
                Pt::new(30.0 - swing * 0.65, 27.0),
                2.0,
                detail,
            );
            canvas.erase_disc(Pt::new(39.0, 10.9 + bob), 0.85);
        }
        RunnerCharacter::Duck => {
            canvas.ellipse(Pt::new(25.0, 16.0 + bob), 13.5, 6.5, solid);
            canvas.disc(Pt::new(38.0, 11.2 + bob), 5.0, solid);
            canvas.ellipse(Pt::new(45.0, 13.0 + bob), 4.6, 1.8, detail);
            canvas.triangle(
                Pt::new(13.0, 15.0 + bob),
                Pt::new(3.0, 11.5 + bob + wave * 0.3),
                Pt::new(13.0, 20.0 + bob),
                detail,
            );
            canvas.line(
                Pt::new(22.0, 20.0 + bob),
                Pt::new(20.0 + swing * 0.65, 27.0),
                2.6,
                detail,
            );
            canvas.line(
                Pt::new(29.0, 20.0 + bob),
                Pt::new(30.0 - swing * 0.65, 27.0),
                2.6,
                solid,
            );
            canvas.erase_disc(Pt::new(39.0, 10.0 + bob), 0.9);
        }
        RunnerCharacter::Sheep => {
            for (x, y, radius) in [
                (14.0, 15.0, 5.4),
                (20.0, 12.0, 6.0),
                (27.0, 13.0, 6.4),
                (32.0, 17.0, 6.0),
                (23.0, 18.0, 6.5),
            ] {
                canvas.disc(Pt::new(x, y + bob), radius, solid);
            }
            canvas.ellipse(Pt::new(39.0, 15.0 + bob), 6.5, 4.8, detail);
            canvas.line(
                Pt::new(18.0, 20.0 + bob),
                Pt::new(16.0 + swing * 0.55, 27.0),
                2.8,
                detail,
            );
            canvas.line(
                Pt::new(31.0, 20.0 + bob),
                Pt::new(32.0 - swing * 0.55, 27.0),
                2.8,
                solid,
            );
            canvas.disc(Pt::new(6.0, 15.0 + bob + wave * 0.3), 3.0, detail);
            canvas.erase_disc(Pt::new(41.0, 13.8 + bob), 0.9);
        }
        RunnerCharacter::Fish => {
            const TAIL_TOP: [f32; 8] = [-2.8, 0.4, 3.0, 1.0, -2.6, -0.4, 2.4, -1.8];
            const TAIL_BOTTOM: [f32; 8] = [2.4, -0.4, -2.8, -1.0, 2.2, 0.6, -2.0, 1.4];
            let tail_top = TAIL_TOP[pose] * (0.86 + load * 0.08);
            let tail_bottom = TAIL_BOTTOM[pose] * (0.86 + load * 0.08);
            canvas.triangle(
                Pt::new(16.0, 14.5),
                Pt::new(2.5, 8.5 + tail_top),
                Pt::new(4.0, 20.5 + tail_bottom),
                detail,
            );
            canvas.ellipse(Pt::new(28.0, 14.5), 14.0, 7.5, solid);
            canvas.triangle(
                Pt::new(24.0, 11.6),
                Pt::new(19.5, 4.8 + tail_top * 1.35),
                Pt::new(32.5, 14.0),
                detail,
            );
            canvas.triangle(
                Pt::new(22.0, 16.2),
                Pt::new(27.5 + tail_bottom * 1.35, 24.6),
                Pt::new(33.0, 16.8),
                detail,
            );
            canvas.line(
                Pt::new(23.0, 18.0),
                Pt::new(18.0 + tail_bottom * 0.5, 22.0),
                1.8,
                detail,
            );
            canvas.triangle(
                Pt::new(30.5, 14.0),
                Pt::new(39.5, 8.0 - tail_top * 1.2),
                Pt::new(36.5, 18.5),
                detail,
            );
            canvas.triangle(
                Pt::new(31.0, 16.4),
                Pt::new(39.0, 22.4 + tail_bottom * 0.95),
                Pt::new(35.8, 18.6),
                detail,
            );
            canvas.erase_disc(Pt::new(39.5, 12.2), 1.0);
        }
        RunnerCharacter::Slime => {
            const SQUASH: [f32; 8] = [-3.2, -2.0, -0.4, 1.5, 3.0, 1.8, 0.2, -1.9];
            const MIN_STRETCH: [f32; 8] = [-3.2, -2.5, -1.0, 0.0, 0.0, 0.0, -0.5, -2.0];
            let squash_motion = 0.60 + load * 1.80;
            let squash = (SQUASH[pose] * squash_motion).max(MIN_STRETCH[pose]);
            let body_ry = 8.8 - squash * 0.52;
            let body_rx = 14.4 + squash * 0.72;
            let body_center_y = 27.0 - body_ry;
            canvas.line(
                Pt::new(12.5, 17.0 - squash * 0.08),
                Pt::new(5.0, 13.2 - squash * 0.32),
                3.0,
                detail,
            );
            canvas.line(
                Pt::new(39.5, 17.0 - squash * 0.08),
                Pt::new(47.0, 13.2 + squash * 0.32),
                3.0,
                detail,
            );
            canvas.ellipse(Pt::new(26.0, body_center_y), body_rx, body_ry, solid);
            canvas.triangle(
                Pt::new(12.5, 20.0),
                Pt::new(17.0, 27.0),
                Pt::new(21.5, 20.0),
                solid,
            );
            canvas.triangle(
                Pt::new(21.0, 20.0),
                Pt::new(27.0, 27.0),
                Pt::new(33.0, 20.0),
                solid,
            );
            canvas.triangle(
                Pt::new(32.5, 20.0),
                Pt::new(38.0, 27.0),
                Pt::new(42.5, 20.0),
                solid,
            );
            canvas.erase_disc(Pt::new(22.0, body_center_y - 2.2), 1.1);
            canvas.erase_disc(Pt::new(31.0, body_center_y - 2.2), 1.1);
        }
        RunnerCharacter::Orbit => {
            let theta = pose as f32 / FRAME_COUNT as f32 * TAU;
            let orbit_motion = 1.5 * load;
            let orbit_lift = 6.0 * load.powi(16);
            let primary_orbit_radius = 14.5 + orbit_motion;
            let secondary_orbit_radius = 9.5 + orbit_motion;
            canvas.line(Pt::new(6.0, 16.0), Pt::new(44.0, 16.0), 2.0, detail);
            canvas.line(Pt::new(14.0, 8.0), Pt::new(36.0, 24.0), 1.8, detail);
            canvas.disc(Pt::new(25.0, 16.0), 6.0, solid);
            canvas.disc(
                Pt::new(
                    25.0 + theta.cos() * primary_orbit_radius,
                    16.0 + theta.sin() * (7.0 + orbit_lift),
                ),
                2.4,
                solid,
            );
            canvas.disc(
                Pt::new(
                    25.0 - theta.sin() * secondary_orbit_radius,
                    16.0 + theta.cos() * 8.0,
                ),
                1.8,
                detail,
            );
            canvas.erase_disc(Pt::new(25.0, 16.0), 2.3);
        }
        RunnerCharacter::Sushi => {
            let serving_motion = 0.55 + load.sqrt() * 0.76 + 5.0 * load.powi(16);
            let serving_bob = BOB[pose] * serving_motion;
            let serving_swing = SWING[pose] * serving_motion;
            let serving_wave = WAVE[pose] * serving_motion;
            canvas.line(Pt::new(2.0, 25.0), Pt::new(48.0, 25.0), 3.2, detail);
            for index in 0..5 {
                let x = 5.0 + ((index * 11 + pose * 3) % 48) as f32;
                canvas.disc(Pt::new(x, 27.0), 2.0, detail);
            }
            canvas.rect(15.0, 12.0 + serving_bob, 37.0, 22.0 + serving_bob, solid);
            canvas.ellipse(
                Pt::new(26.0 + serving_swing * 0.18, 10.5 + serving_bob),
                13.5,
                3.8,
                detail,
            );
            canvas.line(
                Pt::new(17.0, 15.0 + serving_bob),
                Pt::new(8.0, 10.0 + serving_bob + serving_wave * 0.3),
                2.0,
                detail,
            );
            canvas.erase_disc(Pt::new(26.0, 17.0 + serving_bob), 2.0);
        }
        RunnerCharacter::City => {
            let window_phase = (load * 3.0).round() as usize;
            canvas.rect(3.0, 14.0, 11.0, 27.0, solid);
            canvas.rect(12.0, 8.0, 20.0, 27.0, solid);
            canvas.rect(21.0, 12.0, 29.0, 27.0, solid);
            canvas.rect(30.0, 5.0, 39.0, 27.0, solid);
            canvas.rect(40.0, 16.0, 48.0, 27.0, solid);
            const WINDOWS: [(f32, f32, f32, f32); 8] = [
                (5.0, 17.0, 7.5, 20.0),
                (14.0, 11.0, 16.5, 14.0),
                (14.0, 18.0, 16.5, 21.0),
                (23.0, 15.0, 25.5, 18.0),
                (32.0, 9.0, 34.5, 12.0),
                (32.0, 16.0, 34.5, 19.0),
                (42.0, 18.0, 44.5, 21.0),
                (35.5, 22.0, 38.0, 25.0),
            ];
            const WINDOW_MASKS: [u8; 8] = [
                0b0011_1001,
                0b0110_0110,
                0b1100_0011,
                0b1011_1100,
                0b0101_1010,
                0b0011_0111,
                0b1110_0101,
                0b1001_1100,
            ];
            for (index, (left, top, right, bottom)) in WINDOWS.into_iter().enumerate() {
                if (WINDOW_MASKS[(pose + window_phase) % FRAME_COUNT as usize] >> index) & 1 == 1 {
                    canvas.erase_rect(left, top, right, bottom);
                }
            }
            for (x, y) in [
                (9.5, 14.0),
                (18.0, 8.0),
                (27.0, 12.0),
                (39.0, 5.0),
                (46.0, 16.0),
            ] {
                canvas.line(Pt::new(x, y), Pt::new(x, y - 2.0), 1.0, detail);
            }
        }
        RunnerCharacter::Sausage => {
            canvas.ellipse(Pt::new(25.0, 14.5 + bob), 17.0 + load, 6.2, solid);
            canvas.line(
                Pt::new(10.0, 13.5 + bob),
                Pt::new(5.0, 9.0 + bob + swing * 0.38),
                2.4,
                detail,
            );
            canvas.line(
                Pt::new(39.0, 13.5 + bob),
                Pt::new(45.0, 9.0 + bob - swing * 0.38),
                2.4,
                detail,
            );
            canvas.line(
                Pt::new(19.0, 19.0 + bob),
                Pt::new(17.0 + swing * 0.7, 27.0),
                2.7,
                detail,
            );
            canvas.line(
                Pt::new(31.0, 19.0 + bob),
                Pt::new(32.0 - swing * 0.7, 27.0),
                2.7,
                solid,
            );
            canvas.line(
                Pt::new(8.5, 12.0 + bob),
                Pt::new(8.5, 17.0 + bob),
                1.8,
                detail,
            );
            canvas.line(
                Pt::new(41.5, 12.0 + bob),
                Pt::new(41.5, 17.0 + bob),
                1.8,
                detail,
            );
            canvas.erase_disc(Pt::new(34.8, 13.0 + bob), 0.85);
            canvas.erase_disc(Pt::new(38.0, 13.0 + bob), 0.85);
            canvas.erase_disc(Pt::new(36.5, 16.0 + bob), 0.75);
        }
        RunnerCharacter::Party => {
            for (index, x) in [12.0, 25.0, 38.0].into_iter().enumerate() {
                let offset = if index % 2 == 0 { wave } else { -wave };
                let local_bob = bob + offset * 0.18;
                canvas.disc(Pt::new(x, 7.0 + local_bob), 3.0, solid);
                canvas.line(
                    Pt::new(x, 10.0 + local_bob),
                    Pt::new(x, 19.0 + local_bob),
                    2.8,
                    solid,
                );
                canvas.line(
                    Pt::new(x, 12.0 + local_bob),
                    Pt::new(x - 7.0, 8.0 + local_bob + offset * 0.45),
                    2.3,
                    detail,
                );
                canvas.line(
                    Pt::new(x, 12.0 + local_bob),
                    Pt::new(x + 7.0, 8.0 + local_bob - offset * 0.45),
                    2.3,
                    detail,
                );
                canvas.line(
                    Pt::new(x, 19.0 + local_bob),
                    Pt::new(x - 5.0 - swing * 0.2, 27.0),
                    2.5,
                    solid,
                );
                canvas.line(
                    Pt::new(x, 19.0 + local_bob),
                    Pt::new(x + 5.0 + swing * 0.2, 27.0),
                    2.5,
                    detail,
                );
            }
        }
        _ => unreachable!("non-novelty runner reached novelty renderer"),
    }

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
