//! `peterfan-menubar` — live system metrics in the macOS menu bar.
//!
//! Puts a status item in the macOS menu bar (an `NSStatusItem`, via `tray-icon`)
//! showing live CPU usage, with a dropdown of CPU / memory / network detail and
//! a Quit item. It refreshes once a second from the same [`SystemMonitor`] the
//! CLI and TUI use, so the numbers match.
//!
//! Runs as an accessory app (no Dock icon). On Windows the same binary shows a
//! system-tray icon with the metrics in its tooltip. Pass `--mock` to drive it
//! from the simulated machine.
//!
//! This is the "installed program in the top bar" experience, à la Stats —
//! distributed as a normal `.app` later (see docs/ROADMAP.md); today it runs
//! straight from `cargo run -p peterfan-menubar`.

use std::time::{Duration, Instant};

use tao::event::{Event, StartCause};
use tao::event_loop::{ControlFlow, EventLoopBuilder};

#[cfg(target_os = "macos")]
use tao::platform::macos::{ActivationPolicy, EventLoopExtMacOS};

use tray_icon::menu::{Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

use peterfan_core::SystemMonitor;

const REFRESH: Duration = Duration::from_secs(1);

/// Everything the event loop needs to keep alive and mutate each tick.
struct App {
    monitor: Box<dyn SystemMonitor>,
    tray: Option<TrayIcon>,
    cpu_item: Option<MenuItem>,
    mem_item: Option<MenuItem>,
    net_item: Option<MenuItem>,
    quit_id: Option<MenuId>,
}

fn main() {
    let use_mock = std::env::args().any(|a| a == "--mock");
    let monitor: Box<dyn SystemMonitor> = if use_mock {
        peterfan_platform::mock_monitor()
    } else {
        peterfan_platform::system_monitor()
    };

    #[allow(unused_mut)]
    let mut event_loop = EventLoopBuilder::<()>::new().build();
    // Menu-bar-only agent: no Dock icon, no app window.
    #[cfg(target_os = "macos")]
    event_loop.set_activation_policy(ActivationPolicy::Accessory);

    let mut app = App {
        monitor,
        tray: None,
        cpu_item: None,
        mem_item: None,
        net_item: None,
        quit_id: None,
    };

    event_loop.run(move |event, _target, control_flow| {
        *control_flow = ControlFlow::WaitUntil(Instant::now() + REFRESH);

        match event {
            // The status item must be created after the app has finished
            // launching on macOS, so we build it here rather than before run().
            Event::NewEvents(StartCause::Init) => {
                build_tray(&mut app);
                update(&mut app);
            }
            Event::NewEvents(StartCause::ResumeTimeReached { .. }) => {
                update(&mut app);
            }
            _ => {}
        }

        // Drain menu clicks.
        while let Ok(menu_event) = MenuEvent::receiver().try_recv() {
            if app.quit_id.as_ref() == Some(&menu_event.id) {
                *control_flow = ControlFlow::Exit;
            }
        }
    });
}

/// Build the menu and status item, storing handles in `app`.
fn build_tray(app: &mut App) {
    let menu = Menu::new();
    let header = MenuItem::new("PeterFan", false, None);
    let cpu_item = MenuItem::new("CPU    …", false, None);
    let mem_item = MenuItem::new("Memory …", false, None);
    let net_item = MenuItem::new("Net    …", false, None);
    let quit = MenuItem::new("Quit PeterFan", true, None);

    let _ = menu.append_items(&[
        &header,
        &PredefinedMenuItem::separator(),
        &cpu_item,
        &mem_item,
        &net_item,
        &PredefinedMenuItem::separator(),
        &quit,
    ]);

    let mut builder = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_icon(make_icon());
    #[cfg(target_os = "macos")]
    {
        builder = builder.with_icon_as_template(true);
    }

    match builder.build() {
        Ok(tray) => {
            app.quit_id = Some(quit.id().clone());
            app.cpu_item = Some(cpu_item);
            app.mem_item = Some(mem_item);
            app.net_item = Some(net_item);
            app.tray = Some(tray);
        }
        Err(e) => eprintln!("failed to create menu-bar item: {e}"),
    }
}

/// Re-sample metrics and push them to the menu-bar title and dropdown.
fn update(app: &mut App) {
    app.monitor.refresh();
    let cpu = app.monitor.cpu();
    let mem = app.monitor.memory();
    let nets = app.monitor.networks();
    let rx: f64 = nets.iter().map(|n| n.rx_rate).sum();
    let tx: f64 = nets.iter().map(|n| n.tx_rate).sum();

    if let Some(tray) = &app.tray {
        set_menubar_text(tray, &format!("{:.0}%", cpu.usage_percent));
    }
    if let Some(i) = &app.cpu_item {
        i.set_text(format!(
            "CPU      {:>3.0}%      {} MHz",
            cpu.usage_percent, cpu.frequency_mhz
        ));
    }
    if let Some(i) = &app.mem_item {
        i.set_text(format!(
            "Memory   {:>3.0}%      {} / {}",
            mem.used_percent,
            bytes(mem.used),
            bytes(mem.total)
        ));
    }
    if let Some(i) = &app.net_item {
        i.set_text(format!(
            "Net      ↓ {}/s   ↑ {}/s",
            bytes(rx as u64),
            bytes(tx as u64)
        ));
    }
}

/// Set the text shown in the menu bar (macOS) or the tray tooltip (elsewhere).
#[cfg(target_os = "macos")]
fn set_menubar_text(tray: &TrayIcon, text: &str) {
    tray.set_title(Some(text));
}

#[cfg(not(target_os = "macos"))]
fn set_menubar_text(tray: &TrayIcon, text: &str) {
    let _ = tray.set_tooltip(Some(text));
}

/// A simple 32×32 ring icon (a fan hub). Drawn as a template image on macOS so
/// it adapts to the light/dark menu bar automatically.
fn make_icon() -> Icon {
    const W: u32 = 32;
    const H: u32 = 32;
    let (cx, cy) = (15.5_f32, 15.5_f32);
    let (r_out, r_in) = (14.0_f32, 6.5_f32);

    let mut rgba = vec![0u8; (W * H * 4) as usize];
    for y in 0..H {
        for x in 0..W {
            let dx = x as f32 - cx;
            let dy = y as f32 - cy;
            let d = (dx * dx + dy * dy).sqrt();
            // Soft 1px edge for a less jagged ring.
            let alpha = if d > r_out + 0.5 || d < r_in - 0.5 {
                0.0
            } else if d > r_out - 0.5 {
                (r_out + 0.5 - d).clamp(0.0, 1.0)
            } else if d < r_in + 0.5 {
                (d - (r_in - 0.5)).clamp(0.0, 1.0)
            } else {
                1.0
            };
            let idx = ((y * W + x) * 4) as usize;
            rgba[idx + 3] = (alpha * 255.0) as u8;
        }
    }
    Icon::from_rgba(rgba, W, H).expect("valid icon")
}

/// Compact base-1024 byte formatting (kept local to avoid a shared dep).
fn bytes(n: u64) -> String {
    const UNITS: [&str; 6] = ["B", "KB", "MB", "GB", "TB", "PB"];
    let mut v = n as f64;
    let mut i = 0;
    while v >= 1024.0 && i < UNITS.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{n} B")
    } else {
        format!("{v:.1} {}", UNITS[i])
    }
}
