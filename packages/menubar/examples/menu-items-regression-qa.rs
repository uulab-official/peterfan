//! Isolated regression fixture: native items/layers and the production shared
//! transaction. Never reads or writes user settings or operates an installed app.
#![allow(dead_code)]
#[cfg(target_os = "macos")]
#[path = "../src/composite_status.rs"]
mod composite_status;
#[cfg(target_os = "macos")]
#[path = "../src/cpu_widget.rs"]
mod cpu_widget;
#[cfg(target_os = "macos")]
#[path = "../src/cpu_widget_app.rs"]
mod cpu_widget_app;
#[cfg(target_os = "macos")]
#[path = "../src/menu_items_app.rs"]
mod menu_items_app;
#[cfg(target_os = "macos")]
#[path = "../src/native_cpu_widget.rs"]
mod native_cpu_widget;
#[cfg(target_os = "macos")]
#[path = "../src/native_runner.rs"]
mod native_runner;

#[cfg(target_os = "macos")]
use peterfan_core::config::{
    CpuWidgetConfig, CpuWidgetStyle, Language, MenuItemsVisibility, MenubarDisplay,
};
#[cfg(target_os = "macos")]
use std::{
    cell::RefCell,
    collections::VecDeque,
    sync::{atomic::AtomicUsize, Mutex},
};
#[cfg(target_os = "macos")]
static SETTINGS_RESULTS: Mutex<VecDeque<serde_json::Value>> = Mutex::new(VecDeque::new());
#[cfg(target_os = "macos")]
const SETTINGS_RESULTS_MAX: usize = 32;
#[cfg(target_os = "macos")]
#[derive(serde::Serialize)]
#[serde(rename_all = "snake_case")]
enum SettingsOperation {
    MenuItems,
    CpuWidget,
}
#[cfg(target_os = "macos")]
struct SettingsRequest {
    id: String,
    operation: SettingsOperation,
    payload: String,
}
#[cfg(target_os = "macos")]
struct TrayMenu {
    display_items: Vec<(MenubarDisplay, tray_icon::menu::CheckMenuItem)>,
    menu_items: native_cpu_widget::MenuItemsMenu,
    cpu_appearance: native_cpu_widget::CpuAppearanceMenu,
}
#[cfg(target_os = "macos")]
struct CompositeKey {
    visibility: MenuItemsVisibility,
    config: CpuWidgetConfig,
    cpu_width: u32,
    cpu_pixels: Vec<u8>,
}
#[cfg(target_os = "macos")]
struct PreparedComposite {
    key: CompositeKey,
    images: Vec<objc2::rc::Retained<objc2_app_kit::NSImage>>,
}
#[cfg(target_os = "macos")]
struct App {
    display: MenubarDisplay,
    primary_visible: bool,
    cpu_widget_config: CpuWidgetConfig,
    tray: Option<tray_icon::TrayIcon>,
    tray_menu: Option<TrayMenu>,
    cpu_widget_history: cpu_widget::CpuWidgetHistory,
    cpu_widget_native: Option<native_cpu_widget::NativeCpuWidget>,
    composite_images: RefCell<Vec<objc2::rc::Retained<objc2_app_kit::NSImage>>>,
    composite_key: Option<CompositeKey>,
    runner_native_applied_icon: AtomicUsize,
    language: Language,
    last_menubar_text: Option<String>,
    last_menubar_tooltip: Option<String>,
    last_runner_icon: Option<usize>,
    cpu_widget_error: Option<String>,
    runner_native_painter: RefCell<native_runner::NativeRunnerPainter>,
}
#[cfg(target_os = "macos")]
fn stop_native_runner_timer(_: &App) {}
#[cfg(target_os = "macos")]
fn sync_native_runner_timer(_: &App) {}
#[cfg(target_os = "macos")]
fn invalidate_runner_icon(value: &mut Option<usize>) {
    *value = None;
}
#[cfg(target_os = "macos")]
fn report_preference_failure(_: &str, error: impl std::fmt::Display) {
    panic!("{error}");
}

#[cfg(target_os = "macos")]
fn composite_key(
    _: &App,
    visibility: MenuItemsVisibility,
    config: CpuWidgetConfig,
    history: &cpu_widget::CpuWidgetHistory,
) -> Result<CompositeKey, String> {
    let graph = visibility.cpu && config.style != CpuWidgetStyle::Number;
    Ok(CompositeKey {
        visibility,
        config,
        cpu_width: if graph {
            history.pixel_width(config.style)
        } else {
            0
        },
        cpu_pixels: if graph {
            history.render_rgba(config.style)
        } else {
            Vec::new()
        },
    })
}

#[cfg(target_os = "macos")]
fn prepare_composite(_: &App, key: CompositeKey) -> Result<PreparedComposite, String> {
    let graph = key.visibility.cpu && key.config.style != CpuWidgetStyle::Number;
    let mut images = Vec::new();
    for pose in 0..if key.visibility.runner {
        8
    } else if graph {
        1
    } else {
        0
    } {
        let mut runner = vec![0; 48 * 32 * 4];
        if key.visibility.runner {
            for y in 4..26 {
                for x in 8 + pose % 2..38 + pose % 2 {
                    runner[(y * 48 + x) * 4..(y * 48 + x) * 4 + 4]
                        .copy_from_slice(&[23, 127, 207, 255]);
                }
            }
        }
        let frame = composite_status::compose(
            key.visibility,
            key.config.style,
            key.cpu_width,
            &key.cpu_pixels,
            &runner,
            true,
            [255, 255, 255],
        )
        .ok_or("invalid fixture composite frame")?;
        images.push(fixture_composite_image(&frame)?);
    }
    Ok(PreparedComposite { key, images })
}

/// Allocate the same dimensioned RGBA AppKit image as the production
/// composite path, with fixture artwork rather than installed-app data.
#[cfg(target_os = "macos")]
fn fixture_composite_image(
    frame: &composite_status::CompositeFrame,
) -> Result<objc2::rc::Retained<objc2_app_kit::NSImage>, String> {
    use objc2::AllocAnyThread;
    use objc2_app_kit::{
        NSBitmapFormat, NSBitmapImageRep, NSDeviceRGBColorSpace, NSImage, NSImageCacheMode,
    };
    use objc2_foundation::NSSize;
    let bytes_per_row = (frame.width * 4) as isize;
    if frame.rgba.len() != frame.width as usize * frame.height as usize * 4 {
        return Err("invalid fixture composite size".into());
    }
    let rep = unsafe { NSBitmapImageRep::initWithBitmapDataPlanes_pixelsWide_pixelsHigh_bitsPerSample_samplesPerPixel_hasAlpha_isPlanar_colorSpaceName_bitmapFormat_bytesPerRow_bitsPerPixel(
        NSBitmapImageRep::alloc(), std::ptr::null_mut(), frame.width as isize, frame.height as isize,
        8, 4, true, false, NSDeviceRGBColorSpace, NSBitmapFormat::AlphaNonpremultiplied,
        bytes_per_row, 32,
    ) }.ok_or("fixture composite bitmap allocation failed")?;
    if rep.bitmapData().is_null()
        || rep.bytesPerRow() != bytes_per_row
        || rep.pixelsHigh() != frame.height as isize
    {
        return Err("invalid fixture composite storage".into());
    }
    unsafe {
        std::ptr::copy_nonoverlapping(frame.rgba.as_ptr(), rep.bitmapData(), frame.rgba.len());
    }
    let size = NSSize::new(f64::from(frame.width) / 2., f64::from(frame.height) / 2.);
    rep.setSize(size);
    let image = NSImage::initWithSize(NSImage::alloc(), size);
    image.addRepresentation(&rep);
    image.setCacheMode(NSImageCacheMode::Always);
    image.setTemplate(false);
    Ok(image)
}

#[cfg(all(test, target_os = "macos"))]
mod fixture_composite_tests {
    use super::*;

    #[test]
    fn all_visibility_combinations_produce_real_native_composite_images() {
        let mut cpu = vec![0_u8; 8 * 36 * 4];
        cpu[..4].copy_from_slice(&[0, 0, 0, 255]);
        let mut runner = vec![0_u8; 48 * 32 * 4];
        runner[..4].copy_from_slice(&[23, 127, 207, 255]);
        for flags in 1..8 {
            let visible = MenuItemsVisibility {
                cpu: flags & 1 != 0,
                runner: flags & 2 != 0,
                metric: flags & 4 != 0,
            };
            let frame = composite_status::compose(
                visible,
                CpuWidgetStyle::Bar,
                8,
                &cpu,
                &runner,
                true,
                [255, 255, 255],
            );
            assert_eq!(frame.is_some(), visible.cpu || visible.runner, "{flags}");
            if let Some(frame) = frame {
                let expected_width = (if visible.cpu { 8 } else { 0 })
                    + (if visible.cpu && visible.runner { 4 } else { 0 })
                    + (if visible.runner { 66 } else { 0 });
                assert_eq!(frame.width, expected_width, "{flags}");
                let image = fixture_composite_image(&frame).expect("native composite image");
                assert_eq!(image.size().width, f64::from(expected_width) / 2.);
                assert_eq!(image.size().height, 22.);
                assert!(!image.isTemplate(), "combined color must not be recolored");
            }
        }
    }
}

#[cfg(target_os = "macos")]
fn configure_composite_status(app: &App, primary: &native_cpu_widget::PreparedPrimaryItem) {
    let visibility = menu_items_app::visibility(app);
    let images = app.composite_images.borrow();
    let image_width = images.first().map_or(0., |image| image.size().width);
    let cpu_text_width = if visibility.cpu {
        if app.cpu_widget_config.style == CpuWidgetStyle::Number {
            68.
        } else if app.cpu_widget_config.show_percentage {
            38.
        } else {
            0.
        }
    } else {
        0.
    };
    primary.configure_composite(
        image_width,
        !images.is_empty(),
        cpu_text_width,
        if visibility.metric { 64.0 } else { 0.0 },
    );
}

#[cfg(target_os = "macos")]
fn image(visible: bool) -> objc2::rc::Retained<objc2_app_kit::NSImage> {
    let mut rgba = vec![0_u8; 48 * 32 * 4];
    if visible {
        for y in 4..26 {
            for x in 8..38 {
                rgba[(y * 48 + x) * 4..(y * 48 + x) * 4 + 4].copy_from_slice(&[255; 4]);
            }
        }
    }
    make_runner_native_image_from_rgba(peterfan_core::config::RunnerCharacter::Cat, 0, &rgba)
        .unwrap()
}

// The imported native renderer's own unit tests also consume asymmetric RGBA
// frames when Cargo checks this example as a test target.
#[cfg(target_os = "macos")]
fn make_runner_native_image_from_rgba(
    _: peterfan_core::config::RunnerCharacter,
    _: usize,
    rgba: &[u8],
) -> Result<objc2::rc::Retained<objc2_app_kit::NSImage>, String> {
    use objc2::AllocAnyThread;
    use objc2_app_kit::{NSBitmapFormat, NSBitmapImageRep, NSDeviceRGBColorSpace, NSImage};
    use objc2_foundation::NSSize;
    if rgba.len() != 48 * 32 * 4 {
        return Err("invalid fixture RGBA length".into());
    }
    let rep = unsafe {
        NSBitmapImageRep::initWithBitmapDataPlanes_pixelsWide_pixelsHigh_bitsPerSample_samplesPerPixel_hasAlpha_isPlanar_colorSpaceName_bitmapFormat_bytesPerRow_bitsPerPixel(
            NSBitmapImageRep::alloc(), std::ptr::null_mut(), 48, 32, 8, 4, true, false,
            NSDeviceRGBColorSpace, NSBitmapFormat::AlphaNonpremultiplied, 48 * 4, 32,
        )
    }.ok_or("fixture bitmap allocation failed")?;
    if rep.bitmapData().is_null() || rep.bytesPerRow() != 48 * 4 || rep.pixelsHigh() != 32 {
        return Err("invalid fixture bitmap storage".into());
    }
    // AppKit owns exactly the validated nonplanar RGBA8 storage; no pixel
    // reference escapes this copy and the bitmap is retained by NSImage.
    unsafe {
        std::ptr::copy_nonoverlapping(rgba.as_ptr(), rep.bitmapData(), rgba.len());
    }
    rep.setSize(NSSize::new(27., 18.));
    let image = NSImage::initWithSize(NSImage::alloc(), NSSize::new(27., 18.));
    image.addRepresentation(&rep);
    image.setCacheMode(objc2_app_kit::NSImageCacheMode::Always);
    image.setTemplate(true);
    Ok(image)
}

// The imported native renderer tests exercise both color and legacy modes.
#[cfg(all(test, target_os = "macos"))]
fn make_runner_native_image_with_color(
    character: peterfan_core::config::RunnerCharacter,
    index: usize,
    rgba: &[u8],
    color: bool,
) -> Result<objc2::rc::Retained<objc2_app_kit::NSImage>, String> {
    let image = make_runner_native_image_from_rgba(character, index, rgba)?;
    image.setTemplate(!color);
    Ok(image)
}

// Exercise the same attachment ownership/restoration code as the application.
#[cfg(target_os = "macos")]
fn apply_runner_icon(app: &mut App) {
    use objc2::AllocAnyThread;
    use objc2_app_kit::NSImage;
    use objc2_foundation::MainThreadMarker;
    let item = app.tray.as_ref().unwrap().ns_status_item().unwrap();
    let button = item.button(MainThreadMarker::new().unwrap()).unwrap();
    let mut painter = app.runner_native_painter.borrow_mut();
    let images = app.composite_images.borrow();
    let placeholder_size = images.first().map(|image| image.size());
    painter.reconcile(&button, app.display, app.primary_visible, false, 1, || {
        placeholder_size.map(|size| NSImage::initWithSize(NSImage::alloc(), size))
    });
    if let Some(image) = images.first() {
        painter.paint(image);
    } else {
        button.setImage(None);
    }
}

#[cfg(target_os = "macos")]
fn visible_layer_pixels(app: &App) -> usize {
    use objc2_core_graphics::{
        CGBitmapContextCreate, CGColorSpace, CGImageAlphaInfo, CGImageByteOrderInfo,
    };
    use objc2_foundation::MainThreadMarker;
    let button = app
        .tray
        .as_ref()
        .unwrap()
        .ns_status_item()
        .unwrap()
        .button(MainThreadMarker::new().unwrap())
        .unwrap();
    let parent = button.layer().unwrap();
    // AppKit owns this native button layer and its sublayers on the GUI thread.
    let layer = unsafe { parent.sublayers() }
        .unwrap()
        .iter()
        .find(|layer| layer.mask().is_some() || unsafe { layer.contents().is_some() })
        .expect("the owned runner color layer has an image or mask");
    let mut rgba = vec![0_u8; 96 * 48 * 4];
    let space = CGColorSpace::new_device_rgb().unwrap();
    let context = unsafe {
        CGBitmapContextCreate(
            rgba.as_mut_ptr().cast(),
            96,
            48,
            8,
            96 * 4,
            Some(&space),
            CGImageAlphaInfo::PremultipliedLast.0 | CGImageByteOrderInfo::Order32Big.0,
        )
    }
    .unwrap();
    layer.renderInContext(&context);
    drop(context);
    rgba.as_chunks::<4>()
        .0
        .iter()
        .filter(|pixel| pixel[3] > 0)
        .count()
}

#[cfg(target_os = "macos")]
fn main() {
    use peterfan_core::config::{MenuItemsVisibility, ResolvedLanguage};
    use tao::{
        event_loop::{ControlFlow, EventLoopBuilder},
        platform::{
            macos::{ActivationPolicy, EventLoopExtMacOS},
            run_return::EventLoopExtRunReturn,
        },
    };
    use tray_icon::menu::CheckMenuItem;
    let mode = std::env::args().nth(1).expect("runner or menu");
    let output =
        std::path::PathBuf::from(std::env::args().nth(2).expect("private evidence directory"));
    std::fs::create_dir_all(&output).unwrap();
    let mut event_loop = EventLoopBuilder::<()>::new().build();
    event_loop.set_activation_policy(ActivationPolicy::Accessory);
    let mut app = App {
        display: MenubarDisplay::Number,
        primary_visible: true,
        cpu_widget_config: CpuWidgetConfig::default(),
        tray: Some(
            tray_icon::TrayIcon::new(tray_icon::TrayIconAttributes {
                title: Some("QA".into()),
                menu_on_left_click: false,
                ..Default::default()
            })
            .unwrap(),
        ),
        tray_menu: Some(TrayMenu {
            display_items: [
                MenubarDisplay::Number,
                MenubarDisplay::Graph,
                MenubarDisplay::Both,
            ]
            .into_iter()
            .map(|display| {
                (
                    display,
                    CheckMenuItem::new(
                        display.as_str(),
                        true,
                        display == MenubarDisplay::Number,
                        None,
                    ),
                )
            })
            .collect(),
            menu_items: native_cpu_widget::MenuItemsMenu::new(
                ResolvedLanguage::En,
                MenuItemsVisibility {
                    cpu: false,
                    runner: false,
                    metric: true,
                },
            ),
            cpu_appearance: native_cpu_widget::CpuAppearanceMenu::new(
                CpuWidgetConfig::default(),
                ResolvedLanguage::En,
            ),
        }),
        cpu_widget_history: Default::default(),
        cpu_widget_native: None,
        composite_images: RefCell::new(Vec::new()),
        composite_key: None,
        runner_native_applied_icon: AtomicUsize::new(usize::MAX),
        language: Language::English,
        last_menubar_text: None,
        last_menubar_tooltip: None,
        last_runner_icon: None,
        cpu_widget_error: None,
        runner_native_painter: Default::default(),
    };
    let mut phase = 0;
    let mut next = std::time::Instant::now();
    let mut snapshots = Vec::new();
    let mut errors = Vec::new();
    let phases = if mode == "runner" { 8 } else { 5 };
    event_loop.run_return(|_, _, flow| {
        if phase >= phases { *flow = ControlFlow::Exit; return; }
        if std::time::Instant::now() < next { *flow = ControlFlow::WaitUntil(next); return; }
        let visibility = match phase {
            0 | 2 => MenuItemsVisibility { cpu: true, runner: true, metric: false },
            1 => MenuItemsVisibility { cpu: true, runner: false, metric: false },
            3 => MenuItemsVisibility { cpu: false, runner: false, metric: true },
            4 => MenuItemsVisibility { cpu: false, runner: true, metric: true },
            5 => MenuItemsVisibility { cpu: true, runner: false, metric: true },
            6 => MenuItemsVisibility { cpu: true, runner: true, metric: true },
            _ => MenuItemsVisibility { cpu: false, runner: true, metric: false },
        };
        if mode == "runner" {
            menu_items_app::apply(&mut app, visibility, false).unwrap();
        } else {
            let display = match phase {
                0..=2 => MenubarDisplay::Graph,
                3 => MenubarDisplay::Number,
                _ => MenubarDisplay::Both,
            };
            // The same legacy entry point used by Settings, with persistence
            // disabled so this fixture cannot access the user's configuration.
            menu_items_app::apply_display(&mut app, display, false).unwrap();
        }
        let geometry = app.runner_native_painter.borrow().attachment_geometry();
        let checked: Vec<_> = app.tray_menu.as_ref().unwrap().display_items.iter().map(|(_, item)| item.is_checked()).collect();
        let mut pixels = 0;
        if mode == "runner" {
            if phase == 0 { pixels = visible_layer_pixels(&app); }
            if phase == 1 && app.runner_native_painter.borrow().active() { errors.push("hidden primary retained runner layer"); }
            if phase == 1 {
                let item = app.tray.as_ref().unwrap().ns_status_item().unwrap();
                let button = item.button(objc2_foundation::MainThreadMarker::new().unwrap()).unwrap();
                if !item.isVisible() || button.image().is_none() {
                    errors.push("CPU-only composite must remain visible with an image");
                }
            }
            if phase == 2 {
                if geometry.is_none_or(|(rect, _)| rect.size.width < 26. || rect.size.height < 17.) { errors.push("restored runner lost its reserved layer geometry"); }
                pixels = visible_layer_pixels(&app);
                if pixels == 0 { errors.push("restored runner produced no visible layer pixels"); }
            }
        } else {
            let expected = match phase { 0..=2 => [false, true, false], 3 => [true, false, false], _ => [false, false, true] };
            if checked != expected { errors.push("shared commit left legacy display checkmarks stale"); }
        }
        snapshots.push(serde_json::json!({"phase": phase, "visibility": menu_items_app::visibility(&app), "geometry": format!("{geometry:?}"), "layer_active": app.runner_native_painter.borrow().active(), "checked": checked, "visible_layer_pixels": pixels}));
        phase += 1;
        if phase >= phases { *flow = ControlFlow::Exit; } else { next = std::time::Instant::now() + std::time::Duration::from_millis(300); *flow = ControlFlow::WaitUntil(next); }
    });
    let passed = errors.is_empty();
    std::fs::write(output.join(format!("{mode}.json")), serde_json::to_vec_pretty(&serde_json::json!({"passed":passed,"errors":errors,"snapshots":snapshots,"scope":"isolated Rust native fixture; real shared commit/menu/layers; no installed-app/config/helper/timer operations"})).unwrap()).unwrap();
    if !passed {
        eprintln!("{errors:?}");
        std::process::exit(1);
    }
}
#[cfg(not(target_os = "macos"))]
fn main() {
    std::process::exit(2);
}
