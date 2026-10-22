use super::{RgbaFrame, PIXEL_HEIGHT, PIXEL_WIDTH};

const RUNNER_STROKE_SCALE: f32 = 0.82;

#[derive(Clone, Copy)]
pub(crate) struct Pt {
    pub(crate) x: f32,
    pub(crate) y: f32,
}

impl Pt {
    pub(crate) const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

pub(crate) struct RunnerCanvas {
    rgba: RgbaFrame,
}

impl RunnerCanvas {
    pub(crate) fn new() -> Self {
        Self {
            rgba: vec![0u8; (PIXEL_WIDTH * PIXEL_HEIGHT * 4) as usize],
        }
    }

    pub(crate) fn finish(self) -> RgbaFrame {
        self.rgba
    }

    pub(crate) fn line(&mut self, start: Pt, end: Pt, thickness: f32, color: (u8, u8, u8, u8)) {
        draw_line(
            &mut self.rgba,
            PIXEL_WIDTH,
            PIXEL_HEIGHT,
            start,
            end,
            thickness,
            color,
        );
    }

    pub(crate) fn disc(&mut self, center: Pt, radius: f32, color: (u8, u8, u8, u8)) {
        draw_disc(
            &mut self.rgba,
            PIXEL_WIDTH,
            PIXEL_HEIGHT,
            center,
            radius,
            color,
        );
    }

    pub(crate) fn ellipse(&mut self, center: Pt, rx: f32, ry: f32, color: (u8, u8, u8, u8)) {
        draw_ellipse(
            &mut self.rgba,
            PIXEL_WIDTH,
            PIXEL_HEIGHT,
            center,
            rx,
            ry,
            color,
        );
    }

    pub(crate) fn triangle(&mut self, a: Pt, b: Pt, c: Pt, color: (u8, u8, u8, u8)) {
        draw_triangle(&mut self.rgba, PIXEL_WIDTH, PIXEL_HEIGHT, a, b, c, color);
    }

    pub(crate) fn rect(
        &mut self,
        left: f32,
        top: f32,
        right: f32,
        bottom: f32,
        color: (u8, u8, u8, u8),
    ) {
        draw_rect(
            &mut self.rgba,
            PIXEL_WIDTH,
            PIXEL_HEIGHT,
            left,
            top,
            right,
            bottom,
            color,
        );
    }

    pub(crate) fn erase_rect(&mut self, left: f32, top: f32, right: f32, bottom: f32) {
        erase_rect(
            &mut self.rgba,
            PIXEL_WIDTH,
            PIXEL_HEIGHT,
            left,
            top,
            right,
            bottom,
        );
    }

    pub(crate) fn erase_disc(&mut self, center: Pt, radius: f32) {
        erase_disc(&mut self.rgba, PIXEL_WIDTH, PIXEL_HEIGHT, center, radius);
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn draw_rect(
    rgba: &mut [u8],
    w: u32,
    h: u32,
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
    color: (u8, u8, u8, u8),
) {
    let min_x = left.floor().max(0.0) as u32;
    let max_x = right.ceil().min((w - 1) as f32) as u32;
    let min_y = top.floor().max(0.0) as u32;
    let max_y = bottom.ceil().min((h - 1) as f32) as u32;
    for y in min_y..=max_y {
        for x in min_x..=max_x {
            let edge_x = (x as f32 + 0.5 - left).min(right - (x as f32 + 0.5));
            let edge_y = (y as f32 + 0.5 - top).min(bottom - (y as f32 + 0.5));
            let coverage = edge_x.min(edge_y).clamp(0.0, 1.0);
            if coverage > 0.0 {
                blend_pixel(rgba, w, x, y, color, coverage);
            }
        }
    }
}

pub(crate) fn erase_disc(rgba: &mut [u8], w: u32, h: u32, center: Pt, radius: f32) {
    let min_x = (center.x - radius - 1.0).floor().max(0.0) as u32;
    let max_x = (center.x + radius + 1.0).ceil().min((w - 1) as f32) as u32;
    let min_y = (center.y - radius - 1.0).floor().max(0.0) as u32;
    let max_y = (center.y + radius + 1.0).ceil().min((h - 1) as f32) as u32;
    for y in min_y..=max_y {
        for x in min_x..=max_x {
            let dx = x as f32 - center.x;
            let dy = y as f32 - center.y;
            let distance = (dx * dx + dy * dy).sqrt();
            let coverage = (radius + 0.5 - distance).clamp(0.0, 1.0);
            if coverage > 0.0 {
                let index = ((y * w + x) * 4) as usize;
                rgba[index + 3] = (f32::from(rgba[index + 3]) * (1.0 - coverage)).round() as u8;
            }
        }
    }
}

pub(crate) fn erase_rect(
    rgba: &mut [u8],
    w: u32,
    h: u32,
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
) {
    let min_x = left.floor().max(0.0) as u32;
    let max_x = right.ceil().min((w - 1) as f32) as u32;
    let min_y = top.floor().max(0.0) as u32;
    let max_y = bottom.ceil().min((h - 1) as f32) as u32;
    for y in min_y..=max_y {
        for x in min_x..=max_x {
            let edge_x = (x as f32 + 0.5 - left).min(right - (x as f32 + 0.5));
            let edge_y = (y as f32 + 0.5 - top).min(bottom - (y as f32 + 0.5));
            let coverage = edge_x.min(edge_y).clamp(0.0, 1.0);
            if coverage > 0.0 {
                let index = ((y * w + x) * 4) as usize;
                rgba[index + 3] = (f32::from(rgba[index + 3]) * (1.0 - coverage)).round() as u8;
            }
        }
    }
}

pub(crate) fn draw_disc(
    rgba: &mut [u8],
    w: u32,
    h: u32,
    center: Pt,
    radius: f32,
    color: (u8, u8, u8, u8),
) {
    let min_x = (center.x - radius - 1.0).floor().max(0.0) as u32;
    let max_x = (center.x + radius + 1.0).ceil().min((w - 1) as f32) as u32;
    let min_y = (center.y - radius - 1.0).floor().max(0.0) as u32;
    let max_y = (center.y + radius + 1.0).ceil().min((h - 1) as f32) as u32;
    for y in min_y..=max_y {
        for x in min_x..=max_x {
            let dx = x as f32 - center.x;
            let dy = y as f32 - center.y;
            let dist = (dx * dx + dy * dy).sqrt();
            if dist <= radius + 0.5 {
                let coverage = (radius + 0.5 - dist).clamp(0.0, 1.0);
                blend_pixel(rgba, w, x, y, color, coverage);
            }
        }
    }
}

pub(crate) fn draw_ellipse(
    rgba: &mut [u8],
    w: u32,
    h: u32,
    center: Pt,
    rx: f32,
    ry: f32,
    color: (u8, u8, u8, u8),
) {
    draw_rotated_ellipse(rgba, w, h, center, rx, ry, 0.0, color);
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn draw_rotated_ellipse(
    rgba: &mut [u8],
    w: u32,
    h: u32,
    center: Pt,
    rx: f32,
    ry: f32,
    angle: f32,
    color: (u8, u8, u8, u8),
) {
    let radius = rx.max(ry) + 1.0;
    let min_x = (center.x - radius).floor().max(0.0) as u32;
    let max_x = (center.x + radius).ceil().min((w - 1) as f32) as u32;
    let min_y = (center.y - radius).floor().max(0.0) as u32;
    let max_y = (center.y + radius).ceil().min((h - 1) as f32) as u32;
    let sin = angle.sin();
    let cos = angle.cos();
    for y in min_y..=max_y {
        for x in min_x..=max_x {
            let offset_x = x as f32 - center.x;
            let offset_y = y as f32 - center.y;
            let dx = (offset_x * cos + offset_y * sin) / rx;
            let dy = (-offset_x * sin + offset_y * cos) / ry;
            let dist = (dx * dx + dy * dy).sqrt();
            let edge_distance = (1.0 - dist) * rx.min(ry);
            let coverage = (edge_distance + 0.5).clamp(0.0, 1.0);
            if coverage > 0.0 {
                blend_pixel(rgba, w, x, y, color, coverage);
            }
        }
    }
}

pub(crate) fn draw_triangle(
    rgba: &mut [u8],
    w: u32,
    h: u32,
    a: Pt,
    b: Pt,
    c: Pt,
    color: (u8, u8, u8, u8),
) {
    let min_x = a.x.min(b.x).min(c.x).floor().max(0.0) as u32;
    let max_x = a.x.max(b.x).max(c.x).ceil().min((w - 1) as f32) as u32;
    let min_y = a.y.min(b.y).min(c.y).floor().max(0.0) as u32;
    let max_y = a.y.max(b.y).max(c.y).ceil().min((h - 1) as f32) as u32;
    let area = edge(a, b, c);
    if area.abs() < f32::EPSILON {
        return;
    }
    for y in min_y..=max_y {
        for x in min_x..=max_x {
            let p = Pt::new(x as f32 + 0.5, y as f32 + 0.5);
            let w0 = edge(b, c, p);
            let w1 = edge(c, a, p);
            let w2 = edge(a, b, p);
            if (w0 >= 0.0 && w1 >= 0.0 && w2 >= 0.0) || (w0 <= 0.0 && w1 <= 0.0 && w2 <= 0.0) {
                blend_pixel(rgba, w, x, y, color, 1.0);
            }
        }
    }
}

fn edge(a: Pt, b: Pt, p: Pt) -> f32 {
    (p.x - a.x) * (b.y - a.y) - (p.y - a.y) * (b.x - a.x)
}

pub(crate) fn draw_line(
    rgba: &mut [u8],
    w: u32,
    h: u32,
    from: Pt,
    to: Pt,
    thickness: f32,
    color: (u8, u8, u8, u8),
) {
    let thickness = (thickness * RUNNER_STROKE_SCALE).max(1.0);
    let steps = ((to.x - from.x).abs().max((to.y - from.y).abs()) * 2.0)
        .ceil()
        .max(1.0) as u32;
    for i in 0..=steps {
        let t = i as f32 / steps as f32;
        let x = from.x + (to.x - from.x) * t;
        let y = from.y + (to.y - from.y) * t;
        draw_disc(rgba, w, h, Pt::new(x, y), thickness / 2.0, color);
    }
}

fn blend_pixel(rgba: &mut [u8], w: u32, x: u32, y: u32, color: (u8, u8, u8, u8), coverage: f32) {
    let idx = ((y * w + x) * 4) as usize;
    if idx + 3 >= rgba.len() {
        return;
    }
    let alpha = (color.3 as f32 * coverage).round().clamp(0.0, 255.0) as u8;
    if alpha >= rgba[idx + 3] {
        rgba[idx] = color.0;
        rgba[idx + 1] = color.1;
        rgba[idx + 2] = color.2;
        rgba[idx + 3] = alpha;
    }
}
