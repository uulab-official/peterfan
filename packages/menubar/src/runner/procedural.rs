use peterfan_core::config::RunnerCharacter;

use super::draw::{
    draw_disc, draw_line, draw_rect, draw_rotated_ellipse, draw_runner_leg, draw_triangle,
    erase_disc, Pt, RunnerCanvas,
};
use super::{RgbaFrame, FRAME_COUNT, PIXEL_HEIGHT, PIXEL_WIDTH};

pub(crate) fn render(character: RunnerCharacter, cpu_pct: f32, frame: u8) -> RgbaFrame {
    match character {
        RunnerCharacter::Penguin => make_penguin_runner_rgba(cpu_pct, frame),
        RunnerCharacter::Robot => make_robot_runner_rgba(cpu_pct, frame),
        RunnerCharacter::Bird
        | RunnerCharacter::Duck
        | RunnerCharacter::Sheep
        | RunnerCharacter::Fish
        | RunnerCharacter::Slime
        | RunnerCharacter::Pushup
        | RunnerCharacter::Orbit
        | RunnerCharacter::Sushi
        | RunnerCharacter::City
        | RunnerCharacter::Sausage
        | RunnerCharacter::Party => make_novelty_runner_rgba(character, cpu_pct, frame),
        _ => unreachable!("asset-backed runner reached procedural renderer"),
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
            let tail_wave = wave * 0.70;
            canvas.triangle(
                Pt::new(16.0, 14.0 + bob),
                Pt::new(2.0, 6.0 + bob + tail_wave),
                Pt::new(3.0, 23.0 + bob - tail_wave * 0.45),
                detail,
            );
            canvas.ellipse(Pt::new(28.0, 14.5 + bob), 14.0 + load, 7.5, solid);
            canvas.triangle(
                Pt::new(25.0, 15.0 + bob),
                Pt::new(21.0 + swing * 0.25, 27.0 + bob),
                Pt::new(32.0, 18.0 + bob),
                detail,
            );
            canvas.disc(Pt::new(44.0, 6.0 + wave * 0.25), 1.8, detail);
            canvas.disc(Pt::new(47.0, 2.5 + wave * 0.18), 1.1, detail);
            canvas.erase_disc(Pt::new(39.5, 12.0 + bob), 1.0);
        }
        RunnerCharacter::Slime => {
            canvas.line(
                Pt::new(14.0, 18.0 + bob),
                Pt::new(3.0, 14.0 + bob + swing * 0.45),
                4.0,
                detail,
            );
            canvas.line(
                Pt::new(37.0, 18.0 + bob),
                Pt::new(48.0, 14.0 + bob - swing * 0.45),
                4.0,
                detail,
            );
            canvas.ellipse(Pt::new(26.0, 17.0 + bob), 15.5 + load, 9.4, solid);
            canvas.triangle(
                Pt::new(12.0, 20.0 + bob),
                Pt::new(17.0, 28.0 + bob + wave * 0.2),
                Pt::new(22.0, 20.0 + bob),
                solid,
            );
            canvas.triangle(
                Pt::new(22.0, 20.0 + bob),
                Pt::new(28.0, 27.0 + bob - wave * 0.18),
                Pt::new(34.0, 20.0 + bob),
                solid,
            );
            canvas.triangle(
                Pt::new(33.0, 20.0 + bob),
                Pt::new(39.0, 27.0 + bob + wave * 0.15),
                Pt::new(42.0, 19.0 + bob),
                solid,
            );
            canvas.erase_disc(Pt::new(22.0, 14.5 + bob), 1.15);
            canvas.erase_disc(Pt::new(31.0, 14.5 + bob), 1.15);
        }
        RunnerCharacter::Pushup => {
            let flex = (((pose as f32 - 3.5).abs() - 1.75) * motion).clamp(-2.2, 3.2);
            let hip = Pt::new(11.0, 12.0 + flex);
            let shoulder = Pt::new(35.0, 14.0 + flex * 0.72);
            let elbow = Pt::new(31.0 + swing * 0.18, 21.5 + flex * 0.45);
            let hand = Pt::new(39.0 + swing * 0.12, 27.0);
            canvas.line(hip, shoulder, 4.3, solid);
            canvas.disc(Pt::new(41.0, 12.5 + flex * 0.72), 3.8, solid);
            canvas.line(shoulder, elbow, 3.0, detail);
            canvas.line(elbow, hand, 3.0, solid);
            canvas.line(hip, Pt::new(2.0, 25.5), 3.2, detail);
            canvas.line(Pt::new(3.0, 25.5), Pt::new(9.0, 26.0), 2.3, detail);
            canvas.erase_disc(Pt::new(42.2, 11.5 + flex * 0.72), 0.8);
        }
        RunnerCharacter::Orbit => {
            canvas.line(
                Pt::new(3.0, 16.0 + wave * 0.25),
                Pt::new(45.0, 16.0 - wave * 0.25),
                2.0,
                detail,
            );
            canvas.line(
                Pt::new(8.0, 7.0 - wave * 0.2),
                Pt::new(41.0, 25.0 + wave * 0.2),
                1.8,
                detail,
            );
            canvas.disc(Pt::new(25.0, 16.0 + bob), 6.0 + load, solid);
            canvas.disc(
                Pt::new(7.0 + (pose as f32 * 5.1) % 38.0, 8.0 + wave),
                2.5,
                solid,
            );
            canvas.disc(
                Pt::new(43.0 - (pose as f32 * 4.2) % 35.0, 24.0 - wave * 0.7),
                2.0,
                detail,
            );
            canvas.erase_disc(Pt::new(25.0, 16.0 + bob), 2.3);
        }
        RunnerCharacter::Sushi => {
            canvas.line(Pt::new(2.0, 25.0), Pt::new(48.0, 25.0), 3.2, detail);
            for index in 0..5 {
                let x = 5.0 + ((index * 11 + pose * 3) % 48) as f32;
                canvas.disc(Pt::new(x, 27.0), 2.0, detail);
            }
            canvas.rect(15.0, 12.0 + bob, 37.0, 22.0 + bob, solid);
            canvas.ellipse(Pt::new(26.0 + swing * 0.18, 10.5 + bob), 13.5, 3.8, detail);
            canvas.line(
                Pt::new(17.0, 15.0 + bob),
                Pt::new(8.0, 10.0 + bob + wave * 0.3),
                2.0,
                detail,
            );
            canvas.erase_disc(Pt::new(26.0, 17.0 + bob), 2.0);
        }
        RunnerCharacter::City => {
            canvas.rect(3.0, 14.0, 11.0, 27.0, solid);
            canvas.rect(12.0, 8.0 + bob * 0.2, 20.0, 27.0, solid);
            canvas.rect(21.0, 12.0, 29.0, 27.0, solid);
            canvas.rect(30.0, 5.0, 39.0, 27.0, solid);
            canvas.rect(40.0, 16.0, 48.0, 27.0, solid);
            for (x, y) in [
                (7.0, 18.0),
                (16.0, 12.0),
                (25.0, 16.0),
                (34.5, 9.0),
                (44.0, 20.0),
            ] {
                canvas.erase_disc(Pt::new(x, y), 1.1);
            }
            canvas.disc(Pt::new(35.0 + swing * 0.35, 2.0 + wave * 0.2), 2.0, detail);
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
