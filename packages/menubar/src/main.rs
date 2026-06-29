//! `peterfan-menubar` — live system metrics in the macOS menu bar.
//!
//! - **Menu-bar title**: a tiny CPU-usage sparkline + percentage.
//! - **Left-click** the icon: a clean popover dashboard (a borderless WebView
//!   window rendering an HTML/CSS panel — CPU with per-core bars, memory, disk,
//!   battery, network), refreshed once a second. Closes when it loses focus.
//! - **Right-click** the icon: a native menu with the same figures as a
//!   fallback, plus Quit.
//!
//! Runs as an accessory app (no Dock icon). On Windows the same binary shows a
//! tray icon + tooltip and the popover. Pass `--mock` for the simulated machine.

use std::time::{Duration, Instant};

use tao::dpi::{LogicalSize, PhysicalPosition};
use tao::event::{Event, StartCause, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder, EventLoopWindowTarget};
use tao::window::{Window, WindowBuilder};

#[cfg(target_os = "macos")]
use tao::platform::macos::{ActivationPolicy, EventLoopExtMacOS};

use tray_icon::menu::{
    Icon as MenuIcon, IconMenuItem, Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem,
};
use tray_icon::{Icon, MouseButton, MouseButtonState, Rect, TrayIcon, TrayIconBuilder, TrayIconEvent};
use wry::{WebView, WebViewBuilder};

use peterfan_core::SystemMonitor;

const REFRESH: Duration = Duration::from_secs(1);
const SPARK_LEN: usize = 7;
const BAR_WIDTH: usize = 9;
const POPOVER_W: f64 = 360.0;
const POPOVER_H: f64 = 560.0;

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
    window: Option<Window>,
    webview: Option<WebView>,
    popover_visible: bool,
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
        window: None,
        webview: None,
        popover_visible: false,
    };

    event_loop.run(move |event, target, control_flow| {
        *control_flow = ControlFlow::WaitUntil(Instant::now() + REFRESH);

        match event {
            Event::NewEvents(StartCause::Init) => {
                build_tray(&mut app);
                build_popover(&mut app, target);
                update(&mut app);
            }
            Event::NewEvents(StartCause::ResumeTimeReached { .. }) => {
                update(&mut app);
            }
            // Close the popover when the user clicks away.
            Event::WindowEvent {
                event: WindowEvent::Focused(false),
                ..
            } => {
                hide_popover(&mut app);
            }
            _ => {}
        }

        while let Ok(menu_event) = MenuEvent::receiver().try_recv() {
            if app.quit_id.as_ref() == Some(&menu_event.id) {
                *control_flow = ControlFlow::Exit;
            }
        }
        while let Ok(tray_event) = TrayIconEvent::receiver().try_recv() {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                rect,
                ..
            } = tray_event
            {
                toggle_popover(&mut app, rect);
            }
        }
    });
}

// ---------------------------------------------------------------------------
// Popover (WebView dashboard)
// ---------------------------------------------------------------------------

fn build_popover(app: &mut App, target: &EventLoopWindowTarget<()>) {
    let window = match WindowBuilder::new()
        .with_decorations(false)
        .with_resizable(false)
        .with_visible(false)
        .with_always_on_top(true)
        .with_transparent(true)
        .with_inner_size(LogicalSize::new(POPOVER_W, POPOVER_H))
        .build(target)
    {
        Ok(w) => w,
        Err(e) => {
            eprintln!("failed to create popover window: {e}");
            return;
        }
    };

    match WebViewBuilder::new()
        .with_html(DASHBOARD_HTML)
        .with_transparent(true)
        .build(&window)
    {
        Ok(webview) => {
            app.window = Some(window);
            app.webview = Some(webview);
        }
        Err(e) => eprintln!("failed to create popover webview: {e}"),
    }
}

fn toggle_popover(app: &mut App, rect: Rect) {
    if app.popover_visible {
        hide_popover(app);
        return;
    }
    if let Some(w) = &app.window {
        // Right-align the popover under the menu-bar icon.
        let scale = w.scale_factor();
        let win_w = POPOVER_W * scale;
        let x = (rect.position.x + rect.size.width as f64 - win_w).max(8.0);
        let y = rect.position.y + rect.size.height as f64 + 4.0;
        w.set_outer_position(PhysicalPosition::new(x, y));
        w.set_visible(true);
        w.set_focus();
        app.popover_visible = true;
        update(app);
    }
}

fn hide_popover(app: &mut App) {
    if let Some(w) = &app.window {
        w.set_visible(false);
    }
    app.popover_visible = false;
}

// ---------------------------------------------------------------------------
// Tray + native menu (fallback)
// ---------------------------------------------------------------------------

fn build_tray(app: &mut App) {
    let menu = Menu::new();
    let header = MenuItem::new("PeterFan", false, None);
    let cpu_item = IconMenuItem::new("CPU", true, None, None);
    let cores_item = MenuItem::new("Cores", true, None);
    let mem_item = IconMenuItem::new("Memory", true, None, None);
    let disk_item = IconMenuItem::new("Disk", true, None, None);
    let net_item = MenuItem::new("Network", true, None);
    let batt_item = IconMenuItem::new("Battery", true, None, None);
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
        // Left-click opens the popover (we handle the event); right-click the menu.
        .with_menu_on_left_click(false)
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

// ---------------------------------------------------------------------------
// Update: sample once, push to the menu bar, the native menu, and the popover.
// ---------------------------------------------------------------------------

fn update(app: &mut App) {
    app.monitor.refresh();
    let cpu = app.monitor.cpu();
    let mem = app.monitor.memory();
    let disks = app.monitor.disks();
    let nets = app.monitor.networks();
    let battery = if app.has_battery {
        app.monitor.battery()
    } else {
        None
    };
    let rx: f64 = nets.iter().map(|n| n.rx_rate).sum();
    let tx: f64 = nets.iter().map(|n| n.tx_rate).sum();

    // Menu-bar title sparkline + percentage.
    app.history.push(cpu.usage_percent);
    if app.history.len() > SPARK_LEN {
        app.history.remove(0);
    }
    if let Some(tray) = &app.tray {
        set_menubar_text(tray, &format!("{} {:>2.0}%", spark(&app.history), cpu.usage_percent));
    }

    let load_str = cpu
        .load_avg
        .map(|l| format!("   load {:.2} {:.2} {:.2}", l.one, l.five, l.fifteen))
        .unwrap_or_default();
    let ghz = cpu.frequency_mhz as f64 / 1000.0;

    // Native menu rows (fallback).
    if let Some(h) = &app.header {
        h.set_text(format!("PeterFan  ·  {}", cpu.brand));
    }
    if let Some(i) = &app.cpu_item {
        i.set_icon(Some(dot(load_color(cpu.usage_percent))));
        i.set_text(format!("CPU      {}  {:>3.0}%   {:.1} GHz", bar(cpu.usage_percent), cpu.usage_percent, ghz));
    }
    if let Some(i) = &app.cores_item {
        i.set_text(format!("Cores    {}", spark(&cpu.per_core)));
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
            i.set_text(format!("Disk     {}  {:>3.0}%   {}", bar(d.used_percent), d.used_percent, d.mount));
        }
    }
    if let Some(i) = &app.net_item {
        i.set_text(format!("Network      ↓ {}/s    ↑ {}/s", bytes(rx as u64), bytes(tx as u64)));
    }
    if let (Some(i), Some(b)) = (&app.batt_item, &battery) {
        i.set_icon(Some(dot(charge_color(b.charge_percent))));
        i.set_text(format!("Battery  {}  {:>3.0}%   {}", bar(b.charge_percent), b.charge_percent, b.state));
    }

    // Popover (only when visible).
    if app.popover_visible {
        if let Some(wv) = &app.webview {
            let disk = disks.first();
            let payload = serde_json::json!({
                "cpu_pct": cpu.usage_percent,
                "cpu_text": format!("{:.1}%", cpu.usage_percent),
                "cpu_sub": format!("{:.1} GHz{}", ghz, load_str),
                "cores": &cpu.per_core,
                "mem_pct": mem.used_percent,
                "mem_text": format!("{:.1}%", mem.used_percent),
                "mem_sub": format!(
                    "{} / {}    swap {} / {}",
                    bytes(mem.used), bytes(mem.total), bytes(mem.swap_used), bytes(mem.swap_total)
                ),
                "disk_pct": disk.map(|d| d.used_percent).unwrap_or(0.0),
                "disk_text": disk.map(|d| format!("{:.1}%", d.used_percent)).unwrap_or_default(),
                "disk_sub": disk.map(|d| format!("{} / {}    {}", bytes(d.used), bytes(d.total), d.mount)).unwrap_or_default(),
                "batt_present": battery.is_some(),
                "batt_pct": battery.as_ref().map(|b| b.charge_percent).unwrap_or(0.0),
                "batt_text": battery.as_ref().map(|b| format!("{:.0}%", b.charge_percent)).unwrap_or_default(),
                "batt_sub": battery.as_ref().map(|b| {
                    let mut s = b.state.clone();
                    if let Some(c) = b.cycle_count { s.push_str(&format!("    {c} cycles")); }
                    if let Some(h) = b.health_percent { s.push_str(&format!("    health {h:.0}%")); }
                    s
                }).unwrap_or_default(),
                "net_sub": format!("↓ {}/s     ↑ {}/s", bytes(rx as u64), bytes(tx as u64)),
            });
            let _ = wv.evaluate_script(&format!(
                "window.__pf&&window.__pf.update({})",
                payload
            ));
        }
    }
}

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

fn spark(values: &[f32]) -> String {
    const BLOCKS: [char; 9] = [' ', '▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    values
        .iter()
        .map(|&p| BLOCKS[(((p / 100.0).clamp(0.0, 1.0) * 8.0).round() as usize).min(8)])
        .collect()
}

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

fn load_color(pct: f32) -> (u8, u8, u8) {
    match pct {
        x if x < 50.0 => (52, 199, 89),
        x if x < 80.0 => (255, 204, 0),
        _ => (255, 59, 48),
    }
}
fn charge_color(pct: f32) -> (u8, u8, u8) {
    match pct {
        x if x > 50.0 => (52, 199, 89),
        x if x > 20.0 => (255, 204, 0),
        _ => (255, 59, 48),
    }
}

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

// ---------------------------------------------------------------------------
// The popover dashboard (self-contained HTML/CSS/JS).
// ---------------------------------------------------------------------------

const DASHBOARD_HTML: &str = r##"<!doctype html><html><head><meta charset="utf-8"><meta name="color-scheme" content="dark">
<style>
:root{--g:#34c759;--y:#ffcc00;--r:#ff3b30;--accent:#5b9dff;}
*{box-sizing:border-box;margin:0;padding:0;}
html,body{background:transparent;font-family:-apple-system,system-ui,sans-serif;color:#e8edf4;-webkit-user-select:none;cursor:default;}
.panel{background:#1c1c1e;border:1px solid #38383a;border-radius:14px;overflow:hidden;}
.row{display:grid;grid-template-columns:40px 1fr;gap:12px;padding:13px 16px;align-items:start;}
.row + .row{border-top:1px solid #2c2c2e;}
.ic{width:30px;height:30px;color:#8a93a2;margin-top:2px;}
.ic svg{width:100%;height:100%;fill:none;stroke:currentColor;stroke-width:1.5;stroke-linecap:round;stroke-linejoin:round;}
.head{display:flex;justify-content:space-between;align-items:baseline;}
.name{font-size:12px;color:#9aa3b2;letter-spacing:.02em;}
.val{font-size:18px;font-weight:700;letter-spacing:-.01em;}
.sub{font-size:11px;color:#8a93a2;margin-top:3px;line-height:1.5;white-space:pre-line;}
.bar{height:6px;background:#3a3a3c;border-radius:99px;margin-top:9px;overflow:hidden;}
.bar-fill{height:100%;border-radius:99px;width:0;transition:width .3s ease;}
.bar-fill.g{background:var(--g);}.bar-fill.y{background:var(--y);}.bar-fill.r{background:var(--r);}
.cores{display:flex;align-items:flex-end;gap:2px;height:16px;margin-top:9px;}
.core{flex:1;background:var(--accent);border-radius:1px;min-height:2px;opacity:.85;}
.foot{padding:9px 16px;color:#6b7280;font-size:11px;text-align:center;border-top:1px solid #2c2c2e;}
</style></head><body><div class="panel">

<div class="row"><div class="ic"><svg viewBox="0 0 24 24"><rect x="6" y="6" width="12" height="12" rx="2"/><path d="M9 2v3M15 2v3M9 19v3M15 19v3M2 9h3M2 15h3M19 9h3M19 15h3"/></svg></div>
<div><div class="head"><span class="name">CPU</span><span class="val" id="cpu-val">—</span></div>
<div class="sub" id="cpu-sub"></div><div class="cores" id="cores"></div>
<div class="bar"><div class="bar-fill" id="cpu-bar"></div></div></div></div>

<div class="row"><div class="ic"><svg viewBox="0 0 24 24"><rect x="2" y="7" width="20" height="11" rx="1.5"/><path d="M6 18v2M10 18v2M14 18v2M18 18v2M6 10v4M10 10v4M14 10v4"/></svg></div>
<div><div class="head"><span class="name">Memory</span><span class="val" id="mem-val">—</span></div>
<div class="sub" id="mem-sub"></div><div class="bar"><div class="bar-fill" id="mem-bar"></div></div></div></div>

<div class="row"><div class="ic"><svg viewBox="0 0 24 24"><ellipse cx="12" cy="6" rx="8" ry="3"/><path d="M4 6v12c0 1.7 3.6 3 8 3s8-1.3 8-3V6"/><path d="M4 12c0 1.7 3.6 3 8 3s8-1.3 8-3"/></svg></div>
<div><div class="head"><span class="name">Storage</span><span class="val" id="disk-val">—</span></div>
<div class="sub" id="disk-sub"></div><div class="bar"><div class="bar-fill" id="disk-bar"></div></div></div></div>

<div class="row" id="sec-batt"><div class="ic"><svg viewBox="0 0 24 24"><rect x="2" y="8" width="18" height="9" rx="2"/><path d="M22 11v3"/></svg></div>
<div><div class="head"><span class="name">Battery</span><span class="val" id="batt-val">—</span></div>
<div class="sub" id="batt-sub"></div><div class="bar"><div class="bar-fill" id="batt-bar"></div></div></div></div>

<div class="row"><div class="ic"><svg viewBox="0 0 24 24"><circle cx="12" cy="12" r="9"/><path d="M3 12h18M12 3c2.5 2.5 2.5 15 0 18M12 3c-2.5 2.5-2.5 15 0 18"/></svg></div>
<div><div class="head"><span class="name">Network</span><span class="val"></span></div>
<div class="sub" id="net-sub"></div></div></div>

<div class="foot">PeterFan · right-click the icon for the menu</div>
</div>
<script>
window.__pf={update:function(d){
 function cls(p){return p<50?'g':p<80?'y':'r';}
 function setBar(id,p,c){var b=document.getElementById(id);if(b){b.style.width=Math.max(0,Math.min(100,p))+'%';b.className='bar-fill '+(c||cls(p));}}
 function set(id,t){var e=document.getElementById(id);if(e)e.textContent=t;}
 set('cpu-val',d.cpu_text);set('cpu-sub',d.cpu_sub);setBar('cpu-bar',d.cpu_pct);
 var cc=document.getElementById('cores');if(cc){cc.innerHTML='';(d.cores||[]).forEach(function(p){var s=document.createElement('span');s.className='core';s.style.height=Math.max(8,Math.min(100,p))+'%';cc.appendChild(s);});}
 set('mem-val',d.mem_text);set('mem-sub',d.mem_sub);setBar('mem-bar',d.mem_pct);
 set('disk-val',d.disk_text);set('disk-sub',d.disk_sub);setBar('disk-bar',d.disk_pct);
 var sb=document.getElementById('sec-batt');if(sb)sb.style.display=d.batt_present?'':'none';
 if(d.batt_present){set('batt-val',d.batt_text);set('batt-sub',d.batt_sub);setBar('batt-bar',d.batt_pct,d.batt_pct>50?'g':d.batt_pct>20?'y':'r');}
 set('net-sub',d.net_sub);
}};
</script></body></html>"##;
