//! Shared pixel and layout logic for one menu-bar item.
use peterfan_core::config::{CpuWidgetConfig, CpuWidgetStyle, MenuItemsVisibility};

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct CompositeFrame {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) rgba: Vec<u8>,
}

pub(crate) fn compose(
    visibility: MenuItemsVisibility,
    cpu_style: CpuWidgetStyle,
    cpu_width: u32,
    cpu: &[u8],
    runner: &[u8],
    runner_is_color: bool,
    _foreground: [u8; 3],
) -> Option<CompositeFrame> {
    const HEIGHT: u32 = 44;
    const RUNNER_WIDTH: u32 = 66;
    let show_cpu = visibility.cpu && cpu_style != CpuWidgetStyle::Number;
    if !show_cpu && !visibility.runner {
        return None;
    }
    if show_cpu && (cpu_width == 0 || cpu.len() != cpu_width as usize * 36 * 4) {
        return None;
    }
    if visibility.runner && runner.len() != 48 * 32 * 4 {
        return None;
    }
    let gap = if show_cpu && visibility.runner { 4 } else { 0 };
    let width = (if show_cpu { cpu_width } else { 0 })
        + gap
        + if visibility.runner { RUNNER_WIDTH } else { 0 };
    let mut rgba = vec![0; (width * HEIGHT * 4) as usize];
    if show_cpu {
        for y in 0..36_u32 {
            for x in 0..cpu_width {
                let src = ((y * cpu_width + x) * 4) as usize;
                let dst = (((y + 4) * width + x) * 4) as usize;
                let alpha = cpu[src + 3];
                if cpu_style == CpuWidgetStyle::History {
                    // A one-pixel history line cannot contrast with every
                    // possible translucent menu-bar background. Give this
                    // chart its own small, opaque, neutral backdrop. Keep
                    // source opacity as the line/guide strength, while the
                    // other CPU styles retain their original transparency.
                    let corner = (x == 0 || x + 1 == cpu_width) && (y == 0 || y == 35);
                    if corner && alpha == 0 {
                        continue;
                    }
                    const BACKGROUND: u32 = 32;
                    const INK: u32 = 245;
                    let value =
                        ((BACKGROUND * (255 - u32::from(alpha)) + INK * u32::from(alpha) + 127)
                            / 255) as u8;
                    rgba[dst..dst + 4].copy_from_slice(&[value, value, value, 255]);
                    continue;
                }
                if alpha == 0 {
                    continue;
                }
                // Keep every alpha value, including faint 0% tracks and the
                // transparent gaps between cores. Only tint *inside* opaque
                // strokes: opposite edges contrast against light and dark
                // menu bars even when a colored image is replicated.
                let ink = contrast_ink(cpu, cpu_width, x, y, false);
                rgba[dst..dst + 4].copy_from_slice(&[ink, ink, ink, alpha]);
            }
        }
    }
    if visibility.runner {
        let left = if show_cpu { cpu_width + gap } else { 0 };
        for y in 0..HEIGHT {
            for x in 0..RUNNER_WIDTH {
                let src_y = y * 32 / HEIGHT;
                let src_x = x * 48 / RUNNER_WIDTH;
                let src = ((src_y * 48 + src_x) * 4) as usize;
                let dst = ((y * width + left + x) * 4) as usize;
                rgba[dst..dst + 4].copy_from_slice(&runner[src..src + 4]);
                if !runner_is_color {
                    let ink = contrast_ink(runner, 48, src_x, src_y, false);
                    rgba[dst..dst + 3].copy_from_slice(&[ink, ink, ink]);
                }
            }
        }
    }
    Some(CompositeFrame {
        width,
        height: HEIGHT,
        rgba,
    })
}

/// Keep source alpha unchanged; use opposite inner edge tones on replicated
/// displays so both dark and light menu bars retain a contrasting stroke.
fn contrast_ink(pixels: &[u8], width: u32, x: u32, y: u32, stable_thin_stroke: bool) -> u8 {
    let alpha = pixels[((y * width + x) * 4 + 3) as usize];
    if alpha < 128 {
        return 118;
    }
    let opaque = |nx: u32| pixels[((y * width + nx) * 4 + 3) as usize] >= 128;
    let white = x == 0 || !opaque(x - 1);
    let black = x + 1 == width || !opaque(x + 1);
    if white && black {
        if stable_thin_stroke {
            118
        } else if (x + y) & 1 == 0 {
            255
        } else {
            0
        }
    } else if white {
        255
    } else if black {
        0
    } else {
        118
    }
}

pub(crate) fn title(
    visibility: MenuItemsVisibility,
    cpu: CpuWidgetConfig,
    cpu_title: &str,
    metric: &str,
) -> String {
    let cpu_text = if visibility.cpu && (cpu.style == CpuWidgetStyle::Number || cpu.show_percentage)
    {
        cpu_title
    } else {
        ""
    };
    let metric_text = if visibility.metric { metric } else { "" };
    match (cpu_text.is_empty(), metric_text.is_empty()) {
        (false, false) => format!("{cpu_text} {metric_text}"),
        (false, true) => cpu_text.to_string(),
        (true, false) => metric_text.to_string(),
        (true, true) => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seven_visibility_combinations_preserve_each_requested_component() {
        let config = CpuWidgetConfig {
            style: CpuWidgetStyle::Bar,
            show_percentage: true,
            ..Default::default()
        };
        let mut cpu = vec![0; 4 * 36 * 4];
        cpu[..4].copy_from_slice(&[0, 0, 0, 255]);
        let mut runner = vec![0; 48 * 32 * 4];
        runner[..4].copy_from_slice(&[23, 127, 207, 255]);
        for flags in 1..8 {
            let v = MenuItemsVisibility {
                cpu: flags & 1 != 0,
                runner: flags & 2 != 0,
                metric: flags & 4 != 0,
            };
            let rendered = compose(v, config.style, 4, &cpu, &runner, true, [240, 240, 240]);
            assert_eq!(rendered.is_some(), v.cpu || v.runner, "{flags}");
            if let Some(image) = rendered {
                assert_eq!(image.rgba.len(), (image.width * image.height * 4) as usize);
                assert_eq!(
                    image
                        .rgba
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .any(|p| p[3] == 255 && p[0] == p[1] && p[1] == p[2]),
                    v.cpu
                );
                assert_eq!(
                    image.rgba.as_chunks::<4>().0.contains(&[23, 127, 207, 255]),
                    v.runner
                );
            }
            let text = title(v, config, " 37%", " 65°C");
            assert_eq!(text.contains("37%"), v.cpu, "{flags}");
            assert_eq!(text.contains("65°C"), v.metric, "{flags}");
        }
    }

    #[test]
    fn cpu_number_only_is_text_without_graph_image() {
        let v = MenuItemsVisibility {
            cpu: true,
            runner: false,
            metric: false,
        };
        let config = CpuWidgetConfig {
            style: CpuWidgetStyle::Number,
            ..Default::default()
        };
        assert!(compose(v, config.style, 0, &[], &[], false, [0, 0, 0]).is_none());
        assert_eq!(title(v, config, "CPU  37%", " 65°C"), "CPU  37%");
    }

    #[test]
    fn cpu_graph_retains_a_contrast_edge_on_dark_light_and_highlight_backgrounds() {
        let v = MenuItemsVisibility {
            cpu: true,
            runner: false,
            metric: false,
        };
        let mut cpu = vec![0; 4 * 36 * 4];
        for y in 4..32 {
            for x in 1..3 {
                cpu[(y * 4 + x) * 4 + 3] = 255;
            }
        }
        let image = compose(v, CpuWidgetStyle::Cores, 4, &cpu, &[], false, [0, 0, 0]).unwrap();
        let edge = |x: usize| &image.rgba[((14 * 4 + x) * 4)..((14 * 4 + x) * 4 + 4)];
        let luminance = |rgb: [u8; 3]| -> f64 {
            let linear = |v: u8| {
                let v = f64::from(v) / 255.0;
                if v <= 0.04045 {
                    v / 12.92
                } else {
                    ((v + 0.055) / 1.055).powf(2.4)
                }
            };
            0.2126 * linear(rgb[0]) + 0.7152 * linear(rgb[1]) + 0.0722 * linear(rgb[2])
        };
        for background in [[0, 0, 0], [255, 255, 255], [118, 118, 118], [38, 100, 180]] {
            let bg = luminance(background);
            let ratio = [edge(1), edge(2)]
                .into_iter()
                .map(|pixel| {
                    let a = u32::from(pixel[3]);
                    let rgb = std::array::from_fn(|i| {
                        ((u32::from(pixel[i]) * a + u32::from(background[i]) * (255 - a)) / 255)
                            as u8
                    });
                    let value = luminance(rgb);
                    (bg.max(value) + 0.05) / (bg.min(value) + 0.05)
                })
                .fold(0.0_f64, f64::max);
            assert!(
                ratio >= 3.0,
                "insufficient edge contrast {ratio} on {background:?}"
            );
        }
    }

    #[test]
    fn core_tracks_keep_their_alpha_and_empty_gaps_at_zero_and_full_load() {
        let mut history = crate::cpu_widget::CpuWidgetHistory::default();
        history.sample_cpu_at(50.0, &[0.0, 100.0], std::time::Instant::now());
        let width = history.pixel_width(CpuWidgetStyle::Cores);
        let source = history.render_rgba(CpuWidgetStyle::Cores);
        let v = MenuItemsVisibility {
            cpu: true,
            runner: false,
            metric: false,
        };
        let image = compose(
            v,
            CpuWidgetStyle::Cores,
            width,
            &source,
            &[],
            false,
            [0, 0, 0],
        )
        .unwrap();
        for y in 0..36_u32 {
            for x in 0..width {
                let src = ((y * width + x) * 4 + 3) as usize;
                let dst = (((y + 4) * width + x) * 4 + 3) as usize;
                assert_eq!(image.rgba[dst], source[src], "alpha changed at {x},{y}");
            }
            for x in [8_u32, 9] {
                assert_eq!(
                    image.rgba[(((y + 4) * width + x) * 4 + 3) as usize],
                    0,
                    "core gap filled at {x},{y}"
                );
            }
        }
        for y in 3..33_u32 {
            assert_eq!(
                image.rgba[(((y + 4) * width + 3) * 4 + 3) as usize],
                48,
                "0% track appears active"
            );
            assert_eq!(
                image.rgba[(((y + 4) * width + 11) * 4 + 3) as usize],
                255,
                "100% bar missing"
            );
        }
    }

    #[test]
    fn bar_and_core_styles_keep_original_alpha_including_faint_guides() {
        let mut history = crate::cpu_widget::CpuWidgetHistory::default();
        history.sample_cpu_at(50.0, &[0.0, 100.0], std::time::Instant::now());
        let v = MenuItemsVisibility {
            cpu: true,
            runner: false,
            metric: false,
        };
        for style in [CpuWidgetStyle::Bar, CpuWidgetStyle::Cores] {
            let width = history.pixel_width(style);
            let source = history.render_rgba(style);
            let image = compose(v, style, width, &source, &[], false, [0, 0, 0]).unwrap();
            for y in 0..36_u32 {
                for x in 0..width {
                    let src = ((y * width + x) * 4 + 3) as usize;
                    let dst = (((y + 4) * width + x) * 4 + 3) as usize;
                    assert_eq!(image.rgba[dst], source[src], "{style:?} at {x},{y}");
                }
            }
        }
    }

    #[test]
    fn legacy_runner_composited_with_cpu_has_light_and_dark_strokes() {
        let v = MenuItemsVisibility {
            cpu: true,
            runner: true,
            metric: false,
        };
        let cpu = vec![0; 4 * 36 * 4];
        let mut runner = vec![0; 48 * 32 * 4];
        for y in 10..13 {
            for x in 10..13 {
                runner[(y * 48 + x) * 4..(y * 48 + x) * 4 + 4]
                    .copy_from_slice(&[255, 255, 255, 255]);
            }
        }
        let image = compose(
            v,
            CpuWidgetStyle::Bar,
            4,
            &cpu,
            &runner,
            false,
            [255, 255, 255],
        )
        .unwrap();
        let pixels = image.rgba.as_chunks::<4>().0;
        assert!(pixels.contains(&[255, 255, 255, 255]));
        assert!(
            pixels.contains(&[0, 0, 0, 255]),
            "template runner must contrast with a light secondary menu"
        );
    }

    #[test]
    fn history_tile_preserves_a_continuous_bright_stroke_and_dim_guides() {
        let mut history = crate::cpu_widget::CpuWidgetHistory::default();
        history.push(60.0);
        let width = history.pixel_width(CpuWidgetStyle::History);
        let source = history.render_rgba(CpuWidgetStyle::History);
        let v = MenuItemsVisibility {
            cpu: true,
            runner: false,
            metric: false,
        };
        let image = compose(
            v,
            CpuWidgetStyle::History,
            width,
            &source,
            &[],
            false,
            [118, 118, 118],
        )
        .unwrap();
        let mut drawn = 0;
        for y in 0..36_u32 {
            let src = ((y * width + 61) * 4 + 3) as usize;
            if source[src] == 255 {
                let dst = (((y + 4) * width + 61) * 4) as usize;
                assert_eq!(
                    &image.rgba[dst..dst + 4],
                    &[245, 245, 245, 255],
                    "discontinuous history column at y={y}"
                );
                drawn += 1;
            }
        }
        assert!(drawn > 5);
        let pixel = |x: u32, y: u32| {
            let i = (((y + 4) * width + x) * 4) as usize;
            &image.rgba[i..i + 4]
        };
        assert_eq!(pixel(10, 10), &[32, 32, 32, 255], "chart background");
        assert_eq!(pixel(10, 3), &[72, 72, 72, 255], "48-alpha guide");
    }

    #[test]
    fn isolated_history_stroke_does_not_depend_on_system_foreground() {
        let mut history = crate::cpu_widget::CpuWidgetHistory::default();
        history.push(60.0);
        let width = history.pixel_width(CpuWidgetStyle::History);
        let source = history.render_rgba(CpuWidgetStyle::History);
        let visibility = MenuItemsVisibility {
            cpu: true,
            runner: false,
            metric: false,
        };
        let foreground = [13, 37, 99];
        let image = compose(
            visibility,
            CpuWidgetStyle::History,
            width,
            &source,
            &[],
            false,
            foreground,
        )
        .unwrap();
        for y in 0..36_u32 {
            let alpha = ((y * width + 61) * 4 + 3) as usize;
            if source[alpha] == 255 {
                let dst = (((y + 4) * width + 61) * 4) as usize;
                assert_eq!(
                    &image.rgba[dst..dst + 3],
                    &[245, 245, 245],
                    "history ink must contrast with the tile at y={y}"
                );
            }
        }
    }

    #[test]
    fn history_tile_and_runner_are_unchanged_across_appearances() {
        let mut history = crate::cpu_widget::CpuWidgetHistory::default();
        history.push(60.0);
        let width = history.pixel_width(CpuWidgetStyle::History);
        let source = history.render_rgba(CpuWidgetStyle::History);
        let mut runner = vec![0; 48 * 32 * 4];
        runner[..4].copy_from_slice(&[23, 127, 207, 255]);
        let visibility = MenuItemsVisibility {
            cpu: true,
            runner: true,
            metric: false,
        };
        let dark = compose(
            visibility,
            CpuWidgetStyle::History,
            width,
            &source,
            &runner,
            true,
            [255; 3],
        )
        .unwrap();
        let light = compose(
            visibility,
            CpuWidgetStyle::History,
            width,
            &source,
            &runner,
            true,
            [0; 3],
        )
        .unwrap();
        assert_eq!(dark.width, light.width);
        assert_eq!(dark.rgba, light.rgba, "tile must not depend on appearance");
        for (a, b) in dark
            .rgba
            .as_chunks::<4>()
            .0
            .iter()
            .zip(light.rgba.as_chunks::<4>().0)
        {
            assert_eq!(a[3], b[3], "appearance must not change transparency");
        }
        let index = (((20 + 4) * dark.width + 61) * 4) as usize;
        assert_eq!(&dark.rgba[index..index + 4], &[245, 245, 245, 255]);
        assert_eq!(&light.rgba[index..index + 4], &[245, 245, 245, 255]);
        assert!(dark.rgba.as_chunks::<4>().0.contains(&[23, 127, 207, 255]));
        assert!(light.rgba.as_chunks::<4>().0.contains(&[23, 127, 207, 255]));
    }

    #[test]
    fn history_tile_keeps_line_legible_on_dark_grey_and_light_menus() {
        let mut history = crate::cpu_widget::CpuWidgetHistory::default();
        history.push(60.0);
        let width = history.pixel_width(CpuWidgetStyle::History);
        let source = history.render_rgba(CpuWidgetStyle::History);
        let mut runner = vec![0; 48 * 32 * 4];
        runner[..4].copy_from_slice(&[23, 127, 207, 255]);
        let visibility = MenuItemsVisibility {
            cpu: true,
            runner: true,
            metric: false,
        };
        let linear = |value: u8| {
            let value = f64::from(value) / 255.0;
            if value <= 0.04045 {
                value / 12.92
            } else {
                ((value + 0.055) / 1.055).powf(2.4)
            }
        };
        for foreground in [[0; 3], [118; 3], [255; 3]] {
            let image = compose(
                visibility,
                CpuWidgetStyle::History,
                width,
                &source,
                &runner,
                true,
                foreground,
            )
            .unwrap();
            let pixel = |x: u32, y: u32| {
                let i = ((y * image.width + x) * 4) as usize;
                &image.rgba[i..i + 4]
            };
            let line = pixel(61, 24);
            let tile = pixel(10, 19);
            for background in [0_u8, 118, 255] {
                let over = |p: &[u8]| {
                    ((u32::from(p[0]) * u32::from(p[3])
                        + u32::from(background) * (255 - u32::from(p[3])))
                        / 255) as u8
                };
                let a = linear(over(line));
                let b = linear(over(tile));
                let ratio = (a.max(b) + 0.05) / (a.min(b) + 0.05);
                assert!(
                    ratio >= 4.5,
                    "history line blends into tile on {background}: {ratio}"
                );
                // Conservative model for reducing a 2x raster to 1x:
                // a half-pixel stroke averaged with one tile pixel.
                let half_coverage = ((u16::from(over(line)) + u16::from(over(tile))) / 2) as u8;
                let average = linear(half_coverage);
                let ratio_1x = (average.max(b) + 0.05) / (average.min(b) + 0.05);
                assert!(
                    ratio_1x >= 4.5,
                    "half-coverage history contrast: {ratio_1x}"
                );
            }
            for y in 0..image.height {
                for x in width..width + 4 {
                    assert_eq!(pixel(x, y)[3], 0, "group gap changed");
                }
            }
            assert!(image.rgba.as_chunks::<4>().0.contains(&[23, 127, 207, 255]));
        }
    }

    #[test]
    fn core_pixels_and_inter_item_gap_are_preserved_with_runner() {
        let mut history = crate::cpu_widget::CpuWidgetHistory::default();
        history.sample_cpu_at(50.0, &[0.0, 100.0], std::time::Instant::now());
        let width = history.pixel_width(CpuWidgetStyle::Cores);
        let source = history.render_rgba(CpuWidgetStyle::Cores);
        let mut runner = vec![0; 48 * 32 * 4];
        runner[..4].copy_from_slice(&[23, 127, 207, 255]);
        let v = MenuItemsVisibility {
            cpu: true,
            runner: true,
            metric: false,
        };
        let image = compose(
            v,
            CpuWidgetStyle::Cores,
            width,
            &source,
            &runner,
            true,
            [0, 0, 0],
        )
        .unwrap();
        assert_eq!(image.width, width + 4 + 66);
        for y in 0..36_u32 {
            for x in 0..width {
                let src = ((y * width + x) * 4 + 3) as usize;
                let dst = (((y + 4) * image.width + x) * 4 + 3) as usize;
                assert_eq!(image.rgba[dst], source[src], "CPU alpha {x},{y}");
            }
        }
        for y in 0..image.height {
            for x in width..width + 4 {
                assert_eq!(
                    image.rgba[((y * image.width + x) * 4 + 3) as usize],
                    0,
                    "inter-item gap {x},{y}"
                );
            }
        }
    }

    #[test]
    #[ignore = "manual pure-RGBA composition microbenchmark; excludes AppKit and display"]
    fn benchmark_18_cores_and_eight_color_frames() {
        let mut history = crate::cpu_widget::CpuWidgetHistory::default();
        history.sample_cpu_at(50.0, &[50.0; 18], std::time::Instant::now());
        let width = history.pixel_width(CpuWidgetStyle::Cores);
        let cpu = history.render_rgba(CpuWidgetStyle::Cores);
        let source = include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/runner/color/bird.png"
        ));
        let mut reader = png::Decoder::new(std::io::Cursor::new(source))
            .read_info()
            .unwrap();
        let mut pixels = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut pixels).unwrap();
        assert_eq!((info.width, info.height), (384, 32));
        let mut frames: [Vec<u8>; 8] = std::array::from_fn(|_| Vec::new());
        for row in pixels.as_chunks::<{ 384 * 4 }>().0 {
            for (index, frame) in frames.iter_mut().enumerate() {
                frame.extend_from_slice(&row[index * 48 * 4..(index + 1) * 48 * 4]);
            }
        }
        let v = MenuItemsVisibility {
            cpu: true,
            runner: true,
            metric: true,
        };
        let rounds = 200_u32;
        let start = std::time::Instant::now();
        for _ in 0..rounds {
            for frame in &frames {
                std::hint::black_box(
                    compose(
                        v,
                        CpuWidgetStyle::Cores,
                        width,
                        &cpu,
                        frame,
                        true,
                        [0, 0, 0],
                    )
                    .unwrap(),
                );
            }
        }
        let each = start.elapsed().as_secs_f64() * 1000.0 / f64::from(rounds);
        println!("pure_rgba_18_cores_8_frames_ms_per_refresh={each:.4}; profile=debug; appkit_and_display=unmeasured");
    }
}
