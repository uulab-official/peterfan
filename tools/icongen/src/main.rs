use tiny_skia::*;

const SZ: f32 = 1024.0;
const S: f32 = 9.0;   // fan scale (0..100 space -> px)
const O: f32 = 62.0;  // fan offset to center at 512

fn p(pb: &mut PathBuilder, x: f32, y: f32) -> (f32, f32) {
    let _ = pb;
    (x * S + O, y * S + O)
}

fn blade(pb: &mut PathBuilder, c: [(f32, f32); 5]) {
    let m = p(pb, c[0].0, c[0].1);
    pb.move_to(m.0, m.1);
    let a = (c[1].0 * S + O, c[1].1 * S + O);
    let b = (c[2].0 * S + O, c[2].1 * S + O);
    let d = (c[3].0 * S + O, c[3].1 * S + O);
    pb.cubic_to(a.0, a.1, b.0, b.1, d.0, d.1);
    let e = (c[4].0 * S + O, c[4].1 * S + O);
    // second cubic back to center; reuse stored control points inline
    pb.cubic_to(e.0, e.1, 50.0 * S + O, 50.0 * S + O, 50.0 * S + O, 50.0 * S + O);
    pb.close();
}

fn rrect(pb: &mut PathBuilder, x: f32, y: f32, w: f32, h: f32, r: f32) {
    let k = r * 0.5522847;
    pb.move_to(x + r, y);
    pb.line_to(x + w - r, y);
    pb.cubic_to(x + w - r + k, y, x + w, y + r - k, x + w, y + r);
    pb.line_to(x + w, y + h - r);
    pb.cubic_to(x + w, y + h - r + k, x + w - r + k, y + h, x + w - r, y + h);
    pb.line_to(x + r, y + h);
    pb.cubic_to(x + r - k, y + h, x, y + h - r + k, x, y + h - r);
    pb.line_to(x, y + r);
    pb.cubic_to(x, y + r - k, x + r - k, y, x + r, y);
    pb.close();
}

fn main() {
    let mut pm = Pixmap::new(SZ as u32, SZ as u32).unwrap();

    // Background squircle with a teal->sky->blue diagonal gradient.
    let bg = {
        let mut pb = PathBuilder::new();
        rrect(&mut pb, 96.0, 96.0, 832.0, 832.0, 196.0);
        pb.finish().unwrap()
    };
    let mut paint = Paint::default();
    paint.anti_alias = true;
    paint.shader = LinearGradient::new(
        Point::from_xy(150.0, 120.0),
        Point::from_xy(900.0, 940.0),
        vec![
            GradientStop::new(0.0, Color::from_rgba8(94, 234, 212, 255)),
            GradientStop::new(0.5, Color::from_rgba8(56, 189, 248, 255)),
            GradientStop::new(1.0, Color::from_rgba8(37, 99, 235, 255)),
        ],
        SpreadMode::Pad,
        Transform::identity(),
    )
    .unwrap();
    pm.fill_path(&bg, &paint, FillRule::Winding, Transform::identity(), None);

    // Fan blades (white).
    let blades = {
        let mut pb = PathBuilder::new();
        blade(&mut pb, [(50.0, 50.0), (50.0, 20.0), (78.0, 22.0), (70.0, 42.0), (62.0, 60.0)]);
        blade(&mut pb, [(50.0, 50.0), (80.0, 50.0), (78.0, 78.0), (58.0, 70.0), (40.0, 62.0)]);
        blade(&mut pb, [(50.0, 50.0), (50.0, 80.0), (22.0, 78.0), (30.0, 58.0), (38.0, 40.0)]);
        blade(&mut pb, [(50.0, 50.0), (20.0, 50.0), (22.0, 22.0), (42.0, 30.0), (60.0, 38.0)]);
        pb.finish().unwrap()
    };
    let mut white = Paint::default();
    white.anti_alias = true;
    white.shader = Shader::SolidColor(Color::from_rgba8(255, 255, 255, 240));
    pm.fill_path(&blades, &white, FillRule::Winding, Transform::identity(), None);

    // Center hub.
    let hub = PathBuilder::from_circle(512.0, 512.0, 54.0).unwrap();
    let mut hubp = Paint::default();
    hubp.anti_alias = true;
    hubp.shader = Shader::SolidColor(Color::from_rgba8(255, 255, 255, 255));
    pm.fill_path(&hub, &hubp, FillRule::Winding, Transform::identity(), None);

    let out = std::env::args().nth(1).unwrap_or_else(|| "assets/icon-1024.png".to_string());
    pm.save_png(&out).unwrap();
    println!("wrote {out}");
}
