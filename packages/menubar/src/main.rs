//! `peterfan-menubar` — live system metrics in the macOS menu bar.
//!
//! The menu-bar title shows a tiny CPU sparkline + percentage. Clicking the
//! icon (left **or** right / two-finger) toggles a clean popover dashboard — a
//! borderless WebView rendering an HTML/CSS panel with CPU (per-core), memory,
//! storage, temperatures, fans, battery, and network. Quit from the button in
//! the popover. Runs as an accessory app (no Dock icon). `--mock` uses the
//! simulated machine.

// The popover's `update()` payload is one large `serde_json::json!` object —
// each field the dashboard reads adds another layer to the macro's expansion,
// and that payload has grown past the default limit (128) over the course of
// many feature additions. Bumping this is the standard fix (recommended by
// rustc's own error message), not a workaround for a real problem.
#![recursion_limit = "256"]

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use tao::dpi::{LogicalSize, PhysicalPosition};
use tao::event::{Event, StartCause, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder, EventLoopWindowTarget};
use tao::window::{Window, WindowBuilder};

#[cfg(target_os = "macos")]
use tao::platform::macos::{ActivationPolicy, EventLoopExtMacOS};

use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use tray_icon::{
    Icon, MouseButton, MouseButtonState, Rect, TrayIcon, TrayIconAttributes, TrayIconEvent,
};
use wry::{WebView, WebViewBuilder};

use peterfan_core::config::{
    CustomCurveConfig, Language, MenubarDisplay, MenubarMetric, ResolvedLanguage,
};
use peterfan_core::error::CoreError;
use peterfan_core::license::{self, Entitlement};
use peterfan_core::metrics::ProcSort;
use peterfan_core::profile::Profile;
use peterfan_core::types::Celsius;
use peterfan_core::{HardwareProvider, SystemMonitor};

/// Placeholder purchase link — point this at the real store page once one
/// exists (Gumroad/Paddle/Stripe checkout).
const BUY_URL: &str = "https://peterfan.dev/buy";

const REFRESH: Duration = Duration::from_secs(1);
/// Samples kept for the menu-bar graph icon (always shows the short-term
/// trend, independent of the popover's chart range selector) — 120 samples
/// at a 1s tick is a 2-minute rolling window.
const HIST_CAP: usize = 120;

/// Popover chart range, chosen from the "2m / 1h / 1d" tabs. Persisted only
/// for the running session (resets to 2m on relaunch) via a plain atomic —
/// it's a display preference, not worth a config round-trip.
static CHART_RANGE: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

/// Top Processes sort column (0 = CPU, 1 = Memory) — same "session-only
/// display preference" reasoning as `CHART_RANGE`.
static PROC_SORT: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

#[derive(Clone, Copy, PartialEq)]
enum ChartRange {
    TwoMin,
    OneHour,
    OneDay,
}
impl ChartRange {
    fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::OneHour,
            2 => Self::OneDay,
            _ => Self::TwoMin,
        }
    }
    fn as_str(self) -> &'static str {
        match self {
            Self::TwoMin => "2m",
            Self::OneHour => "1h",
            Self::OneDay => "1d",
        }
    }
}

/// Rolling history at three granularities, so the same metric can be charted
/// over the last 2 minutes (raw samples), hour (per-minute averages), or day
/// (per-hour averages) without keeping 86400 raw samples around.
struct RangedHistory {
    minute: VecDeque<f32>,
    hour: VecDeque<f32>,
    day: VecDeque<f32>,
    /// Raw samples accumulated toward the next per-minute average.
    minute_acc: Vec<f32>,
    /// Per-minute averages accumulated toward the next per-hour average.
    hour_acc: Vec<f32>,
}

const RANGE_2M_CAP: usize = HIST_CAP; // 2 min @ 1s
const RANGE_1H_CAP: usize = 60; // 1 hour @ 1 min
const RANGE_1D_CAP: usize = 24; // 1 day @ 1 hour

impl RangedHistory {
    fn new() -> Self {
        Self {
            minute: VecDeque::with_capacity(RANGE_2M_CAP),
            hour: VecDeque::with_capacity(RANGE_1H_CAP),
            day: VecDeque::with_capacity(RANGE_1D_CAP),
            minute_acc: Vec::with_capacity(60),
            hour_acc: Vec::with_capacity(60),
        }
    }

    fn push(&mut self, v: f32) {
        push_capped(&mut self.minute, v, RANGE_2M_CAP);
        self.minute_acc.push(v);
        if self.minute_acc.len() >= 60 {
            let avg = self.minute_acc.iter().sum::<f32>() / self.minute_acc.len() as f32;
            self.minute_acc.clear();
            push_capped(&mut self.hour, avg, RANGE_1H_CAP);
            self.hour_acc.push(avg);
            if self.hour_acc.len() >= 60 {
                let havg = self.hour_acc.iter().sum::<f32>() / self.hour_acc.len() as f32;
                self.hour_acc.clear();
                push_capped(&mut self.day, havg, RANGE_1D_CAP);
            }
        }
    }

    fn range(&self, r: ChartRange) -> &VecDeque<f32> {
        match r {
            ChartRange::TwoMin => &self.minute,
            ChartRange::OneHour => &self.hour,
            ChartRange::OneDay => &self.day,
        }
    }
}

const POPOVER_W: f64 = 348.0;
/// Initial height; the popover then reports its real content height (below) and
/// the window is resized to fit exactly.
const POPOVER_H: f64 = 520.0;

/// Set by the popover's Quit button (via WebView IPC), polled by the loop.
static QUIT: AtomicBool = AtomicBool::new(false);
/// Set by the popover's "Open Detailed Window" link, polled by the loop
/// (opening a window needs `&mut App` + the event-loop target, neither of
/// which the IPC handler closure has access to).
static OPEN_DETAIL: AtomicBool = AtomicBool::new(false);
/// Content height (CSS px) reported by the popover; 0 = not yet measured.
static DESIRED_H: AtomicU32 = AtomicU32::new(0);
/// Height already applied to the window, to avoid resizing every tick.
static APPLIED_H: AtomicU32 = AtomicU32::new(0);
/// Control commands queued by popover buttons (`auto`, `profile:gaming`).
static PENDING: Mutex<Vec<String>> = Mutex::new(Vec::new());
/// Last control result, shown in the popover.
static STATUS: Mutex<String> = Mutex::new(String::new());

/// IDs of the tray context-menu items so we can identify them in MenuEvent.
struct TrayMenu {
    auto: tray_icon::menu::MenuId,
    rules: tray_icon::menu::MenuId,
    profiles: Vec<(String, tray_icon::menu::MenuId)>,
    quit: tray_icon::menu::MenuId,
    /// "Show" submenu — which metric the menu-bar item tracks. Each entry's
    /// checked state is kept in sync with `App.metric` whenever it changes.
    show_items: Vec<(MenubarMetric, tray_icon::menu::CheckMenuItem)>,
    /// "Display" submenu — number / graph / both.
    display_items: Vec<(MenubarDisplay, tray_icon::menu::CheckMenuItem)>,
    /// "Fan Speed" submenu — direct RPM presets, mapped to the same command
    /// strings `execute_control` already understands ("auto", "hold:<pct>").
    fan_speed_items: Vec<(String, tray_icon::menu::MenuId)>,
    /// One-time privileged daemon install — lets fan control work without a
    /// terminal (macOS only; `None` elsewhere).
    #[cfg(target_os = "macos")]
    enable_fan_control: tray_icon::menu::MenuId,
    /// "Launch at Login" checkbox — kept in sync with the actual LaunchAgent
    /// state after each toggle.
    #[cfg(target_os = "macos")]
    launch_at_login: tray_icon::menu::CheckMenuItem,
    check_updates: tray_icon::menu::MenuId,
    open_detail: tray_icon::menu::MenuId,
    /// "Language" submenu — changing this saves to config and asks the user
    /// to relaunch (the native menu's labels are only built once, at
    /// startup, so a live-relabel isn't worth the complexity it'd add).
    language_items: Vec<(Language, tray_icon::menu::CheckMenuItem)>,
}

/// Native-menu + popover copy for the current UI language. Resolved once at
/// tray-build time (native labels) and at each webview-creation time (the
/// popover reads it fresh so a language change takes effect on the very next
/// popover/detail-window open, without needing a full app relaunch).
struct L10n {
    enable_fan_control: &'static str,
    launch_at_login: &'static str,
    auto: &'static str,
    rules: &'static str,
    profile_silent: &'static str,
    profile_balanced: &'static str,
    profile_gaming: &'static str,
    profile_performance: &'static str,
    profile_maximum: &'static str,
    open_detail: &'static str,
    check_updates: &'static str,
    quit: &'static str,
    menu_bar_shows: &'static str,
    menu_bar_style: &'static str,
    fan_speed: &'static str,
    language: &'static str,
    show_cpu: &'static str,
    show_memory: &'static str,
    show_temperature: &'static str,
    show_fan: &'static str,
    show_network: &'static str,
    style_number: &'static str,
    style_graph: &'static str,
    style_both: &'static str,
}

fn strings(lang: ResolvedLanguage) -> L10n {
    match lang {
        ResolvedLanguage::En => L10n {
            enable_fan_control: "Enable Fan Control (One-Time Setup)…",
            launch_at_login: "Launch at Login",
            auto: "Auto (OS-managed)",
            rules: "Rules",
            profile_silent: "Silent",
            profile_balanced: "Balanced",
            profile_gaming: "Gaming",
            profile_performance: "Performance",
            profile_maximum: "Maximum",
            open_detail: "Open Detailed Window…",
            check_updates: "Check for Updates…",
            quit: "Quit PeterFan",
            menu_bar_shows: "Menu Bar Shows",
            menu_bar_style: "Menu Bar Style",
            fan_speed: "Fan Speed",
            language: "Language",
            show_cpu: "CPU",
            show_memory: "Memory",
            show_temperature: "Temperature",
            show_fan: "Fan",
            show_network: "Network",
            style_number: "Number",
            style_graph: "Graph",
            style_both: "Number + Graph",
        },
        ResolvedLanguage::Ko => L10n {
            enable_fan_control: "팬 제어 활성화 (최초 1회 설정)…",
            launch_at_login: "로그인 시 자동 실행",
            auto: "자동 (OS 관리)",
            rules: "규칙",
            profile_silent: "무음",
            profile_balanced: "균형",
            profile_gaming: "게이밍",
            profile_performance: "고성능",
            profile_maximum: "최대",
            open_detail: "상세 창 열기…",
            check_updates: "업데이트 확인…",
            quit: "PeterFan 종료",
            menu_bar_shows: "메뉴 막대 표시 항목",
            menu_bar_style: "메뉴 막대 스타일",
            fan_speed: "팬 속도",
            language: "언어",
            show_cpu: "CPU",
            show_memory: "메모리",
            show_temperature: "온도",
            show_fan: "팬",
            show_network: "네트워크",
            style_number: "숫자",
            style_graph: "그래프",
            style_both: "숫자 + 그래프",
        },
    }
}

struct App {
    monitor: Box<dyn SystemMonitor>,
    /// Shared (not owned) so control actions can run on a background thread
    /// without blocking the event loop — SMC calls take tens to hundreds of
    /// ms, especially when they're failing (no daemon, no root).
    provider: std::sync::Arc<dyn HardwareProvider>,
    has_battery: bool,
    metric: MenubarMetric,
    display: MenubarDisplay,
    language: Language,
    tray: Option<TrayIcon>,
    tray_menu: Option<TrayMenu>,
    window: Option<Window>,
    webview: Option<WebView>,
    popover_visible: bool,
    /// A persistent, resizable, normal-chrome window with the same
    /// dashboard content — for "leave it open while I work" use, unlike the
    /// dropdown popover which hides the moment focus moves elsewhere.
    /// Created lazily on first request.
    detail_window: Option<Window>,
    detail_webview: Option<WebView>,
    /// Short-term (2-minute) history for the menu-bar graph icon only — the
    /// icon always shows the recent trend, independent of the popover's
    /// chart range selector.
    fan_hist: VecDeque<f32>,
    /// Multi-range history (2m/1h/1d) for the popover's own charts.
    cpu_h: RangedHistory,
    mem_h: RangedHistory,
    temp_h: RangedHistory,
    /// Combined rx+tx throughput (bytes/sec) — the chart only ever shows the
    /// total, so there's no need to keep rx/tx as separate series.
    net_h: RangedHistory,
    /// Combined disk read+write throughput (bytes/sec), same reasoning.
    disk_io_h: RangedHistory,
    /// Trial/license state, resolved at startup and after `license:<key>` IPC.
    entitlement: Entitlement,
}

/// Persist the menu-bar's metric + display choice so it survives a relaunch.
fn save_menubar_config(metric: MenubarMetric, display: MenubarDisplay) {
    let mut cfg = peterfan_platform::config::load();
    cfg.menubar.metric = metric;
    cfg.menubar.display = display;
    let _ = peterfan_platform::config::save(&cfg);
}

/// Persist the UI language choice so it survives a relaunch.
fn save_language(language: Language) {
    let mut cfg = peterfan_platform::config::load();
    cfg.menubar.language = language;
    let _ = peterfan_platform::config::save(&cfg);
}

/// Save a hand-drawn fan curve from the Detail Window's curve editor and
/// switch to it. `points_json` is a JSON array of `[temp_c, duty_percent]`
/// pairs, e.g. `[[30,20],[60,50],[90,100]]`.
/// Parse and validate the curve editor's JSON payload — pure, no I/O, so it's
/// safe to unit-test without touching the real (on-disk) config that
/// `save_custom_curve` reads and writes.
fn parse_curve_points(points_json: &str) -> Result<CustomCurveConfig, String> {
    let raw: Vec<[f32; 2]> = serde_json::from_str(points_json).map_err(|_| "invalid curve data")?;
    if raw.len() < 2 {
        return Err("a curve needs at least 2 points".into());
    }
    let curve = CustomCurveConfig {
        points: raw.into_iter().map(|[t, d]| [t, d.min(100.0)]).collect(),
    };
    if curve.to_fan_curve().is_none() {
        return Err("invalid curve".into());
    }
    Ok(curve)
}

fn save_custom_curve(provider: &dyn HardwareProvider, points_json: &str) -> String {
    let curve = match parse_curve_points(points_json) {
        Ok(c) => c,
        Err(e) => return e,
    };
    let fan_curve = curve
        .to_fan_curve()
        .expect("validated by parse_curve_points");
    let mut cfg = peterfan_platform::config::load();
    cfg.custom_curve = Some(curve);
    if peterfan_platform::config::save(&cfg).is_err() {
        return "failed to save curve".into();
    }
    // Prefer the daemon (it re-applies continuously as temps change); fall
    // back to one direct write so the change is felt immediately even
    // without a daemon, same "best effort, no persistent loop" contract as
    // every other local-fallback path in this file.
    #[cfg(unix)]
    if peterfan_platform::ipc::send_command("reload").is_some() {
        let _ = peterfan_platform::ipc::send_command("profile custom");
        return "custom curve saved".into();
    }
    if provider.capabilities().control_fans {
        let hot = provider
            .temperatures()
            .unwrap_or_default()
            .iter()
            .map(|t| t.value.0)
            .fold(0.0_f32, f32::max);
        let duty = fan_curve.duty_at(hot);
        for fan in provider.fans().unwrap_or_default() {
            if fan.controllable {
                let _ = provider.set_fan_duty(&fan.id, duty);
            }
        }
    }
    "custom curve saved".into()
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Resolve trial/license state, stamping `first_run_unix` into config on the
/// very first launch (shared with the daemon, so either one starts the clock).
fn resolve_entitlement() -> Entitlement {
    let mut cfg = peterfan_platform::config::load();
    let now = now_unix();
    if cfg.license.first_run_unix.is_none() {
        cfg.license.first_run_unix = Some(now);
        let _ = peterfan_platform::config::save(&cfg);
    }
    license::check_entitlement(cfg.license.key.as_deref(), cfg.license.first_run_unix, now)
}

/// Verify and persist a license key submitted from the popover. Returns the
/// new entitlement plus a short status line to display.
fn activate_license(key: &str) -> (Entitlement, String) {
    let now = now_unix();
    match license::verify_key(key, now) {
        license::LicenseStatus::Valid { email, .. } => {
            let mut cfg = peterfan_platform::config::load();
            cfg.license.key = Some(key.to_string());
            let _ = peterfan_platform::config::save(&cfg);
            (
                Entitlement::Licensed {
                    email: email.clone(),
                },
                format!("licensed to {email}"),
            )
        }
        license::LicenseStatus::Expired { email, .. } => (
            resolve_entitlement(),
            format!("license for {email} has expired"),
        ),
        license::LicenseStatus::Invalid(reason) => (resolve_entitlement(), reason),
    }
}

/// Push a sample, dropping the oldest once past [`HIST_CAP`].
fn push_hist<T>(buf: &mut VecDeque<T>, v: T) {
    push_capped(buf, v, HIST_CAP);
}

fn push_capped<T>(buf: &mut VecDeque<T>, v: T, cap: usize) {
    buf.push_back(v);
    if buf.len() > cap {
        buf.pop_front();
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("peterfan-menubar {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!(
            "peterfan-menubar {}\n\n\
             Live system metrics in the macOS menu bar.\n\n\
             USAGE:\n    peterfan-menubar [OPTIONS]\n\n\
             OPTIONS:\n    \
             --mock                Use simulated hardware instead of real sensors\n    \
             --metric <cpu|memory|temp|fan|network>  What the menu-bar item tracks\n    \
             --display <number|graph|both>           How it's rendered\n    \
             (Both flags override the saved preference; changing them from the\n    \
             right-click menu persists for next launch.)\n    \
             --version, -V         Print version and exit\n    \
             --help, -h            Print this help and exit",
            env!("CARGO_PKG_VERSION")
        );
        return;
    }
    let use_mock = args.iter().any(|a| a == "--mock");

    let saved = peterfan_platform::config::load().menubar;
    let metric = args
        .iter()
        .position(|a| a == "--metric")
        .and_then(|i| args.get(i + 1))
        .and_then(|v| MenubarMetric::parse(v))
        .unwrap_or(saved.metric);
    let display = args
        .iter()
        .position(|a| a == "--display")
        .and_then(|i| args.get(i + 1))
        .and_then(|v| MenubarDisplay::parse(v))
        .unwrap_or(saved.display);
    let language = saved.language;

    let (monitor, provider): (Box<dyn SystemMonitor>, std::sync::Arc<dyn HardwareProvider>) =
        if use_mock {
            (
                peterfan_platform::mock_monitor(),
                peterfan_platform::mock().into(),
            )
        } else {
            (
                peterfan_platform::system_monitor(),
                peterfan_platform::detect().into(),
            )
        };
    let has_battery = monitor.capabilities().battery;
    let entitlement = resolve_entitlement();

    #[allow(unused_mut)]
    let mut event_loop = EventLoopBuilder::<()>::new().build();
    #[cfg(target_os = "macos")]
    event_loop.set_activation_policy(ActivationPolicy::Accessory);

    let mut app = App {
        monitor,
        provider,
        has_battery,
        metric,
        display,
        language,
        tray: None,
        tray_menu: None,
        window: None,
        webview: None,
        popover_visible: false,
        detail_window: None,
        detail_webview: None,
        fan_hist: VecDeque::with_capacity(HIST_CAP),
        cpu_h: RangedHistory::new(),
        mem_h: RangedHistory::new(),
        temp_h: RangedHistory::new(),
        net_h: RangedHistory::new(),
        disk_io_h: RangedHistory::new(),
        entitlement,
    };

    event_loop.run(move |event, target, control_flow| {
        *control_flow = ControlFlow::WaitUntil(Instant::now() + REFRESH);

        if QUIT.load(Ordering::Relaxed) {
            *control_flow = ControlFlow::Exit;
            return;
        }

        if OPEN_DETAIL.swap(false, Ordering::Relaxed) {
            open_detail_window(&mut app, target);
        }

        match event {
            Event::NewEvents(StartCause::Init) => {
                build_tray(&mut app);
                build_popover(&mut app, target);
                update(&mut app);
                // Offer one-time setup right away instead of leaving it
                // buried in the right-click menu — other fan-control apps
                // ask for this during their installer; we don't have one,
                // so the first launch has to ask instead. Never in --mock:
                // there's no real hardware to control, so the whole flow
                // (including the real privileged install) would be bogus.
                if !use_mock {
                    std::thread::spawn(maybe_prompt_first_run_setup);
                    // Staggered a few seconds after the setup prompt so the
                    // two dialogs (both one-shot, both possible on a fresh
                    // launch) don't pop on top of each other.
                    std::thread::spawn(check_for_updates_on_launch);
                }
            }
            Event::NewEvents(StartCause::ResumeTimeReached { .. }) => {
                update(&mut app);
            }
            Event::WindowEvent {
                event: WindowEvent::Focused(false),
                ..
            } => hide_popover(&mut app),
            // The detail window is a normal decorated window, so its red
            // close button generates this instead of destroying anything —
            // tao/winit never closes a window on its own. Hide it (not
            // drop it) so `open_detail_window`'s re-show path can reuse the
            // existing webview instead of rebuilding it every time.
            Event::WindowEvent {
                window_id,
                event: WindowEvent::CloseRequested,
                ..
            } => {
                if let Some(w) = &app.detail_window {
                    if w.id() == window_id {
                        w.set_visible(false);
                    }
                }
            }
            _ => {}
        }

        // Run any control commands (or a license key) queued by the popover.
        let cmds: Vec<String> = std::mem::take(&mut *PENDING.lock().expect("pending poisoned"));
        if !cmds.is_empty() {
            for c in &cmds {
                if let Some(key) = c.strip_prefix("license:") {
                    let (entitlement, msg) = activate_license(key);
                    app.entitlement = entitlement;
                    *STATUS.lock().expect("status poisoned") = msg;
                } else if let Some(json) = c.strip_prefix("savecurve:") {
                    *STATUS.lock().expect("status poisoned") = "saving curve…".into();
                    let provider = std::sync::Arc::clone(&app.provider);
                    let json = json.to_string();
                    std::thread::spawn(move || {
                        let status = save_custom_curve(provider.as_ref(), &json);
                        *STATUS.lock().expect("status poisoned") = status;
                    });
                } else if c == "enablefancontrol" {
                    // Same admin-prompt install the right-click menu item
                    // triggers — exposed here too so the "update the daemon"
                    // fix is one click from the exact error message that
                    // told the user they needed it, not a hunt through menus.
                    std::thread::spawn(install_fan_control);
                } else {
                    // Hardware I/O (SMC calls) can take hundreds of ms,
                    // especially while failing (no daemon, no root) — run it
                    // off the event-loop thread so the menu bar stays
                    // responsive. The next periodic tick (within 1s) picks
                    // up the result via STATUS.
                    *STATUS.lock().expect("status poisoned") = "applying…".into();
                    let provider = std::sync::Arc::clone(&app.provider);
                    let cmd = c.clone();
                    std::thread::spawn(move || {
                        let status = execute_control(provider.as_ref(), &cmd);
                        *STATUS.lock().expect("status poisoned") = status;
                    });
                }
            }
            update(&mut app); // reflect "applying…" (or the license result) immediately
        }

        // Resize the popover window to the height the WebView reported, so it
        // fits the content exactly (no empty space) — capped so it never
        // runs past the bottom of the screen (the content itself scrolls
        // past that point instead).
        let desired = DESIRED_H.load(Ordering::Relaxed);
        if desired > 0 && desired != APPLIED_H.load(Ordering::Relaxed) {
            if let Some(w) = &app.window {
                let capped = (desired as f64).min(max_popover_height(w));
                w.set_inner_size(LogicalSize::new(POPOVER_W, capped));
                APPLIED_H.store(desired, Ordering::Relaxed);
            }
        }

        // Handle context-menu item selections.
        while let Ok(ev) = MenuEvent::receiver().try_recv() {
            let id = &ev.id;
            let mut matched_metric: Option<MenubarMetric> = None;
            let mut matched_display: Option<MenubarDisplay> = None;
            let mut matched_language: Option<Language> = None;
            let mut open_detail_requested = false;
            let cmd: Option<String> = if let Some(ref tm) = app.tray_menu {
                if id == &tm.auto {
                    Some("auto".into())
                } else if id == &tm.rules {
                    Some("rules".into())
                } else if id == &tm.quit {
                    QUIT.store(true, Ordering::Relaxed);
                    None
                } else if let Some((m, _)) = tm.show_items.iter().find(|(_, item)| item.id() == id)
                {
                    matched_metric = Some(*m);
                    None
                } else if let Some((d, _)) =
                    tm.display_items.iter().find(|(_, item)| item.id() == id)
                {
                    matched_display = Some(*d);
                    None
                } else if let Some((l, _)) =
                    tm.language_items.iter().find(|(_, item)| item.id() == id)
                {
                    matched_language = Some(*l);
                    None
                } else if let Some((cmd, _)) = tm.fan_speed_items.iter().find(|(_, iid)| iid == id)
                {
                    Some(cmd.clone())
                } else if is_enable_fan_control_id(tm, id) {
                    std::thread::spawn(install_fan_control);
                    None
                } else if is_launch_at_login_id(tm, id) {
                    toggle_launch_at_login(tm, app.metric.as_str());
                    None
                } else if tm.check_updates == *id {
                    std::thread::spawn(check_for_updates_interactive);
                    None
                } else if tm.open_detail == *id {
                    open_detail_requested = true;
                    None
                } else {
                    tm.profiles
                        .iter()
                        .find(|(_, pid)| pid == id)
                        .map(|(name, _)| format!("profile:{name}"))
                }
            } else {
                None
            };

            if open_detail_requested {
                open_detail_window(&mut app, target);
            }
            if let Some(m) = matched_metric {
                app.metric = m;
                if let Some(ref tm) = app.tray_menu {
                    for (mm, item) in &tm.show_items {
                        item.set_checked(*mm == m);
                    }
                }
                save_menubar_config(app.metric, app.display);
                update(&mut app);
            }
            if let Some(d) = matched_display {
                app.display = d;
                if let Some(ref tm) = app.tray_menu {
                    for (dd, item) in &tm.display_items {
                        item.set_checked(*dd == d);
                    }
                }
                save_menubar_config(app.metric, app.display);
                update(&mut app);
            }
            if let Some(l) = matched_language {
                app.language = l;
                save_language(l);
                // Rebuild everything that bakes labels in at construction
                // time — the native menu (labels are set once, at build
                // time) and both webviews (the dashboard HTML is generated
                // per-language, not re-translated live) — so the change is
                // visible immediately instead of needing a relaunch.
                build_tray(&mut app);
                let was_visible = app.popover_visible;
                app.window = None;
                app.webview = None;
                build_popover(&mut app, target);
                if was_visible {
                    if let Some(w) = &app.window {
                        w.set_visible(true);
                    }
                    app.popover_visible = true;
                }
                if app.detail_window.is_some() {
                    let was_detail_visible =
                        app.detail_window.as_ref().is_some_and(Window::is_visible);
                    app.detail_window = None;
                    app.detail_webview = None;
                    if was_detail_visible {
                        open_detail_window(&mut app, target);
                    }
                }
                update(&mut app);
            }
            if let Some(c) = cmd {
                // Off the event-loop thread — SMC calls can take hundreds of
                // ms (worse when failing), and this is called directly from
                // menu-click handling, so blocking here freezes the menu bar.
                let provider = std::sync::Arc::clone(&app.provider);
                let cmd = c.clone();
                std::thread::spawn(move || {
                    let status = execute_control(provider.as_ref(), &cmd);
                    // The right-click menu has no visible status line (unlike
                    // the popover), so surface the result as a notification —
                    // otherwise a failed command (no daemon, needs root)
                    // looks like it silently did nothing.
                    notify_control_result(&cmd, control_result_is_ok(&status), &status);
                    *STATUS.lock().expect("status poisoned") = status;
                });
            }
        }

        while let Ok(ev) = TrayIconEvent::receiver().try_recv() {
            // Left click toggles the popover; right click shows the native menu.
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
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
    let s = strings(app.language.resolve());
    // For labeling the Fan Speed % presets with their RPM equivalent — a
    // quick read, not a control action, so it's fine to call synchronously
    // here (same call `update()` already makes every tick).
    let max_fan_rpm = app
        .provider
        .fans()
        .unwrap_or_default()
        .iter()
        .filter_map(|f| f.max_rpm)
        .max()
        .unwrap_or(0);

    // One-time setup: installs the root daemon so fan control works without
    // a terminal or repeated sudo prompts — one macOS admin-password dialog,
    // triggered right here instead of requiring the CLI.
    #[cfg(target_os = "macos")]
    let enable_fan_control_item = MenuItem::new(s.enable_fan_control, true, None);

    // "Launch at Login" — a per-user LaunchAgent, so this never needs an
    // admin password and can toggle instantly (unlike fan control).
    #[cfg(target_os = "macos")]
    let launch_at_login_item = CheckMenuItem::new(
        s.launch_at_login,
        true,
        peterfan_platform::login_item::is_installed(),
        None,
    );

    // Build context menu: Auto | Rules | — | profiles... | — | Quit
    let auto_item = MenuItem::new(s.auto, true, None);
    let rules_item = MenuItem::new(s.rules, true, None);
    let profile_items: Vec<(String, MenuItem)> = Profile::all()
        .iter()
        .map(|p| {
            let label = format!(
                "{}{}",
                match *p {
                    Profile::Silent => s.profile_silent,
                    Profile::Balanced => s.profile_balanced,
                    Profile::Gaming => s.profile_gaming,
                    Profile::Performance => s.profile_performance,
                    Profile::Maximum => s.profile_maximum,
                    _ => p.as_str(),
                },
                p.description().split('.').next().unwrap_or("")
            );
            (p.as_str().to_string(), MenuItem::new(&label, true, None))
        })
        .collect();
    let open_detail_item = MenuItem::new(s.open_detail, true, None);
    let check_updates_item = MenuItem::new(s.check_updates, true, None);
    let quit_item = MenuItem::new(s.quit, true, None);

    // "Show" — which metric the menu-bar item tracks.
    let show_submenu = Submenu::new(s.menu_bar_shows, true);
    let show_items: Vec<(MenubarMetric, CheckMenuItem)> = [
        (MenubarMetric::Cpu, s.show_cpu),
        (MenubarMetric::Memory, s.show_memory),
        (MenubarMetric::Temp, s.show_temperature),
        (MenubarMetric::Fan, s.show_fan),
        (MenubarMetric::Network, s.show_network),
    ]
    .into_iter()
    .map(|(m, label)| {
        let item = CheckMenuItem::new(label, true, app.metric == m, None);
        let _ = show_submenu.append(&item);
        (m, item)
    })
    .collect();

    // "Display" — number only / graph only / both.
    let display_submenu = Submenu::new(s.menu_bar_style, true);
    let display_items: Vec<(MenubarDisplay, CheckMenuItem)> = [
        (MenubarDisplay::Number, s.style_number),
        (MenubarDisplay::Graph, s.style_graph),
        (MenubarDisplay::Both, s.style_both),
    ]
    .into_iter()
    .map(|(d, label)| {
        let item = CheckMenuItem::new(label, true, app.display == d, None);
        let _ = display_submenu.append(&item);
        (d, item)
    })
    .collect();

    // "Language" — each name is shown in its own language regardless of the
    // current selection (standard practice — you must be able to find your
    // way back even if you picked the wrong one by mistake).
    let language_submenu = Submenu::new(s.language, true);
    let language_items: Vec<(Language, CheckMenuItem)> = [
        (Language::System, "System Default"),
        (Language::English, "English"),
        (Language::Korean, "한국어"),
    ]
    .into_iter()
    .map(|(l, label)| {
        let item = CheckMenuItem::new(label, true, app.language == l, None);
        let _ = language_submenu.append(&item);
        (l, item)
    })
    .collect();

    // "Fan Speed" — direct RPM presets, for when a profile curve is more than
    // you want and you just want "half speed, now."
    let fan_speed_submenu = Submenu::new(s.fan_speed, true);
    let fan_speed_auto = MenuItem::new(s.auto, true, None);
    let _ = fan_speed_submenu.append(&fan_speed_auto);
    let _ = fan_speed_submenu.append(&PredefinedMenuItem::separator());
    let fan_speed_presets: Vec<(String, MenuItem)> = [25u8, 50, 75, 100]
        .into_iter()
        .map(|pct| {
            let label = if max_fan_rpm > 0 {
                let rpm = (max_fan_rpm as f32 * pct as f32 / 100.0).round() as u32;
                format!("{pct}%  (~{rpm} RPM)")
            } else {
                format!("{pct}%")
            };
            (format!("hold:{pct}"), MenuItem::new(label, true, None))
        })
        .collect();
    for (_, item) in &fan_speed_presets {
        let _ = fan_speed_submenu.append(item);
    }
    let fan_speed_items: Vec<(String, tray_icon::menu::MenuId)> =
        std::iter::once(("auto".to_string(), fan_speed_auto.id().clone()))
            .chain(
                fan_speed_presets
                    .iter()
                    .map(|(cmd, item)| (cmd.clone(), item.id().clone())),
            )
            .collect();

    let menu = Menu::new();
    #[cfg(target_os = "macos")]
    {
        let _ = menu.append(&enable_fan_control_item);
        let _ = menu.append(&PredefinedMenuItem::separator());
    }
    let _ = menu.append(&auto_item);
    let _ = menu.append(&rules_item);
    let _ = menu.append(&fan_speed_submenu);
    let _ = menu.append(&PredefinedMenuItem::separator());
    for (_, item) in &profile_items {
        let _ = menu.append(item);
    }
    let _ = menu.append(&PredefinedMenuItem::separator());
    let _ = menu.append(&show_submenu);
    let _ = menu.append(&display_submenu);
    let _ = menu.append(&language_submenu);
    #[cfg(target_os = "macos")]
    let _ = menu.append(&launch_at_login_item);
    let _ = menu.append(&PredefinedMenuItem::separator());
    let _ = menu.append(&open_detail_item);
    let _ = menu.append(&check_updates_item);
    let _ = menu.append(&PredefinedMenuItem::separator());
    let _ = menu.append(&quit_item);

    let tray_menu = TrayMenu {
        auto: auto_item.id().clone(),
        rules: rules_item.id().clone(),
        profiles: profile_items
            .iter()
            .map(|(name, item)| (name.clone(), item.id().clone()))
            .collect(),
        quit: quit_item.id().clone(),
        show_items,
        display_items,
        fan_speed_items,
        #[cfg(target_os = "macos")]
        enable_fan_control: enable_fan_control_item.id().clone(),
        #[cfg(target_os = "macos")]
        launch_at_login: launch_at_login_item,
        check_updates: check_updates_item.id().clone(),
        open_detail: open_detail_item.id().clone(),
        language_items,
    };

    match TrayIcon::new(tray_attributes(make_ring_icon(), Box::new(menu))) {
        Ok(tray) => {
            app.tray = Some(tray);
            app.tray_menu = Some(tray_menu);
        }
        Err(e) => eprintln!("failed to create menu-bar item: {e}"),
    }
}

/// (menu_on_left_click, menu_on_right_click). Factored out of
/// `tray_attributes` so it's unit-testable without constructing a real
/// `Menu` — `muda::Menu::new()` panics off the main thread on macOS, which
/// is exactly where `cargo test` runs test bodies. tray-icon shows the
/// attached menu on left-click *by default*, which would pre-empt our own
/// `TrayIconEvent::Click` handling and make the popover dashboard
/// unreachable (this shipped broken once already — v1.9.3 fixed it).
fn click_routing() -> (bool, bool) {
    (false, true)
}

fn tray_attributes(icon: Icon, menu: Box<dyn tray_icon::menu::ContextMenu>) -> TrayIconAttributes {
    let (menu_on_left_click, menu_on_right_click) = click_routing();
    TrayIconAttributes {
        icon: Some(icon),
        menu: Some(menu),
        icon_is_template: cfg!(target_os = "macos"),
        menu_on_left_click,
        menu_on_right_click,
        ..Default::default()
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
        .with_html(dashboard_html(app.language.resolve(), false))
        .with_transparent(true)
        .with_ipc_handler(|req| {
            let body = req.body();
            if body == "quit" {
                QUIT.store(true, Ordering::Relaxed);
            } else if body == "open_detail" {
                OPEN_DETAIL.store(true, Ordering::Relaxed);
            } else if let Some(h) = body.strip_prefix("h:") {
                if let Ok(v) = h.trim().parse::<u32>() {
                    DESIRED_H.store(v, Ordering::Relaxed);
                }
            } else if let Some(cmd) = body.strip_prefix("cmd:") {
                PENDING
                    .lock()
                    .expect("pending poisoned")
                    .push(cmd.to_string());
            } else if body.starts_with("license:") || body.starts_with("savecurve:") {
                // Keep the prefix so the drain loop can tell these apart
                // from a daemon control command.
                PENDING
                    .lock()
                    .expect("pending poisoned")
                    .push(body.to_string());
            } else if let Some(r) = body.strip_prefix("range:") {
                let v = match r {
                    "1h" => 1,
                    "1d" => 2,
                    _ => 0,
                };
                CHART_RANGE.store(v, Ordering::Relaxed);
            } else if let Some(s) = body.strip_prefix("procsort:") {
                PROC_SORT.store(if s == "mem" { 1 } else { 0 }, Ordering::Relaxed);
            } else if let Some(pid) = body
                .strip_prefix("killproc:")
                .and_then(|s| s.parse::<u32>().ok())
            {
                kill_process(pid);
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

/// Opens (or, if already created, shows and focuses) the persistent detail
/// window — same dashboard content as the popover, in an ordinary decorated,
/// resizable, user-positioned window that stays open regardless of focus.
fn open_detail_window(app: &mut App, target: &EventLoopWindowTarget<()>) {
    if let Some(w) = &app.detail_window {
        w.set_visible(true);
        w.set_focus();
        update(app);
        return;
    }

    let window = match WindowBuilder::new()
        .with_title("PeterFan")
        .with_decorations(true)
        .with_resizable(true)
        .with_inner_size(LogicalSize::new(POPOVER_W + 32.0, 680.0))
        .with_min_inner_size(LogicalSize::new(POPOVER_W, 360.0))
        .build(target)
    {
        Ok(w) => w,
        Err(e) => {
            eprintln!("failed to create detail window: {e}");
            return;
        }
    };

    match WebViewBuilder::new()
        .with_html(dashboard_html(app.language.resolve(), true))
        .with_ipc_handler(|req| {
            let body = req.body();
            // Same command surface as the popover, minus "h:" — a resizable
            // window sizes itself; it shouldn't fight the user by snapping
            // to the content's natural height on every tick.
            if body == "quit" {
                QUIT.store(true, Ordering::Relaxed);
            } else if let Some(cmd) = body.strip_prefix("cmd:") {
                PENDING
                    .lock()
                    .expect("pending poisoned")
                    .push(cmd.to_string());
            } else if body.starts_with("license:") {
                PENDING
                    .lock()
                    .expect("pending poisoned")
                    .push(body.to_string());
            } else if let Some(r) = body.strip_prefix("range:") {
                let v = match r {
                    "1h" => 1,
                    "1d" => 2,
                    _ => 0,
                };
                CHART_RANGE.store(v, Ordering::Relaxed);
            } else if let Some(s) = body.strip_prefix("procsort:") {
                PROC_SORT.store(if s == "mem" { 1 } else { 0 }, Ordering::Relaxed);
            } else if let Some(pid) = body
                .strip_prefix("killproc:")
                .and_then(|s| s.parse::<u32>().ok())
            {
                kill_process(pid);
            }
        })
        .build(&window)
    {
        Ok(webview) => {
            app.detail_window = Some(window);
            app.detail_webview = Some(webview);
            update(app);
        }
        Err(e) => eprintln!("failed to create detail webview: {e}"),
    }
}

/// Largest height the popover can be without its bottom edge running past
/// the current monitor — with the CPU/memory/storage/temperature/fans/
/// battery/network/processes/fan-control sections all present, content can
/// genuinely exceed a short display's height. Content beyond this scrolls
/// (see `.panel{overflow-y:auto}`) instead of being cut off or pushed
/// off-screen.
fn max_popover_height(w: &Window) -> f64 {
    let scale = w.scale_factor();
    let Some(monitor) = w.current_monitor() else {
        return 900.0; // generous fallback if the display can't be queried
    };
    let monitor_h = monitor.size().height as f64 / scale;
    let top_y = w
        .outer_position()
        .map(|p| p.y as f64 / scale)
        .unwrap_or(0.0);
    (monitor_h - top_y - 12.0).max(200.0)
}

fn toggle_popover(app: &mut App, rect: Rect) {
    if app.popover_visible {
        hide_popover(app);
        return;
    }
    let Some(w) = &app.window else { return };
    let scale = w.scale_factor();
    let win_w = POPOVER_W * scale;
    let x = (rect.position.x + rect.size.width as f64 - win_w).max(8.0);
    // Flush against the menu bar rather than leaving a visible gap — matches
    // how native menu extras (Control Center, Wi-Fi, …) sit right under the
    // icon instead of floating below it.
    let y = rect.position.y + rect.size.height as f64;
    w.set_outer_position(PhysicalPosition::new(x, y));
    // Snap to the last known content height *before* showing, so repeat
    // opens don't visibly resize (only the very first open of a session —
    // before any height has ever been measured — can still do that).
    let applied = APPLIED_H.load(Ordering::Relaxed);
    if applied > 0 {
        let capped = (applied as f64).min(max_popover_height(w));
        w.set_inner_size(LogicalSize::new(POPOVER_W, capped));
    }
    // No open animation — it should appear in one frame, not fade/scale in.
    w.set_visible(true);
    w.set_focus();
    app.popover_visible = true;
    update(app);
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
    // Gathered unconditionally (cheap — the underlying sysinfo/provider state
    // was already refreshed/held open) so the rolling history stays populated
    // even while the popover is closed and the graph icon keeps moving.
    let mem = app.monitor.memory();
    let nets = app.monitor.networks();
    let temps = app.provider.temperatures().unwrap_or_default();
    let fans = app.provider.fans().unwrap_or_default();
    let hottest = temps.iter().map(|t| t.value.0).fold(0.0_f32, f32::max);
    let fastest_rpm = fans.iter().map(|f| f.rpm).fold(0u32, u32::max);
    let fastest_pct = fans
        .iter()
        .filter_map(|f| {
            f.max_rpm
                .filter(|&m| m > 0)
                .map(|m| f.rpm as f32 / m as f32 * 100.0)
        })
        .fold(0.0_f32, f32::max);
    let rx: f64 = nets.iter().map(|n| n.rx_rate).sum();
    let tx: f64 = nets.iter().map(|n| n.tx_rate).sum();
    // Which interface to label the local IP with: whichever one is actually
    // carrying traffic, falling back to the first with an address at all
    // (e.g. an idle Wi-Fi link) — same "what am I actually connected
    // through" question iStat Menus' network module answers.
    let net_ip_line = nets
        .iter()
        .filter(|n| n.ip.is_some())
        .max_by(|a, b| (a.rx_rate + a.tx_rate).total_cmp(&(b.rx_rate + b.tx_rate)))
        .or_else(|| nets.iter().find(|n| n.ip.is_some()))
        .map(|n| format!("{} · {}", n.name, n.ip.as_deref().unwrap_or("")))
        .unwrap_or_default();

    push_hist(&mut app.fan_hist, fastest_pct);
    app.cpu_h.push(cpu.usage_percent);
    app.mem_h.push(mem.used_percent);
    app.temp_h.push(hottest);
    app.net_h.push((rx + tx) as f32);

    // Menu-bar item: number, graph, or both, tracking whichever metric the
    // user picked from the right-click menu (persisted across relaunches).
    if let Some(tray) = &app.tray {
        // Fixed-width formatting throughout: a menu-bar item that changes
        // width every tick (e.g. "9.5%" → "100.0%") shoves every icon to its
        // left back and forth, which reads as the whole menu bar jittering.
        // Padding to a constant character count keeps the item's width
        // (and everything to its left) stable regardless of the value.
        let title = match app.metric {
            MenubarMetric::Cpu => format!("{:>5.1}%", cpu.usage_percent),
            MenubarMetric::Memory => format!("{:>5.1}%", mem.used_percent),
            MenubarMetric::Temp => {
                if hottest > 0.0 {
                    format!("{hottest:>3.0}°C")
                } else {
                    format!("{:>5.1}%", cpu.usage_percent)
                }
            }
            MenubarMetric::Fan => {
                if fastest_rpm > 0 {
                    format!("{fastest_rpm:>5} RPM")
                } else {
                    format!("{:>5.1}%", cpu.usage_percent)
                }
            }
            MenubarMetric::Network => {
                format!("↓{:>8}/s", bytes(rx as u64))
            }
        };

        match app.display {
            MenubarDisplay::Number => {
                let _ = tray.set_icon(None);
                set_menubar_text(tray, &title);
            }
            MenubarDisplay::Graph => {
                let icon = menubar_graph_icon(app);
                let _ = tray.set_icon_with_as_template(Some(icon), false);
                set_menubar_text(tray, "");
            }
            MenubarDisplay::Both => {
                let icon = menubar_graph_icon(app);
                let _ = tray.set_icon_with_as_template(Some(icon), false);
                set_menubar_text(tray, &title);
            }
        }

        // A quick-glance native OS tooltip on hover — the same "see
        // everything without clicking" convenience iStat Menus' menu-bar
        // items offer, independent of whichever single metric the title/icon
        // happens to be tracking right now.
        let mem_label = if app.language.resolve() == ResolvedLanguage::Ko {
            "메모리"
        } else {
            "Mem"
        };
        let mut tip_parts = vec![format!(
            "CPU {:.1}%   {mem_label} {:.1}%",
            cpu.usage_percent, mem.used_percent
        )];
        if hottest > 0.0 {
            tip_parts.push(format!("{hottest:.0}°C"));
        }
        if fastest_rpm > 0 {
            tip_parts.push(format!("{fastest_rpm} RPM"));
        }
        let _ = tray.set_tooltip(Some(tip_parts.join("   ·   ")));
    }

    let detail_visible = app.detail_window.as_ref().is_some_and(Window::is_visible);
    if !app.popover_visible && !detail_visible {
        return;
    }

    let disks = app.monitor.disks();
    let battery = if app.has_battery {
        app.monitor.battery()
    } else {
        None
    };
    let power = app.provider.power_watts();
    let ghz = cpu.frequency_mhz as f64 / 1000.0;
    let load_str = cpu
        .load_avg
        .map(|l| format!("load {:.2} {:.2} {:.2}", l.one, l.five, l.fifteen))
        .unwrap_or_default();
    let disk = disks.first();
    let disk_io_present = disk.is_some_and(|d| d.read_bytes_per_sec + d.write_bytes_per_sec > 0.0);
    let disk_io_sub = disk
        .map(|d| {
            format!(
                "↓ {}/s   ↑ {}/s",
                bytes(d.read_bytes_per_sec as u64),
                bytes(d.write_bytes_per_sec as u64)
            )
        })
        .unwrap_or_default();
    let disk_io_rate = disk
        .map(|d| (d.read_bytes_per_sec + d.write_bytes_per_sec) as f32)
        .unwrap_or(0.0);
    app.disk_io_h.push(disk_io_rate);

    // Top 5 processes — "what's eating my Mac," sortable by CPU or memory
    // (toggle in the popover; defaults to CPU).
    let proc_sort = if PROC_SORT.load(Ordering::Relaxed) == 1 {
        ProcSort::Memory
    } else {
        ProcSort::Cpu
    };
    let proc_rows: Vec<_> = app
        .monitor
        .processes(5, proc_sort)
        .iter()
        .map(|p| {
            serde_json::json!({
                "name": p.name,
                "cpu": format!("{:.1}%", p.cpu_percent),
                "mem": bytes(p.memory),
                "pid": p.pid,
            })
        })
        .collect();

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
    let fan_overrides = daemon_fan_overrides();
    let fan_rows: Vec<_> = fans
        .iter()
        .map(|f| {
            let pct = match f.max_rpm {
                Some(m) if m > 0 => (f.rpm as f32 / m as f32 * 100.0).clamp(0.0, 100.0),
                _ => 0.0,
            };
            let override_pct = fan_overrides.get(&f.id).copied();
            serde_json::json!({
                "id": f.id,
                "l": f.label,
                "cur_rpm": f.rpm,
                "min_rpm": f.min_rpm.unwrap_or(0),
                "max_rpm": f.max_rpm.unwrap_or(0),
                "pct": pct,
                "controllable": f.controllable,
                "manual": override_pct.is_some(),
                "override_pct": override_pct,
            })
        })
        .collect();

    // Daemon status: poll every tick so the popover always shows current mode.
    let daemon_st = daemon_status_str();
    // Persistent fan control is the paid feature — read-only metrics above
    // stay visible regardless of entitlement.
    let can_control = (app.provider.capabilities().control_fans || !daemon_st.is_empty())
        && app.entitlement.allowed();
    let ctl_status = if !daemon_st.is_empty() {
        daemon_st.clone()
    } else {
        STATUS.lock().expect("status poisoned").clone()
    };
    let (license_line, trial_expired) = match (app.language.resolve(), &app.entitlement) {
        (ResolvedLanguage::Ko, Entitlement::Licensed { email }) => {
            (format!("라이선스 등록됨 — {email}"), false)
        }
        (ResolvedLanguage::Ko, Entitlement::Trial { days_left }) => {
            (format!("체험판 — {days_left}일 남음"), false)
        }
        (ResolvedLanguage::Ko, Entitlement::TrialExpired) => ("체험판 만료됨".to_string(), true),
        (ResolvedLanguage::En, Entitlement::Licensed { email }) => {
            (format!("Licensed — {email}"), false)
        }
        (ResolvedLanguage::En, Entitlement::Trial { days_left }) => {
            (format!("Trial — {days_left} day(s) left"), false)
        }
        (ResolvedLanguage::En, Entitlement::TrialExpired) => ("Trial expired".to_string(), true),
    };
    let chart_range = ChartRange::from_u8(CHART_RANGE.load(Ordering::Relaxed));
    // Seeds the Detail Window's curve editor: the user's saved custom curve
    // if there is one, otherwise Balanced's points as a reasonable starting
    // shape to tweak rather than an empty canvas.
    let curve_points: Vec<[f32; 2]> = peterfan_platform::config::load()
        .custom_curve
        .and_then(|c| c.to_fan_curve())
        .unwrap_or_else(|| Profile::Balanced.default_curve())
        .points()
        .iter()
        .map(|p| [p.temp_c, p.duty_percent as f32])
        .collect();

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
        "disk_io_present": disk_io_present,
        "disk_io_sub": disk_io_sub,
        "procs": proc_rows,
        "proc_sort": if matches!(proc_sort, ProcSort::Memory) { "mem" } else { "cpu" },
        "temp_present": hottest.is_some(),
        "temp_pct": hottest.map(|t| t.value.0).unwrap_or(0.0),
        "temp_text": hottest.map(|t| format!("{:.0}°C", t.value.0)).unwrap_or_default(),
        "temp_cls": hottest.map(|t| temp_cls(t.value)).unwrap_or("g"),
        "temps": temp_rows,
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
        "net_ip": net_ip_line,
        "cpu_hist": to_vec(app.cpu_h.range(chart_range)),
        "mem_hist": to_vec(app.mem_h.range(chart_range)),
        "temp_hist": to_vec(app.temp_h.range(chart_range)),
        "net_hist": to_vec(app.net_h.range(chart_range)),
        "disk_io_hist": to_vec(app.disk_io_h.range(chart_range)),
        "chart_range": chart_range.as_str(),
        "can_control": can_control,
        "ctl_status": ctl_status,
        "daemon_running": !daemon_st.is_empty(),
        "license_line": license_line,
        "trial_expired": trial_expired,
        "buy_url": BUY_URL,
        "curve_points": curve_points,
        "last_cmd_status": STATUS.lock().expect("status poisoned").clone(),
    });
    let script = format!("window.__pf&&window.__pf.update({payload})");
    if app.popover_visible {
        if let Some(wv) = &app.webview {
            let _ = wv.evaluate_script(&script);
        }
    }
    if detail_visible {
        if let Some(wv) = &app.detail_webview {
            let _ = wv.evaluate_script(&script);
        }
    }
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

/// Which fans are currently pinned to a manual duty% by the daemon (fan id ->
/// duty%) — read fresh each tick so the per-fan Auto/Manual toggle in the
/// popover reflects reality even after e.g. a daemon restart. Empty (not an
/// error) when there's no daemon to ask, same as every other daemon-optional
/// read in this file.
#[cfg(unix)]
fn daemon_fan_overrides() -> std::collections::HashMap<String, u8> {
    peterfan_platform::ipc::send_command("temps")
        .and_then(|reply| reply.strip_prefix("ok ").map(str::to_string))
        .and_then(|json_str| serde_json::from_str::<serde_json::Value>(&json_str).ok())
        .and_then(|v| v.get("fan_overrides").cloned())
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default()
}
#[cfg(not(unix))]
fn daemon_fan_overrides() -> std::collections::HashMap<String, u8> {
    std::collections::HashMap::new()
}

/// Run a popover control action (`auto` or `profile:<name>`). Prefers the
/// running `peterfand` daemon (so the unprivileged app needs no root); falls
/// back to controlling fans directly if this process happens to have access.
/// Returns a short human-readable status for the popover.
fn execute_control(provider: &dyn HardwareProvider, cmd: &str) -> String {
    let line = if let Some(name) = cmd.strip_prefix("profile:") {
        format!("profile {name}\n")
    } else if let Some(pct) = cmd.strip_prefix("hold:") {
        format!("hold {pct}\n")
    } else if let Some(rest) = cmd.strip_prefix("fanhold:") {
        // "fanhold:<fan_id>:<pct>" — split on the LAST colon since fan ids
        // are dot-separated (e.g. "fan.cpu") but never contain one.
        match rest.rsplit_once(':') {
            Some((id, pct)) => format!("fanhold {id} {pct}\n"),
            None => format!("{cmd}\n"),
        }
    } else if let Some(id) = cmd.strip_prefix("fanauto:") {
        format!("fanauto {id}\n")
    } else {
        format!("{cmd}\n")
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
    } else if let Some(pct) = cmd.strip_prefix("hold:") {
        match pct.parse::<u8>() {
            Ok(duty) => {
                let duty = duty.min(100);
                (
                    fans.iter()
                        .try_for_each(|id| provider.set_fan_duty(id, duty)),
                    format!("hold {duty}%"),
                )
            }
            Err(_) => return "invalid percent".into(),
        }
    } else if let Some(rest) = cmd.strip_prefix("fanhold:") {
        // One-shot direct write, same as the other local-fallback branches —
        // there's no daemon loop here to keep reasserting a per-fan pin.
        match rest
            .rsplit_once(':')
            .and_then(|(id, pct)| pct.parse::<u8>().ok().map(|d| (id.to_string(), d.min(100))))
        {
            Some((id, duty)) => (
                provider.set_fan_duty(&id, duty),
                format!("{id} hold {duty}%"),
            ),
            None => return "fanhold requires <fan_id>:<percent>".into(),
        }
    } else if let Some(id) = cmd.strip_prefix("fanauto:") {
        (provider.set_fan_auto(id), format!("{id} auto"))
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

/// Whether an `execute_control`/`apply_local` result string represents
/// success — both use these exact prefixes/substrings by construction.
fn control_result_is_ok(result: &str) -> bool {
    result.starts_with("daemon:") || result.contains("applied")
}

/// Send SIGTERM to a process by PID, from the "×" button on a Top Processes
/// row (confirmed client-side first). No elevated privileges are used or
/// needed — the OS enforces the same rule it always does for `kill(2)`: this
/// only succeeds against processes the signing user already owns.
#[cfg(unix)]
fn kill_process(pid: u32) {
    unsafe {
        libc::kill(pid as libc::pid_t, libc::SIGTERM);
    }
}
#[cfg(not(unix))]
fn kill_process(_pid: u32) {}

/// Show a desktop notification for a control action triggered from the
/// right-click menu — those aren't visible in the popover unless it's open,
/// so without this a failed fan command (e.g. no daemon, needs root) looks
/// like it silently did nothing.
#[cfg(target_os = "macos")]
fn notify_control_result(action: &str, ok: bool, result: &str) {
    let title = if ok {
        "PeterFan"
    } else {
        "PeterFan — action needed"
    };
    let script = format!(
        "display notification {} with title {}",
        applescript_quote(result),
        applescript_quote(&format!("{title} · {action}"))
    );
    let _ = std::process::Command::new("osascript")
        .arg("-e")
        .arg(script)
        .status();
}
#[cfg(not(target_os = "macos"))]
fn notify_control_result(_action: &str, _ok: bool, _result: &str) {}

#[cfg(target_os = "macos")]
fn applescript_quote(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

#[cfg(target_os = "macos")]
fn is_enable_fan_control_id(tm: &TrayMenu, id: &tray_icon::menu::MenuId) -> bool {
    tm.enable_fan_control == *id
}
#[cfg(not(target_os = "macos"))]
fn is_enable_fan_control_id(_tm: &TrayMenu, _id: &tray_icon::menu::MenuId) -> bool {
    false
}

#[cfg(target_os = "macos")]
fn is_launch_at_login_id(tm: &TrayMenu, id: &tray_icon::menu::MenuId) -> bool {
    tm.launch_at_login.id() == id
}
#[cfg(not(target_os = "macos"))]
fn is_launch_at_login_id(_tm: &TrayMenu, _id: &tray_icon::menu::MenuId) -> bool {
    false
}

/// Toggle the "Launch at Login" LaunchAgent — a per-user agent, so this
/// never needs an admin password and can happen instantly on click.
#[cfg(target_os = "macos")]
fn toggle_launch_at_login(tm: &TrayMenu, metric: &str) {
    use peterfan_platform::login_item;
    let now_installed = if login_item::is_installed() {
        // Still installed if removal failed.
        login_item::remove().is_err()
    } else {
        login_item::install(None, metric).is_ok()
    };
    tm.launch_at_login.set_checked(now_installed);
}
#[cfg(not(target_os = "macos"))]
fn toggle_launch_at_login(_tm: &TrayMenu, _metric: &str) {}

/// Run the one-time privileged daemon install (macOS admin-password dialog)
/// from the menu bar directly — a GUI-only user never has to open a
/// terminal. Blocks on the dialog, so it must run off the event-loop thread.
#[cfg(target_os = "macos")]
fn install_fan_control() {
    use peterfan_platform::daemon_install::InstallOutcome;
    let (ok, message) = match peterfan_platform::daemon_install::install(false) {
        Ok(InstallOutcome::Installed) => (
            true,
            "Fan control enabled — the daemon is running.".to_string(),
        ),
        Ok(InstallOutcome::InstalledButUnreachable) => (
            false,
            "Installed, but the daemon isn't answering yet — check /var/log/peterfand.err".into(),
        ),
        Ok(InstallOutcome::DryRun(_)) => unreachable!("menu bar never passes dry_run=true"),
        Err(e) => (false, e),
    };
    notify_control_result("Enable Fan Control", ok, &message);
}
#[cfg(not(target_os = "macos"))]
fn install_fan_control() {}

/// Fan control is only reachable via a running daemon or by running as root
/// ourselves — mirrors the check `peterfan doctor` reports.
#[cfg(target_os = "macos")]
fn fan_control_ready() -> bool {
    if peterfan_platform::daemon_reachable() {
        return true;
    }
    // SAFETY: geteuid() is always safe and has no preconditions.
    unsafe { libc::geteuid() == 0 }
}

/// On first launch (and every launch after, until the user opts out), ask
/// right away whether to set up fan control — instead of leaving the user to
/// discover "Enable Fan Control" in the right-click menu themselves. Other
/// fan-control apps do this during their installer; PeterFan doesn't have
/// one, so the first launch asks in its place. Runs off the event-loop
/// thread since the dialog blocks until the user responds.
#[cfg(target_os = "macos")]
fn maybe_prompt_first_run_setup() {
    let cfg = peterfan_platform::config::load();
    if cfg.menubar.setup_prompt_dismissed || fan_control_ready() {
        return;
    }
    // Give the tray icon a moment to settle before popping a dialog over it.
    std::thread::sleep(Duration::from_millis(600));

    let script = r#"display dialog "PeterFan needs one-time permission to control your Mac's fans.\n\nYou'll see one macOS password prompt — after that, fan control works without sudo." with title "PeterFan — Set Up Fan Control" buttons {"Don't Ask Again", "Not Now", "Set Up Now"} default button "Set Up Now" cancel button "Not Now""#;
    let output = std::process::Command::new("osascript")
        .arg("-e")
        .arg(script)
        .output();

    let Ok(output) = output else { return };
    let stdout = String::from_utf8_lossy(&output.stdout);
    if stdout.contains("Set Up Now") {
        install_fan_control();
    } else if stdout.contains("Don't Ask Again") {
        let mut cfg = peterfan_platform::config::load();
        cfg.menubar.setup_prompt_dismissed = true;
        let _ = peterfan_platform::config::save(&cfg);
    }
    // "Not Now" (or Escape, which maps to the cancel button) — ask again
    // next launch, nothing to persist.
}
#[cfg(not(target_os = "macos"))]
fn maybe_prompt_first_run_setup() {}

/// Silent background check, run once shortly after launch. Only speaks up
/// (via [`prompt_update_available`]) if a newer release actually exists —
/// "already up to date" isn't worth interrupting anyone for.
#[cfg(target_os = "macos")]
fn check_for_updates_on_launch() {
    // Staggered well past the fan-control setup prompt's own 600ms delay so
    // the two one-shot dialogs never compete for the user's attention.
    std::thread::sleep(Duration::from_secs(4));
    if let Ok(release) = peterfan_platform::updater::fetch_latest_release() {
        if peterfan_platform::updater::is_newer(env!("CARGO_PKG_VERSION"), &release.version) {
            prompt_update_available(&release);
        }
    }
    // Network hiccup or GitHub rate limit: fail silently, try again next launch.
}
#[cfg(not(target_os = "macos"))]
fn check_for_updates_on_launch() {}

/// "Check for Updates…" menu click — unlike the launch check, this always
/// reports back (including "you're up to date"), since the user asked.
#[cfg(target_os = "macos")]
fn check_for_updates_interactive() {
    match peterfan_platform::updater::fetch_latest_release() {
        Ok(release) => {
            if peterfan_platform::updater::is_newer(env!("CARGO_PKG_VERSION"), &release.version) {
                prompt_update_available(&release);
            } else {
                notify_control_result(
                    "Check for Updates",
                    true,
                    &format!("You're up to date (v{}).", env!("CARGO_PKG_VERSION")),
                );
            }
        }
        Err(e) => {
            notify_control_result("Check for Updates", false, &format!("Couldn't check: {e}"))
        }
    }
}
#[cfg(not(target_os = "macos"))]
fn check_for_updates_interactive() {}

/// Ask whether to install `release` now. "Update Now" downloads, extracts,
/// and queues a detached script that replaces the running `.app` bundle and
/// relaunches once this process quits — see
/// `peterfan_platform::updater::download_and_install`.
#[cfg(target_os = "macos")]
fn prompt_update_available(release: &peterfan_platform::updater::ReleaseInfo) {
    let Some(asset_url) = release.asset_url.clone() else {
        // No macOS asset on this release — all we can offer is the page.
        let script = format!(
            r#"display dialog "PeterFan {} is available (you have {}), but this release has no macOS download yet." with title "PeterFan Update" buttons {{"OK", "View Release"}} default button "View Release""#,
            applescript_quote(&release.tag),
            applescript_quote(env!("CARGO_PKG_VERSION")),
        );
        if let Ok(out) = std::process::Command::new("osascript")
            .arg("-e")
            .arg(&script)
            .output()
        {
            if String::from_utf8_lossy(&out.stdout).contains("View Release") {
                let _ = std::process::Command::new("open")
                    .arg(&release.html_url)
                    .status();
            }
        }
        return;
    };

    let script = format!(
        r#"display dialog "PeterFan {} is available — you have {}.\n\nUpdate now? PeterFan will quit and relaunch." with title "PeterFan Update" buttons {{"View Release", "Not Now", "Update Now"}} default button "Update Now" cancel button "Not Now""#,
        applescript_quote(&release.tag),
        applescript_quote(env!("CARGO_PKG_VERSION")),
    );
    let Ok(out) = std::process::Command::new("osascript")
        .arg("-e")
        .arg(&script)
        .output()
    else {
        return;
    };
    let stdout = String::from_utf8_lossy(&out.stdout);

    if stdout.contains("Update Now") {
        match peterfan_platform::updater::download_and_install(&asset_url) {
            Ok(()) => QUIT.store(true, Ordering::Relaxed),
            Err(e) => {
                notify_control_result("Update PeterFan", false, &format!("Update failed: {e}"))
            }
        }
    } else if stdout.contains("View Release") {
        let _ = std::process::Command::new("open")
            .arg(&release.html_url)
            .status();
    }
    // "Not Now" / Escape — no state to persist; the next launch (or manual
    // "Check for Updates…") just asks again.
}
#[cfg(not(target_os = "macos"))]
fn prompt_update_available(_release: &peterfan_platform::updater::ReleaseInfo) {}

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

/// Render a small colored bar-chart sparkline of recent samples for the menu-bar
/// icon — the "graph at a glance" look iStat-style monitors are known for.
/// Bar color reflects the latest sample's load band (green/yellow/red).
/// Build the menu-bar sparkline icon for whichever metric is currently
/// selected, pulling from that metric's own rolling history buffer.
fn menubar_graph_icon(app: &App) -> Icon {
    // Always the short-term (2-minute) trend, independent of whatever range
    // the popover's chart tabs are set to.
    match app.metric {
        MenubarMetric::Cpu => make_graph_icon(&to_vec(&app.cpu_h.minute), Some(100.0)),
        MenubarMetric::Memory => make_graph_icon(&to_vec(&app.mem_h.minute), Some(100.0)),
        MenubarMetric::Temp => make_graph_icon(&to_vec(&app.temp_h.minute), Some(100.0)),
        MenubarMetric::Fan => make_graph_icon(&to_vec(&app.fan_hist), Some(100.0)),
        MenubarMetric::Network => make_graph_icon(&to_vec(&app.net_h.minute), None),
    }
}

fn to_vec(hist: &VecDeque<f32>) -> Vec<f32> {
    hist.iter().copied().collect()
}

/// `max_val`: `Some(v)` pins the y-axis (e.g. 100 for a percentage); `None`
/// auto-scales to the visible window's own peak (used for network throughput,
/// which has no fixed ceiling).
fn make_graph_icon(history: &[f32], max_val: Option<f32>) -> Icon {
    const W: u32 = 32;
    const H: u32 = 32;
    let mut rgba = vec![0u8; (W * H * 4) as usize];
    if history.is_empty() {
        return Icon::from_rgba(rgba, W, H).expect("valid icon");
    }

    let n = history.len().clamp(1, 20);
    let recent: Vec<f32> = history[history.len() - n..].to_vec();
    let max_val =
        max_val.unwrap_or_else(|| recent.iter().cloned().fold(1.0_f32, f32::max).max(1.0));
    let latest = *recent.last().unwrap_or(&0.0);
    let latest_frac = (latest / max_val).clamp(0.0, 1.0);
    let (r, g, b) = match latest_frac {
        x if x < 0.5 => (48u8, 209u8, 88u8),  // green
        x if x < 0.8 => (255u8, 214u8, 10u8), // yellow
        _ => (255u8, 69u8, 58u8),             // red
    };

    let bar_w = W as f32 / recent.len() as f32;
    for (i, &v) in recent.iter().enumerate() {
        let frac = (v / max_val).clamp(0.0, 1.0);
        let bar_h = ((H as f32 - 2.0) * frac).round().max(1.0) as u32;
        let x0 = (i as f32 * bar_w).round() as u32;
        let x1 = (((i + 1) as f32) * bar_w).round().max((x0 + 1) as f32) as u32;
        for y in H.saturating_sub(bar_h)..H {
            for x in x0..x1.min(W) {
                let idx = ((y * W + x) * 4) as usize;
                if idx + 3 < rgba.len() {
                    rgba[idx] = r;
                    rgba[idx + 1] = g;
                    rgba[idx + 2] = b;
                    rgba[idx + 3] = 225;
                }
            }
        }
    }
    Icon::from_rgba(rgba, W, H).expect("valid icon")
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

/// Build the popover/detail-window HTML for the given language. The template
/// itself is authored in English and Korean labels are substituted in by
/// exact `>Label<`/string match — cheap, and safe because each source string
/// only ever appears where a translation actually belongs (verified by hand,
/// covered by `dashboard_html_translates_known_labels` below).
fn dashboard_html(lang: ResolvedLanguage, show_curve_editor: bool) -> String {
    let lang_tag = match lang {
        ResolvedLanguage::En => "en",
        ResolvedLanguage::Ko => "ko",
    };
    let html = DASHBOARD_HTML_EN
        .replace("__LANG__", lang_tag)
        .replace("__SHOWCURVE__", if show_curve_editor { "1" } else { "0" });
    match lang {
        ResolvedLanguage::En => html,
        ResolvedLanguage::Ko => html
            .replace(">Fan control<", ">팬 제어<")
            .replace(">Memory<", ">메모리<")
            .replace(">Storage<", ">저장공간<")
            .replace(">Temperature<", ">온도<")
            .replace(">Fans<", ">팬<")
            .replace(">Battery<", ">배터리<")
            .replace(">Network<", ">네트워크<")
            .replace(">Top Processes<", ">실행 중 프로세스<")
            .replace(">MEM<", ">메모리<")
            .replace("Buy License →", "라이선스 구매 →")
            .replace(">Activate<", ">활성화<")
            .replace("Open Detailed Window…", "상세 창 열기…")
            .replace(">Quit PeterFan<", ">PeterFan 종료<")
            .replace(">Fan Curve<", ">팬 커브<")
            .replace(">Reset<", ">초기화<")
            .replace(">Remove Point<", ">점 삭제<")
            .replace(">Save &amp; Apply<", ">저장 및 적용<")
            .replace(
                "Drag points to reshape. Click empty space to add a point.",
                "점을 드래그해서 모양을 바꾸세요. 빈 공간을 클릭하면 점이 추가됩니다.",
            )
            .replace(
                "Tip: run peterfan install-daemon once for persistent control at boot.",
                "팁: peterfan install-daemon을 한 번 실행하면 부팅 시에도 설정이 유지됩니다.",
            ),
    }
}

const DASHBOARD_HTML_EN: &str = r##"<!doctype html><html><head><meta charset="utf-8"><meta name="color-scheme" content="light dark">
<style>
:root{--g:#30d158;--y:#ffd60a;--r:#ff453a;--accent:#5b9dff;--text:#f4f6fa;--dim:#7f8896;--line:rgba(255,255,255,.07);--panel-bg:#1b1b1d;--panel-border:rgba(255,255,255,.09);--chip-bg:rgba(255,255,255,.06);--chip-hover:rgba(91,157,255,.28);--track:rgba(255,255,255,.08);--track-hover:rgba(255,255,255,.06);--shadow:0 20px 50px rgba(0,0,0,.45),0 2px 10px rgba(0,0,0,.3);}
@media (prefers-color-scheme: light){
:root{--text:#1c1e21;--dim:#6b7280;--line:rgba(0,0,0,.08);--panel-bg:#f7f8fa;--panel-border:rgba(0,0,0,.09);--chip-bg:rgba(0,0,0,.05);--chip-hover:rgba(59,130,246,.16);--track:rgba(0,0,0,.08);--track-hover:rgba(0,0,0,.05);--shadow:0 20px 46px rgba(0,0,0,.16),0 2px 8px rgba(0,0,0,.08);}
}
*{box-sizing:border-box;margin:0;padding:0;}
html,body{background:transparent;font-family:-apple-system,system-ui,sans-serif;color:var(--text);-webkit-user-select:none;cursor:default;-webkit-font-smoothing:antialiased;overflow:hidden;}
.panel{background:var(--panel-bg);border:1px solid var(--panel-border);border-radius:13px;overflow-y:auto;overflow-x:hidden;box-shadow:var(--shadow);max-height:100vh;}
.row{display:grid;grid-template-columns:24px 1fr;gap:12px;padding:8px 15px;align-items:center;}
.row + .row{border-top:1px solid var(--line);}
.ic{width:21px;height:21px;color:var(--dim);}
.ic svg{width:100%;height:100%;fill:none;stroke:currentColor;stroke-width:1.6;stroke-linecap:round;stroke-linejoin:round;}
.content{min-width:0;}
.head{display:flex;justify-content:space-between;align-items:baseline;gap:10px;}
.name{font-size:9.5px;font-weight:600;color:var(--dim);letter-spacing:.08em;text-transform:uppercase;}
.val{font-size:14px;font-weight:600;letter-spacing:-.01em;white-space:nowrap;font-variant-numeric:tabular-nums;}
.sub{font-size:10px;color:var(--dim);margin-top:1px;line-height:1.45;white-space:nowrap;overflow:hidden;text-overflow:ellipsis;font-variant-numeric:tabular-nums;}
.bar{height:3px;background:var(--track);border-radius:99px;margin-top:7px;overflow:hidden;}
.bar-fill{height:100%;border-radius:99px;width:0;transition:width .35s ease;}
.bar-fill.g{background:var(--g);}.bar-fill.y{background:var(--y);}.bar-fill.r{background:var(--r);}.bar-fill.b{background:var(--accent);}
.cores{display:flex;align-items:flex-end;gap:2.5px;height:22px;margin-top:8px;background:var(--track);border-radius:4px;padding:2px 3px 0;}
.core{flex:1;border-radius:1px 1px 0 0;min-height:2px;transition:height .3s ease;cursor:default;}
.core.g{background:var(--g);}.core.y{background:var(--y);}.core.r{background:var(--r);}
.trow{display:flex;justify-content:space-between;align-items:baseline;font-size:10.5px;margin-top:5px;}
.trow .l{color:var(--dim);}
.trow .v{font-weight:600;font-variant-numeric:tabular-nums;}
.v.g{color:var(--g);}.v.y{color:var(--y);}.v.r{color:var(--r);}
.prow{display:grid;grid-template-columns:1fr auto auto auto;gap:9px;align-items:baseline;font-size:10.5px;margin-top:5px;}
.pkill{opacity:0;background:none;border:0;color:var(--r);font:inherit;font-size:13px;font-weight:700;line-height:1;padding:0 1px;cursor:pointer;transition:opacity .15s;}
.prow:hover .pkill{opacity:1;}
.prow .n{white-space:nowrap;overflow:hidden;text-overflow:ellipsis;}
.prow .c{color:var(--accent);font-weight:600;font-variant-numeric:tabular-nums;white-space:nowrap;}
.prow .m{color:var(--dim);font-variant-numeric:tabular-nums;white-space:nowrap;}
.ctl{padding:7px 15px 8px;border-top:1px solid var(--line);}
.ctl-head{display:flex;justify-content:space-between;align-items:baseline;margin-bottom:5px;}
.ctl-head .name{font-size:9.5px;font-weight:600;color:var(--dim);letter-spacing:.08em;text-transform:uppercase;}
.ctl-status{font-size:10px;color:var(--dim);font-variant-numeric:tabular-nums;}
.fan-cards{display:flex;flex-direction:column;}
.fan-card{padding:5px 0;}
.fan-card+.fan-card{border-top:1px solid var(--line);}
.fan-card-head{display:flex;justify-content:space-between;align-items:baseline;font-size:10.5px;margin-bottom:3px;}
.fan-card-head .fn{font-weight:600;}
.fan-card-head .fv{font-variant-numeric:tabular-nums;color:var(--dim);}
.fan-bar{height:3px;background:var(--track);border-radius:99px;overflow:hidden;margin-bottom:4px;}
.fan-bar i{display:block;height:100%;background:var(--accent);border-radius:99px;width:0;transition:width .35s;}
.fan-bottom{display:flex;justify-content:space-between;align-items:center;gap:8px;}
.fan-rpm-text{font-size:9px;color:var(--dim);font-variant-numeric:tabular-nums;white-space:nowrap;}
.fan-seg{display:flex;gap:4px;flex:0 0 auto;}
.fan-seg button{background:var(--chip-bg);border:1px solid transparent;color:var(--dim);font:inherit;font-size:9px;font-weight:600;padding:3px 8px;border-radius:5px;cursor:pointer;white-space:nowrap;transition:background .15s,color .15s;}
.fan-seg button.active{background:var(--panel-bg);color:var(--text);border-color:rgba(91,157,255,.4);}
.fan-rpm-row{display:grid;grid-template-columns:auto 1fr auto;gap:7px;align-items:center;margin-top:5px;transition:opacity .15s;}
.fan-rpm-row.inactive{opacity:.35;pointer-events:none;}
.fan-rpm-row span{font-size:9px;color:var(--dim);font-variant-numeric:tabular-nums;white-space:nowrap;}
.fan-rpm-row input[type=range]{-webkit-appearance:none;height:3px;border-radius:99px;background:var(--track);outline:none;cursor:pointer;}
.fan-rpm-row input[type=range]::-webkit-slider-thumb{-webkit-appearance:none;width:14px;height:14px;border-radius:50%;background:var(--accent);cursor:pointer;}
.ctl-note{font-size:10.5px;color:var(--dim);line-height:1.5;margin-top:6px;}
.note-fix-btn{margin-top:5px;background:rgba(91,157,255,.22);border:1px solid rgba(91,157,255,.5);color:var(--accent);font:inherit;font-size:10px;font-weight:600;padding:5px 10px;border-radius:6px;cursor:pointer;}
.note-fix-btn:hover{background:rgba(91,157,255,.32);}
#curve-canvas{width:100%;height:120px;display:block;border-radius:6px;background:var(--track);cursor:crosshair;touch-action:none;margin-top:8px;}
.curve-actions{display:flex;gap:6px;margin-top:8px;}
.curve-actions button{flex:1;background:var(--chip-bg);border:1px solid transparent;color:var(--text);font:inherit;font-size:10px;font-weight:600;padding:6px 4px;border-radius:7px;cursor:pointer;transition:background .15s;}
.curve-actions button:hover{background:var(--chip-hover);}
.curve-actions button.primary{background:rgba(91,157,255,.22);border-color:rgba(91,157,255,.5);color:var(--accent);}
.chart{width:100%;height:28px;display:block;margin-top:8px;border-radius:4px;cursor:crosshair;}
.chart-tip{position:fixed;pointer-events:none;background:rgba(20,20,22,.92);color:#fff;font-size:9.5px;font-weight:600;padding:3px 7px;border-radius:5px;display:none;z-index:999;white-space:nowrap;font-variant-numeric:tabular-nums;}
.chart-stats{font-size:9px;color:var(--dim);text-align:right;margin-top:3px;font-variant-numeric:tabular-nums;}
.lic{padding:8px 15px;border-top:1px solid var(--line);font-size:10.5px;color:var(--dim);display:flex;align-items:center;justify-content:space-between;gap:8px;}
.lic.expired{background:rgba(255,159,10,.14);color:var(--text);}
.lic-actions{display:flex;align-items:center;gap:10px;flex:0 0 auto;}
.lic-link{color:var(--accent);cursor:pointer;font-weight:600;background:none;border:0;font:inherit;font-size:10.5px;padding:0;}
.lic-buy{color:var(--accent);text-decoration:none;font-weight:600;display:none;}
.lic-form{display:none;gap:6px;padding:0 15px 9px;}
.lic-form.show{display:flex;}
.lic-form input{flex:1;min-width:0;background:var(--chip-bg);border:1px solid var(--panel-border);border-radius:6px;color:var(--text);font:inherit;font-size:10.5px;padding:6px 8px;outline:none;}
.lic-form button{background:var(--accent);color:#fff;border:0;border-radius:6px;font:inherit;font-size:10.5px;font-weight:600;padding:6px 10px;cursor:pointer;white-space:nowrap;}
.foot{border-top:1px solid var(--line);padding:3px;}
.quit{display:block;width:100%;background:transparent;border:0;color:var(--dim);font:inherit;font-size:10.5px;letter-spacing:.02em;padding:8px;border-radius:8px;cursor:pointer;transition:background .15s,color .15s;}
.quit:hover{background:var(--track-hover);color:var(--text);}
.range-tabs{display:flex;gap:4px;padding:8px 15px 0;justify-content:flex-end;}
.sort-tabs{display:flex;gap:4px;}
.range-tab{background:var(--chip-bg);border:1px solid transparent;color:var(--dim);font:inherit;font-size:9.5px;font-weight:600;padding:3px 9px;border-radius:99px;cursor:pointer;transition:background .15s,color .15s;}
.range-tab:hover{background:var(--chip-hover);}
.range-tab.active{background:rgba(91,157,255,.22);color:var(--accent);}
</style></head><body><div class="panel">

<div class="range-tabs">
<button class="range-tab active" data-range="2m" onclick="setChartRange('2m')">2m</button>
<button class="range-tab" data-range="1h" onclick="setChartRange('1h')">1h</button>
<button class="range-tab" data-range="1d" onclick="setChartRange('1d')">1d</button>
</div>

<div class="ctl" style="border-top:0;border-bottom:1px solid var(--line)">
<div class="ctl-head"><span class="name">Fan control</span><span class="ctl-status" id="ctl-status"></span></div>
<div class="fan-cards" id="fan-cards"></div>
<div class="ctl-note" id="ctl-note" style="display:none"></div>
</div>

<div class="row" id="curve-editor-section" style="display:none;border-bottom:1px solid var(--line)"><span class="ic"><svg viewBox="0 0 24 24"><path d="M3 17l5-6 4 3 9-9"/><path d="M3 21h18"/></svg></span>
<div class="content"><div class="head"><span class="name">Fan Curve</span></div>
<canvas id="curve-canvas"></canvas>
<div class="sub" id="curve-hint">Drag points to reshape. Click empty space to add a point.</div>
<div class="curve-actions">
<button onclick="resetCurve()">Reset</button>
<button onclick="removeCurvePoint()">Remove Point</button>
<button class="primary" onclick="saveCurve()">Save &amp; Apply</button>
</div>
</div></div>

<div class="row"><span class="ic"><svg viewBox="0 0 24 24"><rect x="6" y="6" width="12" height="12" rx="2"/><path d="M9 2v3M15 2v3M9 19v3M15 19v3M2 9h3M2 15h3M19 9h3M19 15h3"/></svg></span>
<div class="content"><div class="head"><span class="name">CPU</span><span class="val" id="cpu-val">—</span></div>
<div class="sub" id="cpu-sub"></div><div class="cores" id="cores"></div>
<div class="bar"><div class="bar-fill" id="cpu-bar"></div></div>
<canvas class="chart" id="cpu-chart"></canvas><div class="chart-stats" id="cpu-chart-stats"></div></div></div>

<div class="row"><span class="ic"><svg viewBox="0 0 24 24"><rect x="2" y="7" width="20" height="11" rx="1.5"/><path d="M6 18v2M10 18v2M14 18v2M18 18v2M6 10v4M10 10v4M14 10v4"/></svg></span>
<div class="content"><div class="head"><span class="name">Memory</span><span class="val" id="mem-val">—</span></div>
<div class="sub" id="mem-sub"></div><div class="bar"><div class="bar-fill" id="mem-bar"></div></div>
<canvas class="chart" id="mem-chart"></canvas><div class="chart-stats" id="mem-chart-stats"></div></div></div>

<div class="row"><span class="ic"><svg viewBox="0 0 24 24"><ellipse cx="12" cy="6" rx="8" ry="3"/><path d="M4 6v12c0 1.7 3.6 3 8 3s8-1.3 8-3V6"/><path d="M4 12c0 1.7 3.6 3 8 3s8-1.3 8-3"/></svg></span>
<div class="content"><div class="head"><span class="name">Storage</span><span class="val" id="disk-val">—</span></div>
<div class="sub" id="disk-sub"></div><div class="bar"><div class="bar-fill" id="disk-bar"></div></div>
<div class="sub" id="disk-io-sub" style="display:none;margin-top:4px"></div>
<canvas class="chart" id="disk-io-chart" style="display:none"></canvas><div class="chart-stats" id="disk-io-chart-stats"></div></div></div>

<div class="row" id="sec-temp"><span class="ic"><svg viewBox="0 0 24 24"><path d="M14 14.76V5a2 2 0 0 0-4 0v9.76a4 4 0 1 0 4 0z"/></svg></span>
<div class="content"><div class="head"><span class="name">Temperature</span><span class="val" id="temp-val">—</span></div>
<div class="bar"><div class="bar-fill" id="temp-bar"></div></div><div id="temp-list"></div>
<canvas class="chart" id="temp-chart"></canvas><div class="chart-stats" id="temp-chart-stats"></div></div></div>

<div class="row" id="sec-batt"><span class="ic"><svg viewBox="0 0 24 24"><rect x="2" y="8" width="18" height="9" rx="2"/><path d="M22 11v3"/></svg></span>
<div class="content"><div class="head"><span class="name">Battery</span><span class="val" id="batt-val">—</span></div>
<div class="sub" id="batt-sub"></div><div class="bar"><div class="bar-fill" id="batt-bar"></div></div></div></div>

<div class="row"><span class="ic"><svg viewBox="0 0 24 24"><circle cx="12" cy="12" r="9"/><path d="M3 12h18M12 3c2.5 2.5 2.5 15 0 18M12 3c-2.5 2.5-2.5 15 0 18"/></svg></span>
<div class="content"><div class="head"><span class="name">Network</span><span class="val"></span></div>
<div class="sub" id="net-sub"></div>
<div class="sub" id="net-ip" style="display:none"></div>
<canvas class="chart" id="net-chart"></canvas><div class="chart-stats" id="net-chart-stats"></div></div></div>

<div class="row" id="sec-procs"><span class="ic"><svg viewBox="0 0 24 24"><rect x="3" y="4" width="18" height="16" rx="2"/><path d="M3 9h18M8 4v5"/></svg></span>
<div class="content"><div class="head"><span class="name">Top Processes</span><span class="sort-tabs"><button class="range-tab" id="ps-cpu" onclick="setProcSort('cpu')">CPU</button><button class="range-tab" id="ps-mem" onclick="setProcSort('mem')">MEM</button></span></div>
<div id="procs-list"></div></div></div>

<div class="lic" id="lic-row">
<span id="lic-text"></span>
<span class="lic-actions">
<a class="lic-buy" id="lic-buy" href="#" target="_blank" rel="noopener">Buy License →</a>
<button class="lic-link" id="lic-toggle" onclick="toggleLicForm()">Activate</button>
</span>
</div>
<div class="lic-form" id="lic-form">
<input type="text" id="lic-input" placeholder="PFAN1-..." spellcheck="false">
<button onclick="submitLicense()">Activate</button>
</div>

<div class="foot"><button class="quit" onclick="window.ipc.postMessage('open_detail')">Open Detailed Window…</button><button class="quit" onclick="window.ipc.postMessage('quit')">Quit PeterFan</button></div>
</div>
<div class="chart-tip" id="chart-tip"></div>
<script>
var LANG='__LANG__';
var SHOW_CURVE_EDITOR='__SHOWCURVE__';
window.__pf={
 update:function(d){
 function cls(p){return p<50?'g':p<80?'y':'r';}
 function bar(id,p,c){var b=document.getElementById(id);if(b){b.style.width=Math.max(0,Math.min(100,p))+'%';b.className='bar-fill '+(c||cls(p));}}
 function set(id,t){var e=document.getElementById(id);if(e)e.textContent=t;}
 function show(id,on){var e=document.getElementById(id);if(e)e.style.display=on?'':'none';}
 set('cpu-val',d.cpu_text);set('cpu-sub',d.cpu_sub);bar('cpu-bar',d.cpu_pct);
 var cc=document.getElementById('cores');if(cc){cc.innerHTML='';(d.cores||[]).forEach(function(p,i){var s=document.createElement('span');s.className='core '+cls(p);s.style.height=Math.max(8,Math.min(100,p))+'%';s.title='Core '+(i+1)+': '+p.toFixed(1)+'%';cc.appendChild(s);});}
 set('mem-val',d.mem_text);set('mem-sub',d.mem_sub);bar('mem-bar',d.mem_pct);
 set('disk-val',d.disk_text);set('disk-sub',d.disk_sub);bar('disk-bar',d.disk_pct);
 show('disk-io-sub',d.disk_io_present);if(d.disk_io_present)set('disk-io-sub',d.disk_io_sub);
 show('disk-io-chart',d.disk_io_present);
 show('disk-io-chart-stats',d.disk_io_present);
 if(d.disk_io_present)drawChart('disk-io-chart', d.disk_io_hist, '#ff9f0a', null, fmtBytesPerSec);
 show('sec-temp',d.temp_present);if(d.temp_present){set('temp-val',d.temp_text);bar('temp-bar',d.temp_pct,d.temp_cls);
   var tl=document.getElementById('temp-list');if(tl){tl.innerHTML='';(d.temps||[]).forEach(function(t){var r=document.createElement('div');r.className='trow';r.innerHTML='<span class="l"></span><span class="v"></span>';r.children[0].textContent=t.l;r.children[1].textContent=t.c;r.children[1].className='v '+t.cls;tl.appendChild(r);});}}
 show('sec-batt',d.batt_present);if(d.batt_present){set('batt-val',d.batt_text);set('batt-sub',d.batt_sub);bar('batt-bar',d.batt_pct,d.batt_pct>50?'g':d.batt_pct>20?'y':'r');}
 set('net-sub',d.net_sub);
 show('net-ip',!!d.net_ip);if(d.net_ip)set('net-ip',d.net_ip);
 var psCpu=document.getElementById('ps-cpu'),psMem=document.getElementById('ps-mem');
 if(psCpu)psCpu.classList.toggle('active',d.proc_sort!=='mem');
 if(psMem)psMem.classList.toggle('active',d.proc_sort==='mem');
 var pl=document.getElementById('procs-list');
 if(pl){pl.innerHTML='';(d.procs||[]).forEach(function(p){var r=document.createElement('div');r.className='prow';r.innerHTML='<span class="n"></span><span class="c"></span><span class="m"></span><button class="pkill" title="Quit process">×</button>';r.children[0].textContent=p.name;r.children[1].textContent=p.cpu;r.children[2].textContent=p.mem;r.children[3].onclick=function(){quitProcess(p.pid,p.name);};pl.appendChild(r);});}
 CHART_RANGE_LABEL=d.chart_range;
 drawChart('cpu-chart', d.cpu_hist, '#5b9dff', 100, function(v){return v.toFixed(1)+'%';});
 drawChart('mem-chart', d.mem_hist, '#5b9dff', 100, function(v){return v.toFixed(1)+'%';});
 if(d.temp_present) drawChart('temp-chart', d.temp_hist, '#ff9f0a', null, function(v){return v.toFixed(0)+'°C';});
 drawChart('net-chart', d.net_hist, '#30d158', null, fmtBytesPerSec);
 document.querySelectorAll('.range-tabs .range-tab').forEach(function(b){b.classList.toggle('active',b.dataset.range===d.chart_range);});
 set('lic-text', d.license_line);
 var licRow=document.getElementById('lic-row');
 if(licRow)licRow.className='lic'+(d.trial_expired?' expired':'');
 var licBuy=document.getElementById('lic-buy');
 if(licBuy){licBuy.style.display=d.trial_expired?'':'none';licBuy.href=d.buy_url||'#';}
 var licForm=document.getElementById('lic-form');
 if(d.trial_expired&&licForm)licForm.classList.add('show');
 var licToggle=document.getElementById('lic-toggle');
 if(licToggle)licToggle.style.display=d.trial_expired?'none':'';
 var note=document.getElementById('ctl-note');
 if(d.can_control){
   set('ctl-status', d.ctl_status||'');
   if(note){
     // A command failure (e.g. a running daemon too old to understand a
     // command we just sent it) used to be silently swallowed — ctl-status
     // only ever shows the daemon's own global mode string, never a
     // per-command result. Surface it here instead, taking priority over
     // the "install the daemon" tip.
     var isErr=d.last_cmd_status&&/error|invalid|unknown|failed|needs root|needs at least/i.test(d.last_cmd_status);
     if(isErr){
       note.style.display='';
       // "unknown command" specifically means the running daemon predates
       // whatever command we just sent it — the fix is a daemon update, not
       // a config change, so offer it as a one-click button right here
       // instead of pointing at a menu item the user has to go find.
       var isUnknownCmd=/unknown command/i.test(d.last_cmd_status);
       note.innerHTML='';
       var msg=document.createElement('span');
       msg.textContent=(LANG==='ko'?'오류: ':'Error: ')+d.last_cmd_status;
       note.appendChild(msg);
       if(isUnknownCmd){
         var fixBtn=document.createElement('button');
         fixBtn.className='note-fix-btn';
         fixBtn.textContent=LANG==='ko'?'데몬 업데이트':'Update Daemon';
         fixBtn.onclick=function(){window.ipc.postMessage('cmd:enablefancontrol');};
         note.appendChild(document.createElement('br'));
         note.appendChild(fixBtn);
       }
     } else if(!d.daemon_running){
       note.style.display='';
       note.textContent='Tip: run peterfan install-daemon once for persistent control at boot.';
     } else {
       note.style.display='none';
     }
   }
   renderFanCards(d.fans);
 } else {
   set('ctl-status','unavailable');
   if(note){note.style.display='';note.textContent='Fan control unavailable on this Mac — showing live RPM only.';}
   var fc=document.getElementById('fan-cards');if(fc)fc.innerHTML='';
 }
 if(SHOW_CURVE_EDITOR==='1'){
   var ces=document.getElementById('curve-editor-section');
   if(ces)ces.style.display='';
   if(d.curve_points){
     CURVE_POINTS_SAVED=d.curve_points.map(function(p){return p.slice();});
     if(CURVE_POINTS===null)CURVE_POINTS=CURVE_POINTS_SAVED.map(function(p){return p.slice();});
   }
   initCurveEditor();
   drawCurveEditor();
 }
 reportHeight();
}};
// One card per controllable fan — independent Auto/Manual toggle + a slider
// bounded to that fan's own min/max RPM (not a 0-100% abstraction), so you
// can pin e.g. just the left fan while the right one keeps following the
// curve. Built once per fan id and updated in place on every tick, so an
// in-progress slider drag never gets clobbered by the next poll.
function renderFanCards(fans){
  var container=document.getElementById('fan-cards');
  if(!container)return;
  var seen={};
  (fans||[]).forEach(function(f){
    if(!f.controllable)return;
    seen[f.id]=true;
    var card=container.querySelector('[data-fan-id="'+f.id+'"]');
    if(!card){
      card=document.createElement('div');
      card.className='fan-card';
      card.setAttribute('data-fan-id',f.id);
      card.innerHTML='<div class="fan-card-head"><span class="fn"></span><span class="fv"></span></div>'+
        '<div class="fan-bar"><i></i></div>'+
        '<div class="fan-bottom"><span class="fan-rpm-text"></span><span class="fan-seg"><button class="fa-auto"></button><button class="fa-manual"></button></span></div>'+
        '<div class="fan-rpm-row inactive"><span class="fa-min"></span><input type="range"><span class="fa-max"></span></div>';
      var btnAuto=card.querySelector('.fa-auto');
      var btnManual=card.querySelector('.fa-manual');
      btnAuto.textContent=LANG==='ko'?'자동':'Auto';
      btnManual.textContent=LANG==='ko'?'사용자 지정…':'Custom…';
      btnAuto.onclick=function(){window.ipc.postMessage('cmd:fanauto:'+f.id);};
      btnManual.onclick=function(){
        // Pin right where the fan already is instead of jumping to a
        // default — read the live % off the card, not this closure's
        // (potentially stale, first-render-time) copy of `f`.
        var curPct=Math.round(parseFloat(card.dataset.curPct||'50'));
        window.ipc.postMessage('cmd:fanhold:'+f.id+':'+curPct);
        card.querySelector('.fan-rpm-row').classList.remove('inactive');
      };
      var slider=card.querySelector('input[type=range]');
      slider.addEventListener('input',function(){
        var v=parseInt(slider.value,10);
        var useRpm=slider.dataset.useRpm==='1';
        card.querySelector('.fv').textContent=useRpm?(v+' RPM'):(v+'%');
      });
      slider.addEventListener('change',function(){
        var v=parseInt(slider.value,10);
        var min=parseInt(slider.min,10),max=parseInt(slider.max,10);
        var span=max-min;
        var pct=slider.dataset.useRpm==='1'?(span>0?Math.round((v-min)/span*100):0):v;
        window.ipc.postMessage('cmd:fanhold:'+f.id+':'+Math.max(0,Math.min(100,pct)));
      });
      container.appendChild(card);
    }
    var manual=!!f.manual;
    var useRpm=f.max_rpm>0;
    card.dataset.curPct=f.pct;
    card.querySelector('.fn').textContent=f.l;
    card.querySelector('.fan-bar i').style.width=Math.max(0,Math.min(100,f.pct))+'%';
    card.querySelector('.fan-rpm-text').textContent=useRpm
      ?(f.min_rpm+' — '+f.cur_rpm+' — '+f.max_rpm)
      :(Math.round(f.pct)+'%');
    card.querySelector('.fa-auto').classList.toggle('active',!manual);
    card.querySelector('.fa-manual').classList.toggle('active',manual);
    card.querySelector('.fa-min').textContent=useRpm?f.min_rpm:'0%';
    card.querySelector('.fa-max').textContent=useRpm?f.max_rpm:'100%';
    // Always occupies the same layout space (opacity/pointer-events toggle
    // only, never display) — hiding it outright used to change the
    // popover's total content height, which triggers a full window resize
    // (see reportHeight/DESIRED_H) and made every chart below visibly
    // redraw at a new width, which read as "the graphs randomly changed."
    card.querySelector('.fan-rpm-row').classList.toggle('inactive', !manual);
    var slider=card.querySelector('input[type=range]');
    slider.dataset.useRpm=useRpm?'1':'0';
    slider.min=useRpm?f.min_rpm:0;
    slider.max=useRpm?Math.max(f.max_rpm,f.min_rpm+1):100;
    if(slider!==document.activeElement){
      var targetRpm=useRpm?Math.round(f.min_rpm+(f.max_rpm-f.min_rpm)*f.pct/100):Math.round(f.pct);
      slider.value=targetRpm;
      card.querySelector('.fv').textContent=manual?(useRpm?(targetRpm+' RPM'):(targetRpm+'%')):(Math.round(f.pct)+'%');
    }
  });
  Array.prototype.slice.call(container.children).forEach(function(c){
    if(!seen[c.getAttribute('data-fan-id')])c.remove();
  });
}
// Detail-Window-only visual fan curve editor. `CURVE_POINTS` is the working
// copy the user is editing; `CURVE_POINTS_SAVED` mirrors whatever's actually
// saved server-side, refreshed every tick but never used to clobber
// `CURVE_POINTS` mid-edit — only `resetCurve()` pulls from it explicitly.
var CURVE_POINTS=null, CURVE_POINTS_SAVED=null, CURVE_DRAG=-1, CURVE_LAST=-1;
var CURVE_TMIN=0, CURVE_TMAX=100;
function curveScale(cv){
  var w=cv.clientWidth||300;
  return {w:w, h:120, px:function(t){return (t-CURVE_TMIN)/(CURVE_TMAX-CURVE_TMIN)*w;}, py:function(d){return 120-(d/100)*120;}};
}
function drawCurveEditor(){
  var cv=document.getElementById('curve-canvas');
  if(!cv||!CURVE_POINTS)return;
  var s=curveScale(cv);
  if(cv.width!==s.w)cv.width=s.w;
  if(cv.height!==s.h)cv.height=s.h;
  var ctx=cv.getContext('2d');
  ctx.clearRect(0,0,s.w,s.h);
  ctx.strokeStyle='rgba(127,136,150,.15)';ctx.lineWidth=1;
  [25,50,75].forEach(function(g){var y=s.py(g);ctx.beginPath();ctx.moveTo(0,y);ctx.lineTo(s.w,y);ctx.stroke();});
  var sorted=CURVE_POINTS.slice().sort(function(a,b){return a[0]-b[0];});
  ctx.beginPath();
  sorted.forEach(function(p,i){var x=s.px(p[0]),y=s.py(p[1]);if(i===0)ctx.moveTo(x,y);else ctx.lineTo(x,y);});
  ctx.strokeStyle='#5b9dff';ctx.lineWidth=1.5;ctx.stroke();
  sorted.forEach(function(p){
    ctx.beginPath();ctx.arc(s.px(p[0]),s.py(p[1]),4,0,Math.PI*2);
    ctx.fillStyle='#5b9dff';ctx.fill();
  });
}
function curveEventToPoint(cv,e){
  var rect=cv.getBoundingClientRect();
  var t=CURVE_TMIN+((e.clientX-rect.left)/rect.width)*(CURVE_TMAX-CURVE_TMIN);
  var d=100-((e.clientY-rect.top)/rect.height)*100;
  return [Math.max(CURVE_TMIN,Math.min(CURVE_TMAX,Math.round(t))),Math.max(0,Math.min(100,Math.round(d)))];
}
function findNearestCurvePoint(cv,e){
  var rect=cv.getBoundingClientRect();
  var mx=e.clientX-rect.left, my=e.clientY-rect.top;
  var best=-1,bestDist=14;
  CURVE_POINTS.forEach(function(p,i){
    var x=(p[0]-CURVE_TMIN)/(CURVE_TMAX-CURVE_TMIN)*rect.width;
    var y=(1-p[1]/100)*rect.height;
    var dist=Math.sqrt((x-mx)*(x-mx)+(y-my)*(y-my));
    if(dist<bestDist){bestDist=dist;best=i;}
  });
  return best;
}
function initCurveEditor(){
  var cv=document.getElementById('curve-canvas');
  if(!cv||cv.dataset.bound)return;
  cv.dataset.bound='1';
  cv.addEventListener('mousedown',function(e){
    var idx=findNearestCurvePoint(cv,e);
    if(idx===-1&&CURVE_POINTS.length<8){
      CURVE_POINTS.push(curveEventToPoint(cv,e));
      idx=CURVE_POINTS.length-1;
      drawCurveEditor();
    }
    CURVE_DRAG=idx;CURVE_LAST=idx;
  });
  cv.addEventListener('mousemove',function(e){
    if(CURVE_DRAG<0)return;
    CURVE_POINTS[CURVE_DRAG]=curveEventToPoint(cv,e);
    drawCurveEditor();
  });
  window.addEventListener('mouseup',function(){CURVE_DRAG=-1;});
}
function resetCurve(){
  if(CURVE_POINTS_SAVED)CURVE_POINTS=CURVE_POINTS_SAVED.map(function(p){return p.slice();});
  drawCurveEditor();
}
function removeCurvePoint(){
  if(!CURVE_POINTS||CURVE_POINTS.length<=2)return;
  var idx=(CURVE_LAST>=0&&CURVE_LAST<CURVE_POINTS.length)?CURVE_LAST:CURVE_POINTS.length-1;
  CURVE_POINTS.splice(idx,1);
  CURVE_LAST=-1;
  drawCurveEditor();
}
function saveCurve(){
  if(!CURVE_POINTS||CURVE_POINTS.length<2)return;
  window.ipc.postMessage('savecurve:'+JSON.stringify(CURVE_POINTS));
}
function toggleLicForm(){var f=document.getElementById('lic-form');if(f)f.classList.toggle('show');}
function setChartRange(r){
  document.querySelectorAll('.range-tabs .range-tab').forEach(function(b){b.classList.toggle('active',b.dataset.range===r);});
  window.ipc.postMessage('range:'+r);
}
function setProcSort(s){
  var cpu=document.getElementById('ps-cpu'),mem=document.getElementById('ps-mem');
  if(cpu)cpu.classList.toggle('active',s==='cpu');
  if(mem)mem.classList.toggle('active',s==='mem');
  window.ipc.postMessage('procsort:'+s);
}
function quitProcess(pid,name){
  var msg=LANG==='ko'?('"'+name+'" 프로세스를 종료할까요?'):('Quit "'+name+'"?');
  if(!confirm(msg))return;
  window.ipc.postMessage('killproc:'+pid);
}
function submitLicense(){
  var inp=document.getElementById('lic-input');
  var v=inp&&inp.value.trim();
  if(!v)return;
  window.ipc.postMessage('license:'+v);
  inp.value='';
}
// Draws a filled area + line sparkline of `data` into the <canvas id=id>.
// `fixedMax` pins the y-axis (e.g. 100 for percentages); null auto-scales to the data's own peak.
// `fmt(v)` formats a raw sample for the hover tooltip.
function drawChart(id,data,color,fixedMax,fmt){
  var cv=document.getElementById(id);
  if(!cv||!data||!data.length)return;
  var w=cv.clientWidth||300,h=cv.height||28;
  if(cv.width!==w)cv.width=w;
  if(cv.height!==28)cv.height=28;
  var ctx=cv.getContext('2d');
  ctx.clearRect(0,0,w,h);
  var max=fixedMax||Math.max.apply(null,data.concat([1]));
  var n=data.length;
  function px(i){return n>1?(i/(n-1))*w:w;}
  function py(v){return h-Math.max(0,Math.min(1,v/max))*(h-2)-1;}
  ctx.beginPath();
  for(var i=0;i<n;i++){var x=px(i),y=py(data[i]);if(i===0)ctx.moveTo(x,y);else ctx.lineTo(x,y);}
  ctx.lineTo(w,h);ctx.lineTo(0,h);ctx.closePath();
  ctx.fillStyle=color+'2a';
  ctx.fill();
  ctx.beginPath();
  for(var j=0;j<n;j++){var x2=px(j),y2=py(data[j]);if(j===0)ctx.moveTo(x2,y2);else ctx.lineTo(x2,y2);}
  ctx.strokeStyle=color;ctx.lineWidth=1.25;ctx.stroke();
  cv._data=data;
  cv._fmt=fmt||function(v){return v.toFixed(1);};
  bindChartTooltip(cv);
  var stats=document.getElementById(id+'-stats');
  if(stats){
    var avgV=data.reduce(function(a,b){return a+b;},0)/n;
    var peakV=Math.max.apply(null,data);
    var avgLabel=LANG==='ko'?'평균':'avg';
    var peakLabel=LANG==='ko'?'최고':'peak';
    stats.textContent=avgLabel+' '+cv._fmt(avgV)+'   ·   '+peakLabel+' '+cv._fmt(peakV);
  }
}
// Which range's samples are on screen right now, so hover labels know the
// per-sample time step (2m = 1s/sample raw history, 1h = 1min/sample,
// 1d = 1h/sample — see RangedHistory on the Rust side).
var CHART_RANGE_LABEL='2m';
function fmtBytesPerSec(v){
  var u=['B','KB','MB','GB'],i=0;
  while(v>=1024&&i<u.length-1){v/=1024;i++;}
  return v.toFixed(1)+' '+u[i]+'/s';
}
function timeAgoLabel(i,n){
  var step=CHART_RANGE_LABEL==='1h'?60:CHART_RANGE_LABEL==='1d'?3600:1;
  var secAgo=(n-1-i)*step;
  if(LANG==='ko'){
    if(secAgo<=0)return '지금';
    if(secAgo<60)return secAgo+'초 전';
    if(secAgo<3600)return Math.round(secAgo/60)+'분 전';
    return Math.round(secAgo/3600)+'시간 전';
  }
  if(secAgo<=0)return 'now';
  if(secAgo<60)return secAgo+'s ago';
  if(secAgo<3600)return Math.round(secAgo/60)+'m ago';
  return Math.round(secAgo/3600)+'h ago';
}
// Bound once per canvas (dataset flag) since drawChart runs every tick but
// the canvas element itself is only ever created once.
function bindChartTooltip(cv){
  if(cv.dataset.tipBound)return;
  cv.dataset.tipBound='1';
  var tip=document.getElementById('chart-tip');
  if(!tip)return;
  cv.addEventListener('mousemove',function(e){
    var data=cv._data;
    if(!data||!data.length)return;
    var rect=cv.getBoundingClientRect();
    var frac=rect.width>0?Math.max(0,Math.min(1,(e.clientX-rect.left)/rect.width)):0;
    var i=Math.round(frac*(data.length-1));
    tip.textContent=cv._fmt(data[i])+'  ·  '+timeAgoLabel(i,data.length);
    tip.style.left=(e.clientX+10)+'px';
    tip.style.top=(e.clientY-26)+'px';
    tip.style.display='block';
  });
  cv.addEventListener('mouseleave',function(){tip.style.display='none';});
}
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Regression test for the v1.9.3 bug: tray-icon shows the attached menu
    /// on left-click by default, silently pre-empting our own click handler
    /// and making the popover permanently unreachable. No OS/window-server
    /// interaction needed — `TrayIconAttributes` is plain data.
    #[test]
    fn tray_attributes_route_clicks_correctly() {
        let (menu_on_left_click, menu_on_right_click) = click_routing();
        assert!(
            !menu_on_left_click,
            "left-click must NOT show the native menu — it must fall through \
             to our TrayIconEvent::Click handler so it can open the popover"
        );
        assert!(
            menu_on_right_click,
            "right-click should still show the native context menu"
        );
    }

    #[test]
    fn dashboard_html_translates_known_labels() {
        let en = dashboard_html(ResolvedLanguage::En, true);
        assert!(en.contains(">Fan control<"));
        assert!(en.contains(">Quit PeterFan<"));
        assert!(en.contains("var LANG='en';"));
        assert!(!en.contains("__LANG__"));
        assert!(!en.contains("__SHOWCURVE__"));

        let ko = dashboard_html(ResolvedLanguage::Ko, false);
        assert!(ko.contains(">팬 제어<"));
        assert!(ko.contains(">PeterFan 종료<"));
        // Auto/Manual per-fan card labels are rendered by JS at runtime
        // (LANG==='ko' ? ...), not baked into the static markup — both
        // languages ship the same script, just a different LANG value.
        assert!(ko.contains("'자동':'Auto'"));
        assert!(ko.contains("var LANG='ko';"));
        assert!(!ko.contains("__LANG__"));
        assert!(!ko.contains("__SHOWCURVE__"));
        assert!(en.contains("var SHOW_CURVE_EDITOR='1';"));
        assert!(ko.contains("var SHOW_CURVE_EDITOR='0';"));
        // Nothing English-only should survive the swap for the labels we
        // actually translate.
        assert!(!ko.contains(">Fan control<"));
        assert!(!ko.contains(">Quit PeterFan<"));
        // Both languages must still be well-formed enough to contain the
        // dynamic element IDs the JS `update()` function looks up — a typo'd
        // replacement (e.g. matching too broadly) would silently break these.
        for html in [&en, &ko] {
            assert!(html.contains(r#"id="cpu-val""#));
            assert!(html.contains(r#"id="ctl-status""#));
            assert!(html.contains(r#"id="disk-io-chart-stats""#));
            assert!(html.contains(r#"id="net-ip""#));
            assert!(html.contains(r#"id="ps-cpu""#));
            assert!(html.contains("quitProcess"));
            assert!(html.contains("renderFanCards"));
            assert!(html.contains(r#"id="fan-cards""#));
            assert!(html.contains("cmd:fanhold:"));
            assert!(html.contains("cmd:fanauto:"));
        }
    }

    #[test]
    fn profile_duty_ceilings_match_default_curves() {
        // Silent is the one built-in profile that doesn't ramp to 100% —
        // worth pinning down even though the UI no longer surfaces it
        // directly, since it's a real, deliberate difference between curves.
        assert_eq!(Profile::Silent.default_curve().duty_at(200.0), 70);
        assert_eq!(Profile::Maximum.default_curve().duty_at(200.0), 100);
    }

    #[test]
    fn parse_curve_points_accepts_a_valid_curve() {
        let curve = parse_curve_points("[[30,20],[60,50],[90,100]]").unwrap();
        assert_eq!(
            curve.points,
            vec![[30.0, 20.0], [60.0, 50.0], [90.0, 100.0]]
        );
    }

    #[test]
    fn parse_curve_points_clamps_duty_over_100() {
        let curve = parse_curve_points("[[30,20],[60,150]]").unwrap();
        assert_eq!(curve.points[1], [60.0, 100.0]);
    }

    #[test]
    fn parse_curve_points_rejects_fewer_than_two_points() {
        assert_eq!(
            parse_curve_points("[[30,20]]").unwrap_err(),
            "a curve needs at least 2 points"
        );
        assert_eq!(
            parse_curve_points("[]").unwrap_err(),
            "a curve needs at least 2 points"
        );
    }

    #[test]
    fn parse_curve_points_rejects_malformed_json() {
        assert_eq!(
            parse_curve_points("not json").unwrap_err(),
            "invalid curve data"
        );
    }

    #[test]
    fn ranged_history_rolls_up_minute_to_hour_to_day() {
        let mut h = RangedHistory::new();

        // Fewer than 60 samples: only the raw "minute" tier has data.
        for i in 0..59 {
            h.push(i as f32);
        }
        assert_eq!(h.minute.len(), 59);
        assert!(h.hour.is_empty());
        assert!(h.day.is_empty());

        // The 60th sample completes a minute — one averaged point lands in "hour".
        h.push(59.0);
        assert_eq!(h.minute.len(), 60);
        assert_eq!(h.hour.len(), 1);
        let expected_avg = (0..60).sum::<i32>() as f32 / 60.0;
        assert!((h.hour[0] - expected_avg).abs() < 0.01);
        assert!(h.day.is_empty());

        // 60 minutes' worth (3600 more samples, all zero) completes an hour.
        for _ in 0..3600 {
            h.push(0.0);
        }
        assert_eq!(h.day.len(), 1);
    }

    #[test]
    fn ranged_history_caps_each_tier_independently() {
        let mut h = RangedHistory::new();
        for i in 0..(RANGE_2M_CAP * 3) {
            h.push(i as f32);
        }
        assert_eq!(h.minute.len(), RANGE_2M_CAP, "minute tier must stay capped");
        // Most recent raw sample should be the last one pushed.
        assert_eq!(*h.minute.back().unwrap(), (RANGE_2M_CAP * 3 - 1) as f32);
    }

    #[test]
    fn apply_local_handles_hold_preset() {
        let provider = peterfan_platform::mock();
        let result = apply_local(provider.as_ref(), "hold:50");
        assert!(
            result.contains("applied locally"),
            "expected success, got: {result}"
        );
        let fans = provider.fans().unwrap();
        assert!(fans.iter().all(|f| f.duty_percent == Some(50)));
    }

    #[test]
    fn apply_local_rejects_bad_percent() {
        let provider = peterfan_platform::mock();
        let result = apply_local(provider.as_ref(), "hold:notanumber");
        assert_eq!(result, "invalid percent");
    }

    #[test]
    fn apply_local_still_handles_auto_and_profile() {
        let provider = peterfan_platform::mock();
        assert!(apply_local(provider.as_ref(), "auto").contains("applied locally"));
        assert!(apply_local(provider.as_ref(), "profile:balanced").contains("applied locally"));
        assert_eq!(apply_local(provider.as_ref(), "bogus"), "unknown command");
    }
}
