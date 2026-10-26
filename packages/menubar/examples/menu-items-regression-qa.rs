//! Isolated regression fixture: native items/layers and the production shared
//! transaction. Never reads or writes user settings or operates an installed app.
#![allow(dead_code)]
#[cfg(target_os = "macos")]
#[path = "../src/cpu_widget.rs"]
mod cpu_widget;
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
use peterfan_core::config::{CpuWidgetConfig, Language, MenubarDisplay};
#[cfg(target_os = "macos")]
use std::{cell::RefCell, collections::VecDeque, sync::Mutex};
#[cfg(target_os = "macos")]
static SETTINGS_RESULTS: Mutex<VecDeque<serde_json::Value>> = Mutex::new(VecDeque::new());
#[cfg(target_os = "macos")]
const SETTINGS_RESULTS_MAX: usize = 32;
#[cfg(target_os = "macos")]
#[derive(serde::Serialize)]
enum SettingsOperation {
    MenuItems,
}
#[cfg(target_os = "macos")]
struct SettingsRequest {
    id: String,
    payload: String,
}
#[cfg(target_os = "macos")]
struct TrayMenu {
    display_items: Vec<(MenubarDisplay, tray_icon::menu::CheckMenuItem)>,
    menu_items: native_cpu_widget::MenuItemsMenu,
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
    language: Language,
    last_menubar_text: Option<String>,
    last_menubar_tooltip: Option<String>,
    last_runner_icon: Option<usize>,
    cpu_widget_error: Option<String>,
    painter: RefCell<native_runner::NativeRunnerPainter>,
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

// Exercise the same attachment ownership/restoration code as the application.
#[cfg(target_os = "macos")]
fn apply_runner_icon(app: &mut App) {
    use objc2_foundation::MainThreadMarker;
    let item = app.tray.as_ref().unwrap().ns_status_item().unwrap();
    let button = item.button(MainThreadMarker::new().unwrap()).unwrap();
    let mut painter = app.painter.borrow_mut();
    painter.reconcile(&button, app.display, app.primary_visible, false, 1, || {
        Some(image(false))
    });
    button.setImagePosition(native_cpu_widget::primary_image_position(app.display));
    if app.primary_visible && app.display != MenubarDisplay::Number {
        painter.paint(&image(true));
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
        .find(|layer| layer.mask().is_some())
        .expect("the owned runner color layer has a mask");
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
        }),
        cpu_widget_history: Default::default(),
        cpu_widget_native: None,
        language: Language::English,
        last_menubar_text: None,
        last_menubar_tooltip: None,
        last_runner_icon: None,
        cpu_widget_error: None,
        painter: Default::default(),
    };
    let mut phase = 0;
    let mut next = std::time::Instant::now();
    let mut snapshots = Vec::new();
    let mut errors = Vec::new();
    event_loop.run_return(|_, _, flow| {
        if phase > 4 { *flow = ControlFlow::Exit; return; }
        if std::time::Instant::now() < next { *flow = ControlFlow::WaitUntil(next); return; }
        let visibility = match phase {
            0 | 2 => MenuItemsVisibility { cpu: true, runner: true, metric: false },
            1 => MenuItemsVisibility { cpu: true, runner: false, metric: false },
            3 => MenuItemsVisibility { cpu: false, runner: false, metric: true },
            _ => MenuItemsVisibility { cpu: false, runner: true, metric: true },
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
        let geometry = app.painter.borrow().attachment_geometry();
        let checked: Vec<_> = app.tray_menu.as_ref().unwrap().display_items.iter().map(|(_, item)| item.is_checked()).collect();
        let mut pixels = 0;
        if mode == "runner" {
            if phase == 0 { pixels = visible_layer_pixels(&app); }
            if phase == 1 && app.painter.borrow().active() { errors.push("hidden primary retained runner layer"); }
            if phase == 2 {
                if geometry.is_none_or(|(rect, _)| rect.size.width < 26. || rect.size.height < 17.) { errors.push("restored runner lost its reserved layer geometry"); }
                pixels = visible_layer_pixels(&app);
                if pixels == 0 { errors.push("restored runner produced no visible layer pixels"); }
            }
        } else {
            let expected = match phase { 0..=2 => [false, true, false], 3 => [true, false, false], _ => [false, false, true] };
            if checked != expected { errors.push("shared commit left legacy display checkmarks stale"); }
        }
        snapshots.push(serde_json::json!({"phase": phase, "visibility": menu_items_app::visibility(&app), "geometry": format!("{geometry:?}"), "layer_active": app.painter.borrow().active(), "checked": checked, "visible_layer_pixels": pixels}));
        phase += 1;
        if phase > 4 { *flow = ControlFlow::Exit; } else { next = std::time::Instant::now() + std::time::Duration::from_millis(300); *flow = ControlFlow::WaitUntil(next); }
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
