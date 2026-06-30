//! `peterfan-menubar` — live system metrics in the macOS menu bar.
//!
//! The menu-bar title shows a tiny CPU sparkline + percentage. Clicking the
//! icon (left **or** right / two-finger) toggles a clean popover dashboard — a
//! borderless WebView rendering an HTML/CSS panel with CPU (per-core), memory,
//! storage, temperatures, fans, battery, and network. Quit from the button in
//! the popover. Runs as an accessory app (no Dock icon). `--mock` uses the
//! simulated machine.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use tao::dpi::{LogicalSize, PhysicalPosition};
use tao::event::{Event, StartCause, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder, EventLoopWindowTarget};
use tao::window::{Window, WindowBuilder};

#[cfg(target_os = "macos")]
use tao::platform::macos::{ActivationPolicy, EventLoopExtMacOS};

use tray_icon::{Icon, MouseButtonState, Rect, TrayIcon, TrayIconBuilder, TrayIconEvent};
use wry::{WebView, WebViewBuilder};

use peterfan_core::error::CoreError;
use peterfan_core::profile::Profile;
use peterfan_core::types::Celsius;
use peterfan_core::{HardwareProvider, SystemMonitor};

const REFRESH: Duration = Duration::from_secs(1);
const POPOVER_W: f64 = 348.0;
/// Initial height; the popover then reports its real content height (below) and
/// the window is resized to fit exactly.
const POPOVER_H: f64 = 520.0;

/// Set by the popover's Quit button (via WebView IPC), polled by the loop.
static QUIT: AtomicBool = AtomicBool::new(false);
/// Content height (CSS px) reported by the popover; 0 = not yet measured.
static DESIRED_H: AtomicU32 = AtomicU32::new(0);
/// Height already applied to the window, to avoid resizing every tick.
static APPLIED_H: AtomicU32 = AtomicU32::new(0);
/// Control commands queued by popover buttons (`auto`, `profile:gaming`).
static PENDING: Mutex<Vec<String>> = Mutex::new(Vec::new());
/// Last control result, shown in the popover.
static STATUS: Mutex<String> = Mutex::new(String::new());

struct App {
    monitor: Box<dyn SystemMonitor>,
    provider: Box<dyn HardwareProvider>,
    has_battery: bool,
    tray: Option<TrayIcon>,
    window: Option<Window>,
    webview: Option<WebView>,
    popover_visible: bool,
}

fn main() {
    let use_mock = std::env::args().any(|a| a == "--mock");
    let (monitor, provider): (Box<dyn SystemMonitor>, Box<dyn HardwareProvider>) = if use_mock {
        (peterfan_platform::mock_monitor(), peterfan_platform::mock())
    } else {
        (
            peterfan_platform::system_monitor(),
            peterfan_platform::detect(),
        )
    };
    let has_battery = monitor.capabilities().battery;

    #[allow(unused_mut)]
    let mut event_loop = EventLoopBuilder::<()>::new().build();
    #[cfg(target_os = "macos")]
    event_loop.set_activation_policy(ActivationPolicy::Accessory);

    let mut app = App {
        monitor,
        provider,
        has_battery,
        tray: None,
        window: None,
        webview: None,
        popover_visible: false,
    };

    event_loop.run(move |event, target, control_flow| {
        *control_flow = ControlFlow::WaitUntil(Instant::now() + REFRESH);

        if QUIT.load(Ordering::Relaxed) {
            *control_flow = ControlFlow::Exit;
            return;
        }

        match event {
            Event::NewEvents(StartCause::Init) => {
                build_tray(&mut app);
                build_popover(&mut app, target);
                update(&mut app);
            }
            Event::NewEvents(StartCause::ResumeTimeReached { .. }) => {
                update(&mut app);
            }
            Event::WindowEvent {
                event: WindowEvent::Focused(false),
                ..
            } => hide_popover(&mut app),
            _ => {}
        }

        // Run any control commands queued by the popover buttons.
        let cmds: Vec<String> = std::mem::take(&mut *PENDING.lock().expect("pending poisoned"));
        if !cmds.is_empty() {
            for c in &cmds {
                let status = execute_control(app.provider.as_ref(), c);
                *STATUS.lock().expect("status poisoned") = status;
            }
            update(&mut app); // reflect the new status immediately
        }

        // Resize the popover window to the height the WebView reported, so it
        // fits the content exactly (no empty space, no clipping).
        let desired = DESIRED_H.load(Ordering::Relaxed);
        if desired > 0 && desired != APPLIED_H.load(Ordering::Relaxed) {
            if let Some(w) = &app.window {
                w.set_inner_size(LogicalSize::new(POPOVER_W, desired as f64));
                APPLIED_H.store(desired, Ordering::Relaxed);
            }
        }

        while let Ok(ev) = TrayIconEvent::receiver().try_recv() {
            // Left or right (two-finger) click both toggle the popover.
            if let TrayIconEvent::Click {
                button_state: MouseButtonState::Up,
                rect,
                ..
            } = ev
            {
                toggle_popover(&mut app, rect);
            }
        }
    });
}

// ---------------------------------------------------------------------------
// Tray icon (no native menu — the popover is the whole UI)
// ---------------------------------------------------------------------------

fn build_tray(app: &mut App) {
    #[allow(unused_mut)]
    let mut builder = TrayIconBuilder::new().with_icon(make_ring_icon());
    #[cfg(target_os = "macos")]
    {
        builder = builder.with_icon_as_template(true);
    }
    match builder.build() {
        Ok(tray) => app.tray = Some(tray),
        Err(e) => eprintln!("failed to create menu-bar item: {e}"),
    }
}

// ---------------------------------------------------------------------------
// Popover
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
        .with_ipc_handler(|req| {
            let body = req.body();
            if body == "quit" {
                QUIT.store(true, Ordering::Relaxed);
            } else if let Some(h) = body.strip_prefix("h:") {
                if let Ok(v) = h.trim().parse::<u32>() {
                    DESIRED_H.store(v, Ordering::Relaxed);
                }
            } else if let Some(cmd) = body.strip_prefix("cmd:") {
                PENDING
                    .lock()
                    .expect("pending poisoned")
                    .push(cmd.to_string());
            }
        })
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
// Update: sample once, refresh the menu-bar title and (if open) the popover.
// ---------------------------------------------------------------------------

fn update(app: &mut App) {
    app.monitor.refresh();
    let cpu = app.monitor.cpu();

    // Clean, readable menu-bar title — the CPU percentage at the same precision
    // as the popover (one decimal), so the two never disagree.
    if let Some(tray) = &app.tray {
        set_menubar_text(tray, &format!("{:.1}%", cpu.usage_percent));
    }

    if !app.popover_visible {
        return;
    }
    let Some(wv) = &app.webview else { return };

    let mem = app.monitor.memory();
    let disks = app.monitor.disks();
    let nets = app.monitor.networks();
    let battery = if app.has_battery {
        app.monitor.battery()
    } else {
        None
    };
    let temps = app.provider.temperatures().unwrap_or_default();
    let fans = app.provider.fans().unwrap_or_default();
    let power = app.provider.power_watts();

    let rx: f64 = nets.iter().map(|n| n.rx_rate).sum();
    let tx: f64 = nets.iter().map(|n| n.tx_rate).sum();
    let ghz = cpu.frequency_mhz as f64 / 1000.0;
    let load_str = cpu
        .load_avg
        .map(|l| format!("load {:.2} {:.2} {:.2}", l.one, l.five, l.fifteen))
        .unwrap_or_default();
    let disk = disks.first();

    // Temperatures: hottest is the headline; every sensor is listed below
    // (so multiple CPU-die clusters / sensors are all visible).
    let hottest = temps.iter().max_by(|a, b| {
        a.value
            .0
            .partial_cmp(&b.value.0)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let temp_rows: Vec<_> = temps
        .iter()
        .map(|t| {
            serde_json::json!({
                "l": t.label,
                "c": format!("{:.0}°C", t.value.0),
                "cls": temp_cls(t.value),
            })
        })
        .collect();

    // Fans: every fan listed with its own RPM and a speed bar (rpm / max).
    let fan_rows: Vec<_> = fans
        .iter()
        .map(|f| {
            let pct = match f.max_rpm {
                Some(m) if m > 0 => (f.rpm as f32 / m as f32 * 100.0).clamp(0.0, 100.0),
                _ => 0.0,
            };
            serde_json::json!({ "l": f.label, "rpm": format!("{} rpm", f.rpm), "pct": pct })
        })
        .collect();

    // Daemon status: poll every tick so the popover always shows current mode.
    let daemon_st = daemon_status_str();
    let can_control = app.provider.capabilities().control_fans || !daemon_st.is_empty();
    let ctl_status = if !daemon_st.is_empty() {
        daemon_st.clone()
    } else {
        STATUS.lock().expect("status poisoned").clone()
    };

    let payload = serde_json::json!({
        "cpu_pct": cpu.usage_percent,
        "cpu_text": format!("{:.1}%", cpu.usage_percent),
        "cpu_sub": format!(
            "{:.1} GHz   {}{}",
            ghz,
            load_str,
            power.map(|w| format!("   {w:.1} W")).unwrap_or_default()
        ),
        "cores": &cpu.per_core,
        "mem_pct": mem.used_percent,
        "mem_text": format!("{:.1}%", mem.used_percent),
        "mem_sub": format!(
            "{} / {}   swap {} / {}",
            bytes(mem.used), bytes(mem.total), bytes(mem.swap_used), bytes(mem.swap_total)
        ),
        "disk_pct": disk.map(|d| d.used_percent).unwrap_or(0.0),
        "disk_text": disk.map(|d| format!("{:.1}%", d.used_percent)).unwrap_or_default(),
        "disk_sub": disk.map(|d| format!("{} / {}   {}", bytes(d.used), bytes(d.total), d.mount)).unwrap_or_default(),
        "temp_present": hottest.is_some(),
        "temp_pct": hottest.map(|t| t.value.0).unwrap_or(0.0),
        "temp_text": hottest.map(|t| format!("{:.0}°C", t.value.0)).unwrap_or_default(),
        "temp_cls": hottest.map(|t| temp_cls(t.value)).unwrap_or("g"),
        "temps": temp_rows,
        "fans_present": !fans.is_empty(),
        "fans_text": if fans.len() > 1 { format!("{} fans", fans.len()) } else { fans.first().map(|f| format!("{} rpm", f.rpm)).unwrap_or_default() },
        "fans": fan_rows,
        "batt_present": battery.is_some(),
        "batt_pct": battery.as_ref().map(|b| b.charge_percent).unwrap_or(0.0),
        "batt_text": battery.as_ref().map(|b| format!("{:.0}%", b.charge_percent)).unwrap_or_default(),
        "batt_sub": battery.as_ref().map(|b| {
            let mut s = b.state.clone();
            if let Some(c) = b.cycle_count { s.push_str(&format!("   {c} cycles")); }
            if let Some(h) = b.health_percent { s.push_str(&format!("   health {h:.0}%")); }
            s
        }).unwrap_or_default(),
        "net_sub": format!("↓ {}/s     ↑ {}/s", bytes(rx as u64), bytes(tx as u64)),
        "can_control": can_control,
        "ctl_status": ctl_status,
        "daemon_running": !daemon_st.is_empty(),
    });
    let _ = wv.evaluate_script(&format!("window.__pf&&window.__pf.update({})", payload));
}

/// Query the running daemon for its current mode/profile, for the status line.
/// Returns an empty string when no daemon is reachable.
fn daemon_status_str() -> String {
    #[cfg(unix)]
    if let Some(reply) = peterfan_platform::ipc::send_command("status") {
        if let Some(rest) = reply.strip_prefix("ok ") {
            return rest.to_string();
        }
    }
    String::new()
}

/// Run a popover control action (`auto` or `profile:<name>`). Prefers the
/// running `peterfand` daemon (so the unprivileged app needs no root); falls
/// back to controlling fans directly if this process happens to have access.
/// Returns a short human-readable status for the popover.
fn execute_control(provider: &dyn HardwareProvider, cmd: &str) -> String {
    let line = match cmd.strip_prefix("profile:") {
        Some(name) => format!("profile {name}\n"),
        None => format!("{cmd}\n"),
    };

    #[cfg(unix)]
    if let Some(mut stream) = peterfan_platform::ipc::connect() {
        use std::io::{Read, Write};
        let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
        if stream.write_all(line.as_bytes()).is_ok() {
            let mut buf = [0u8; 96];
            let n = stream.read(&mut buf).unwrap_or(0);
            let reply = String::from_utf8_lossy(&buf[..n]).trim().to_string();
            return format!("daemon: {}", if reply.is_empty() { "ok" } else { &reply });
        }
    }

    apply_local(provider, cmd)
}

/// Apply a control action directly via the hardware provider (needs privileges).
fn apply_local(provider: &dyn HardwareProvider, cmd: &str) -> String {
    if !provider.capabilities().control_fans {
        return "no fan control on this backend".into();
    }
    let fans: Vec<String> = provider
        .fans()
        .unwrap_or_default()
        .into_iter()
        .filter(|f| f.controllable)
        .map(|f| f.id)
        .collect();

    let (result, label) = if cmd == "auto" {
        (
            fans.iter().try_for_each(|id| provider.set_fan_auto(id)),
            "auto".to_string(),
        )
    } else if let Some(name) = cmd.strip_prefix("profile:") {
        match Profile::parse(name) {
            Some(p) => {
                let temps = provider.temperatures().unwrap_or_default();
                let hot = temps.iter().map(|t| t.value.0).fold(0.0_f32, f32::max);
                let duty = p.default_curve().duty_at(hot);
                (
                    fans.iter()
                        .try_for_each(|id| provider.set_fan_duty(id, duty)),
                    format!("{} ({duty}%)", p.as_str()),
                )
            }
            None => return "unknown profile".into(),
        }
    } else {
        return "unknown command".into();
    };

    match result {
        Ok(()) => format!("{label} — applied locally"),
        Err(CoreError::PermissionDenied(_)) => "start peterfand (needs root)".into(),
        Err(e) => format!("error: {e}"),
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
// Helpers
// ---------------------------------------------------------------------------

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

fn temp_cls(c: Celsius) -> &'static str {
    match c.0 {
        x if x < 50.0 => "g",
        x if x < 70.0 => "y",
        _ => "r",
    }
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
// Popover dashboard (self-contained HTML/CSS/JS).
// ---------------------------------------------------------------------------

const DASHBOARD_HTML: &str = r##"<!doctype html><html><head><meta charset="utf-8"><meta name="color-scheme" content="dark">
<style>
:root{--g:#30d158;--y:#ffd60a;--r:#ff453a;--accent:#5b9dff;--text:#f4f6fa;--dim:#7f8896;--line:rgba(255,255,255,.07);}
*{box-sizing:border-box;margin:0;padding:0;}
html,body{background:transparent;font-family:-apple-system,system-ui,sans-serif;color:var(--text);-webkit-user-select:none;cursor:default;-webkit-font-smoothing:antialiased;overflow:hidden;}
.panel{background:#1b1b1d;border:1px solid rgba(255,255,255,.09);border-radius:13px;overflow:hidden;}
.row{display:grid;grid-template-columns:24px 1fr;gap:12px;padding:8px 15px;align-items:center;}
.row + .row{border-top:1px solid var(--line);}
.ic{width:21px;height:21px;color:var(--dim);}
.ic svg{width:100%;height:100%;fill:none;stroke:currentColor;stroke-width:1.6;stroke-linecap:round;stroke-linejoin:round;}
.content{min-width:0;}
.head{display:flex;justify-content:space-between;align-items:baseline;gap:10px;}
.name{font-size:9.5px;font-weight:600;color:var(--dim);letter-spacing:.08em;text-transform:uppercase;}
.val{font-size:14px;font-weight:600;letter-spacing:-.01em;white-space:nowrap;font-variant-numeric:tabular-nums;}
.sub{font-size:10px;color:var(--dim);margin-top:1px;line-height:1.45;white-space:nowrap;overflow:hidden;text-overflow:ellipsis;font-variant-numeric:tabular-nums;}
.bar{height:3px;background:rgba(255,255,255,.08);border-radius:99px;margin-top:7px;overflow:hidden;}
.bar-fill{height:100%;border-radius:99px;width:0;transition:width .35s ease;}
.bar-fill.g{background:var(--g);}.bar-fill.y{background:var(--y);}.bar-fill.r{background:var(--r);}.bar-fill.b{background:var(--accent);}
.cores{display:flex;align-items:flex-end;gap:2px;height:11px;margin-top:7px;}
.core{flex:1;background:var(--accent);border-radius:1px;min-height:2px;opacity:.8;}
.trow{display:flex;justify-content:space-between;align-items:baseline;font-size:10.5px;margin-top:5px;}
.trow .l{color:var(--dim);}
.trow .v{font-weight:600;font-variant-numeric:tabular-nums;}
.v.g{color:var(--g);}.v.y{color:var(--y);}.v.r{color:var(--r);}
.frow{display:grid;grid-template-columns:auto 1fr auto;gap:9px;align-items:center;font-size:10.5px;margin-top:6px;}
.frow .l{color:var(--dim);white-space:nowrap;}
.frow .v{font-variant-numeric:tabular-nums;white-space:nowrap;}
.fbar{height:3px;background:rgba(255,255,255,.08);border-radius:99px;overflow:hidden;}
.fbar i{display:block;height:100%;background:var(--accent);border-radius:99px;width:0;transition:width .35s;}
.ctl{display:flex;flex-wrap:wrap;gap:5px;padding:9px 15px;border-top:1px solid var(--line);}
.ctl-head{flex:1 1 100%;display:flex;justify-content:space-between;align-items:baseline;margin-bottom:3px;}
.ctl-head .name{font-size:9.5px;font-weight:600;color:var(--dim);letter-spacing:.08em;text-transform:uppercase;}
.ctl-status{font-size:10px;color:var(--dim);font-variant-numeric:tabular-nums;}
.chip{flex:1 1 28%;background:rgba(255,255,255,.06);border:0;color:var(--text);font:inherit;font-size:10px;font-weight:600;padding:6px 4px;border-radius:7px;cursor:pointer;transition:background .15s;}
.chip:hover{background:rgba(91,157,255,.28);}
.chip.auto{background:rgba(48,209,88,.16);color:var(--g);}
.ctl-note{flex:1 1 100%;font-size:10.5px;color:var(--dim);line-height:1.5;}
.foot{border-top:1px solid var(--line);padding:3px;}
.quit{display:block;width:100%;background:transparent;border:0;color:var(--dim);font:inherit;font-size:10.5px;letter-spacing:.02em;padding:8px;border-radius:8px;cursor:pointer;transition:background .15s,color .15s;}
.quit:hover{background:rgba(255,255,255,.06);color:var(--text);}
</style></head><body><div class="panel">

<div class="row"><span class="ic"><svg viewBox="0 0 24 24"><rect x="6" y="6" width="12" height="12" rx="2"/><path d="M9 2v3M15 2v3M9 19v3M15 19v3M2 9h3M2 15h3M19 9h3M19 15h3"/></svg></span>
<div class="content"><div class="head"><span class="name">CPU</span><span class="val" id="cpu-val">—</span></div>
<div class="sub" id="cpu-sub"></div><div class="cores" id="cores"></div>
<div class="bar"><div class="bar-fill" id="cpu-bar"></div></div></div></div>

<div class="row"><span class="ic"><svg viewBox="0 0 24 24"><rect x="2" y="7" width="20" height="11" rx="1.5"/><path d="M6 18v2M10 18v2M14 18v2M18 18v2M6 10v4M10 10v4M14 10v4"/></svg></span>
<div class="content"><div class="head"><span class="name">Memory</span><span class="val" id="mem-val">—</span></div>
<div class="sub" id="mem-sub"></div><div class="bar"><div class="bar-fill" id="mem-bar"></div></div></div></div>

<div class="row"><span class="ic"><svg viewBox="0 0 24 24"><ellipse cx="12" cy="6" rx="8" ry="3"/><path d="M4 6v12c0 1.7 3.6 3 8 3s8-1.3 8-3V6"/><path d="M4 12c0 1.7 3.6 3 8 3s8-1.3 8-3"/></svg></span>
<div class="content"><div class="head"><span class="name">Storage</span><span class="val" id="disk-val">—</span></div>
<div class="sub" id="disk-sub"></div><div class="bar"><div class="bar-fill" id="disk-bar"></div></div></div></div>

<div class="row" id="sec-temp"><span class="ic"><svg viewBox="0 0 24 24"><path d="M14 14.76V5a2 2 0 0 0-4 0v9.76a4 4 0 1 0 4 0z"/></svg></span>
<div class="content"><div class="head"><span class="name">Temperature</span><span class="val" id="temp-val">—</span></div>
<div class="bar"><div class="bar-fill" id="temp-bar"></div></div><div id="temp-list"></div></div></div>

<div class="row" id="sec-fans"><span class="ic"><svg viewBox="0 0 24 24"><circle cx="12" cy="12" r="2.5"/><path d="M12 9.5c0-4 .5-6 2.5-6S18 6 14.5 10M12 14.5c4 0 6 .5 6 2.5s-2.5 3.5-6.5 0M9.5 12c-4 0-6-.5-6-2.5S6 6 10 9.5"/></svg></span>
<div class="content"><div class="head"><span class="name">Fans</span><span class="val" id="fans-val">—</span></div>
<div id="fans-list"></div></div></div>

<div class="row" id="sec-batt"><span class="ic"><svg viewBox="0 0 24 24"><rect x="2" y="8" width="18" height="9" rx="2"/><path d="M22 11v3"/></svg></span>
<div class="content"><div class="head"><span class="name">Battery</span><span class="val" id="batt-val">—</span></div>
<div class="sub" id="batt-sub"></div><div class="bar"><div class="bar-fill" id="batt-bar"></div></div></div></div>

<div class="row"><span class="ic"><svg viewBox="0 0 24 24"><circle cx="12" cy="12" r="9"/><path d="M3 12h18M12 3c2.5 2.5 2.5 15 0 18M12 3c-2.5 2.5-2.5 15 0 18"/></svg></span>
<div class="content"><div class="head"><span class="name">Network</span><span class="val"></span></div>
<div class="sub" id="net-sub"></div></div></div>

<div class="ctl">
<div class="ctl-head"><span class="name">Fan control</span><span class="ctl-status" id="ctl-status"></span></div>
<button class="chip auto" onclick="window.ipc.postMessage('cmd:auto')">Auto</button>
<button class="chip" onclick="window.ipc.postMessage('cmd:profile:silent')">Silent</button>
<button class="chip" onclick="window.ipc.postMessage('cmd:profile:balanced')">Balanced</button>
<button class="chip" onclick="window.ipc.postMessage('cmd:profile:gaming')">Gaming</button>
<button class="chip" onclick="window.ipc.postMessage('cmd:profile:performance')">Perf</button>
<button class="chip" onclick="window.ipc.postMessage('cmd:profile:maximum')">Max</button>
<div class="ctl-note" id="ctl-note" style="display:none"></div>
</div>
<div class="foot"><button class="quit" onclick="window.ipc.postMessage('quit')">Quit PeterFan</button></div>
</div>
<script>
window.__pf={update:function(d){
 function cls(p){return p<50?'g':p<80?'y':'r';}
 function bar(id,p,c){var b=document.getElementById(id);if(b){b.style.width=Math.max(0,Math.min(100,p))+'%';b.className='bar-fill '+(c||cls(p));}}
 function set(id,t){var e=document.getElementById(id);if(e)e.textContent=t;}
 function show(id,on){var e=document.getElementById(id);if(e)e.style.display=on?'':'none';}
 set('cpu-val',d.cpu_text);set('cpu-sub',d.cpu_sub);bar('cpu-bar',d.cpu_pct);
 var cc=document.getElementById('cores');if(cc){cc.innerHTML='';(d.cores||[]).forEach(function(p){var s=document.createElement('span');s.className='core';s.style.height=Math.max(8,Math.min(100,p))+'%';cc.appendChild(s);});}
 set('mem-val',d.mem_text);set('mem-sub',d.mem_sub);bar('mem-bar',d.mem_pct);
 set('disk-val',d.disk_text);set('disk-sub',d.disk_sub);bar('disk-bar',d.disk_pct);
 show('sec-temp',d.temp_present);if(d.temp_present){set('temp-val',d.temp_text);bar('temp-bar',d.temp_pct,d.temp_cls);
   var tl=document.getElementById('temp-list');if(tl){tl.innerHTML='';(d.temps||[]).forEach(function(t){var r=document.createElement('div');r.className='trow';r.innerHTML='<span class="l"></span><span class="v"></span>';r.children[0].textContent=t.l;r.children[1].textContent=t.c;r.children[1].className='v '+t.cls;tl.appendChild(r);});}}
 show('sec-fans',d.fans_present);if(d.fans_present){set('fans-val',d.fans_text);
   var fl=document.getElementById('fans-list');if(fl){fl.innerHTML='';(d.fans||[]).forEach(function(f){var r=document.createElement('div');r.className='frow';r.innerHTML='<span class="l"></span><span class="fbar"><i></i></span><span class="v"></span>';r.children[0].textContent=f.l;r.querySelector('.fbar i').style.width=Math.max(0,Math.min(100,f.pct))+'%';r.children[2].textContent=f.rpm;fl.appendChild(r);});}}
 show('sec-batt',d.batt_present);if(d.batt_present){set('batt-val',d.batt_text);set('batt-sub',d.batt_sub);bar('batt-bar',d.batt_pct,d.batt_pct>50?'g':d.batt_pct>20?'y':'r');}
 set('net-sub',d.net_sub);
 var chips=document.querySelectorAll(‘.chip’);
 for(var i=0;i<chips.length;i++){chips[i].style.display=d.can_control?’’:’none’;}
 var note=document.getElementById(‘ctl-note’);
 if(d.can_control){
   set(‘ctl-status’, d.ctl_status||’’);
   if(note){
     if(!d.daemon_running){
       note.style.display=’’;
       note.textContent=’Tip: run peterfan install-daemon once for persistent control at boot.’;
     } else {
       note.style.display=’none’;
     }
   }
 } else {
   set(‘ctl-status’,’unavailable’);
   if(note){note.style.display=’’;note.textContent=’Fan control unavailable on this Mac — showing live RPM only.’;}
 }
 reportHeight();
}};
function reportHeight(){
  if(!window.ipc)return;
  // Measure after layout settles so populated lists are included.
  requestAnimationFrame(function(){
    var h=Math.max(document.body.scrollHeight,document.documentElement.scrollHeight);
    window.ipc.postMessage('h:'+Math.ceil(h));
  });
}
// Height is reported from update() once real data has populated the lists,
// so the window snaps to the exact content height instead of an empty one.
</script></body></html>"##;
