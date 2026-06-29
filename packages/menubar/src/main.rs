//! `peterfan-menubar` — live system metrics in the macOS menu bar.
//!
//! Puts a status item in the macOS menu bar (an `NSStatusItem`, via `tray-icon`)
//! showing a tiny CPU sparkline + percentage, with a polished dropdown: each
//! metric row has a load-colored status dot, a block-bar gauge, and the figures
//! — CPU (with per-core sparkline), memory, disk, network, and battery. It
//! refreshes once a second from the same [`SystemMonitor`] the CLI and TUI use.
//!
//! Runs as an accessory app (no Dock icon). On Windows the same binary shows a
//! system-tray icon with the metrics in its tooltip. Pass `--mock` to drive it
//! from the simulated machine.

use std::time::{Duration, Instant};

use tao::event::{Event, StartCause};
use tao::event_loop::{ControlFlow, EventLoopBuilder};

#[cfg(target_os = "macos")]
use tao::platform::macos::{ActivationPolicy, EventLoopExtMacOS};

use tray_icon::menu::{
    Icon as MenuIcon, IconMenuItem, Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem,
};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

use peterfan_core::SystemMonitor;

const REFRESH: Duration = Duration::from_secs(1);
/// Number of recent CPU samples kept for the menu-bar sparkline.
const SPARK_LEN: usize = 7;
/// Width of the block-bar gauges in the dropdown.
const BAR_WIDTH: usize = 9;

/// Everything the event loop needs to keep alive and mutate each tick.
struct App {
    monitor: Box<dyn SystemMonitor>,
    has_battery: bool,
    tray: Option<TrayIcon>,
    header: Option<MenuItem>,
    cpu_item: Option<IconMenuItem>,
    cores_item: Option<MenuItem>,
    mem_item: Option<IconMenuItem>,
    disk_item: Option<IconMenuItem>,
    net_item: Option<MenuItem>,
    batt_item: Option<IconMenuItem>,
    quit_id: Option<MenuId>,
    history: Vec<f32>,
}

fn main() {
    let use_mock = std::env::args().any(|a| a == "--mock");
    let monitor: Box<dyn SystemMonitor> = if use_mock {
        peterfan_platform::mock_monitor()
    } else {
        peterfan_platform::system_monitor()
    };
    let has_battery = monitor.capabilities().battery;

    #[allow(unused_mut)]
    let mut event_loop = EventLoopBuilder::<()>::new().build();
    #[cfg(target_os = "macos")]
    event_loop.set_activation_policy(ActivationPolicy::Accessory);

    let mut app = App {
        monitor,
        has_battery,
        tray: None,
        header: None,
        cpu_item: None,
        cores_item: None,
        mem_item: None,
        disk_item: None,
        net_item: None,
        batt_item: None,
        quit_id: None,
        history: Vec::with_capacity(SPARK_LEN),
    };

    event_loop.run(move |event, _target, control_flow| {
        *control_flow = ControlFlow::WaitUntil(Instant::now() + REFRESH);

        match event {
            // The status item must be created after the app finishes launching.
            Event::NewEvents(StartCause::Init) => {
                build_tray(&mut app);
                update(&mut app);
            }
            Event::NewEvents(StartCause::ResumeTimeReached { .. }) => {
                update(&mut app);
            }
            _ => {}
        }

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
    let cpu_item = IconMenuItem::new("CPU", false, None, None);
    let cores_item = MenuItem::new("Cores", false, None);
    let mem_item = IconMenuItem::new("Memory", false, None, None);
    let disk_item = IconMenuItem::new("Disk", false, None, None);
    let net_item = MenuItem::new("Network", false, None);
    let batt_item = IconMenuItem::new("Battery", false, None, None);
    let quit = MenuItem::new("Quit PeterFan", true, None);
    let sep = PredefinedMenuItem::separator();
    let sep2 = PredefinedMenuItem::separator();

    let mut items: Vec<&dyn tray_icon::menu::IsMenuItem> = vec![
        &header,
        &sep,
        &cpu_item,
        &cores_item,
        &mem_item,
        &disk_item,
        &net_item,
    ];
    if app.has_battery {
        items.push(&batt_item);
    }
    items.push(&sep2);
    items.push(&quit);
    let _ = menu.append_items(&items);

    let mut builder = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_icon(make_ring_icon());
    #[cfg(target_os = "macos")]
    {
        builder = builder.with_icon_as_template(true);
    }

    match builder.build() {
        Ok(tray) => {
            app.quit_id = Some(quit.id().clone());
            app.header = Some(header);
            app.cpu_item = Some(cpu_item);
            app.cores_item = Some(cores_item);
            app.mem_item = Some(mem_item);
            app.disk_item = Some(disk_item);
            app.net_item = Some(net_item);
            app.batt_item = Some(batt_item);
            app.tray = Some(tray);
        }
        Err(e) => eprintln!("failed to create menu-bar item: {e}"),
    }
}

/// Re-sample metrics and push them to the menu bar and dropdown.
fn update(app: &mut App) {
    app.monitor.refresh();
    let cpu = app.monitor.cpu();
    let mem = app.monitor.memory();
    let disks = app.monitor.disks();
    let nets = app.monitor.networks();
    let rx: f64 = nets.iter().map(|n| n.rx_rate).sum();
    let tx: f64 = nets.iter().map(|n| n.tx_rate).sum();

    // Menu-bar title: a tiny CPU sparkline + percentage.
    app.history.push(cpu.usage_percent);
    if app.history.len() > SPARK_LEN {
        app.history.remove(0);
    }
    if let Some(tray) = &app.tray {
        let title = format!("{} {:>2.0}%", spark(&app.history), cpu.usage_percent);
        set_menubar_text(tray, &title);
    }

    if let Some(h) = &app.header {
        h.set_text(format!("PeterFan  ·  {}", cpu.brand));
    }
    if let Some(i) = &app.cpu_item {
        i.set_icon(Some(dot(load_color(cpu.usage_percent))));
        i.set_text(format!(
            "CPU      {}  {:>3.0}%   {:.1} GHz",
            bar(cpu.usage_percent),
            cpu.usage_percent,
            cpu.frequency_mhz as f64 / 1000.0
        ));
    }
    if let Some(i) = &app.cores_item {
        i.set_text(format!("Cores    {}", core_spark(&cpu.per_core)));
    }
    if let Some(i) = &app.mem_item {
        i.set_icon(Some(dot(load_color(mem.used_percent))));
        i.set_text(format!(
            "Memory   {}  {:>3.0}%   {} / {}",
            bar(mem.used_percent),
            mem.used_percent,
            bytes(mem.used),
            bytes(mem.total)
        ));
    }
    if let Some(i) = &app.disk_item {
        if let Some(d) = disks.first() {
            i.set_icon(Some(dot(load_color(d.used_percent))));
            i.set_text(format!(
                "Disk     {}  {:>3.0}%   {}",
                bar(d.used_percent),
                d.used_percent,
                d.mount
            ));
        }
    }
    if let Some(i) = &app.net_item {
        i.set_text(format!(
            "Network      ↓ {}/s    ↑ {}/s",
            bytes(rx as u64),
            bytes(tx as u64)
        ));
    }
    if app.has_battery {
        if let (Some(i), Some(b)) = (&app.batt_item, app.monitor.battery()) {
            i.set_icon(Some(dot(charge_color(b.charge_percent))));
            i.set_text(format!(
                "Battery  {}  {:>3.0}%   {}",
                bar(b.charge_percent),
                b.charge_percent,
                b.state
            ));
        }
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

// ---------------------------------------------------------------------------
// Formatting helpers
// ---------------------------------------------------------------------------

/// A `▕████░░░░░▏` block-bar gauge for a 0..=100 percentage.
fn bar(pct: f32) -> String {
    let filled = ((pct / 100.0).clamp(0.0, 1.0) * BAR_WIDTH as f32).round() as usize;
    let mut s = String::with_capacity(BAR_WIDTH + 2);
    s.push('▕');
    for i in 0..BAR_WIDTH {
        s.push(if i < filled { '█' } else { '░' });
    }
    s.push('▏');
    s
}

/// A history sparkline using vertical block characters.
fn spark(values: &[f32]) -> String {
    const BLOCKS: [char; 9] = [' ', '▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    values
        .iter()
        .map(|&p| BLOCKS[(((p / 100.0).clamp(0.0, 1.0) * 8.0).round() as usize).min(8)])
        .collect()
}

/// Per-core load as a compact sparkline.
fn core_spark(per_core: &[f32]) -> String {
    spark(per_core)
}

/// Compact base-1024 byte formatting.
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

// ---------------------------------------------------------------------------
// Icons
// ---------------------------------------------------------------------------

/// Green / amber / red by load (macOS system-ish colors).
fn load_color(pct: f32) -> (u8, u8, u8) {
    match pct {
        x if x < 50.0 => (52, 199, 89),
        x if x < 80.0 => (255, 204, 0),
        _ => (255, 59, 48),
    }
}

/// Battery color: green when full, red when low.
fn charge_color(pct: f32) -> (u8, u8, u8) {
    match pct {
        x if x > 50.0 => (52, 199, 89),
        x if x > 20.0 => (255, 204, 0),
        _ => (255, 59, 48),
    }
}

/// A small filled, anti-aliased colored disc used as a per-row status dot.
fn dot(color: (u8, u8, u8)) -> MenuIcon {
    const S: u32 = 18;
    let c = (S as f32 - 1.0) / 2.0;
    let r = 6.5_f32;
    let mut rgba = vec![0u8; (S * S * 4) as usize];
    for y in 0..S {
        for x in 0..S {
            let d = (((x as f32 - c).powi(2)) + ((y as f32 - c).powi(2))).sqrt();
            let a = (r + 0.5 - d).clamp(0.0, 1.0);
            let idx = ((y * S + x) * 4) as usize;
            rgba[idx] = color.0;
            rgba[idx + 1] = color.1;
            rgba[idx + 2] = color.2;
            rgba[idx + 3] = (a * 255.0) as u8;
        }
    }
    MenuIcon::from_rgba(rgba, S, S).expect("valid dot icon")
}

/// The 32×32 ring (fan hub) menu-bar icon, drawn as a template on macOS.
fn make_ring_icon() -> Icon {
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
