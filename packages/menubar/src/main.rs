//! `peterfan-menubar` — live system metrics in the macOS menu bar.
//!
//! The menu-bar title shows a tiny CPU sparkline + percentage. Clicking the
//! icon with the left button toggles a clean popover dashboard; right-click
//! opens native controls and diagnostics. The borderless WebView shows memory,
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
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[cfg(any(not(target_os = "macos"), test))]
use tao::dpi::PhysicalPosition;
use tao::dpi::{LogicalPosition, LogicalSize};
use tao::event::{Event, StartCause, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy, EventLoopWindowTarget};
#[cfg(not(target_os = "macos"))]
use tao::monitor::MonitorHandle;
use tao::window::{Window, WindowBuilder};

#[cfg(target_os = "macos")]
use tao::platform::macos::{ActivationPolicy, EventLoopExtMacOS, WindowExtMacOS};

#[cfg(target_os = "macos")]
use objc2_app_kit::{NSEvent, NSScreen, NSWindow};
#[cfg(target_os = "macos")]
use objc2_foundation::{MainThreadMarker, NSPoint, NSRect, NSSize};

use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use tray_icon::{
    Icon, MouseButton, MouseButtonState, Rect, TrayIcon, TrayIconAttributes, TrayIconEvent,
};
use wry::{WebView, WebViewBuilder, RGBA};

use peterfan_core::config::{
    CustomCurveConfig, Language, MenubarDisplay, ResolvedLanguage, TemperatureSource,
};
use peterfan_core::error::CoreError;
use peterfan_core::metrics::ProcSort;
use peterfan_core::profile::Profile;
use peterfan_core::thermals::{representative_temperature_c, safety_temperature_c};
use peterfan_core::types::SensorKind;
#[cfg(test)]
use peterfan_core::types::SensorSource;
use peterfan_core::types::{Celsius, Fan, TempSensor};
use peterfan_core::{HardwareProvider, SystemMonitor};

const REFRESH: Duration = Duration::from_secs(1);
const TEMPERATURE_REFRESH: Duration = Duration::from_secs(2);
const FAN_REFRESH: Duration = Duration::from_secs(1);
// The runner should be nearly still at idle and unmistakably fast under load.
// Frames are pre-rendered, so each tick only swaps a cached status-item image.
const RUNNER_MIN_INTERVAL: Duration = Duration::from_millis(140);
const RUNNER_MAX_INTERVAL: Duration = Duration::from_millis(1000);
const POPOVER_PREWARM_DELAY: Duration = Duration::from_millis(1200);
const POPOVER_SHOW_DELAY: Duration = Duration::from_millis(35);
const DASHBOARD_OPEN_GRACE: Duration = Duration::from_millis(900);
const DASHBOARD_SLOW_REFRESH: Duration = Duration::from_secs(3);
const DASHBOARD_SLOW_OPEN_GRACE: Duration = Duration::from_millis(450);
const ALL_TEMP_REFRESH: Duration = Duration::from_secs(10);
const DAEMON_REFRESH: Duration = Duration::from_secs(2);
const TEMPERATURE_STALE_AFTER: Duration = Duration::from_secs(6);
const ALL_TEMPERATURE_STALE_AFTER: Duration = Duration::from_secs(30);
const CONTROL_CONFIRM_REFRESH: Duration = Duration::from_millis(200);
const CONTROL_CONFIRM_WINDOW: Duration = Duration::from_secs(8);
const SINGLE_INSTANCE_LOCK_BASENAME: &str = "kr.co.uulab.peterfan.menubar";
const MENUBAR_LOG_MAX_BYTES: u64 = 512 * 1024;
const DASHBOARD_BACKGROUND: RGBA = (27, 27, 29, 255);
/// Samples kept for the menu-bar runner icon (always shows the short-term
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
static DAEMON_VERSION_CACHE: Mutex<Option<(Instant, Option<String>)>> = Mutex::new(None);
static MENUBAR_LOG_LOCK: Mutex<()> = Mutex::new(());

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

const POPOVER_W: f64 = 440.0;
/// Fixed popover height. Route-specific overflow belongs inside `.main-pane`;
/// resizing the native window on every content change makes the action rail
/// feel unstable and can move the popover away from the clicked menu-bar item.
const POPOVER_H: f64 = 520.0;

/// Set by the popover's Quit button (via WebView IPC), polled by the loop.
static QUIT: AtomicBool = AtomicBool::new(false);
/// Set by the popover's "Open Detailed Window" link, polled by the loop
/// (opening a window needs `&mut App` + the event-loop target, neither of
/// which the IPC handler closure has access to).
static OPEN_DETAIL: AtomicBool = AtomicBool::new(false);
/// Control commands queued by popover buttons (`auto`, `profile:gaming`).
static PENDING: Mutex<Vec<String>> = Mutex::new(Vec::new());
/// Last control result, shown in the popover.
static STATUS: Mutex<String> = Mutex::new(String::new());
/// A completed fan command invalidates the cached daemon snapshot. The event
/// loop consumes this on its next wake so confirmed UI state does not wait for
/// the normal two-second daemon refresh interval.
static CONTROL_REFRESH_REQUESTED: AtomicBool = AtomicBool::new(false);
const FAN_ACTION_LOG_MAX: usize = 12;
static FAN_ACTION_LOG: Mutex<VecDeque<serde_json::Value>> = Mutex::new(VecDeque::new());
/// Guards `install_fan_control()` process-wide. The popover and Detail
/// Window each track their own "installing…" button state in per-webview JS
/// (`FAN_CONTROL_FIX_PENDING`), which doesn't stop both windows from firing
/// the install thread within the same tick and stacking two macOS
/// admin-password dialogs.
static INSTALL_FAN_CONTROL_IN_FLIGHT: AtomicBool = AtomicBool::new(false);
#[cfg(target_os = "macos")]
static LOGIN_ITEM_TOGGLE_IN_FLIGHT: AtomicBool = AtomicBool::new(false);
/// Shadow of `apply_local`'s per-fan pins, consulted only when no daemon is
/// reachable (`daemon_temps_json()` returns `None` in that case, since it's
/// a daemon IPC query). Without this, pinning a fan via a direct SMC write
/// leaves the UI reporting "Auto" on the very next tick even though the fan
/// is genuinely still pinned in hardware.
static LOCAL_FAN_OVERRIDES: Mutex<Option<std::collections::HashMap<String, u8>>> = Mutex::new(None);

struct BackgroundRead<T> {
    in_flight: AtomicBool,
    completed: Mutex<Option<T>>,
}

impl<T> Default for BackgroundRead<T> {
    fn default() -> Self {
        Self {
            in_flight: AtomicBool::new(false),
            completed: Mutex::new(None),
        }
    }
}

impl<T: Send + 'static> BackgroundRead<T> {
    fn start<F>(self: &Arc<Self>, task: F) -> bool
    where
        F: FnOnce() -> Option<T> + Send + 'static,
    {
        if self
            .in_flight
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return false;
        }
        let state = Arc::clone(self);
        std::thread::spawn(move || {
            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(task)) {
                Ok(Some(value)) => {
                    *state.completed.lock().expect("background read poisoned") = Some(value);
                }
                Ok(None) => {}
                Err(_) => log_menubar_event("background hardware read panicked"),
            }
            state.in_flight.store(false, Ordering::Release);
        });
        true
    }

    fn take(&self) -> Option<T> {
        self.completed
            .lock()
            .expect("background read poisoned")
            .take()
    }
}

struct TimedSample<T> {
    values: T,
    sampled_at: Instant,
    sampled_at_unix_ms: u64,
}

/// IDs of the tray context-menu items so we can identify them in MenuEvent.
struct TrayMenu {
    auto: tray_icon::menu::MenuId,
    rules: tray_icon::menu::MenuId,
    profiles: Vec<(String, tray_icon::menu::MenuId)>,
    quit: tray_icon::menu::MenuId,
    /// "Display" submenu — number / cat / both.
    display_items: Vec<(MenubarDisplay, tray_icon::menu::CheckMenuItem)>,
    /// "CPU Temperature Source" submenu — which sensor family feeds the
    /// headline/menu-bar temperature.
    temperature_source_items: Vec<(TemperatureSource, tray_icon::menu::CheckMenuItem)>,
    /// "Fan Speed" submenu — direct RPM presets, mapped to the same command
    /// strings `execute_control` already understands ("auto", "hold:<pct>").
    fan_speed_items: Vec<(String, tray_icon::menu::MenuId)>,
    /// One-time privileged daemon install — lets fan control work without a
    /// terminal (macOS only; `None` elsewhere).
    #[cfg(target_os = "macos")]
    enable_fan_control: tray_icon::menu::MenuId,
    check_updates: tray_icon::menu::MenuId,
    open_detail: tray_icon::menu::MenuId,
    open_diagnostics: tray_icon::menu::MenuId,
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
    auto: &'static str,
    rules: &'static str,
    profile_silent: &'static str,
    profile_balanced: &'static str,
    profile_gaming: &'static str,
    profile_performance: &'static str,
    profile_maximum: &'static str,
    open_detail: &'static str,
    open_diagnostics: &'static str,
    check_updates: &'static str,
    quit: &'static str,
    menu_bar_style: &'static str,
    temperature_source: &'static str,
    fan_speed: &'static str,
    language: &'static str,
    style_number: &'static str,
    style_graph: &'static str,
    style_both: &'static str,
}

fn strings(lang: ResolvedLanguage) -> L10n {
    match lang {
        ResolvedLanguage::En => L10n {
            enable_fan_control: "Enable Fan Control (One-Time Setup)…",
            auto: "Auto (OS-managed)",
            rules: "Rules",
            profile_silent: "Silent",
            profile_balanced: "Balanced",
            profile_gaming: "Gaming",
            profile_performance: "Performance",
            profile_maximum: "Maximum",
            open_detail: "Open Detailed Window…",
            open_diagnostics: "Open Diagnostic Log…",
            check_updates: "Check for Updates…",
            quit: "Quit PeterFan",
            menu_bar_style: "Menu Bar Style",
            temperature_source: "Dashboard Temperature",
            fan_speed: "Fan Speed",
            language: "Language",
            style_number: "Number",
            style_graph: "Cat",
            style_both: "Number + Cat",
        },
        ResolvedLanguage::Ko => L10n {
            enable_fan_control: "팬 제어 활성화 (최초 1회 설정)…",
            auto: "자동 (OS 관리)",
            rules: "규칙",
            profile_silent: "무음",
            profile_balanced: "균형",
            profile_gaming: "게이밍",
            profile_performance: "고성능",
            profile_maximum: "최대",
            open_detail: "상세 창 열기…",
            open_diagnostics: "진단 로그 열기…",
            check_updates: "업데이트 확인…",
            quit: "PeterFan 종료",
            menu_bar_style: "메뉴 막대 스타일",
            temperature_source: "대시보드 온도",
            fan_speed: "팬 속도",
            language: "언어",
            style_number: "숫자",
            style_graph: "고양이",
            style_both: "숫자 + 고양이",
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
    display: MenubarDisplay,
    temperature_source: TemperatureSource,
    critical_temp_c: f32,
    language: Language,
    tray: Option<TrayIcon>,
    tray_menu: Option<TrayMenu>,
    window: Option<Window>,
    webview: Option<WebView>,
    popover_visible: bool,
    popover_show_at: Option<Instant>,
    /// `tray-icon` normally sends both Down and Up. Open on Down for macOS
    /// menu bars that swallow Up, then consume the matching Up event.
    left_button_down_seen: bool,
    /// A persistent, resizable, normal-chrome window with the same
    /// dashboard content — for "leave it open while I work" use, unlike the
    /// dropdown popover which hides the moment focus moves elsewhere.
    /// Created lazily on first request.
    detail_window: Option<Window>,
    detail_webview: Option<WebView>,
    /// Short-term (2-minute) history for the menu-bar runner icon only — the
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
    dashboard_slow_cache: DashboardSlowCache,
    next_dashboard_slow_refresh: Instant,
    /// Small animation frame for the RunCat-style menu-bar character. It
    /// advances on each refresh, with bigger CPU load taking larger strides.
    runner_frame: u8,
    runner_cpu_pct: f32,
    runner_icons: Vec<Icon>,
    last_runner_icon: Option<usize>,
    temperature_cache: Vec<TempSensor>,
    temperature_sampled_at: Option<Instant>,
    temperature_sampled_at_unix_ms: Option<u64>,
    next_temperature_refresh: Instant,
    temperature_read: Arc<BackgroundRead<TimedSample<Vec<TempSensor>>>>,
    fan_cache: Vec<Fan>,
    next_fan_refresh: Instant,
    fan_read: Arc<BackgroundRead<Vec<Fan>>>,
    all_temp_rows_cache: Vec<serde_json::Value>,
    all_temp_sampled_at: Option<Instant>,
    all_temp_sampled_at_unix_ms: Option<u64>,
    next_all_temp_refresh: Instant,
    all_temp_read: Arc<BackgroundRead<TimedSample<Vec<TempSensor>>>>,
    daemon_json_cache: Option<serde_json::Value>,
    next_daemon_refresh: Instant,
    update_install_result: Option<peterfan_platform::updater::UpdateInstallResult>,
    next_update_result_refresh: Instant,
    control_confirm_until: Option<Instant>,
}

struct DashboardSlowCache {
    proc_sort: ProcSort,
    procs: Vec<serde_json::Value>,
    disk_pct: f32,
    disk_text: String,
    disk_sub: String,
    disk_io_present: bool,
    disk_io_sub: String,
    disk_io_rate: f32,
    power_w: Option<f32>,
    batt_present: bool,
    batt_pct: f32,
    batt_text: String,
    batt_sub: String,
    curve_points: Vec<[f32; 2]>,
}

impl Default for DashboardSlowCache {
    fn default() -> Self {
        Self {
            proc_sort: ProcSort::Cpu,
            procs: Vec::new(),
            disk_pct: 0.0,
            disk_text: String::new(),
            disk_sub: String::new(),
            disk_io_present: false,
            disk_io_sub: String::new(),
            disk_io_rate: 0.0,
            power_w: None,
            batt_present: false,
            batt_pct: 0.0,
            batt_text: String::new(),
            batt_sub: String::new(),
            curve_points: default_curve_points(),
        }
    }
}

/// Persist the menu-bar appearance. The metric itself is intentionally fixed
/// to CPU Core Average so every surface reports the same number.
fn save_menubar_display(display: MenubarDisplay) {
    let mut cfg = peterfan_platform::config::load();
    cfg.menubar.display = display;
    let _ = peterfan_platform::config::save(&cfg);
}

fn save_temperature_source(source: TemperatureSource) {
    let mut cfg = peterfan_platform::config::load();
    cfg.menubar.temperature_source = source;
    let _ = peterfan_platform::config::save(&cfg);
}

/// Persist the UI language choice so it survives a relaunch.
fn save_language(language: Language) {
    let mut cfg = peterfan_platform::config::load();
    cfg.menubar.language = language;
    let _ = peterfan_platform::config::save(&cfg);
}

fn hottest_temperature(temps: &[TempSensor]) -> Option<&TempSensor> {
    temps.iter().max_by(|a, b| {
        a.value
            .0
            .partial_cmp(&b.value.0)
            .unwrap_or(std::cmp::Ordering::Equal)
    })
}

struct SelectedTemperature {
    id: String,
    value: f32,
    label_hint: Option<&'static str>,
}

fn average_cpu_non_hot(temps: &[TempSensor]) -> Option<f32> {
    let mut values = Vec::new();
    for temp in temps {
        if temp.kind != SensorKind::Cpu || temp.id.contains("hot") {
            continue;
        }

        if temp.id.contains("proximity")
            || temp.id.contains("airflow")
            || temp.id.contains("ambient")
            || temp.id.contains("board")
            || temp.id.contains("memory")
        {
            continue;
        }

        if temp.value.0.is_nan() {
            continue;
        }

        values.push(temp.value.0);
    }
    if values.is_empty() {
        return None;
    }
    Some(values.iter().sum::<f32>() / values.len() as f32)
}

fn best_cpu_average(temps: &[TempSensor]) -> Option<SelectedTemperature> {
    if let Some(cpu_die) = temps
        .iter()
        .find(|t| t.id == "cpu.die" && t.kind == SensorKind::Cpu && !t.id.contains("hot"))
    {
        return Some(SelectedTemperature {
            id: cpu_die.id.clone(),
            value: cpu_die.value.0,
            label_hint: Some("CPU average"),
        });
    }

    let stable_candidate = temps
        .iter()
        .filter(|t| t.kind == SensorKind::Cpu && !t.id.contains("hot"))
        .filter(|t| {
            matches!(
                t.id.as_str(),
                "cpu.smc.die" | "cpu.smc.aggregate" | "cpu.smc.summary"
            )
        })
        .max_by(|a, b| {
            a.value
                .0
                .partial_cmp(&b.value.0)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
    if let Some(candidate) = stable_candidate {
        return Some(SelectedTemperature {
            id: candidate.id.clone(),
            value: candidate.value.0,
            label_hint: Some("CPU average"),
        });
    }

    if let Some(iohid) = temps
        .iter()
        .find(|t| matches!(t.id.as_str(), "cpu.iohid.tdie" | "cpu.iohid.cpu"))
    {
        return Some(SelectedTemperature {
            id: iohid.id.clone(),
            value: iohid.value.0,
            label_hint: Some("CPU average"),
        });
    }

    average_cpu_non_hot(temps)
        .map(|avg| SelectedTemperature {
            id: "cpu.die".to_string(),
            value: avg,
            label_hint: Some("CPU average"),
        })
        .or_else(|| hottest_cpu_temperature(temps))
}

fn hottest_cpu_temperature(temps: &[TempSensor]) -> Option<SelectedTemperature> {
    temps
        .iter()
        .filter(|t| t.kind == SensorKind::Cpu)
        .max_by(|a, b| {
            a.value
                .0
                .partial_cmp(&b.value.0)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|t| SelectedTemperature {
            id: t.id.clone(),
            value: t.value.0,
            label_hint: Some("hottest"),
        })
}

#[cfg(test)]
fn display_temperature(temps: &[TempSensor]) -> Option<&TempSensor> {
    temps
        .iter()
        .find(|t| t.id == "cpu.die")
        .or_else(|| hottest_temperature(temps))
}

fn primary_menu_temperature(
    temps: &[TempSensor],
    source: TemperatureSource,
) -> Option<SelectedTemperature> {
    let by_id = |id: &'static str| {
        temps
            .iter()
            .find(|t| t.id == id)
            .map(|t| SelectedTemperature {
                id: t.id.clone(),
                value: t.value.0,
                label_hint: None,
            })
    };

    match source {
        TemperatureSource::CoreAverage => return best_cpu_average(temps),
        TemperatureSource::IohidTdie => {
            if let Some(v) = by_id("cpu.iohid.tdie").or_else(|| by_id("cpu.iohid.cpu")) {
                return Some(v);
            }
        }
        TemperatureSource::SmcSummary => {
            if let Some(v) = by_id("cpu.smc.summary") {
                return Some(v);
            }
        }
        TemperatureSource::SmcAggregate => {
            if let Some(v) = by_id("cpu.smc.aggregate") {
                return Some(v);
            }
        }
        TemperatureSource::Hottest => {
            if let Some(v) = by_id("cpu.die.hot") {
                return Some(v);
            }
            if let Some(v) = temps
                .iter()
                .find(|t| t.kind == SensorKind::Cpu && t.id.contains("hot"))
                .map(|t| SelectedTemperature {
                    id: "cpu.die.hot".to_string(),
                    value: t.value.0,
                    label_hint: None,
                })
            {
                return Some(v);
            }
        }
    }

    if source == TemperatureSource::Hottest {
        hottest_cpu_temperature(temps)
    } else {
        best_cpu_average(temps)
    }
    .or_else(|| {
        hottest_temperature(temps).map(|t| SelectedTemperature {
            id: "hottest".to_string(),
            value: t.value.0,
            label_hint: Some("hottest"),
        })
    })
}

fn display_temperature_source(
    lang: ResolvedLanguage,
    sensor: Option<&SelectedTemperature>,
) -> String {
    let Some(sensor) = sensor else {
        return String::new();
    };
    if sensor.id == "cpu.smc.die" {
        match lang {
            ResolvedLanguage::Ko => "CPU 다이".to_string(),
            ResolvedLanguage::En => "CPU die".to_string(),
        }
    } else if sensor.id == "cpu.iohid.cpu" || sensor.id == "cpu.iohid.cpu.hot" {
        match lang {
            ResolvedLanguage::Ko => "CPU 다이".to_string(),
            ResolvedLanguage::En => "CPU IOHID CPU".to_string(),
        }
    } else if sensor.id == "cpu.iohid.tdie" {
        match lang {
            ResolvedLanguage::Ko => "CPU 다이".to_string(),
            ResolvedLanguage::En => "CPU Tdie".to_string(),
        }
    } else if sensor.id == "cpu.smc.aggregate" {
        match lang {
            ResolvedLanguage::Ko => "SMC 집계".to_string(),
            ResolvedLanguage::En => "SMC aggregate".to_string(),
        }
    } else if sensor.id == "cpu.smc.summary" {
        match lang {
            ResolvedLanguage::Ko => "SMC 요약".to_string(),
            ResolvedLanguage::En => "SMC summary".to_string(),
        }
    } else if sensor.id == "cpu.die" && sensor.label_hint != Some("hottest") {
        match lang {
            ResolvedLanguage::Ko => "CPU Core Average".to_string(),
            ResolvedLanguage::En => "CPU Core Average".to_string(),
        }
    } else if sensor.label_hint == Some("hottest") || sensor.id.contains("hot") {
        match lang {
            ResolvedLanguage::Ko => "최고".to_string(),
            ResolvedLanguage::En => "hottest".to_string(),
        }
    } else {
        sensor.id.clone()
    }
}

fn display_temperature_source_for_temps(
    lang: ResolvedLanguage,
    _temps: &[TempSensor],
    sensor: Option<&SelectedTemperature>,
) -> String {
    display_temperature_source(lang, sensor)
}

fn temperature_source_label(lang: ResolvedLanguage, source: TemperatureSource) -> &'static str {
    match (lang, source) {
        (ResolvedLanguage::Ko, TemperatureSource::CoreAverage) => "CPU Core Average",
        (ResolvedLanguage::Ko, TemperatureSource::IohidTdie) => "IOHID tdie",
        (ResolvedLanguage::Ko, TemperatureSource::SmcSummary) => "SMC summary",
        (ResolvedLanguage::Ko, TemperatureSource::SmcAggregate) => "SMC aggregate",
        (ResolvedLanguage::Ko, TemperatureSource::Hottest) => "CPU Hottest",
        (ResolvedLanguage::En, TemperatureSource::CoreAverage) => "CPU Core Average",
        (ResolvedLanguage::En, TemperatureSource::IohidTdie) => "IOHID tdie",
        (ResolvedLanguage::En, TemperatureSource::SmcSummary) => "SMC summary",
        (ResolvedLanguage::En, TemperatureSource::SmcAggregate) => "SMC aggregate",
        (ResolvedLanguage::En, TemperatureSource::Hottest) => "CPU Hottest",
    }
}

fn temperature_row_label(lang: ResolvedLanguage, sensor: &TempSensor) -> String {
    match sensor.id.as_str() {
        "cpu.smc.die" => match lang {
            ResolvedLanguage::Ko => "CPU 다이".to_string(),
            ResolvedLanguage::En => "CPU die".to_string(),
        },
        "cpu.iohid.cpu" => "CPU IOHID CPU".to_string(),
        "cpu.iohid.cpu.hot" => "CPU IOHID hottest".to_string(),
        "cpu.die" => match lang {
            ResolvedLanguage::Ko => "CPU Core Average".to_string(),
            ResolvedLanguage::En => "CPU Core Average".to_string(),
        },
        "cpu.die.hot" => match lang {
            ResolvedLanguage::Ko => "CPU Core Hottest".to_string(),
            ResolvedLanguage::En => "CPU Core Hottest".to_string(),
        },
        "cpu.iohid.tdie" => "CPU IOHID tdie".to_string(),
        "cpu.iohid.tdie.hot" => "CPU IOHID tdie hottest".to_string(),
        "cpu.smc.summary" => "CPU SMC summary".to_string(),
        "cpu.smc.aggregate" => "CPU SMC aggregate".to_string(),
        "cpu.smc.hotspot" => "CPU SMC hotspot average".to_string(),
        "cpu.smc.hotspot.hot" => "CPU SMC hotspot hottest".to_string(),
        _ => sensor.label.clone(),
    }
}

fn raw_temperature_row_label(sensor: &TempSensor) -> String {
    format!("{} · {}", sensor.label, sensor.id)
}

fn sensor_group_label(lang: ResolvedLanguage, kind: SensorKind) -> &'static str {
    match (lang, kind) {
        (_, SensorKind::Cpu) => "CPU",
        (_, SensorKind::Gpu) => "GPU",
        (ResolvedLanguage::Ko, SensorKind::Memory) => "메모리",
        (ResolvedLanguage::Ko, SensorKind::Storage) => "저장장치",
        (ResolvedLanguage::Ko, SensorKind::Mainboard) => "메인보드",
        (ResolvedLanguage::Ko, SensorKind::Battery) => "배터리",
        (ResolvedLanguage::Ko, SensorKind::Other) => "기타",
        (ResolvedLanguage::En, SensorKind::Memory) => "Memory",
        (ResolvedLanguage::En, SensorKind::Storage) => "Storage",
        (ResolvedLanguage::En, SensorKind::Mainboard) => "Mainboard",
        (ResolvedLanguage::En, SensorKind::Battery) => "Battery",
        (ResolvedLanguage::En, SensorKind::Other) => "Other",
    }
}

fn setup_tone(daemon_running: bool, daemon_update_needed: bool) -> &'static str {
    if daemon_update_needed {
        "warn"
    } else if daemon_running {
        "ok"
    } else {
        "warn"
    }
}

fn setup_title(
    lang: ResolvedLanguage,
    daemon_running: bool,
    daemon_update_needed: bool,
) -> &'static str {
    match (lang, daemon_update_needed, daemon_running) {
        (ResolvedLanguage::Ko, true, _) => "팬 제어 재설치",
        (ResolvedLanguage::Ko, false, true) => "준비 완료",
        (ResolvedLanguage::Ko, false, false) => "설정 필요",
        (ResolvedLanguage::En, true, _) => "Reinstall Fan Control",
        (ResolvedLanguage::En, false, true) => "Ready",
        (ResolvedLanguage::En, false, false) => "Setup needed",
    }
}

fn setup_detail(
    lang: ResolvedLanguage,
    daemon_running: bool,
    daemon_update_needed: bool,
    daemon_version: Option<&str>,
) -> String {
    match (lang, daemon_update_needed, daemon_running) {
        (ResolvedLanguage::Ko, true, _) => format!(
            "데몬 v{} → v{} · {}",
            daemon_version.unwrap_or("unknown"),
            peterfan_platform::MIN_REQUIRED_DAEMON_VERSION,
            daemon_reinstall_hint(lang, daemon_version)
        ),
        (ResolvedLanguage::Ko, false, true) => {
            format!(
                "데몬 v{} · 추가 승인 없음",
                daemon_version.unwrap_or("unknown")
            )
        }
        (ResolvedLanguage::Ko, false, false) => "팬 제어 미설정 · 최초 승인 1회".to_string(),
        (ResolvedLanguage::En, true, _) => format!(
            "daemon v{} → v{} · {}",
            daemon_version.unwrap_or("unknown"),
            peterfan_platform::MIN_REQUIRED_DAEMON_VERSION,
            daemon_reinstall_hint(lang, daemon_version)
        ),
        (ResolvedLanguage::En, false, true) => {
            format!(
                "daemon v{} · no additional approval",
                daemon_version.unwrap_or("unknown")
            )
        }
        (ResolvedLanguage::En, false, false) => {
            "fan control not set up · one initial approval".to_string()
        }
    }
}

fn daemon_reinstall_hint(lang: ResolvedLanguage, daemon_version: Option<&str>) -> &'static str {
    let quiet = daemon_version.is_some_and(peterfan_platform::daemon_self_reinstall_supported);
    match (lang, quiet) {
        (ResolvedLanguage::Ko, true) => "조용히 가능",
        (ResolvedLanguage::Ko, false) => "이번 한 번 승인 필요",
        (ResolvedLanguage::En, true) => "no prompt",
        (ResolvedLanguage::En, false) => "one approval this time",
    }
}

fn cached_installed_daemon_version() -> Option<String> {
    let now = Instant::now();
    let mut cache = DAEMON_VERSION_CACHE
        .lock()
        .expect("daemon version cache poisoned");
    if let Some((at, version)) = &*cache {
        if now.duration_since(*at) < Duration::from_secs(30) {
            return version.clone();
        }
    }
    let version = peterfan_platform::installed_daemon_version();
    *cache = Some((now, version.clone()));
    version
}

fn clear_daemon_version_cache() {
    *DAEMON_VERSION_CACHE
        .lock()
        .expect("daemon version cache poisoned") = None;
}

fn daemon_control_usable(daemon_version: Option<&str>) -> bool {
    daemon_version.is_some_and(|version| !peterfan_platform::daemon_update_required(version))
}

fn clear_daemon_update_prompt_state(cfg: &mut peterfan_core::config::Config) {
    cfg.menubar.daemon_update_prompt_dismissed_for = None;
    cfg.menubar.daemon_update_prompt_snoozed_until_unix = None;
}

fn persist_clear_daemon_update_prompt_state() {
    let mut cfg = peterfan_platform::config::load();
    clear_daemon_update_prompt_state(&mut cfg);
    let _ = peterfan_platform::config::save(&cfg);
}

fn active_profile_from_mode(mode: &str) -> Option<&str> {
    let mode = mode.split_whitespace().next().unwrap_or(mode);
    mode.strip_prefix("manual:")
        .or_else(|| mode.strip_prefix("rules:"))
        .or_else(|| mode.strip_prefix("profile:"))
        .filter(|profile| !profile.is_empty())
}

fn active_control_mode_from_mode(mode: &str) -> &'static str {
    let mode = mode.split_whitespace().next().unwrap_or(mode);
    if mode == "auto" {
        "auto"
    } else if mode.starts_with("manual:")
        || mode.starts_with("rules:")
        || mode.starts_with("profile:")
    {
        "profile"
    } else if mode.starts_with("hold:") {
        "hold"
    } else {
        ""
    }
}

fn resolved_active_control_mode(mode: Option<&str>, has_manual_overrides: bool) -> &'static str {
    let reported = mode.map(active_control_mode_from_mode).unwrap_or_default();
    if !reported.is_empty() {
        reported
    } else if has_manual_overrides {
        "hold"
    } else {
        "auto"
    }
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
    if daemon_control_usable(cached_installed_daemon_version().as_deref())
        && peterfan_platform::ipc::send_command("reload").is_some()
    {
        let _ = peterfan_platform::ipc::send_command("profile custom");
        return "custom curve saved".into();
    }
    if provider.capabilities().control_fans {
        let temps = provider.temperatures().unwrap_or_default();
        let temp = representative_temperature_c(&temps).unwrap_or(0.0);
        let duty = fan_curve.duty_at(temp);
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

fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u64::MAX as u128) as u64)
        .unwrap_or(0)
}

fn menubar_log_path() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .map(|home| home.join("Library/Logs/PeterFan/menubar.log"))
    }
    #[cfg(not(target_os = "macos"))]
    {
        peterfan_platform::config::path()?
            .parent()
            .map(|dir| dir.join("menubar.log"))
    }
}

fn log_menubar_event(message: &str) {
    let Some(path) = menubar_log_path() else {
        return;
    };
    let _guard = MENUBAR_LOG_LOCK.lock().expect("menubar log poisoned");
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if path
        .metadata()
        .is_ok_and(|metadata| metadata.len() >= MENUBAR_LOG_MAX_BYTES)
    {
        let previous = path.with_extension("previous.log");
        let _ = std::fs::remove_file(&previous);
        let _ = std::fs::rename(&path, previous);
    }
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(
            file,
            "{} v{} {}",
            now_unix_ms(),
            env!("CARGO_PKG_VERSION"),
            message
        );
    }
}

fn open_menubar_log() {
    log_menubar_event("diagnostic log requested");
    let Some(path) = menubar_log_path() else {
        return;
    };
    #[cfg(target_os = "macos")]
    let mut command = std::process::Command::new("open");
    #[cfg(target_os = "windows")]
    let mut command = std::process::Command::new("explorer.exe");
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let mut command = std::process::Command::new("xdg-open");
    let _ = command.arg(path).spawn();
}

fn sample_age(sampled_at: Option<Instant>, now: Instant) -> Option<Duration> {
    sampled_at.map(|sampled| now.saturating_duration_since(sampled))
}

fn sample_is_stale(sampled_at: Option<Instant>, now: Instant, limit: Duration) -> bool {
    sample_age(sampled_at, now)
        .map(|age| age > limit)
        .unwrap_or(true)
}

fn fan_action_log_path() -> Option<PathBuf> {
    peterfan_platform::config::path()?
        .parent()
        .map(|dir| dir.join("fan-actions.json"))
}

fn load_fan_action_log() {
    let Some(path) = fan_action_log_path() else {
        return;
    };
    let Ok(bytes) = std::fs::read(path) else {
        return;
    };
    let Ok(entries) = serde_json::from_slice::<Vec<serde_json::Value>>(&bytes) else {
        return;
    };
    *FAN_ACTION_LOG.lock().expect("fan action log poisoned") = entries
        .into_iter()
        .filter(|entry| entry.is_object())
        .take(FAN_ACTION_LOG_MAX)
        .collect();
}

fn fan_action_log_snapshot() -> Vec<serde_json::Value> {
    FAN_ACTION_LOG
        .lock()
        .expect("fan action log poisoned")
        .iter()
        .cloned()
        .collect()
}

fn record_fan_action(action: &str, result: &str, ok: bool) {
    let result = result.chars().take(180).collect::<String>();
    let mut log = FAN_ACTION_LOG.lock().expect("fan action log poisoned");
    log.push_front(serde_json::json!({
        "at": now_unix(),
        "action": action,
        "result": result,
        "ok": ok,
    }));
    log.truncate(FAN_ACTION_LOG_MAX);
    let snapshot = log.iter().cloned().collect::<Vec<_>>();
    drop(log);

    let Some(path) = fan_action_log_path() else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(bytes) = serde_json::to_vec_pretty(&snapshot) {
        let _ = std::fs::write(path, bytes);
    }
}

fn control_action_label(cmd: &str) -> String {
    if let Some(profile) = cmd.strip_prefix("profile:") {
        format!("profile {profile}")
    } else if let Some(rest) = cmd.strip_prefix("fanhold:") {
        format!("fan {rest}")
    } else if let Some(fan) = cmd.strip_prefix("fanauto:") {
        format!("fan {fan} auto")
    } else {
        cmd.to_string()
    }
}

fn single_instance_lock_path(mock: bool) -> PathBuf {
    #[cfg(unix)]
    let suffix = unsafe {
        // SAFETY: geteuid has no preconditions and only reads process state.
        libc::geteuid()
    };
    #[cfg(not(unix))]
    let suffix = 0u32;

    let mode = if mock { "mock" } else { "app" };
    std::env::temp_dir().join(format!(
        "{SINGLE_INSTANCE_LOCK_BASENAME}.{mode}.{suffix}.lock"
    ))
}

fn acquire_single_instance_lock(mock: bool) -> Result<File, String> {
    acquire_single_instance_lock_at(&single_instance_lock_path(mock))
}

#[cfg(unix)]
fn acquire_single_instance_lock_at(path: &Path) -> Result<File, String> {
    use std::os::fd::AsRawFd;

    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path)
        .map_err(|e| format!("could not open {}: {e}", path.display()))?;
    let rc = unsafe {
        // SAFETY: the file descriptor is valid for the lifetime of `file`.
        libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB)
    };
    if rc == 0 {
        Ok(file)
    } else {
        Err(format!(
            "could not lock {}: {}",
            path.display(),
            std::io::Error::last_os_error()
        ))
    }
}

#[cfg(not(unix))]
fn acquire_single_instance_lock_at(path: &Path) -> Result<File, String> {
    OpenOptions::new()
        .create_new(true)
        .read(true)
        .write(true)
        .open(path)
        .map_err(|e| format!("could not create {}: {e}", path.display()))
}

/// Push a sample, dropping the oldest once past [`HIST_CAP`].
fn push_hist<T>(buf: &mut VecDeque<T>, v: T) {
    push_capped(buf, v, HIST_CAP);
}

fn normalized_fan_speed_percent(fan: &Fan) -> Option<f32> {
    match (fan.min_rpm, fan.max_rpm) {
        (Some(min), Some(max)) if max > min => Some(
            (fan.rpm.saturating_sub(min) as f32 / (max - min) as f32 * 100.0).clamp(0.0, 100.0),
        ),
        _ => None,
    }
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
        println!("{}", help_text());
        return;
    }
    if let Some(arg) = unsupported_menubar_arg(&args) {
        eprintln!(
            "PeterFan.app contains the menu-bar app, not the peterfan CLI.\n\
             Unsupported argument: {arg}\n\n\
             For CLI commands such as doctor, status, or update, use the \
             `peterfan` binary from the release tarball."
        );
        std::process::exit(2);
    }
    let use_mock = args.iter().any(|a| a == "--mock");
    let _single_instance = match acquire_single_instance_lock(use_mock) {
        Ok(lock) => lock,
        Err(e) => {
            log_menubar_event(&format!("startup rejected: single instance lock: {e}"));
            eprintln!("PeterFan is already running ({e}).");
            return;
        }
    };
    log_menubar_event(&format!("startup mock={use_mock}"));
    load_fan_action_log();

    let saved_config = peterfan_platform::config::load();
    let critical_temp_c = saved_config.critical_temp_c;
    let saved = saved_config.menubar;
    let display = args
        .iter()
        .position(|a| a == "--display")
        .and_then(|i| args.get(i + 1))
        .and_then(|v| MenubarDisplay::parse(v))
        .unwrap_or(saved.display);
    let temperature_source = saved.temperature_source;
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

    #[allow(unused_mut)]
    let mut event_loop = EventLoopBuilder::<()>::new().build();
    #[cfg(target_os = "macos")]
    event_loop.set_activation_policy(ActivationPolicy::Accessory);
    let event_proxy = event_loop.create_proxy();

    let mut app = App {
        monitor,
        provider,
        has_battery,
        display,
        temperature_source,
        critical_temp_c,
        language,
        tray: None,
        tray_menu: None,
        window: None,
        webview: None,
        popover_visible: false,
        popover_show_at: None,
        left_button_down_seen: false,
        detail_window: None,
        detail_webview: None,
        fan_hist: VecDeque::with_capacity(HIST_CAP),
        cpu_h: RangedHistory::new(),
        mem_h: RangedHistory::new(),
        temp_h: RangedHistory::new(),
        net_h: RangedHistory::new(),
        disk_io_h: RangedHistory::new(),
        dashboard_slow_cache: DashboardSlowCache::default(),
        next_dashboard_slow_refresh: Instant::now() + DASHBOARD_SLOW_REFRESH,
        runner_frame: 0,
        runner_cpu_pct: 0.0,
        runner_icons: make_runner_icons(),
        last_runner_icon: None,
        temperature_cache: Vec::new(),
        temperature_sampled_at: None,
        temperature_sampled_at_unix_ms: None,
        next_temperature_refresh: Instant::now(),
        temperature_read: Arc::new(BackgroundRead::default()),
        fan_cache: Vec::new(),
        next_fan_refresh: Instant::now(),
        fan_read: Arc::new(BackgroundRead::default()),
        all_temp_rows_cache: Vec::new(),
        all_temp_sampled_at: None,
        all_temp_sampled_at_unix_ms: None,
        next_all_temp_refresh: Instant::now() + ALL_TEMP_REFRESH,
        all_temp_read: Arc::new(BackgroundRead::default()),
        daemon_json_cache: None,
        next_daemon_refresh: Instant::now() + DAEMON_REFRESH,
        update_install_result: peterfan_platform::updater::read_update_install_result(),
        next_update_result_refresh: Instant::now() + Duration::from_secs(1),
        control_confirm_until: None,
    };

    let mut next_metric_at = Instant::now();
    let mut next_runner_at = Instant::now();
    let mut prewarm_popover_at = Some(Instant::now() + POPOVER_PREWARM_DELAY);
    event_loop.run(move |event, target, control_flow| {
        if QUIT.load(Ordering::Relaxed) {
            *control_flow = ControlFlow::Exit;
            return;
        }

        if OPEN_DETAIL.swap(false, Ordering::Relaxed) {
            hide_popover(&mut app);
            open_detail_window(&mut app, target, &event_proxy);
        }

        match event {
            Event::NewEvents(StartCause::Init) => {
                build_tray(&mut app);
                // Offer one-time setup right away instead of leaving it
                // buried in the right-click menu — other fan-control apps
                // ask for this during their installer; we don't have one,
                // so the first launch has to ask instead. Never in --mock:
                // there's no real hardware to control, so the whole flow
                // (including the real privileged install) would be bogus.
                if !use_mock {
                    if should_auto_prompt_first_run_setup_on_launch() {
                        std::thread::spawn(maybe_prompt_first_run_setup);
                    }
                    // App updates are informational; fan-control daemon
                    // updates are privileged and stay user-initiated through
                    // the Setup row's Update button.
                    if should_auto_prompt_stale_daemon_update_on_launch() {
                        std::thread::spawn(maybe_prompt_stale_daemon_update);
                    }
                    std::thread::spawn(check_for_updates_on_launch);
                }
            }
            Event::NewEvents(StartCause::ResumeTimeReached { .. }) => {}
            Event::WindowEvent {
                event: WindowEvent::Focused(false),
                window_id,
                ..
            } => {
                if app.window.as_ref().is_some_and(|w| w.id() == window_id) {
                    log_menubar_event("popover focus lost");
                    hide_popover(&mut app);
                }
            }
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

        let now = Instant::now();
        if app.popover_show_at.is_some_and(|at| now >= at) {
            if let Some(w) = &app.window {
                w.set_visible(true);
                app.popover_visible = true;
                log_menubar_event("popover shown");
                defer_dashboard_io_after_open(&mut app);
            }
            app.popover_show_at = None;
        }
        if let Some(at) = prewarm_popover_at {
            if now >= at {
                if app.window.is_none() {
                    let _ = build_popover(&mut app, target, &event_proxy);
                }
                prewarm_popover_at = None;
            }
        }
        if CONTROL_REFRESH_REQUESTED.swap(false, Ordering::Relaxed) {
            app.next_daemon_refresh = now;
            next_metric_at = now;
        }
        let confirming_control = app.control_confirm_until.is_some_and(|until| now < until);
        if app.control_confirm_until.is_some_and(|until| now >= until) {
            app.control_confirm_until = None;
        }
        if confirming_control {
            app.next_daemon_refresh = now;
        }
        if now >= next_metric_at {
            update(&mut app);
            next_metric_at = now
                + if confirming_control {
                    CONTROL_CONFIRM_REFRESH
                } else {
                    REFRESH
                };
            if runner_enabled(app.display) {
                // A CPU spike must accelerate the runner immediately instead
                // of waiting for the old idle-speed deadline to expire.
                next_runner_at =
                    next_runner_at.min(now + runner_frame_interval(app.runner_cpu_pct));
            }
        }
        if runner_enabled(app.display) && now >= next_runner_at {
            animate_runner(&mut app);
            next_runner_at = now + runner_frame_interval(app.runner_cpu_pct);
        }
        let next_tick = [
            Some(next_metric_at),
            runner_enabled(app.display).then_some(next_runner_at),
            prewarm_popover_at,
            app.popover_show_at,
        ]
        .into_iter()
        .flatten()
        .min()
        .unwrap_or(next_metric_at);
        *control_flow = ControlFlow::WaitUntil(next_tick);

        // Run any control commands queued by the popover.
        let cmds: Vec<String> = std::mem::take(&mut *PENDING.lock().expect("pending poisoned"));
        if !cmds.is_empty() {
            let mut refresh_after_pending = false;
            for c in &cmds {
                if let Some(json) = c.strip_prefix("savecurve:") {
                    *STATUS.lock().expect("status poisoned") = "saving curve…".into();
                    let provider = std::sync::Arc::clone(&app.provider);
                    let json = json.to_string();
                    std::thread::spawn(move || {
                        let status = save_custom_curve(provider.as_ref(), &json);
                        record_fan_action("save custom curve", &status, status.contains("saved"));
                        *STATUS.lock().expect("status poisoned") = status;
                    });
                    refresh_after_pending = true;
                } else if c == "enablefancontrol" {
                    // Same admin-prompt install the right-click menu item
                    // triggers — exposed here too so the "update the daemon"
                    // fix is one click from the exact error message that
                    // told the user they needed it, not a hunt through menus.
                    std::thread::spawn(install_fan_control);
                    refresh_after_pending = true;
                } else if c == "checkupdates" {
                    std::thread::spawn(check_for_updates_interactive);
                    refresh_after_pending = true;
                } else if c == "toggle-login-item" || c == "togglelogin" {
                    std::thread::spawn(move || {
                        let status = toggle_login_item();
                        *STATUS.lock().expect("status poisoned") = status;
                    });
                    refresh_after_pending = true;
                } else if c == "diagnosefan" {
                    *STATUS.lock().expect("status poisoned") = "running fan diagnostics…".into();
                    let provider = std::sync::Arc::clone(&app.provider);
                    std::thread::spawn(move || {
                        let (ok, status) = run_fan_diagnostic(provider.as_ref());
                        record_fan_action("diagnostic", &status, ok);
                        *STATUS.lock().expect("status poisoned") = status;
                    });
                    refresh_after_pending = true;
                } else if c == "ready" {
                    // The WebView may be built hidden during idle prewarm.
                    // Let the normal tick deliver data so `ready` never
                    // triggers a full sensor scan on the event-loop thread.
                } else {
                    // Hardware I/O (SMC calls) can take hundreds of ms,
                    // especially while failing (no daemon, no root) — run it
                    // off the event-loop thread so the menu bar stays
                    // responsive. The next periodic tick (within 1s) picks
                    // up the result via STATUS.
                    *STATUS.lock().expect("status poisoned") = "applying…".into();
                    let provider = std::sync::Arc::clone(&app.provider);
                    let cmd = c.clone();
                    let proxy = event_proxy.clone();
                    log_menubar_event(&format!("fan command received cmd={cmd}"));
                    std::thread::spawn(move || {
                        let status = execute_control_logged(provider.as_ref(), &cmd);
                        log_menubar_event(&format!(
                            "fan command completed cmd={cmd} ok={} result={status}",
                            control_result_is_ok(&status)
                        ));
                        *STATUS.lock().expect("status poisoned") = status;
                        let _ = proxy.send_event(());
                    });
                    app.control_confirm_until = Some(now + CONTROL_CONFIRM_WINDOW);
                    next_metric_at = now;
                    refresh_after_pending = true;
                }
            }
            if refresh_after_pending {
                update(&mut app);
            }
        }

        // Handle context-menu item selections.
        while let Ok(ev) = MenuEvent::receiver().try_recv() {
            let id = &ev.id;
            let mut matched_display: Option<MenubarDisplay> = None;
            let mut matched_temperature_source: Option<TemperatureSource> = None;
            let mut matched_language: Option<Language> = None;
            let mut open_detail_requested = false;
            let mut open_diagnostics_requested = false;
            let cmd: Option<String> = if let Some(ref tm) = app.tray_menu {
                if id == &tm.auto {
                    Some("auto".into())
                } else if id == &tm.rules {
                    Some("rules".into())
                } else if id == &tm.quit {
                    QUIT.store(true, Ordering::Relaxed);
                    None
                } else if let Some((d, _)) =
                    tm.display_items.iter().find(|(_, item)| item.id() == id)
                {
                    matched_display = Some(*d);
                    None
                } else if let Some((source, _)) = tm
                    .temperature_source_items
                    .iter()
                    .find(|(_, item)| item.id() == id)
                {
                    matched_temperature_source = Some(*source);
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
                } else if tm.check_updates == *id {
                    std::thread::spawn(check_for_updates_interactive);
                    None
                } else if tm.open_detail == *id {
                    open_detail_requested = true;
                    None
                } else if tm.open_diagnostics == *id {
                    open_diagnostics_requested = true;
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
                hide_popover(&mut app);
                open_detail_window(&mut app, target, &event_proxy);
            }
            if open_diagnostics_requested {
                open_menubar_log();
            }
            if let Some(d) = matched_display {
                app.display = d;
                next_runner_at = Instant::now();
                if let Some(ref tm) = app.tray_menu {
                    for (dd, item) in &tm.display_items {
                        item.set_checked(*dd == d);
                    }
                }
                save_menubar_display(app.display);
                update(&mut app);
            }
            if let Some(source) = matched_temperature_source {
                app.temperature_source = source;
                if let Some(ref tm) = app.tray_menu {
                    for (candidate, item) in &tm.temperature_source_items {
                        item.set_checked(*candidate == source);
                    }
                }
                save_temperature_source(source);
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
                if app.window.is_some() {
                    app.window = None;
                    app.webview = None;
                    let _ = build_popover(&mut app, target, &event_proxy);
                    if was_visible {
                        if let Some(w) = &app.window {
                            w.set_visible(true);
                        }
                        app.popover_visible = true;
                    }
                }
                if app.detail_window.is_some() {
                    let was_detail_visible =
                        app.detail_window.as_ref().is_some_and(Window::is_visible);
                    app.detail_window = None;
                    app.detail_webview = None;
                    if was_detail_visible {
                        open_detail_window(&mut app, target, &event_proxy);
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
                let proxy = event_proxy.clone();
                std::thread::spawn(move || {
                    let status = execute_control_logged(provider.as_ref(), &cmd);
                    // The right-click menu has no visible status line (unlike
                    // the popover), so surface the result as a notification —
                    // otherwise a failed command (no daemon, needs root)
                    // looks like it silently did nothing.
                    notify_control_result(&cmd, control_result_is_ok(&status), &status);
                    *STATUS.lock().expect("status poisoned") = status;
                    let _ = proxy.send_event(());
                });
            }
        }

        while let Ok(ev) = TrayIconEvent::receiver().try_recv() {
            // Open on mouse-down. Some macOS menu-bar configurations swallow
            // mouse-up while the bar is auto-hiding or changing displays.
            // Consume the matching Up event so a normal click toggles once.
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state,
                rect,
                ..
            } = ev
            {
                let should_toggle = route_left_click(button_state, &mut app.left_button_down_seen);
                log_menubar_event(&format!(
                    "tray left-click state={button_state:?} toggle={should_toggle} rect={rect:?}"
                ));
                if should_toggle {
                    toggle_popover(&mut app, target, rect, &event_proxy);
                    if let Some(show_at) = app.popover_show_at {
                        // Tray events are drained after the normal wake deadline is
                        // calculated. Pull both the window show and first payload
                        // forward to the placement tick instead of waiting for up
                        // to one full metrics interval.
                        next_metric_at = show_at;
                        *control_flow = ControlFlow::WaitUntil(show_at);
                    }
                }
            }
        }
    });
}

fn help_text() -> String {
    format!(
        "peterfan-menubar {}\n\n\
         Live system metrics in the macOS menu bar.\n\n\
         USAGE:\n    peterfan-menubar [OPTIONS]\n\n\
         OPTIONS:\n    \
         --mock                Use simulated hardware instead of real sensors\n    \
         --display <number|cat|both>             How it's rendered (cat also accepts legacy graph)\n    \
         (The flag overrides the saved preference; changing it from the\n    \
         right-click menu persists for next launch.)\n    \
         --version, -V         Print version and exit\n    \
         --help, -h            Print this help and exit",
        env!("CARGO_PKG_VERSION")
    )
}

fn open_external_url(url: &str) {
    if !url.starts_with("https://github.com/uulab-official/peterfan/releases/") {
        return;
    }
    let _ = std::process::Command::new("open").arg(url).status();
}

fn unsupported_menubar_arg(args: &[String]) -> Option<&str> {
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--mock" => i += 1,
            "--metric" | "--display" => {
                if i + 1 >= args.len() {
                    return Some(args[i].as_str());
                }
                i += 2;
            }
            arg if arg.starts_with("-psn_") => i += 1,
            arg => return Some(arg),
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Tray icon (no native menu — the popover is the whole UI)
// ---------------------------------------------------------------------------

fn build_tray(app: &mut App) {
    let s = strings(app.language.resolve());

    // One-time setup: installs the root daemon so fan control works without
    // a terminal or repeated sudo prompts — one macOS admin-password dialog,
    // triggered right here instead of requiring the CLI.
    #[cfg(target_os = "macos")]
    let enable_fan_control_item = MenuItem::new(s.enable_fan_control, true, None);

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
    let open_diagnostics_item = MenuItem::new(s.open_diagnostics, true, None);
    let check_updates_item = MenuItem::new(s.check_updates, true, None);
    let quit_item = MenuItem::new(s.quit, true, None);

    // "Display" — number only / cat only / both.
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

    // "CPU Temperature Source" — different monitoring apps pick different
    // Apple Silicon sensor families, so let users pin the one they compare
    // against instead of pretending there is only one true CPU temperature.
    let temperature_source_submenu = Submenu::new(s.temperature_source, true);
    let temperature_source_items: Vec<(TemperatureSource, CheckMenuItem)> = [
        TemperatureSource::Hottest,
        TemperatureSource::CoreAverage,
        TemperatureSource::IohidTdie,
        TemperatureSource::SmcSummary,
        TemperatureSource::SmcAggregate,
    ]
    .into_iter()
    .map(|source| {
        let item = CheckMenuItem::new(
            temperature_source_label(app.language.resolve(), source),
            true,
            app.temperature_source == source,
            None,
        );
        let _ = temperature_source_submenu.append(&item);
        (source, item)
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
            (
                format!("hold:{pct}"),
                MenuItem::new(format!("{pct}%"), true, None),
            )
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
    let _ = menu.append(&display_submenu);
    let _ = menu.append(&temperature_source_submenu);
    let _ = menu.append(&language_submenu);
    let _ = menu.append(&PredefinedMenuItem::separator());
    let _ = menu.append(&open_detail_item);
    let _ = menu.append(&open_diagnostics_item);
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
        display_items,
        temperature_source_items,
        fan_speed_items,
        #[cfg(target_os = "macos")]
        enable_fan_control: enable_fan_control_item.id().clone(),
        check_updates: check_updates_item.id().clone(),
        open_detail: open_detail_item.id().clone(),
        open_diagnostics: open_diagnostics_item.id().clone(),
        language_items,
    };

    let initial_runner_icon = runner_enabled(app.display)
        .then(|| runner_icon_index(app.runner_cpu_pct, app.runner_frame));
    let initial_icon = initial_runner_icon.and_then(|index| app.runner_icons.get(index).cloned());
    match TrayIcon::new(tray_attributes(initial_icon, Box::new(menu))) {
        Ok(tray) => {
            app.tray = Some(tray);
            app.tray_menu = Some(tray_menu);
            app.last_runner_icon = initial_runner_icon;
            log_menubar_event("tray created");
        }
        Err(e) => {
            log_menubar_event(&format!("tray creation failed: {e}"));
            eprintln!("failed to create menu-bar item: {e}");
        }
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

fn route_left_click(state: MouseButtonState, down_seen: &mut bool) -> bool {
    match state {
        MouseButtonState::Down => {
            *down_seen = true;
            true
        }
        MouseButtonState::Up => !std::mem::replace(down_seen, false),
    }
}

fn tray_attributes(
    icon: Option<Icon>,
    menu: Box<dyn tray_icon::menu::ContextMenu>,
) -> TrayIconAttributes {
    let (menu_on_left_click, menu_on_right_click) = click_routing();
    TrayIconAttributes {
        icon,
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

fn build_popover(
    app: &mut App,
    target: &EventLoopWindowTarget<()>,
    event_proxy: &EventLoopProxy<()>,
) -> bool {
    let window = match WindowBuilder::new()
        .with_decorations(false)
        .with_resizable(false)
        .with_visible(false)
        .with_focused(false)
        .with_always_on_top(true)
        .with_inner_size(LogicalSize::new(POPOVER_W, POPOVER_H))
        .build(target)
    {
        Ok(w) => w,
        Err(e) => {
            log_menubar_event(&format!("popover window creation failed: {e}"));
            eprintln!("failed to create popover window: {e}");
            return false;
        }
    };

    let ipc_proxy = event_proxy.clone();
    match WebViewBuilder::new()
        .with_html(dashboard_html(app.language.resolve(), false))
        .with_background_color(DASHBOARD_BACKGROUND)
        .with_accept_first_mouse(true)
        .with_ipc_handler(move |req| {
            let body = req.body();
            if body == "quit" {
                QUIT.store(true, Ordering::Relaxed);
            } else if body == "open_detail" {
                OPEN_DETAIL.store(true, Ordering::Relaxed);
            } else if let Some(url) = body.strip_prefix("open:") {
                open_external_url(url);
            } else if body == "ready"
                || body == "checkupdates"
                || body == "toggle-login-item"
                || body == "togglelogin"
            {
                PENDING
                    .lock()
                    .expect("pending poisoned")
                    .push(body.to_string());
            } else if body.starts_with("h:") {
                // Kept for compatibility with older dashboard HTML. The
                // popover is intentionally fixed-height now; content scrolls
                // inside `.main-pane` instead of resizing the native window.
            } else if let Some(cmd) = body.strip_prefix("cmd:") {
                PENDING
                    .lock()
                    .expect("pending poisoned")
                    .push(cmd.to_string());
            } else if body.starts_with("savecurve:") {
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
            let _ = ipc_proxy.send_event(());
        })
        .build(&window)
    {
        Ok(webview) => {
            app.window = Some(window);
            app.webview = Some(webview);
            log_menubar_event("popover webview created");
            true
        }
        Err(e) => {
            log_menubar_event(&format!("popover webview creation failed: {e}"));
            eprintln!("failed to create popover webview: {e}");
            false
        }
    }
}

/// Opens (or, if already created, shows) the persistent detail
/// window — same dashboard content as the popover, in an ordinary decorated,
/// resizable, user-positioned window that stays open regardless of focus.
fn open_detail_window(
    app: &mut App,
    target: &EventLoopWindowTarget<()>,
    event_proxy: &EventLoopProxy<()>,
) {
    if let Some(w) = &app.detail_window {
        w.set_visible(true);
        defer_dashboard_io_after_open(app);
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

    let ipc_proxy = event_proxy.clone();
    match WebViewBuilder::new()
        .with_html(dashboard_html(app.language.resolve(), true))
        .with_background_color(DASHBOARD_BACKGROUND)
        .with_ipc_handler(move |req| {
            let body = req.body();
            // Same command surface as the popover, minus "h:" — a resizable
            // window sizes itself; it shouldn't fight the user by snapping
            // to the content's natural height on every tick.
            if body == "quit" {
                QUIT.store(true, Ordering::Relaxed);
            } else if body == "open_detail" {
                OPEN_DETAIL.store(true, Ordering::Relaxed);
            } else if let Some(url) = body.strip_prefix("open:") {
                open_external_url(url);
            } else if body == "ready"
                || body == "checkupdates"
                || body == "toggle-login-item"
                || body == "togglelogin"
            {
                PENDING
                    .lock()
                    .expect("pending poisoned")
                    .push(body.to_string());
            } else if let Some(cmd) = body.strip_prefix("cmd:") {
                PENDING
                    .lock()
                    .expect("pending poisoned")
                    .push(cmd.to_string());
            } else if body.starts_with("savecurve:") {
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
            let _ = ipc_proxy.send_event(());
        })
        .build(&window)
    {
        Ok(webview) => {
            window.set_visible(true);
            app.detail_window = Some(window);
            app.detail_webview = Some(webview);
            defer_dashboard_io_after_open(app);
        }
        Err(e) => eprintln!("failed to create detail webview: {e}"),
    }
}

/// Largest height the popover can be without its bottom edge running past
/// the current monitor — with the CPU/memory/storage/temperature/fans/
/// battery/network/processes/fan-control sections all present, content can
/// genuinely exceed a short display's height. Content beyond this scrolls
/// inside the left `.main-pane` instead of dragging the action rail along or
/// being cut off.
#[cfg(test)]
fn max_popover_height_for_bounds(display: DisplayBounds, popover_top_y: f64) -> f64 {
    (display.bottom() - popover_top_y - 12.0).max(200.0)
}

#[cfg(any(not(target_os = "macos"), test))]
fn popover_height_for_rect(rect: Rect, scale: f64, displays: &[DisplayBounds]) -> f64 {
    let anchor_x = rect.position.x + rect.size.width as f64;
    let anchor_y = rect.position.y + rect.size.height as f64;
    let display = displays
        .iter()
        .copied()
        .find(|d| d.contains_point(rect.position.x, rect.position.y))
        .or_else(|| {
            displays
                .iter()
                .copied()
                .find(|d| d.contains_point(anchor_x, anchor_y))
        });
    let Some(display) = display else {
        return POPOVER_H;
    };
    let available = ((display.bottom() - anchor_y) / scale - 12.0).max(200.0);
    POPOVER_H.min(available)
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct DisplayBounds {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

impl DisplayBounds {
    fn right(self) -> f64 {
        self.x + self.width
    }

    fn bottom(self) -> f64 {
        self.y + self.height
    }

    #[cfg(any(not(target_os = "macos"), test))]
    fn contains_point(self, x: f64, y: f64) -> bool {
        x >= self.x && x < self.right() && y >= self.y && y < self.bottom()
    }
}

#[cfg(not(target_os = "macos"))]
fn monitor_bounds(monitor: &MonitorHandle) -> DisplayBounds {
    let pos = monitor.position();
    let size = monitor.size();
    DisplayBounds {
        x: pos.x as f64,
        y: pos.y as f64,
        width: size.width as f64,
        height: size.height as f64,
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct LogicalPopoverAnchor {
    x: f64,
    y: f64,
    display: DisplayBounds,
    scale: f64,
}

fn popover_position_for_anchor(
    anchor: LogicalPopoverAnchor,
    popover_width: f64,
) -> LogicalPosition<f64> {
    let min_x = anchor.display.x + 8.0;
    let max_x = anchor.display.right() - popover_width - 8.0;
    let x = if min_x <= max_x {
        (anchor.x - popover_width).clamp(min_x, max_x)
    } else {
        min_x
    };
    LogicalPosition::new(x, anchor.y)
}

fn popover_height_for_anchor(anchor: LogicalPopoverAnchor) -> f64 {
    let available = (anchor.display.bottom() - anchor.y - 12.0).max(200.0);
    POPOVER_H.min(available)
}

/// Resolve the clicked status item's screen in AppKit's global point space.
/// `tray-icon` scales the global origin by the clicked screen's backing scale,
/// while Tao later divides a physical position by the hidden window's previous
/// screen scale. Those two scales differ on mixed-DPI desktops and can place a
/// popover between displays. Keeping the complete placement in logical points
/// avoids both conversions.
#[cfg(target_os = "macos")]
fn native_logical_popover_anchor(rect: Rect) -> Option<LogicalPopoverAnchor> {
    // SAFETY: Tao's macOS event loop and tray event drain both execute on the
    // process main thread. Using an unchecked marker here prevents a silent
    // fallback to mixed physical coordinates if runtime marker detection is
    // unavailable on a particular macOS release.
    let mtm = unsafe { MainThreadMarker::new_unchecked() };
    let screens = NSScreen::screens(mtm);
    let primary = screens.iter().next()?;
    let primary_frame = primary.frame();
    let primary_top = primary_frame.origin.y + primary_frame.size.height;
    let mouse = NSEvent::mouseLocation();
    let screen = screens
        .iter()
        .find(|screen| appkit_frame_contains(screen.frame(), mouse))
        .or_else(|| {
            screens.iter().min_by(|left, right| {
                appkit_frame_distance_sq(left.frame(), mouse)
                    .total_cmp(&appkit_frame_distance_sq(right.frame(), mouse))
            })
        })?;
    let frame = screen.frame();
    let scale = screen.backingScaleFactor();
    if !scale.is_finite() || scale <= 0.0 {
        return None;
    }

    let display = DisplayBounds {
        x: frame.origin.x,
        y: primary_top - (frame.origin.y + frame.size.height),
        width: frame.size.width,
        height: frame.size.height,
    };
    let tray_width = rect.size.width as f64 / scale;
    let tray_height = rect.size.height as f64 / scale;
    let reported_left = rect.position.x / scale;
    let reported_right = reported_left + tray_width;
    // The event rectangle is normally exact after removing the screen-local
    // backing scale. Fall back to the live pointer when a tray implementation
    // reports an origin outside the screen so the popup still stays local.
    let anchor_x = if reported_right >= display.x && reported_left <= display.right() {
        reported_right
    } else {
        mouse.x + tray_width / 2.0
    };

    Some(LogicalPopoverAnchor {
        x: anchor_x,
        y: display.y + tray_height,
        display,
        scale,
    })
}

#[cfg(target_os = "macos")]
fn appkit_frame_contains(frame: NSRect, point: NSPoint) -> bool {
    point.x >= frame.origin.x
        && point.x < frame.origin.x + frame.size.width
        && point.y >= frame.origin.y
        && point.y < frame.origin.y + frame.size.height
}

#[cfg(target_os = "macos")]
fn appkit_frame_distance_sq(frame: NSRect, point: NSPoint) -> f64 {
    let nearest_x = point
        .x
        .clamp(frame.origin.x, frame.origin.x + frame.size.width);
    let nearest_y = point
        .y
        .clamp(frame.origin.y, frame.origin.y + frame.size.height);
    (point.x - nearest_x).powi(2) + (point.y - nearest_y).powi(2)
}

#[cfg(target_os = "macos")]
fn configure_native_popover_window(
    window: &Window,
    position: LogicalPosition<f64>,
    width: f64,
    height: f64,
) -> Option<NSRect> {
    // SAFETY: This function is called only from Tao's main-thread event loop,
    // and `ns_window()` remains valid for the lifetime of `window`.
    let mtm = unsafe { MainThreadMarker::new_unchecked() };
    let primary = NSScreen::screens(mtm).iter().next()?;
    let primary_frame = primary.frame();
    let primary_top = primary_frame.origin.y + primary_frame.size.height;
    let raw = window.ns_window().cast::<NSWindow>();
    if raw.is_null() {
        return None;
    }
    let native_window = unsafe { &*raw };
    native_window.setContentSize(NSSize::new(width, height));
    native_window.setFrameTopLeftPoint(NSPoint::new(position.x, primary_top - position.y));
    Some(native_window.frame())
}

#[cfg(any(not(target_os = "macos"), test))]
fn popover_position_for_rect(
    rect: Rect,
    popover_width: f64,
    displays: &[DisplayBounds],
) -> PhysicalPosition<f64> {
    let anchor_x = rect.position.x + rect.size.width as f64;
    let anchor_y = rect.position.y + rect.size.height as f64;
    let display = displays
        .iter()
        .copied()
        .find(|d| d.contains_point(rect.position.x, rect.position.y))
        .or_else(|| {
            displays
                .iter()
                .copied()
                .find(|d| d.contains_point(anchor_x, anchor_y))
        });

    let x = display.map_or_else(
        || (anchor_x - popover_width).max(8.0),
        |d| {
            let min_x = d.x + 8.0;
            let max_x = d.right() - popover_width - 8.0;
            if min_x <= max_x {
                (anchor_x - popover_width).clamp(min_x, max_x)
            } else {
                d.x + 8.0
            }
        },
    );
    PhysicalPosition::new(x, anchor_y)
}

fn toggle_popover(
    app: &mut App,
    target: &EventLoopWindowTarget<()>,
    rect: Rect,
    event_proxy: &EventLoopProxy<()>,
) {
    if app.popover_visible || app.popover_show_at.is_some() {
        log_menubar_event("popover closed by tray click");
        hide_popover(app);
        return;
    }
    if let Some(detail) = &app.detail_window {
        detail.set_visible(false);
    }
    if app.window.is_none() && !build_popover(app, target, event_proxy) {
        log_menubar_event("popover toggle aborted: window unavailable");
        return;
    }
    let Some(w) = &app.window else { return };

    #[cfg(target_os = "macos")]
    {
        let Some(anchor) = native_logical_popover_anchor(rect) else {
            log_menubar_event("popover placement aborted: AppKit reported no screens");
            return;
        };
        let height = popover_height_for_anchor(anchor);
        let position = popover_position_for_anchor(anchor, POPOVER_W);
        let Some(frame) = configure_native_popover_window(w, position, POPOVER_W, height) else {
            log_menubar_event("popover placement aborted: native window unavailable");
            return;
        };
        app.popover_show_at = Some(Instant::now() + POPOVER_SHOW_DELAY);
        log_menubar_event(&format!(
            "popover scheduled native position=({:.0},{:.0}) size=({POPOVER_W:.0},{height:.0}) frame=({:.0},{:.0},{:.0}x{:.0}) clicked_display=({:.0},{:.0},{:.0}x{:.0}) scale={:.2}",
            position.x,
            position.y,
            frame.origin.x,
            frame.origin.y,
            frame.size.width,
            frame.size.height,
            anchor.display.x,
            anchor.display.y,
            anchor.display.width,
            anchor.display.height,
            anchor.scale
        ));
    }

    #[cfg(not(target_os = "macos"))]
    {
        let scale = w.scale_factor();
        let win_w = POPOVER_W * scale;
        // Flush against the menu bar rather than leaving a visible gap — matches
        // how native menu extras (Control Center, Wi-Fi, …) sit right under the
        // icon instead of floating below it.
        let displays: Vec<_> = w.available_monitors().map(|m| monitor_bounds(&m)).collect();
        // Snap to the product-defined fixed height before showing. On very short
        // displays we cap to the available area; the left pane owns scrolling.
        let height = popover_height_for_rect(rect, scale, &displays);
        let position = popover_position_for_rect(rect, win_w, &displays);
        w.set_inner_size(LogicalSize::new(POPOVER_W, height));
        w.set_outer_position(position);
        // macOS can apply size/position asynchronously. Showing on the next short
        // event-loop tick prevents the user from seeing the prewarmed hidden
        // window flash at its old/default location and then slide into place.
        w.set_outer_position(position);
        app.popover_show_at = Some(Instant::now() + POPOVER_SHOW_DELAY);
        log_menubar_event(&format!(
        "popover scheduled position=({:.0},{:.0}) size=({POPOVER_W:.0},{height:.0}) scale={scale:.2}",
        position.x, position.y
    ));
    }
}

fn hide_popover(app: &mut App) {
    app.popover_show_at = None;
    if let Some(w) = &app.window {
        w.set_visible(false);
    }
    app.popover_visible = false;
}

fn defer_dashboard_io_after_open(app: &mut App) {
    let now = Instant::now();
    app.next_dashboard_slow_refresh = now + DASHBOARD_SLOW_OPEN_GRACE;
    app.next_daemon_refresh = now + DASHBOARD_OPEN_GRACE;
    app.next_all_temp_refresh = now + DASHBOARD_OPEN_GRACE + Duration::from_millis(500);
}

fn default_curve_points() -> Vec<[f32; 2]> {
    Profile::Balanced
        .default_curve()
        .points()
        .iter()
        .map(|p| [p.temp_c, p.duty_percent as f32])
        .collect()
}

fn refresh_dashboard_slow_cache(app: &mut App, proc_sort: ProcSort) {
    let disks = app.monitor.disks();
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

    let procs: Vec<_> = app
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

    let battery = if app.has_battery {
        app.monitor.battery()
    } else {
        None
    };
    let (batt_present, batt_pct, batt_text, batt_sub) = battery
        .as_ref()
        .map(|b| {
            let mut sub = b.state.clone();
            if let Some(c) = b.cycle_count {
                sub.push_str(&format!("   {c} cycles"));
            }
            if let Some(h) = b.health_percent {
                sub.push_str(&format!("   health {h:.0}%"));
            }
            (
                true,
                b.charge_percent,
                format!("{:.0}%", b.charge_percent),
                sub,
            )
        })
        .unwrap_or_else(|| (false, 0.0, String::new(), String::new()));

    let curve_points = peterfan_platform::config::load()
        .custom_curve
        .and_then(|c| c.to_fan_curve())
        .map(|curve| {
            curve
                .points()
                .iter()
                .map(|p| [p.temp_c, p.duty_percent as f32])
                .collect()
        })
        .unwrap_or_else(default_curve_points);

    app.dashboard_slow_cache = DashboardSlowCache {
        proc_sort,
        procs,
        disk_pct: disk.map(|d| d.used_percent).unwrap_or(0.0),
        disk_text: disk
            .map(|d| format!("{:.1}%", d.used_percent))
            .unwrap_or_default(),
        disk_sub: disk
            .map(|d| format!("{} / {}   {}", bytes(d.used), bytes(d.total), d.mount))
            .unwrap_or_default(),
        disk_io_present,
        disk_io_sub,
        disk_io_rate,
        power_w: app.provider.power_watts(),
        batt_present,
        batt_pct,
        batt_text,
        batt_sub,
        curve_points,
    };
}

// ---------------------------------------------------------------------------
// Update: sample once, refresh the menu-bar title and (if open) the popover.
// ---------------------------------------------------------------------------

fn refresh_temperature_cache(app: &mut App, now: Instant) {
    if let Some(sample) = app.temperature_read.take() {
        if !sample.values.is_empty() {
            app.temperature_cache = sample.values;
            app.temperature_sampled_at = Some(sample.sampled_at);
            app.temperature_sampled_at_unix_ms = Some(sample.sampled_at_unix_ms);
        }
    }
    if app.temperature_cache.is_empty() || now >= app.next_temperature_refresh {
        app.next_temperature_refresh = now + TEMPERATURE_REFRESH;
        let provider = Arc::clone(&app.provider);
        app.temperature_read.start(move || {
            provider
                .temperatures()
                .ok()
                .filter(|sample| !sample.is_empty())
                .map(|values| TimedSample {
                    values,
                    sampled_at: Instant::now(),
                    sampled_at_unix_ms: now_unix_ms(),
                })
        });
    }
}

fn refresh_fan_cache(app: &mut App, now: Instant, dashboard_visible: bool) {
    if let Some(fans) = app.fan_read.take() {
        app.fan_cache = fans;
    }
    if dashboard_visible && now >= app.next_fan_refresh {
        app.next_fan_refresh = now + FAN_REFRESH;
        let provider = Arc::clone(&app.provider);
        app.fan_read.start(move || provider.fans().ok());
    }
}

fn refresh_all_temperature_cache(app: &mut App, now: Instant, dashboard_visible: bool) {
    if let Some(sample) = app.all_temp_read.take() {
        app.all_temp_rows_cache = sample
            .values
            .iter()
            .map(|temp| {
                serde_json::json!({
                    "l": raw_temperature_row_label(temp),
                    "c": format!("{:.0}°C", temp.value.0),
                    "cls": temp_cls(temp.value),
                    "group": sensor_group_label(app.language.resolve(), temp.kind),
                    "source": temp.source.short(),
                })
            })
            .collect();
        app.all_temp_sampled_at = Some(sample.sampled_at);
        app.all_temp_sampled_at_unix_ms = Some(sample.sampled_at_unix_ms);
    }
    if dashboard_visible && now >= app.next_all_temp_refresh {
        app.next_all_temp_refresh = now + ALL_TEMP_REFRESH;
        app.all_temp_read.start(|| {
            let values = peterfan_platform::all_temperature_sensors();
            (!values.is_empty()).then(|| TimedSample {
                values,
                sampled_at: Instant::now(),
                sampled_at_unix_ms: now_unix_ms(),
            })
        });
    }
}

fn update(app: &mut App) {
    let now = Instant::now();
    app.monitor.refresh_quick();
    let detail_visible = app.detail_window.as_ref().is_some_and(Window::is_visible);
    let dashboard_visible = app.popover_visible || detail_visible;
    let proc_sort = if PROC_SORT.load(Ordering::Relaxed) == 1 {
        ProcSort::Memory
    } else {
        ProcSort::Cpu
    };
    let refresh_slow_metrics = dashboard_visible
        && (now >= app.next_dashboard_slow_refresh
            || app.dashboard_slow_cache.proc_sort != proc_sort);
    if refresh_slow_metrics {
        app.monitor.refresh_slow();
    }
    let cpu = app.monitor.cpu();
    // Gathered unconditionally (cheap — the underlying sysinfo/provider state
    // was already refreshed/held open) so the rolling history stays populated
    // even while the popover is closed and the runner icon keeps moving.
    let mem = app.monitor.memory();
    let nets = app.monitor.networks();
    // CPU temperatures are much more expensive than sysinfo counters because
    // they cross SMC and IOHID. Never perform those calls on the event-loop
    // thread: on unsupported or waking hardware they can take long enough to
    // make the menu-bar item appear unclickable.
    refresh_temperature_cache(app, now);
    refresh_fan_cache(app, now, dashboard_visible);
    let temperature_stale =
        sample_is_stale(app.temperature_sampled_at, now, TEMPERATURE_STALE_AFTER);
    let temperature_age_secs = sample_age(app.temperature_sampled_at, now)
        .map(|age| age.as_secs())
        .unwrap_or(0);
    let temps = app.temperature_cache.clone();
    let fans = if dashboard_visible {
        app.fan_cache.clone()
    } else {
        Vec::new()
    };
    let display_temp = (!temperature_stale)
        .then(|| primary_menu_temperature(&temps, TemperatureSource::CoreAverage))
        .flatten()
        .map(|t| t.value)
        .or_else(|| {
            (!temperature_stale)
                .then(|| representative_temperature_c(&temps))
                .flatten()
        });
    let safety_temp = (!temperature_stale)
        .then(|| safety_temperature_c(&temps))
        .flatten();
    let core_hottest = (!temperature_stale)
        .then(|| {
            temps
                .iter()
                .find(|temp| temp.id == "cpu.die.hot")
                .map(|temp| temp.value.0)
        })
        .flatten();
    let fastest_rpm = fans.iter().map(|f| f.rpm).fold(0u32, u32::max);
    let fastest_pct = fans
        .iter()
        .filter_map(normalized_fan_speed_percent)
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
    if let Some(temp) = display_temp {
        app.temp_h.push(temp);
    }
    app.net_h.push((rx + tx) as f32);
    app.runner_cpu_pct = cpu.usage_percent;

    // Menu-bar item: keep it calm and literal. The top bar shows only the CPU
    // Core Average temperature; richer metrics live inside the popover.
    apply_runner_icon(app);
    if let Some(tray) = &app.tray {
        // Fixed-width formatting throughout: a menu-bar item that changes
        // width every tick (e.g. "9.5%" → "100.0%") shoves every icon to its
        // left back and forth, which reads as the whole menu bar jittering.
        // Padding to a constant character count keeps the item's width
        // (and everything to its left) stable regardless of the value.
        let title = if let Some(temp) = display_temp.filter(|t| *t > 0.0) {
            format!("{temp:>3.0}°C")
        } else {
            " --°C".to_string()
        };

        let show_number = matches!(app.display, MenubarDisplay::Number | MenubarDisplay::Both);
        set_menubar_text(tray, if show_number { &title } else { "" });
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
        if let Some(temp) = display_temp.filter(|t| *t > 0.0) {
            tip_parts.push(format!("CPU {temp:.0}°C"));
        }
        if let (Some(display), Some(hot)) = (display_temp, safety_temp) {
            if hot > display + 1.0 {
                let label = if app.language.resolve() == ResolvedLanguage::Ko {
                    "최고"
                } else {
                    "Hot"
                };
                tip_parts.push(format!("{label} {hot:.0}°C"));
            }
        }
        if fastest_rpm > 0 {
            tip_parts.push(format!("{fastest_rpm} RPM"));
        }
        let _ = tray.set_tooltip(Some(tip_parts.join("   ·   ")));
    }

    if !app.popover_visible && !detail_visible {
        return;
    }

    // The detached updater launches the replacement app before its health
    // check completes. Refresh this one tiny record while a dashboard is open
    // so `pending` becomes `installed` (or `rolled_back`) without another
    // relaunch. This stays off the closed-popover fast path.
    if now >= app.next_update_result_refresh {
        app.update_install_result = peterfan_platform::updater::read_update_install_result();
        app.next_update_result_refresh = now + Duration::from_secs(2);
    }

    if refresh_slow_metrics {
        refresh_dashboard_slow_cache(app, proc_sort);
        app.next_dashboard_slow_refresh = now + DASHBOARD_SLOW_REFRESH;
    }
    let ghz = cpu.frequency_mhz as f64 / 1000.0;
    let load_str = cpu
        .load_avg
        .map(|l| format!("load {:.2} {:.2} {:.2}", l.one, l.five, l.fifteen))
        .unwrap_or_default();
    app.disk_io_h.push(app.dashboard_slow_cache.disk_io_rate);

    // Temperatures: CPU average is the headline users compare with iStat/Stats;
    // raw diagnostic sensors remain listed below, while fan safety uses the
    // mapped core hottest value exposed by the platform backend.
    let selected_temp = (!temperature_stale)
        .then(|| primary_menu_temperature(&temps, app.temperature_source))
        .flatten()
        .or_else(|| {
            (!temperature_stale)
                .then(|| representative_temperature_c(&temps))
                .flatten()
                .map(|value| SelectedTemperature {
                    id: "cpu.die".to_string(),
                    value,
                    label_hint: None,
                })
        });
    let display_temp_value = selected_temp.as_ref().map(|t| t.value);
    let temp_rows: Vec<_> = temps
        .iter()
        .map(|t| {
            serde_json::json!({
                "l": temperature_row_label(app.language.resolve(), t),
                "c": format!("{:.0}°C", t.value.0),
                "cls": temp_cls(t.value),
                "sampled_at_unix_ms": app.temperature_sampled_at_unix_ms,
                "age_secs": temperature_age_secs,
                "stale": temperature_stale,
            })
        })
        .collect();
    refresh_all_temperature_cache(app, now, dashboard_visible);
    let all_temp_stale = sample_is_stale(app.all_temp_sampled_at, now, ALL_TEMPERATURE_STALE_AFTER);
    let all_temp_age_secs = sample_age(app.all_temp_sampled_at, now)
        .map(|age| age.as_secs())
        .unwrap_or(0);
    let all_temp_rows: Vec<_> = app
        .all_temp_rows_cache
        .iter()
        .cloned()
        .map(|mut row| {
            row["sampled_at_unix_ms"] = serde_json::json!(app.all_temp_sampled_at_unix_ms);
            row["age_secs"] = serde_json::json!(all_temp_age_secs);
            row["stale"] = serde_json::json!(all_temp_stale);
            row
        })
        .collect();

    // Fans: every fan listed with its own RPM and a speed bar (rpm / max).
    // Daemon status is useful, but a stale/missing socket can block for its
    // read timeout. Cache it briefly so opening the popover never waits on IPC.
    if now >= app.next_daemon_refresh {
        app.daemon_json_cache = daemon_temps_json();
        app.next_daemon_refresh = now + DAEMON_REFRESH;
    }
    let daemon_json = app.daemon_json_cache.clone();
    let daemon_st = daemon_json
        .as_ref()
        .map(|v| {
            let mode = v.get("mode").and_then(|m| m.as_str()).unwrap_or("");
            let backend = v.get("backend").and_then(|b| b.as_str()).unwrap_or("");
            format!("{mode} ({backend})")
        })
        .unwrap_or_default();
    let daemon_running = !daemon_st.is_empty();
    let daemon_version = cached_installed_daemon_version();
    let daemon_update_needed = daemon_running
        && daemon_version
            .as_deref()
            .is_some_and(peterfan_platform::daemon_update_required);
    let daemon_usable = daemon_running && daemon_control_usable(daemon_version.as_deref());
    // Without a daemon to ask, fall back to the local shadow state that
    // `apply_local` maintains for its one-shot direct writes.
    let fan_overrides = if daemon_usable {
        daemon_json
            .as_ref()
            .and_then(|v| v.get("fan_overrides").cloned())
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default()
    } else {
        local_fan_overrides()
    };
    let fan_targets: std::collections::HashMap<String, u8> = daemon_json
        .as_ref()
        .and_then(|value| value.get("fan_targets").cloned())
        .and_then(|value| serde_json::from_value(value).ok())
        .unwrap_or_default();
    let fan_readback_status: std::collections::HashMap<String, String> = daemon_json
        .as_ref()
        .and_then(|value| value.get("fan_readbacks"))
        .and_then(serde_json::Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(|row| {
                    Some((
                        row.get("id")?.as_str()?.to_string(),
                        row.get("status")?.as_str()?.to_string(),
                    ))
                })
                .collect()
        })
        .unwrap_or_default();
    let control_health = daemon_json
        .as_ref()
        .and_then(|value| value.get("control_health").cloned())
        .unwrap_or_else(|| {
            serde_json::json!({
                "failsafe_active": false,
                "sensor_failure_count": 0,
                "consecutive_sensor_failures": 0,
                "fan_write_failure_count": 0,
                "consecutive_fan_write_failures": 0,
                "fan_readback_failure_count": 0,
                "consecutive_fan_readback_failures": 0,
                "last_fan_readback_ok_unix": null,
                "stale_fan_ids": [],
                "retry_after_unix": null,
                "last_sensor_ok_unix": null,
                "last_error": null,
            })
        });
    // Compatibility controls whether we can trust the daemon for writes and
    // detailed cached state. Its mode string is still safe read-only state,
    // so an older daemon reporting `auto` must keep Auto selected in the UI.
    let reported_mode = daemon_json
        .as_ref()
        .and_then(|v| v.get("mode").and_then(|m| m.as_str()));
    let active_profile = reported_mode
        .and_then(active_profile_from_mode)
        .unwrap_or_default();
    let active_control_mode =
        resolved_active_control_mode(reported_mode, !fan_overrides.is_empty());
    let control_revision = daemon_json
        .as_ref()
        .and_then(|value| value.get("control_revision"))
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    let applied_control_revision = daemon_json
        .as_ref()
        .and_then(|value| value.get("applied_control_revision"))
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    let fan_rows: Vec<_> = fans
        .iter()
        .map(|f| {
            // Fan commands map 0-100% onto the physical [minimum, maximum]
            // range. Use that same basis here so the live bar, manual slider,
            // and daemon target all describe the same speed.
            let pct = normalized_fan_speed_percent(f).unwrap_or(0.0);
            let override_pct = fan_overrides.get(&f.id).copied();
            let target_pct = fan_targets.get(&f.id).copied();
            let readback_status = fan_readback_status.get(&f.id).cloned();
            let target_rpm = target_pct.and_then(|target| match (f.min_rpm, f.max_rpm) {
                (Some(min), Some(max)) if max > min => {
                    Some((min as f32 + (target as f32 / 100.0) * (max - min) as f32).round() as u32)
                }
                _ => None,
            });
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
                "target_pct": target_pct,
                "target_rpm": target_rpm,
                "readback_status": readback_status,
            })
        })
        .collect();

    let can_control = app.provider.capabilities().control_fans || daemon_usable;
    let ctl_status = if daemon_update_needed {
        match app.language.resolve() {
            ResolvedLanguage::Ko => "팬 제어 재설치 필요".to_string(),
            ResolvedLanguage::En => "reinstall fan control".to_string(),
        }
    } else if daemon_usable {
        daemon_st.clone()
    } else {
        STATUS.lock().expect("status poisoned").clone()
    };
    let chart_range = ChartRange::from_u8(CHART_RANGE.load(Ordering::Relaxed));
    let (daemon_binary_installed, daemon_path, launch_daemon_installed) = daemon_install_metadata();

    let payload = serde_json::json!({
        "cpu_pct": cpu.usage_percent,
        "cpu_text": format!("{:.1}%", cpu.usage_percent),
        "cpu_sub": format!(
            "{:.1} GHz   {}{}",
            ghz,
            load_str,
            app.dashboard_slow_cache
                .power_w
                .map(|w| format!("   {w:.1} W"))
                .unwrap_or_default()
        ),
        "cores": &cpu.per_core,
        "mem_pct": mem.used_percent,
        "mem_text": format!("{:.1}%", mem.used_percent),
        "mem_sub": format!(
            "{} / {}   swap {} / {}",
            bytes(mem.used), bytes(mem.total), bytes(mem.swap_used), bytes(mem.swap_total)
        ),
        "disk_pct": app.dashboard_slow_cache.disk_pct,
        "disk_text": &app.dashboard_slow_cache.disk_text,
        "disk_sub": &app.dashboard_slow_cache.disk_sub,
        "disk_io_present": app.dashboard_slow_cache.disk_io_present,
        "disk_io_sub": &app.dashboard_slow_cache.disk_io_sub,
        "procs": &app.dashboard_slow_cache.procs,
        "proc_sort": if matches!(proc_sort, ProcSort::Memory) { "mem" } else { "cpu" },
        "temp_present": !temps.is_empty(),
        "temp_stale": temperature_stale,
        "temp_age_secs": temperature_age_secs,
        "temp_sampled_at_unix_ms": app.temperature_sampled_at_unix_ms,
        "temp_pct": display_temp_value.unwrap_or(0.0),
        "temp_text": if temperature_stale { "--°C".to_string() } else { display_temp_value.map(|t| format!("{t:.0}°C")).unwrap_or_default() },
        "temp_cls": display_temp_value.map(|t| temp_cls(Celsius(t))).unwrap_or("g"),
        "temp_source": display_temperature_source_for_temps(
            app.language.resolve(),
            &temps,
            selected_temp.as_ref()
        ),
        "temps": temp_rows,
        "all_temps": &all_temp_rows,
        "fans": fan_rows,
        "batt_present": app.dashboard_slow_cache.batt_present,
        "batt_pct": app.dashboard_slow_cache.batt_pct,
        "batt_text": &app.dashboard_slow_cache.batt_text,
        "batt_sub": &app.dashboard_slow_cache.batt_sub,
        "network_count": nets.len(),
        "network_active": nets.iter().any(|n| n.ip.is_some() || n.rx_rate > 0.0 || n.tx_rate > 0.0),
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
        "daemon_version": daemon_version.clone(),
        "daemon_required_version": peterfan_platform::MIN_REQUIRED_DAEMON_VERSION,
        "daemon_update_needed": daemon_update_needed,
        "daemon_binary_installed": daemon_binary_installed,
        "daemon_path": daemon_path,
        "launch_daemon_installed": launch_daemon_installed,
        "team_id": peterfan_platform::updater::EXPECTED_TEAM_ID,
        "login_item_installed": login_item_installed(),
        "login_item_supported": login_item_supported(),
        "active_profile": active_profile,
        "active_control_mode": active_control_mode,
        "control_revision": control_revision,
        "applied_control_revision": applied_control_revision,
        "fan_setup_needed": (!daemon_running || daemon_update_needed) && can_control,
        "fan_count": fans.len(),
        "controllable_fan_count": fans.iter().filter(|f| f.controllable).count(),
        "fan_curve_input_temp_c": display_temp,
        "fan_core_hottest_temp_c": core_hottest,
        "fan_safety_temp_c": safety_temp,
        "fan_critical_temp_c": app.critical_temp_c,
        "control_health": control_health,
        "fan_action_log": fan_action_log_snapshot(),
        "app_version": env!("CARGO_PKG_VERSION"),
        "update_install_result": &app.update_install_result,
        "setup_tone": setup_tone(!daemon_st.is_empty(), daemon_update_needed),
        "setup_title": setup_title(app.language.resolve(), !daemon_st.is_empty(), daemon_update_needed),
        "setup_detail": setup_detail(app.language.resolve(), !daemon_st.is_empty(), daemon_update_needed, daemon_version.as_deref()),
        "curve_points": &app.dashboard_slow_cache.curve_points,
        "last_cmd_status": STATUS.lock().expect("status poisoned").clone(),
    });
    let script = format!(
        "window.__pf_pending={payload};window.__pf&&window.__pf.update(window.__pf_pending)"
    );
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

fn animate_runner(app: &mut App) {
    if !runner_enabled(app.display) {
        return;
    }
    app.runner_frame = app.runner_frame.wrapping_add(1);
    apply_runner_icon(app);
}

/// Single daemon IPC round-trip for everything the popover needs per tick —
/// mode/backend (for the status line) and per-fan overrides (for the
/// Auto/Manual toggle) both live in the "temps" reply already, so there's no
/// need for a separate "status" query too (that used to double the daemon
/// IPC traffic every second for a value already present in this payload).
/// `None` when no daemon is reachable.
#[cfg(unix)]
fn daemon_temps_json() -> Option<serde_json::Value> {
    let reply = peterfan_platform::ipc::send_command("temps")?;
    serde_json::from_str(reply.strip_prefix("ok ")?).ok()
}
#[cfg(not(unix))]
fn daemon_temps_json() -> Option<serde_json::Value> {
    None
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

fn execute_control_logged(provider: &dyn HardwareProvider, cmd: &str) -> String {
    let result = execute_control(provider, cmd);
    CONTROL_REFRESH_REQUESTED.store(true, Ordering::Relaxed);
    record_fan_action(
        &control_action_label(cmd),
        &result,
        control_result_is_ok(&result),
    );
    result
}

struct FanDiagnosticInput<'a> {
    fan_count: usize,
    controllable_count: usize,
    average_c: Option<f32>,
    safety_c: Option<f32>,
    critical_c: f32,
    daemon_version: Option<&'a str>,
    daemon_reachable: bool,
    readback_stale: bool,
}

fn format_fan_diagnostic(input: FanDiagnosticInput<'_>) -> (bool, String) {
    let FanDiagnosticInput {
        fan_count,
        controllable_count,
        average_c,
        safety_c,
        critical_c,
        daemon_version,
        daemon_reachable,
        readback_stale,
    } = input;
    let daemon_update = daemon_version.is_some_and(peterfan_platform::daemon_update_required);
    let daemon = match (daemon_reachable, daemon_version) {
        (true, Some(version)) if daemon_update => format!("daemon v{version} needs update"),
        (true, Some(version)) => format!("daemon v{version} ready"),
        (true, None) => "daemon version unknown".to_string(),
        (false, _) => "daemon not running".to_string(),
    };
    let temp = |value: Option<f32>| {
        value
            .map(|value| format!("{value:.0}°C"))
            .unwrap_or_else(|| "—".to_string())
    };
    let ok = fan_count > 0
        && controllable_count > 0
        && average_c.is_some()
        && daemon_reachable
        && !daemon_update
        && !readback_stale;
    let readback = if readback_stale {
        ", RPM readback stale"
    } else {
        ", RPM readback healthy"
    };
    (
        ok,
        format!(
            "diagnostic: fans {controllable_count}/{fan_count}, CPU avg {}, safety {}, limit {critical_c:.0}°C, {daemon}{readback}",
            temp(average_c),
            temp(safety_c),
        ),
    )
}

fn run_fan_diagnostic(provider: &dyn HardwareProvider) -> (bool, String) {
    let temps = provider.temperatures().unwrap_or_default();
    let fans = provider.fans().unwrap_or_default();
    let controllable_count = fans.iter().filter(|fan| fan.controllable).count();
    let config = peterfan_platform::config::load();
    let daemon_json = daemon_temps_json();
    let readback_stale = daemon_json.as_ref().is_some_and(|value| {
        value
            .pointer("/control_health/stale_fan_ids")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|ids| !ids.is_empty())
            || value
                .pointer("/control_health/failsafe_active")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false)
    });
    let daemon_version = peterfan_platform::installed_daemon_version();
    format_fan_diagnostic(FanDiagnosticInput {
        fan_count: fans.len(),
        controllable_count,
        average_c: representative_temperature_c(&temps),
        safety_c: safety_temperature_c(&temps),
        critical_c: config.critical_temp_c,
        daemon_version: daemon_version.as_deref(),
        daemon_reachable: peterfan_platform::daemon_reachable(),
        readback_stale,
    })
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
                let temp = representative_temperature_c(&temps).unwrap_or(0.0);
                let duty = p.default_curve().duty_at(temp);
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
        Ok(()) => {
            // Mirror the daemon's own bookkeeping locally so the UI's per-fan
            // "manual" flag survives past the next tick even without a
            // daemon running to ask (see `local_fan_overrides`).
            if cmd == "auto" || cmd.starts_with("profile:") || cmd.starts_with("hold:") {
                clear_local_fan_overrides();
            } else if let Some(rest) = cmd.strip_prefix("fanhold:") {
                if let Some((id, duty)) = rest
                    .rsplit_once(':')
                    .and_then(|(id, pct)| pct.parse::<u8>().ok().map(|d| (id, d.min(100))))
                {
                    set_local_fan_override(id, Some(duty));
                }
            } else if let Some(id) = cmd.strip_prefix("fanauto:") {
                set_local_fan_override(id, None);
            }
            format!("{label} — applied locally")
        }
        Err(CoreError::PermissionDenied(_)) => "start peterfand (needs root)".into(),
        Err(e) => format!("error: {e}"),
    }
}

/// Read the local per-fan-pin shadow state (see `LOCAL_FAN_OVERRIDES`).
fn local_fan_overrides() -> std::collections::HashMap<String, u8> {
    LOCAL_FAN_OVERRIDES
        .lock()
        .expect("local fan overrides poisoned")
        .clone()
        .unwrap_or_default()
}

fn set_local_fan_override(id: &str, pct: Option<u8>) {
    let mut guard = LOCAL_FAN_OVERRIDES
        .lock()
        .expect("local fan overrides poisoned");
    let map = guard.get_or_insert_with(std::collections::HashMap::new);
    match pct {
        Some(d) => {
            map.insert(id.to_string(), d);
        }
        None => {
            map.remove(id);
        }
    }
}

fn clear_local_fan_overrides() {
    *LOCAL_FAN_OVERRIDES
        .lock()
        .expect("local fan overrides poisoned") = Some(std::collections::HashMap::new());
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
    if let Some(reply) = result.strip_prefix("daemon:") {
        // The daemon's own reply is forwarded verbatim after this prefix
        // (see `execute_control`) — an incompatible/older daemon can reply
        // "error: unknown command" here, which still starts with "daemon:"
        // and must not be reported as success just because *a* reply came
        // back. Mirror the popover's own error-detection wording.
        let lower = reply.to_lowercase();
        return ![
            "error",
            "invalid",
            "unknown",
            "failed",
            "needs root",
            "needs at least",
        ]
        .iter()
        .any(|kw| lower.contains(kw));
    }
    result.contains("applied")
}

#[cfg(target_os = "macos")]
fn login_item_installed() -> bool {
    peterfan_platform::login_item::is_installed()
}

#[cfg(not(target_os = "macos"))]
fn login_item_installed() -> bool {
    false
}

#[cfg(target_os = "macos")]
fn login_item_supported() -> bool {
    true
}

#[cfg(not(target_os = "macos"))]
fn login_item_supported() -> bool {
    false
}

#[cfg(target_os = "macos")]
fn daemon_install_metadata() -> (bool, &'static str, bool) {
    let binary = peterfan_platform::daemon_install::DAEMON_BIN;
    let plist = peterfan_platform::daemon_install::DAEMON_PLIST;
    (
        Path::new(binary).is_file(),
        binary,
        Path::new(plist).is_file(),
    )
}

#[cfg(not(target_os = "macos"))]
fn daemon_install_metadata() -> (bool, &'static str, bool) {
    (false, "", false)
}

fn toggle_login_item() -> String {
    #[cfg(target_os = "macos")]
    {
        if LOGIN_ITEM_TOGGLE_IN_FLIGHT
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return "login item already updating".into();
        }

        let result = if peterfan_platform::login_item::is_installed() {
            match peterfan_platform::login_item::remove() {
                Ok(true) => "startup disabled".to_string(),
                Ok(false) => "startup already disabled".to_string(),
                Err(e) => format!("login item disable failed: {e}"),
            }
        } else {
            match peterfan_platform::login_item::install(None, "temp") {
                Ok((_bin, _plist)) => "startup enabled".to_string(),
                Err(e) => format!("login item enable failed: {e}"),
            }
        };

        LOGIN_ITEM_TOGGLE_IN_FLIGHT.store(false, Ordering::SeqCst);
        result
    }

    #[cfg(not(target_os = "macos"))]
    {
        "startup toggle is not available on this platform".to_string()
    }
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

/// Run the one-time privileged daemon install (macOS admin-password dialog)
/// from the menu bar directly — a GUI-only user never has to open a
/// terminal. Blocks on the dialog, so it must run off the event-loop thread.
#[cfg(target_os = "macos")]
fn install_fan_control() {
    use peterfan_platform::daemon_install::InstallOutcome;
    // compare_exchange (not a plain store) so a second concurrent call —
    // fired from the other window before this one's dialog even appears —
    // finds the flag already set and bails instead of piling on a second
    // admin-password prompt.
    if INSTALL_FAN_CONTROL_IN_FLIGHT
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return;
    }
    let old_version = cached_installed_daemon_version();
    let updating_existing = old_version
        .as_deref()
        .is_some_and(peterfan_platform::daemon_update_required);
    clear_daemon_version_cache();
    let install_result = if updating_existing {
        peterfan_platform::daemon_install::reinstall_via_running_daemon(false)
            .or_else(|_| peterfan_platform::daemon_install::install(false))
    } else {
        peterfan_platform::daemon_install::install(false)
    };
    let (ok, message) = match install_result {
        Ok(InstallOutcome::Installed) => {
            clear_daemon_version_cache();
            persist_clear_daemon_update_prompt_state();
            let installed_version = cached_installed_daemon_version()
                .unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string());
            if updating_existing {
                (
                    true,
                    format!("Fan control reinstalled — daemon is now v{installed_version}."),
                )
            } else {
                (
                    true,
                    "Fan control enabled — the daemon is running.".to_string(),
                )
            }
        }
        Ok(InstallOutcome::InstalledButUnreachable) => (
            false,
            "Installed, but the daemon isn't answering yet — check /var/log/peterfand.err".into(),
        ),
        Ok(InstallOutcome::DryRun(_)) => unreachable!("menu bar never passes dry_run=true"),
        Err(e) => (false, e),
    };
    INSTALL_FAN_CONTROL_IN_FLIGHT.store(false, Ordering::SeqCst);
    record_fan_action(
        if updating_existing {
            "reinstall fan control"
        } else {
            "enable fan control"
        },
        &message,
        ok,
    );
    *STATUS.lock().expect("status poisoned") = message.clone();
    notify_control_result(
        if updating_existing {
            "Reinstall Fan Control"
        } else {
            "Enable Fan Control"
        },
        ok,
        &message,
    );
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

fn should_auto_prompt_first_run_setup_on_launch() -> bool {
    false
}

#[cfg(target_os = "macos")]
fn stale_daemon_version() -> Option<String> {
    if !peterfan_platform::daemon_reachable() {
        return None;
    }
    let version = peterfan_platform::installed_daemon_version()?;
    if peterfan_platform::daemon_update_required(&version) {
        Some(version)
    } else {
        None
    }
}
#[cfg(not(target_os = "macos"))]
fn stale_daemon_version() -> Option<String> {
    None
}

fn should_prompt_stale_daemon_update(
    _cfg: &peterfan_core::config::Config,
    _current_version: &str,
    _now_unix: u64,
) -> bool {
    false
}

fn should_auto_prompt_stale_daemon_update_on_launch() -> bool {
    false
}

/// After an app update, the bundled helper may be newer while the root
/// LaunchDaemon remains whatever was previously installed. Only surface this
/// when the installed daemon is below the minimum version this app actually
/// requires; UI-only releases should not ask for an admin password.
#[cfg(target_os = "macos")]
fn maybe_prompt_stale_daemon_update() {
    std::thread::sleep(Duration::from_secs(2));
    let cfg = peterfan_platform::config::load();
    if !should_prompt_stale_daemon_update(
        &cfg,
        peterfan_platform::MIN_REQUIRED_DAEMON_VERSION,
        now_unix(),
    ) {
        return;
    }
    let Some(old_version) = stale_daemon_version() else {
        return;
    };

    let lang = cfg.menubar.language.resolve();
    let (title, message, dont_ask, not_now, update) = match lang {
        ResolvedLanguage::Ko => (
            "PeterFan — 팬 제어 재설치",
            format!(
                "이 Mac에 설치된 팬 제어 데몬은 v{old_version}입니다. 이 PeterFan 앱은 팬 제어 데몬 v{} 이상이 필요합니다.\n\n지금 팬 제어를 재설치할까요? macOS가 관리자 암호를 한 번 요청합니다.",
                peterfan_platform::MIN_REQUIRED_DAEMON_VERSION
            ),
            "다시 묻지 않기",
            "나중에",
            "팬 제어 재설치",
        ),
        ResolvedLanguage::En => (
            "PeterFan — Reinstall Fan Control",
            format!(
                "The fan-control daemon installed on this Mac is v{old_version}. This PeterFan app requires fan-control daemon v{} or newer.\n\nReinstall fan control now? macOS will ask for your password once.",
                peterfan_platform::MIN_REQUIRED_DAEMON_VERSION
            ),
            "Don't Ask Again",
            "Not Now",
            "Reinstall Fan Control",
        ),
    };
    let script = format!(
        r#"display dialog {} with title {} buttons {{{}, {}, {}}} default button {} cancel button {}"#,
        applescript_quote(&message),
        applescript_quote(title),
        applescript_quote(dont_ask),
        applescript_quote(not_now),
        applescript_quote(update),
        applescript_quote(update),
        applescript_quote(not_now),
    );
    let output = std::process::Command::new("osascript")
        .arg("-e")
        .arg(script)
        .output();

    let Ok(output) = output else { return };
    let stdout = String::from_utf8_lossy(&output.stdout);
    if stdout.contains(update) {
        install_fan_control();
    } else if stdout.contains(dont_ask) {
        let mut cfg = peterfan_platform::config::load();
        cfg.menubar.daemon_update_prompt_dismissed_for =
            Some(peterfan_platform::MIN_REQUIRED_DAEMON_VERSION.to_string());
        let _ = peterfan_platform::config::save(&cfg);
    } else if stdout.contains(not_now) {
        let mut cfg = peterfan_platform::config::load();
        cfg.menubar.daemon_update_prompt_snoozed_until_unix = Some(now_unix() + 24 * 60 * 60);
        let _ = peterfan_platform::config::save(&cfg);
    }
}
#[cfg(not(target_os = "macos"))]
fn maybe_prompt_stale_daemon_update() {}

/// Silent background check, run once after launch. It deliberately does not
/// open a dialog; menu-bar apps launched at login should not steal focus.
/// The manual "Check for Updates…" action still presents the installer flow.
#[cfg(target_os = "macos")]
fn check_for_updates_on_launch() {
    std::thread::sleep(Duration::from_secs(20));
    if let Ok(release) = peterfan_platform::updater::fetch_latest_release() {
        if peterfan_platform::updater::is_newer(env!("CARGO_PKG_VERSION"), &release.version) {
            *STATUS.lock().expect("status poisoned") = format!(
                "update available: {} (use Check for Updates)",
                release.version
            );
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
    if release.asset_url.is_none() {
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
    }

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
        match peterfan_platform::updater::download_and_install_release(release) {
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

fn to_vec(hist: &VecDeque<f32>) -> Vec<f32> {
    hist.iter().copied().collect()
}

fn runner_frame_interval(cpu_pct: f32) -> Duration {
    let load = cpu_pct.clamp(0.0, 100.0) / 100.0;
    let min_ms = RUNNER_MIN_INTERVAL.as_millis() as f32;
    let max_ms = RUNNER_MAX_INTERVAL.as_millis() as f32;
    // Ease-out curve: modest load is visibly faster, while the upper range
    // has enough separation to feel like a sprint rather than a color change.
    let idle_weight = (1.0 - load).powi(2);
    let ms = min_ms + (max_ms - min_ms) * idle_weight;
    Duration::from_millis(ms.round() as u64)
}

fn runner_enabled(display: MenubarDisplay) -> bool {
    matches!(display, MenubarDisplay::Graph | MenubarDisplay::Both)
}

fn runner_load_band(cpu_pct: f32) -> usize {
    match cpu_pct.clamp(0.0, 100.0) {
        x if x < 20.0 => 0,
        x if x < 55.0 => 1,
        x if x < 80.0 => 2,
        _ => 3,
    }
}

fn runner_icon_index(cpu_pct: f32, frame: u8) -> usize {
    runner_load_band(cpu_pct) * 4 + usize::from(frame % 4)
}

fn make_runner_icons() -> Vec<Icon> {
    [0.0, 30.0, 65.0, 90.0]
        .into_iter()
        .flat_map(|cpu| (0..4).map(move |frame| make_runner_icon(cpu, frame)))
        .collect()
}

fn apply_runner_icon(app: &mut App) {
    let desired = runner_enabled(app.display)
        .then(|| runner_icon_index(app.runner_cpu_pct, app.runner_frame));
    if desired == app.last_runner_icon {
        return;
    }

    let icon = desired.and_then(|index| app.runner_icons.get(index).cloned());
    if let Some(tray) = &app.tray {
        if tray.set_icon_with_as_template(icon, false).is_ok() {
            app.last_runner_icon = desired;
        }
    }
}

#[cfg(test)]
fn menubar_runner_icon(cpu_pct: f32, frame: u8) -> Icon {
    make_runner_icon(cpu_pct, frame)
}

fn make_runner_icon(cpu_pct: f32, frame: u8) -> Icon {
    let rgba = make_cat_runner_rgba(cpu_pct, frame);
    Icon::from_rgba(rgba, 32, 32).expect("valid icon")
}

fn make_cat_runner_rgba(cpu_pct: f32, frame: u8) -> Vec<u8> {
    const W: u32 = 32;
    const H: u32 = 32;
    let mut rgba = vec![0u8; (W * H * 4) as usize];

    let (r, g, b) = match cpu_pct.clamp(0.0, 100.0) {
        x if x < 20.0 => (91u8, 157u8, 255u8), // calm blue
        x if x < 55.0 => (48u8, 209u8, 88u8),  // green
        x if x < 80.0 => (255u8, 214u8, 10u8), // yellow
        _ => (255u8, 69u8, 58u8),              // red
    };

    let phase = frame % 4;
    let stride = match phase {
        0 => -2.4,
        1 => 1.8,
        2 => 2.4,
        _ => -1.8,
    };
    let bounce = if matches!(phase, 1 | 3) { -0.8 } else { 0.0 };
    let tail_lift = if matches!(phase, 0 | 1) { -2.0 } else { 1.4 };

    draw_ellipse(
        &mut rgba,
        W,
        H,
        Pt::new(15.2, 17.2 + bounce),
        8.4,
        5.2,
        (r, g, b, 238),
    );
    draw_disc(
        &mut rgba,
        W,
        H,
        Pt::new(23.0, 13.0 + bounce),
        4.1,
        (r, g, b, 245),
    );
    draw_triangle(
        &mut rgba,
        W,
        H,
        Pt::new(20.4, 10.4 + bounce),
        Pt::new(21.7, 6.6 + bounce),
        Pt::new(23.3, 10.6 + bounce),
        (r, g, b, 235),
    );
    draw_triangle(
        &mut rgba,
        W,
        H,
        Pt::new(24.0, 10.3 + bounce),
        Pt::new(26.0, 7.0 + bounce),
        Pt::new(26.4, 11.3 + bounce),
        (r, g, b, 235),
    );
    draw_line(
        &mut rgba,
        W,
        H,
        Pt::new(8.4, 15.0 + bounce),
        Pt::new(4.8, 11.0 + tail_lift),
        2.7,
        (r, g, b, 235),
    );
    draw_line(
        &mut rgba,
        W,
        H,
        Pt::new(4.8, 11.0 + tail_lift),
        Pt::new(3.4, 6.7 + tail_lift * 0.6),
        2.5,
        (r, g, b, 225),
    );
    draw_line(
        &mut rgba,
        W,
        H,
        Pt::new(10.2, 20.5 + bounce),
        Pt::new(8.4 + stride, 27.0),
        2.3,
        (r, g, b, 220),
    );
    draw_line(
        &mut rgba,
        W,
        H,
        Pt::new(14.4, 21.0 + bounce),
        Pt::new(14.1 - stride, 27.0),
        2.3,
        (r, g, b, 220),
    );
    draw_line(
        &mut rgba,
        W,
        H,
        Pt::new(18.6, 20.5 + bounce),
        Pt::new(19.2 + stride, 26.6),
        2.3,
        (r, g, b, 220),
    );
    draw_line(
        &mut rgba,
        W,
        H,
        Pt::new(22.0, 18.4 + bounce),
        Pt::new(23.8 - stride, 25.8),
        2.1,
        (r, g, b, 220),
    );
    draw_disc(
        &mut rgba,
        W,
        H,
        Pt::new(24.2, 12.5 + bounce),
        0.8,
        (0, 0, 0, 150),
    );
    draw_disc(
        &mut rgba,
        W,
        H,
        Pt::new(27.2, 13.5 + bounce),
        0.9,
        (r, g, b, 235),
    );
    draw_line(
        &mut rgba,
        W,
        H,
        Pt::new(5.0, 28.0),
        Pt::new(28.0, 28.0),
        1.0,
        (r, g, b, 65),
    );

    rgba
}

#[derive(Clone, Copy)]
struct Pt {
    x: f32,
    y: f32,
}

impl Pt {
    fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

fn draw_disc(rgba: &mut [u8], w: u32, h: u32, center: Pt, radius: f32, color: (u8, u8, u8, u8)) {
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

fn draw_ellipse(
    rgba: &mut [u8],
    w: u32,
    h: u32,
    center: Pt,
    rx: f32,
    ry: f32,
    color: (u8, u8, u8, u8),
) {
    let min_x = (center.x - rx - 1.0).floor().max(0.0) as u32;
    let max_x = (center.x + rx + 1.0).ceil().min((w - 1) as f32) as u32;
    let min_y = (center.y - ry - 1.0).floor().max(0.0) as u32;
    let max_y = (center.y + ry + 1.0).ceil().min((h - 1) as f32) as u32;
    for y in min_y..=max_y {
        for x in min_x..=max_x {
            let dx = (x as f32 - center.x) / rx;
            let dy = (y as f32 - center.y) / ry;
            let dist = (dx * dx + dy * dy).sqrt();
            if dist <= 1.08 {
                blend_pixel(rgba, w, x, y, color, (1.08 - dist).clamp(0.0, 1.0));
            }
        }
    }
}

fn draw_triangle(rgba: &mut [u8], w: u32, h: u32, a: Pt, b: Pt, c: Pt, color: (u8, u8, u8, u8)) {
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

fn draw_line(
    rgba: &mut [u8],
    w: u32,
    h: u32,
    from: Pt,
    to: Pt,
    thickness: f32,
    color: (u8, u8, u8, u8),
) {
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
            .replace(">Status<", ">상태<")
            .replace(">Memory<", ">메모리<")
            .replace(">Storage<", ">저장공간<")
            .replace(">Temperature<", ">온도<")
            .replace(">Fan<", ">팬<")
            .replace(">Fans<", ">팬<")
            .replace(">Battery<", ">배터리<")
            .replace(">Network<", ">네트워크<")
            .replace(">Top Processes<", ">실행 중 프로세스<")
            .replace(">MEM<", ">메모리<")
            .replace(">Ready<", ">준비 완료<")
            .replace(">Set Up<", ">설정<")
            .replace(">Settings<", ">설정<")
            .replace(">General Settings<", ">일반 설정<")
            .replace(">App Preferences<", ">앱 설정<")
            .replace(">Fan Control Health<", ">팬 제어 상태<")
            .replace(">Technical details<", ">기술 정보<")
            .replace(">Hardware Availability<", ">하드웨어 감지 상태<")
            .replace(">Daemon<", ">데몬<")
            .replace(">Control Path<", ">제어 경로<")
            .replace(">Last Command<", ">마지막 명령<")
            .replace(">Safety State<", ">안전 상태<")
            .replace(">Sensor Failures<", ">센서 실패<")
            .replace(">Fan Write Failures<", ">팬 쓰기 실패<")
            .replace(">Fan RPM Verification<", ">팬 RPM 검증<")
            .replace(">Control Retry<", ">제어 재시도<")
            .replace(">Last Control Error<", ">마지막 제어 오류<")
            .replace(">Fans Detected<", ">감지된 팬<")
            .replace(">Admin Approval<", ">관리자 승인<")
            .replace(">App<", ">앱<")
            .replace(">Update<", ">업데이트<")
            .replace(">Current<", ">현재<")
            .replace(">Latest<", ">최신<")
            .replace(">Release Notes<", ">릴리즈 노트<")
            .replace(">Auto<", ">자동<")
            .replace(">Silent<", ">저소음<")
            .replace(">Balanced<", ">균형<")
            .replace(">Gaming<", ">게임<")
            .replace(">Performance<", ">성능<")
            .replace(">Max<", ">최대<")
            .replace("Open Detailed Window…", "상세 창 열기…")
            .replace(">Quit PeterFan<", ">PeterFan 종료<")
            .replace(">Fan Curve<", ">팬 커브<")
            .replace(">Curve Input<", ">커브 입력<")
            .replace(">Core Hottest<", ">코어 최고<")
            .replace(">Safety Hottest<", ">안전 최고<")
            .replace(">Critical Limit<", ">임계값<")
            .replace(">Helper<", ">도우미<")
            .replace(">Recent Fan Actions<", ">최근 팬 제어 이력<")
            .replace(">Run Diagnostics<", ">진단 실행<")
            .replace(">No fan actions yet<", ">팬 제어 이력 없음<")
            .replace(">Detail<", ">상세<")
            .replace(">Updates<", ">업데이트<")
            .replace(">System<", ">시스템<")
            .replace(">Quit<", ">종료<")
            .replace(">System Metrics<", ">시스템 지표<")
            .replace(">Live<", ">실시간<")
            .replace(">Open Detail Window<", ">상세 창 열기<")
            .replace(">Open Detail Window…<", ">상세 창 열기…<")
            .replace(
                "Storage, battery, network, and active processes.",
                "저장공간, 배터리, 네트워크와 실행 중인 프로세스를 확인합니다.",
            )
            .replace(
                "No controllable fans detected. PeterFan can still monitor CPU, memory, temperature, and network activity.",
                "제어 가능한 팬을 찾지 못했습니다. PeterFan은 CPU, 메모리, 온도, 네트워크 상태는 계속 모니터링합니다.",
            )
            .replace(
                "Manage startup and fan-control safety.",
                "시작 동작과 팬 제어 안전 상태를 관리합니다.",
            )
            .replace(">Startup<", ">시작 설정<")
            .replace("Start on login", "Run on startup")
            .replace("Run PeterFan automatically on startup.", "부팅 시 PeterFan을 자동으로 실행합니다.")
            .replace(">Selected point<", ">선택한 점<")
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
:root{--g:#30d158;--y:#ffd60a;--r:#ff453a;--accent:#5b9dff;--text:#f4f6fa;--dim:#7f8896;--line:rgba(255,255,255,.07);--panel-bg:#1b1b1d;--panel-border:rgba(255,255,255,.09);--chip-bg:rgba(255,255,255,.06);--chip-hover:rgba(91,157,255,.28);--track:rgba(255,255,255,.08);--track-hover:rgba(255,255,255,.06);--shadow:0 20px 50px rgba(0,0,0,.45),0 2px 10px rgba(0,0,0,.3);--content-x:15px;--section-y:11px;--panel-pad:14px;}
@media (prefers-color-scheme: light){
:root{--text:#1c1e21;--dim:#6b7280;--line:rgba(0,0,0,.08);--panel-bg:#f7f8fa;--panel-border:rgba(0,0,0,.09);--chip-bg:rgba(0,0,0,.05);--chip-hover:rgba(59,130,246,.16);--track:rgba(0,0,0,.08);--track-hover:rgba(0,0,0,.05);--shadow:0 20px 46px rgba(0,0,0,.16),0 2px 8px rgba(0,0,0,.08);}
}
*{box-sizing:border-box;margin:0;padding:0;}
html,body{background:var(--panel-bg);font-family:-apple-system,system-ui,sans-serif;color:var(--text);-webkit-user-select:none;cursor:default;-webkit-font-smoothing:antialiased;overflow:hidden;}
.panel{background:var(--panel-bg);border:1px solid var(--panel-border);border-radius:13px;overflow:hidden;box-shadow:var(--shadow);max-height:100vh;}
.dashboard-shell{display:grid;grid-template-columns:minmax(0,1fr) 78px;gap:8px;padding:7px;height:100vh;max-height:100vh;}
.main-pane{min-width:0;min-height:0;max-height:calc(100vh - 14px);border:1px solid var(--line);border-radius:9px;overflow-y:auto;overflow-x:hidden;scrollbar-gutter:stable;scrollbar-width:none;background:rgba(255,255,255,.015);contain:layout paint;}
.main-pane::-webkit-scrollbar{display:none;}
body.compact .compact-extra{display:none!important;}
body.compact[data-rail-view="more"] .compact-extra{display:grid!important;}
body.compact[data-rail-view="more"] .foot.compact-extra{display:block!important;}
.action-rail{display:flex;flex-direction:column;gap:7px;align-self:start;contain:layout paint;}
.rail-btn{height:56px;width:100%;display:flex;flex-direction:column;align-items:center;justify-content:center;gap:5px;background:var(--chip-bg);border:1px solid var(--panel-border);border-radius:8px;color:var(--text);font:inherit;font-size:10px;font-weight:700;cursor:pointer;color-scheme:inherit;}
.rail-btn:hover{background:var(--chip-hover);border-color:rgba(91,157,255,.35);}
.rail-btn:active{background:rgba(91,157,255,.24);}
.rail-btn svg{width:21px;height:21px;fill:none;stroke:currentColor;stroke-width:1.7;stroke-linecap:round;stroke-linejoin:round;color:var(--text);}
.rail-btn.active{background:rgba(91,157,255,.16);border-color:rgba(91,157,255,.38);color:var(--accent);}
.setup{position:relative;display:flex;justify-content:space-between;align-items:center;gap:10px;padding:10px var(--content-x);border-bottom:1px solid var(--line);}
.setup-main{display:flex;align-items:center;gap:6px;font-size:11px;font-weight:700;}
.setup-dot{width:7px;height:7px;border-radius:50%;background:var(--dim);box-shadow:0 0 0 3px transparent;flex:0 0 auto;}
.setup-dot.ok{background:var(--g);box-shadow:0 0 0 3px rgba(48,209,88,.12);}
.setup-dot.info{background:var(--accent);box-shadow:0 0 0 3px rgba(91,157,255,.12);}
.setup-dot.warn{background:var(--y);box-shadow:0 0 0 3px rgba(255,214,10,.14);}
.setup-copy{min-width:0;}
.setup-sub{font-size:10px;color:var(--dim);margin-top:1px;font-variant-numeric:tabular-nums;white-space:nowrap;overflow:hidden;text-overflow:ellipsis;max-width:215px;}
.setup-actions{display:flex;gap:4px;flex:0 0 auto;}
.setup-actions button{background:var(--chip-bg);border:1px solid transparent;color:var(--dim);font:inherit;font-size:9px;font-weight:700;padding:4px 7px;border-radius:6px;cursor:pointer;white-space:nowrap;transition:background .15s,color .15s,border-color .15s;}
.setup-actions button:hover{background:var(--chip-hover);color:var(--text);}
.setup-actions button.primary{background:rgba(91,157,255,.22);border-color:rgba(91,157,255,.5);color:var(--accent);}
.setup-actions button.active{color:var(--g);border-color:rgba(48,209,88,.35);}
.setup-menu-wrap{position:relative;}
.setup-actions .setup-more{width:24px;height:24px;padding:0;font-size:15px;line-height:20px;color:var(--text);}
.setup-menu{display:none;position:absolute;right:0;top:29px;min-width:142px;padding:5px;background:var(--panel-bg);border:1px solid var(--panel-border);border-radius:8px;box-shadow:var(--shadow);z-index:20;}
.setup-menu.show{display:block;}
.setup-actions .setup-menu-item{display:block;width:100%;padding:6px 8px;border-radius:6px;text-align:left;color:var(--text);font-size:10.5px;font-weight:600;background:transparent;}
.setup-actions .setup-menu-item:hover{background:var(--track-hover);}
.setup-actions .setup-menu-item:focus-visible,.setup-actions .setup-more:focus-visible{outline:2px solid var(--accent);outline-offset:2px;}
.setup-actions .setup-menu-item.active{color:var(--g);border-color:transparent;}
.row{display:grid;grid-template-columns:24px 1fr;gap:12px;padding:var(--section-y) var(--content-x);align-items:center;}
#sec-mem,#sec-temp,#sec-batt,#sec-network,#sec-procs{border-top:1px solid var(--line);}
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
.trow.stale .l,.trow.stale .src,.trow.stale .v{color:var(--dim);opacity:.72;}
.val.stale{color:var(--dim);}
.all-temp-head{font-size:9px;font-weight:700;color:var(--dim);text-transform:uppercase;margin-top:8px;padding-top:7px;border-top:1px solid var(--line);letter-spacing:.08em;cursor:pointer;}
.all-temp-head:hover{color:var(--accent);}
.all-temp-list .trow{font-size:9.5px;margin-top:4px;gap:10px;}
.all-temp-list .trow .l{white-space:nowrap;overflow:hidden;text-overflow:ellipsis;}
.all-temp-list .trow .src{flex:0 0 auto;color:var(--dim);font-size:8px;font-weight:700;letter-spacing:.04em;}
.sensor-group-head{margin-top:8px;padding-top:6px;border-top:1px solid var(--line);color:var(--text);font-size:8.5px;font-weight:750;text-transform:uppercase;letter-spacing:.08em;}
.sensor-group-head:first-child{margin-top:3px;padding-top:0;border-top:0;}
.prow{display:grid;grid-template-columns:1fr auto auto auto;gap:9px;align-items:baseline;font-size:10.5px;margin-top:5px;}
.pkill{opacity:0;background:none;border:0;color:var(--r);font:inherit;font-size:13px;font-weight:700;line-height:1;padding:0 1px;cursor:pointer;transition:opacity .15s;}
.prow:hover .pkill{opacity:1;}
.prow .n{white-space:nowrap;overflow:hidden;text-overflow:ellipsis;}
.prow .c{color:var(--accent);font-weight:600;font-variant-numeric:tabular-nums;white-space:nowrap;}
.prow .m{color:var(--dim);font-variant-numeric:tabular-nums;white-space:nowrap;}
.ctl{padding:10px var(--content-x);border-top:1px solid var(--line);}
.ctl.focus-pulse{background:rgba(91,157,255,.09);box-shadow:inset 0 0 0 1px rgba(91,157,255,.45);}
.ctl-head{display:flex;justify-content:space-between;align-items:baseline;margin-bottom:5px;}
.ctl-head .name{font-size:9.5px;font-weight:600;color:var(--dim);letter-spacing:.08em;text-transform:uppercase;}
.ctl-status{font-size:10px;color:var(--dim);font-variant-numeric:tabular-nums;}
.fan-inputs{display:grid;grid-template-columns:repeat(3,minmax(0,1fr));gap:4px;margin:0 0 7px;}
.fan-input{min-width:0;padding:5px 6px;border:1px solid var(--line);border-radius:6px;background:rgba(255,255,255,.018);}
.fan-input span{display:block;font-size:8px;font-weight:700;color:var(--dim);white-space:nowrap;overflow:hidden;text-overflow:ellipsis;}
.fan-input b{display:block;margin-top:2px;font-size:10px;font-weight:750;font-variant-numeric:tabular-nums;white-space:nowrap;}
.profile-strip{display:grid;grid-template-columns:repeat(6,minmax(0,1fr));gap:4px;margin:3px 0 7px;}
.profile-strip button{min-width:0;background:var(--chip-bg);border:1px solid transparent;color:var(--dim);font:inherit;font-size:9px;font-weight:700;padding:4px 2px;border-radius:6px;cursor:pointer;white-space:nowrap;overflow:hidden;text-overflow:ellipsis;transition:background .15s,color .15s,border-color .15s,opacity .15s;}
.profile-strip button:hover{background:var(--chip-hover);color:var(--text);}
.profile-strip button.active{background:rgba(91,157,255,.2);border-color:rgba(91,157,255,.48);color:var(--accent);}
.profile-strip.disabled button{opacity:.42;pointer-events:none;}
.profile-strip.pending button{cursor:progress;}
.profile-strip.pending button:not(.active){opacity:.48;}
.profile-strip.pending button.active{box-shadow:inset 0 -2px 0 var(--accent);}
.fan-apply-status{min-height:15px;margin:-2px 0 5px;color:var(--dim);font-size:9.5px;line-height:1.45;font-variant-numeric:tabular-nums;white-space:nowrap;overflow:hidden;text-overflow:ellipsis;}
.fan-apply-status.pending{color:var(--accent);}
.fan-apply-status.ok{color:var(--g);}
.fan-apply-status.error{color:var(--r);}
.fan-cards{display:flex;flex-direction:column;}
.fan-card{padding:5px 0;}
.fan-card+.fan-card{border-top:1px solid var(--line);}
.fan-card-head{display:flex;justify-content:space-between;align-items:baseline;font-size:10.5px;margin-bottom:3px;}
.fan-card-head .fn{font-weight:600;}
.fan-card-head .fv{font-variant-numeric:tabular-nums;color:var(--dim);}
.fan-bar{height:3px;background:var(--track);border-radius:99px;position:relative;margin-bottom:4px;}
.fan-bar i{display:block;height:100%;background:var(--accent);border-radius:99px;width:0;transition:width .35s;}
.fan-target-marker{display:none;position:absolute;top:-2px;width:2px;height:7px;margin-left:-1px;border-radius:1px;background:var(--text);box-shadow:0 0 0 1px var(--bg);transition:left .2s;}
.fan-card.ramping .fan-card-head .fv{color:var(--accent);}
.fan-card.stale .fan-card-head .fv{color:var(--y);}
.fan-bottom{display:flex;justify-content:space-between;align-items:center;gap:8px;}
.fan-rpm-text{font-size:9.5px;color:var(--dim);font-variant-numeric:tabular-nums;white-space:nowrap;}
.fan-seg{display:flex;gap:4px;flex:0 0 auto;}
.fan-seg button{background:var(--chip-bg);border:1px solid transparent;color:var(--dim);font:inherit;font-size:9px;font-weight:600;padding:3px 8px;border-radius:5px;cursor:pointer;white-space:nowrap;transition:background .15s,color .15s;}
.fan-seg button.active{background:var(--panel-bg);color:var(--text);border-color:rgba(91,157,255,.4);}
.fan-card.pending .fan-seg button{cursor:progress;}
.fan-card.pending .fan-seg button:not(.active){opacity:.48;}
.fan-rpm-row{display:grid;grid-template-columns:auto 1fr auto auto;gap:6px;align-items:center;margin-top:5px;transition:opacity .15s;}
.fan-rpm-row.inactive{opacity:.35;pointer-events:none;}
.fan-rpm-row span{font-size:9px;color:var(--dim);font-variant-numeric:tabular-nums;white-space:nowrap;}
.fan-rpm-row input[type=range]{-webkit-appearance:none;height:3px;border-radius:99px;background:var(--track);outline:none;cursor:pointer;}
.fan-rpm-row input[type=range]::-webkit-slider-thumb{-webkit-appearance:none;width:14px;height:14px;border-radius:50%;background:var(--accent);cursor:pointer;}
.fan-rpm-row input[type=number]{width:44px;background:var(--track);border:1px solid transparent;border-radius:4px;color:var(--text);font:inherit;font-size:9px;font-variant-numeric:tabular-nums;text-align:center;padding:3px 0;-moz-appearance:textfield;}
.fan-rpm-row input[type=number]::-webkit-inner-spin-button,.fan-rpm-row input[type=number]::-webkit-outer-spin-button{-webkit-appearance:none;margin:0;}
.fan-rpm-row input[type=number]:focus{border-color:var(--accent);outline:none;}
.empty-state{padding:9px 10px;border:1px dashed var(--line);border-radius:8px;color:var(--dim);font-size:10.5px;line-height:1.45;background:rgba(255,255,255,.018);}
.ctl-note{font-size:10.5px;color:var(--dim);line-height:1.5;margin-top:6px;}
.note-fix-btn{margin-top:5px;background:rgba(91,157,255,.22);border:1px solid rgba(91,157,255,.5);color:var(--accent);font:inherit;font-size:10px;font-weight:600;padding:5px 10px;border-radius:6px;cursor:pointer;}
.note-fix-btn:hover{background:rgba(91,157,255,.32);}
#curve-canvas{width:100%;height:120px;display:block;border-radius:6px;background:var(--track);cursor:crosshair;touch-action:none;margin-top:8px;}
.curve-point-row{display:flex;align-items:center;gap:5px;margin-top:8px;font-size:9px;color:var(--dim);font-variant-numeric:tabular-nums;}
.curve-point-row .cpr-arrow{color:var(--dim);}
.curve-point-row input[type=number]{width:40px;background:var(--track);border:1px solid transparent;border-radius:4px;color:var(--text);font:inherit;font-size:9px;font-variant-numeric:tabular-nums;text-align:center;padding:3px 0;-moz-appearance:textfield;}
.curve-point-row input[type=number]::-webkit-inner-spin-button,.curve-point-row input[type=number]::-webkit-outer-spin-button{-webkit-appearance:none;margin:0;}
.curve-point-row input[type=number]:focus{border-color:var(--accent);outline:none;}
.curve-actions{display:flex;gap:6px;margin-top:8px;}
.curve-actions button{flex:1;background:var(--chip-bg);border:1px solid transparent;color:var(--text);font:inherit;font-size:10px;font-weight:600;padding:6px 4px;border-radius:7px;cursor:pointer;transition:background .15s;}
.curve-actions button:hover{background:var(--chip-hover);}
.curve-actions button.primary{background:rgba(91,157,255,.22);border-color:rgba(91,157,255,.5);color:var(--accent);}
.chart{width:100%;height:28px;display:block;margin-top:8px;border-radius:4px;cursor:crosshair;}
.chart-tip{position:fixed;pointer-events:none;background:rgba(20,20,22,.92);color:#fff;font-size:9.5px;font-weight:600;padding:3px 7px;border-radius:5px;display:none;z-index:999;white-space:nowrap;font-variant-numeric:tabular-nums;}
.chart-stats{font-size:9px;color:var(--dim);text-align:right;margin-top:3px;font-variant-numeric:tabular-nums;}
.rail-panel{display:none;padding:var(--panel-pad) var(--content-x);border-bottom:1px solid var(--line);}
.panel-title-row{display:flex;align-items:center;justify-content:space-between;gap:10px;margin-bottom:6px;}
.rail-panel .panel-title{font-size:14px;font-weight:700;min-width:0;}
.panel-pill{display:inline-flex;align-items:center;height:20px;padding:0 8px;border-radius:99px;background:var(--chip-bg);color:var(--dim);font-size:9px;font-weight:800;white-space:nowrap;font-variant-numeric:tabular-nums;}
.panel-pill.ok{background:rgba(48,209,88,.14);color:var(--g);}
.panel-pill.warn{background:rgba(255,214,10,.16);color:var(--y);}
.panel-pill.info{background:rgba(91,157,255,.16);color:var(--accent);}
.rail-panel .panel-copy{font-size:10.5px;color:var(--dim);line-height:1.5;margin-bottom:12px;}
.rail-panel .panel-action{min-height:28px;background:rgba(91,157,255,.22);border:1px solid rgba(91,157,255,.5);color:var(--accent);font:inherit;font-size:10px;font-weight:700;padding:6px 10px;border-radius:7px;cursor:pointer;}
.rail-panel .panel-action.secondary{background:var(--chip-bg);border-color:transparent;color:var(--text);}
.rail-panel .panel-action.danger{background:rgba(255,69,58,.16);border-color:rgba(255,69,58,.36);color:var(--r);}
.rail-panel .panel-actions{display:flex;gap:8px;align-items:center;flex-wrap:wrap;}
.release-notes-card{margin-top:10px;padding:9px 10px;border:1px solid var(--line);border-radius:8px;background:rgba(255,255,255,.025);}
.release-notes-title{font-size:10px;font-weight:800;color:var(--text);margin-bottom:5px;}
.release-notes-body{font-size:9.5px;line-height:1.45;color:var(--dim);white-space:pre-wrap;max-height:118px;overflow:hidden;}
.settings-list{display:flex;flex-direction:column;gap:10px;}
.settings-item{display:flex;align-items:center;justify-content:space-between;gap:10px;padding:9px 10px;border:1px solid var(--line);border-radius:8px;background:rgba(255,255,255,.025);}
.settings-item-title{font-size:11px;font-weight:700;color:var(--text);}
.settings-item-copy{font-size:9.5px;color:var(--dim);line-height:1.35;margin-top:2px;}
.health-card{padding:10px;border:1px solid var(--line);border-radius:8px;background:rgba(255,255,255,.025);}
.health-head{display:flex;align-items:center;justify-content:space-between;gap:10px;margin-bottom:8px;}
.health-title{font-size:11px;font-weight:800;color:var(--text);}
.health-grid{display:grid;grid-template-columns:1fr;gap:6px;}
.health-row{display:flex;align-items:baseline;justify-content:space-between;gap:12px;font-size:10.5px;border-top:1px solid var(--line);padding-top:6px;}
.health-row:first-child{border-top:0;padding-top:0;}
.health-label{color:var(--dim);font-weight:650;}
.health-value{font-weight:650;text-align:right;font-variant-numeric:tabular-nums;white-space:normal;overflow-wrap:anywhere;}
.health-value.ok{color:var(--g);}.health-value.warn{color:var(--y);}.health-value.info{color:var(--accent);}
.health-action{background:var(--chip-bg);border:1px solid transparent;color:var(--accent);font:inherit;font-size:9px;font-weight:750;padding:4px 7px;border-radius:6px;cursor:pointer;white-space:nowrap;}
.health-action:disabled{opacity:.5;cursor:default;}
.health-details{margin-top:8px;padding-top:7px;border-top:1px solid var(--line);}
.health-details summary{color:var(--accent);font-size:10px;font-weight:700;cursor:pointer;list-style-position:inside;}
.health-details[open] summary{margin-bottom:7px;}
.action-log{display:flex;flex-direction:column;gap:6px;}
.action-log-empty{font-size:9.5px;color:var(--dim);}
.action-log-row{display:grid;grid-template-columns:48px minmax(0,1fr);gap:7px;padding-top:6px;border-top:1px solid var(--line);font-size:9.5px;}
.action-log-row:first-child{border-top:0;padding-top:0;}
.action-log-time{color:var(--dim);font-variant-numeric:tabular-nums;}
.action-log-main{min-width:0;}
.action-log-action{font-weight:750;color:var(--text);white-space:nowrap;overflow:hidden;text-overflow:ellipsis;}
.action-log-result{margin-top:2px;color:var(--dim);line-height:1.35;overflow-wrap:anywhere;}
.action-log-row.ok .action-log-action{color:var(--g);}.action-log-row.warn .action-log-action{color:var(--y);}
.foot{border-top:1px solid var(--line);padding:3px;}
.quit{display:block;width:100%;background:transparent;border:0;color:var(--dim);font:inherit;font-size:10.5px;letter-spacing:.02em;padding:8px;border-radius:8px;cursor:pointer;transition:background .15s,color .15s;}
.quit:hover{background:var(--track-hover);color:var(--text);}
.range-tabs{display:flex;gap:4px;padding:10px var(--content-x);justify-content:flex-end;}
.sort-tabs{display:flex;gap:4px;}
.range-tab{background:var(--chip-bg);border:1px solid transparent;color:var(--dim);font:inherit;font-size:9.5px;font-weight:600;padding:3px 9px;border-radius:99px;cursor:pointer;transition:background .15s,color .15s;}
.range-tab:hover{background:var(--chip-hover);}
.range-tab.active{background:rgba(91,157,255,.22);color:var(--accent);}
</style></head><body class="compact" data-rail-view="overview"><div class="panel"><div class="dashboard-shell"><main class="main-pane">

<div class="range-tabs" id="range-tabs">
<button class="range-tab active" data-range="2m" onclick="setChartRange('2m')">2m</button>
<button class="range-tab" data-range="1h" onclick="setChartRange('1h')">1h</button>
<button class="range-tab" data-range="1d" onclick="setChartRange('1d')">1d</button>
</div>

<div class="setup" id="setup-row">
<div class="setup-copy"><div class="setup-main"><span class="setup-dot" id="setup-dot"></span><span id="setup-title">Ready</span></div><div class="setup-sub" id="setup-detail"></div></div>
<div class="setup-actions">
<button id="setup-fan" class="primary" onclick="startFanControlSetup(this)">Set Up</button>
<div class="setup-menu-wrap">
<button id="setup-more" class="setup-more" onclick="toggleSetupMenu(event)" onkeydown="handleSetupMoreKey(event)" aria-label="Setup actions" aria-haspopup="menu" aria-expanded="false" title="Setup actions">…</button>
<div class="setup-menu" id="setup-menu" role="menu" onkeydown="handleSetupMenuKey(event)">
<button class="setup-menu-item" role="menuitem" id="setup-startup" onclick="closeSetupMenu();toggleStartupItem(this)">Start on login</button>
<button class="setup-menu-item" role="menuitem" id="setup-update" onclick="closeSetupMenu();checkAppUpdates(this)">Update</button>
</div>
</div>
</div>
</div>

<div class="rail-panel" id="rail-update-panel">
<div class="panel-title-row"><div class="panel-title">Updates</div><span class="panel-pill info" id="rail-update-pill">Ready</span></div>
<div class="panel-copy" id="rail-update-copy">PeterFan is ready to check for updates.</div>
<div class="health-grid" id="update-version-grid">
<div class="health-row"><span class="health-label">Current</span><span class="health-value" id="update-current-version">—</span></div>
<div class="health-row"><span class="health-label">Latest</span><span class="health-value" id="update-latest-version">—</span></div>
<div class="health-row"><span class="health-label">Status</span><span class="health-value" id="update-check-result">—</span></div>
</div>
<div class="release-notes-card" id="update-release-notes-card" style="display:none"><div class="release-notes-title">Release Notes</div><div class="release-notes-body" id="update-release-notes">—</div></div>
<div class="panel-actions" style="margin-top:10px"><button class="panel-action" id="rail-update-check" onclick="checkAppUpdates(this)">Check Updates</button><button class="panel-action secondary" id="update-release-link" onclick="openLatestRelease()" style="display:none">View Release</button></div>
</div>

<div class="rail-panel" id="rail-settings-panel">
<div class="panel-title-row"><div class="panel-title">General Settings</div><span class="panel-pill info" id="rail-settings-pill">App Preferences</span></div>
<div class="panel-copy">Manage startup and fan-control safety.</div>
<div class="settings-list">
<div class="health-card" id="fan-health-card">
<div class="health-head"><div class="health-title">Fan Control Health</div><span class="panel-pill info" id="health-pill">Ready</span></div>
<div class="health-grid">
<div class="health-row"><span class="health-label">Daemon</span><span class="health-value" id="health-daemon">—</span></div>
<div class="health-row"><span class="health-label">Control Path</span><span class="health-value" id="health-control-path">—</span></div>
<div class="health-row"><span class="health-label">Last Command</span><span class="health-value" id="health-last-command">—</span></div>
<div class="health-row"><span class="health-label">Safety State</span><span class="health-value" id="health-safety-state">—</span></div>
<div class="health-row"><span class="health-label">Fans Detected</span><span class="health-value" id="health-fans">—</span></div>
<div class="health-row"><span class="health-label">Admin Approval</span><span class="health-value" id="health-approval">—</span></div>
<div class="health-row"><span class="health-label">App</span><span class="health-value" id="health-app">—</span></div>
</div>
<details class="health-details"><summary>Technical details</summary><div class="health-grid">
<div class="health-row"><span class="health-label">Helper</span><span class="health-value" id="health-helper">—</span></div>
<div class="health-row"><span class="health-label">LaunchDaemon</span><span class="health-value" id="health-launch-daemon">—</span></div>
<div class="health-row"><span class="health-label">Team ID</span><span class="health-value" id="health-team-id">—</span></div>
<div class="health-row"><span class="health-label">Curve Input</span><span class="health-value" id="health-curve-input">—</span></div>
<div class="health-row"><span class="health-label">Core Hottest</span><span class="health-value" id="health-core-hottest">—</span></div>
<div class="health-row"><span class="health-label">Safety Hottest</span><span class="health-value" id="health-safety-hottest">—</span></div>
<div class="health-row"><span class="health-label">Critical Limit</span><span class="health-value" id="health-critical-limit">—</span></div>
<div class="health-row"><span class="health-label">Sensor Failures</span><span class="health-value" id="health-sensor-failures">—</span></div>
<div class="health-row"><span class="health-label">Fan Write Failures</span><span class="health-value" id="health-write-failures">—</span></div>
<div class="health-row"><span class="health-label">Fan RPM Verification</span><span class="health-value" id="health-readback">—</span></div>
<div class="health-row"><span class="health-label">Control Retry</span><span class="health-value" id="health-control-retry">—</span></div>
<div class="health-row"><span class="health-label">Last Control Error</span><span class="health-value" id="health-control-error">—</span></div>
</div></details>
</div>
<div class="health-card" id="fan-action-log-card">
<div class="health-head"><div class="health-title">Recent Fan Actions</div><button class="health-action" id="fan-diagnostic-button" onclick="runFanDiagnostics(this)">Run Diagnostics</button></div>
<div class="action-log" id="fan-action-log"><div class="action-log-empty">No fan actions yet</div></div>
</div>
<div class="settings-item" id="startup-setting">
<div><div class="settings-item-title">Start on login</div><div class="settings-item-copy">Run PeterFan automatically on startup.</div></div>
<button id="startup-toggle" class="panel-action secondary" onclick="toggleStartupItem(this)">Enable</button>
</div>
</div>
</div>

<div class="rail-panel" id="rail-more-panel">
<div class="panel-title-row"><div class="panel-title">System Metrics</div><span class="panel-pill info" id="rail-more-pill">Live</span></div>
<div class="panel-copy">Storage, battery, network, and active processes.</div>
<div class="health-card" id="hardware-availability-card">
<div class="health-head"><div class="health-title">Hardware Availability</div><span class="panel-pill info" id="hardware-pill">Ready</span></div>
<div class="health-grid">
<div class="health-row"><span class="health-label">Fans Detected</span><span class="health-value" id="hardware-fans">—</span></div>
<div class="health-row"><span class="health-label">Battery</span><span class="health-value" id="hardware-battery">—</span></div>
<div class="health-row"><span class="health-label">Network</span><span class="health-value" id="hardware-network">—</span></div>
</div>
</div>
<div class="panel-actions"><button class="panel-action secondary" onclick="window.ipc.postMessage('open_detail')">Open Detail Window</button></div>
</div>

<div class="ctl" id="fan-control-section" style="border-top:0;border-bottom:1px solid var(--line)">
<div class="ctl-head"><span class="name">Fan control</span><span class="ctl-status" id="ctl-status"></span></div>
<div class="fan-inputs" id="fan-inputs">
<div class="fan-input"><span>Curve Input</span><b id="fan-curve-input">—</b></div>
<div class="fan-input"><span>Safety Hottest</span><b id="fan-safety-hottest">—</b></div>
<div class="fan-input"><span>Critical Limit</span><b id="fan-critical-limit">—</b></div>
</div>
<div class="profile-strip" id="profile-strip">
<button class="active" data-mode="auto" title="Auto" onclick="setAuto()">Auto</button>
<button data-mode="profile" data-profile="silent" title="Silent" onclick="setProfile('silent')">Silent</button>
<button data-mode="profile" data-profile="balanced" title="Balanced" onclick="setProfile('balanced')">Balanced</button>
<button data-mode="profile" data-profile="gaming" title="Gaming" onclick="setProfile('gaming')">Gaming</button>
<button data-mode="profile" data-profile="performance" title="Performance" onclick="setProfile('performance')">Performance</button>
<button data-mode="profile" data-profile="maximum" title="Maximum" onclick="setProfile('maximum')">Max</button>
</div>
<div class="fan-apply-status" id="fan-apply-status"></div>
<div class="fan-cards" id="fan-cards"></div>
<div class="empty-state" id="fan-empty-state" style="display:none">No controllable fans detected. PeterFan can still monitor CPU, memory, temperature, and network activity.</div>
<div class="ctl-note" id="ctl-note" style="display:none"></div>
</div>

<div class="row" id="curve-editor-section" style="display:none;border-bottom:1px solid var(--line)"><span class="ic"><svg viewBox="0 0 24 24"><path d="M3 17l5-6 4 3 9-9"/><path d="M3 21h18"/></svg></span>
<div class="content"><div class="head"><span class="name">Fan Curve</span></div>
<canvas id="curve-canvas"></canvas>
<div class="sub" id="curve-hint">Drag points to reshape. Click empty space to add a point.</div>
<div class="curve-point-row" id="curve-point-row" style="display:none">
<span id="curve-point-label">Selected point</span>
<input type="number" id="cp-temp" min="0" max="100"><span>°C</span>
<span class="cpr-arrow">→</span>
<input type="number" id="cp-duty" min="0" max="100"><span>%</span>
</div>
<div class="curve-actions">
<button onclick="resetCurve()">Reset</button>
<button onclick="removeCurvePoint()">Remove Point</button>
<button class="primary" onclick="saveCurve()">Save &amp; Apply</button>
</div>
</div></div>

<div class="row" id="sec-cpu"><span class="ic"><svg viewBox="0 0 24 24"><rect x="6" y="6" width="12" height="12" rx="2"/><path d="M9 2v3M15 2v3M9 19v3M15 19v3M2 9h3M2 15h3M19 9h3M19 15h3"/></svg></span>
<div class="content"><div class="head"><span class="name">CPU</span><span class="val" id="cpu-val">—</span></div>
<div class="sub" id="cpu-sub"></div><div class="cores" id="cores"></div>
<div class="bar"><div class="bar-fill" id="cpu-bar"></div></div>
<canvas class="chart" id="cpu-chart"></canvas><div class="chart-stats" id="cpu-chart-stats"></div></div></div>

<div class="row" id="sec-mem"><span class="ic"><svg viewBox="0 0 24 24"><rect x="2" y="7" width="20" height="11" rx="1.5"/><path d="M6 18v2M10 18v2M14 18v2M18 18v2M6 10v4M10 10v4M14 10v4"/></svg></span>
<div class="content"><div class="head"><span class="name">Memory</span><span class="val" id="mem-val">—</span></div>
<div class="sub" id="mem-sub"></div><div class="bar"><div class="bar-fill" id="mem-bar"></div></div>
<canvas class="chart" id="mem-chart"></canvas><div class="chart-stats" id="mem-chart-stats"></div></div></div>

<div class="row compact-extra" id="sec-storage" data-compact-extra="storage"><span class="ic"><svg viewBox="0 0 24 24"><ellipse cx="12" cy="6" rx="8" ry="3"/><path d="M4 6v12c0 1.7 3.6 3 8 3s8-1.3 8-3V6"/><path d="M4 12c0 1.7 3.6 3 8 3s8-1.3 8-3"/></svg></span>
<div class="content"><div class="head"><span class="name">Storage</span><span class="val" id="disk-val">—</span></div>
<div class="sub" id="disk-sub"></div><div class="bar"><div class="bar-fill" id="disk-bar"></div></div>
<div class="sub" id="disk-io-sub" style="display:none;margin-top:4px"></div>
<canvas class="chart" id="disk-io-chart" style="display:none"></canvas><div class="chart-stats" id="disk-io-chart-stats"></div></div></div>

<div class="row" id="sec-temp"><span class="ic"><svg viewBox="0 0 24 24"><path d="M14 14.76V5a2 2 0 0 0-4 0v9.76a4 4 0 1 0 4 0z"/></svg></span>
<div class="content"><div class="head"><span class="name" id="temp-name">Temperature</span><span class="val" id="temp-val">—</span></div>
<div class="bar"><div class="bar-fill" id="temp-bar"></div></div><div id="temp-list"></div>
<div class="all-temp-head" id="all-temp-head" onclick="toggleRawTemps()">All sensors</div><div class="all-temp-list" id="all-temp-list"></div>
<canvas class="chart" id="temp-chart"></canvas><div class="chart-stats" id="temp-chart-stats"></div></div></div>

<div class="row compact-extra" id="sec-batt" data-compact-extra="battery"><span class="ic"><svg viewBox="0 0 24 24"><rect x="2" y="8" width="18" height="9" rx="2"/><path d="M22 11v3"/></svg></span>
<div class="content"><div class="head"><span class="name">Battery</span><span class="val" id="batt-val">—</span></div>
<div class="sub" id="batt-sub"></div><div class="bar"><div class="bar-fill" id="batt-bar"></div></div></div></div>

<div class="row compact-extra" id="sec-network" data-compact-extra="network"><span class="ic"><svg viewBox="0 0 24 24"><circle cx="12" cy="12" r="9"/><path d="M3 12h18M12 3c2.5 2.5 2.5 15 0 18M12 3c-2.5 2.5-2.5 15 0 18"/></svg></span>
<div class="content"><div class="head"><span class="name">Network</span><span class="val"></span></div>
<div class="sub" id="net-sub"></div>
<div class="sub" id="net-ip" style="display:none"></div>
<canvas class="chart" id="net-chart"></canvas><div class="chart-stats" id="net-chart-stats"></div></div></div>

<div class="row compact-extra" id="sec-procs" data-compact-extra="processes"><span class="ic"><svg viewBox="0 0 24 24"><rect x="3" y="4" width="18" height="16" rx="2"/><path d="M3 9h18M8 4v5"/></svg></span>
<div class="content"><div class="head"><span class="name">Top Processes</span><span class="sort-tabs"><button class="range-tab" id="ps-cpu" onclick="setProcSort('cpu')">CPU</button><button class="range-tab" id="ps-mem" onclick="setProcSort('mem')">MEM</button></span></div>
<div id="procs-list"></div></div></div>

<div class="foot compact-extra" data-compact-extra="quit"><button class="quit" onclick="window.ipc.postMessage('quit')">Quit PeterFan</button></div>
</main>
<aside class="action-rail" aria-label="Quick actions">
<button class="rail-btn active" id="railDetail" data-rail-action="detail" aria-pressed="true" onclick="runRailAction('detail',this)" title="Status overview"><svg viewBox="0 0 24 24"><rect x="4" y="5" width="16" height="14" rx="2"/><path d="M8 10h8M8 14h5"/></svg><span>Status</span></button>
<button class="rail-btn" id="railFan" data-rail-action="fan" aria-pressed="false" onclick="runRailAction('fan',this)" title="Fan control"><svg viewBox="0 0 24 24"><circle cx="12" cy="12" r="2.2"/><path d="M12 4c3 0 4.5 2 3 4.5L12 12M20 12c0 3-2 4.5-4.5 3L12 12M12 20c-3 0-4.5-2-3-4.5L12 12M4 12c0-3 2-4.5 4.5-3L12 12"/></svg><span>Fan</span></button>
<button class="rail-btn" id="railUpdate" data-rail-action="update" aria-pressed="false" onclick="runRailAction('update',this)" title="Check for Updates…"><svg viewBox="0 0 24 24"><path d="M4 12a8 8 0 0 1 13.7-5.6"/><path d="M18 3v5h-5"/><path d="M20 12a8 8 0 0 1-13.7 5.6"/><path d="M6 21v-5h5"/></svg><span>Updates</span></button>
<button class="rail-btn" id="railSettings" data-rail-action="settings" aria-pressed="false" onclick="runRailAction('settings',this)" title="Settings"><svg viewBox="0 0 24 24"><path d="M12 15.5a3.5 3.5 0 1 0 0-7 3.5 3.5 0 0 0 0 7z"/><path d="M19.4 15a1.7 1.7 0 0 0 .3 1.9l.1.1-2 3.4-.2-.1a1.7 1.7 0 0 0-1.9-.1 8 8 0 0 1-1.4.8 1.7 1.7 0 0 0-1.1 1.5V23h-4v-.5A1.7 1.7 0 0 0 8.1 21a8 8 0 0 1-1.4-.8 1.7 1.7 0 0 0-1.9.1l-.2.1-2-3.4.1-.1A1.7 1.7 0 0 0 3 15a8.6 8.6 0 0 1 0-1.7 1.7 1.7 0 0 0-.3-1.9l-.1-.1 2-3.4.2.1a1.7 1.7 0 0 0 1.9.1A8 8 0 0 1 8.1 7a1.7 1.7 0 0 0 1.1-1.5V5h4v.5A1.7 1.7 0 0 0 14.3 7a8 8 0 0 1 1.4.8 1.7 1.7 0 0 0 1.9-.1l.2-.1 2 3.4-.1.1a1.7 1.7 0 0 0-.3 1.9 8.6 8.6 0 0 1 0 2z"/></svg><span>Settings</span></button>
<button class="rail-btn" id="railMore" data-rail-action="more" aria-pressed="false" onclick="runRailAction('more',this)" title="System metrics"><svg viewBox="0 0 24 24"><circle cx="12" cy="12" r="9"/><path d="M3 12h18M12 3c2.5 2.5 2.5 15 0 18M12 3c-2.5 2.5-2.5 15 0 18"/></svg><span>System</span></button>
</aside></div></div>
<div class="chart-tip" id="chart-tip"></div>
<script>
var LANG='__LANG__';
var SHOW_CURVE_EDITOR='__SHOWCURVE__';
var FAN_CONTROL_FIX_PENDING=false;
var FAN_DIAGNOSTIC_PENDING=false;
var FAN_DIAGNOSTIC_STARTED_AT=0;
var LOGIN_ITEM_TOGGLE_PENDING=false;
var APP_UPDATE_CHECK_PENDING=false;
var APP_UPDATE_STATUS=null;
var APP_PERSISTED_UPDATE_KEY='';
var FAN_CONTROL_PENDING=null;
var FAN_CONTROL_RESULT=null;
if(!('__pf_pending' in window))window.__pf_pending=null;
function applyPendingUpdate(){
  if(window.__pf&&window.__pf.update&&window.__pf_pending)window.__pf.update(window.__pf_pending);
}
function storageGet(k){
  try{return localStorage.getItem(k);}catch(e){return null;}
}
function storageSet(k,v){
  try{localStorage.setItem(k,v);}catch(e){}
}
function popoverCompact(){
  return storageGet('pf.compact')!=='0';
}
function setPopoverExpanded(expanded){
  storageSet('pf.compact',expanded?'0':'1');
  applyPopoverMode();
}
function togglePopoverExpanded(){
  setPopoverExpanded(popoverCompact());
}
var RAIL_VIEW=storageGet('pf.rail.view')||'overview';
function railView(){
  return RAIL_VIEW||'overview';
}
function setRailView(view){
  RAIL_VIEW=view;
  storageSet('pf.rail.view',view);
  document.body.setAttribute('data-rail-view',view);
  applyRailView(true);
  if(window.__pf&&window.__pf.update&&window.__pf_pending)window.__pf.update(window.__pf_pending);
}
function setVisible(id,on){
  var el=document.getElementById(id);
  if(!el)return;
  if(on&&el.classList.contains('rail-panel'))el.style.display='block';
  else if(on){
    el.style.display='';
  } else {
    el.style.display='none';
  }
}
function setRailButtonActive(id,on){
  var el=document.getElementById(id);
  if(el){
    el.classList.toggle('active',!!on);
    el.setAttribute('aria-pressed',on?'true':'false');
  }
}
function resetRailPaneScroll(){
  var pane=document.querySelector('.main-pane');
  if(pane)pane.scrollTop=0;
}
function applyRailView(resetScroll){
  var view=railView();
  document.body.setAttribute('data-rail-view',view);
  var all=['range-tabs','setup-row','fan-control-section','curve-editor-section','sec-cpu','sec-mem','sec-storage','sec-temp','sec-batt','sec-network','sec-procs','foot','rail-update-panel','rail-settings-panel','rail-more-panel'];
  all.forEach(function(id){setVisible(id,false);});
  if(view==='fan'){
    ['setup-row','fan-control-section'].forEach(function(id){setVisible(id,true);});
    if(SHOW_CURVE_EDITOR==='1')setVisible('curve-editor-section',true);
  } else if(view==='update'){
    setVisible('rail-update-panel',true);
  } else if(view==='settings'){
    setVisible('rail-settings-panel',true);
  } else if(view==='more'){
    ['rail-more-panel','sec-storage','sec-batt','sec-network','sec-procs','foot'].forEach(function(id){setVisible(id,true);});
  } else {
    ['range-tabs','sec-cpu','sec-mem','sec-temp'].forEach(function(id){setVisible(id,true);});
  }
  ['Detail','Fan','Update','Settings','More'].forEach(function(name){
    var key=name.toLowerCase();
    if(key==='detail')key='overview';
    setRailButtonActive('rail'+name,view===key);
  });
  if(resetScroll)resetRailPaneScroll();
  reportHeight();
}
function applyPopoverMode(){
  var compact=true;
  document.body.classList.toggle('compact',compact);
  document.body.classList.toggle('expanded',!compact);
  var more=document.getElementById('railMore');
  if(more){
    more.title=LANG==='ko'?'시스템 지표':'System metrics';
    var label=more.querySelector('span');
    if(label)label.textContent=LANG==='ko'?'시스템':'System';
  }
  reportHeight();
}
applyPopoverMode();
setRailView(railView());
function setButtonLabel(btn,label){
  if(!btn)return;
  var span=btn.querySelector('span');
  if(span)span.textContent=label;
  else btn.textContent=label;
}
function setPanelPill(id,text,tone){
  var el=document.getElementById(id);
  if(!el)return;
  el.textContent=text;
  el.className='panel-pill '+(tone||'');
}
function setText(id,text){
  var el=document.getElementById(id);
  if(el)el.textContent=text||'—';
}
function compareVersions(a,b){
  function parts(v){
    return String(v||'').replace(/^v/,'').split('.').slice(0,3).map(function(p){
      var n=parseInt(p,10);
      return isNaN(n)?0:n;
    });
  }
  var aa=parts(a),bb=parts(b);
  for(var i=0;i<3;i++){
    var av=aa[i]||0,bv=bb[i]||0;
    if(av<bv)return -1;
    if(av>bv)return 1;
  }
  return 0;
}
function formatReleaseNotes(body){
  var raw=String(body||'').replace(/\r/g,'');
  var lines=raw.split('\n');
  var out=[];
  for(var i=0;i<lines.length;i++){
    var line=lines[i]
      .replace(/^#{1,6}\s*/,'')
      .replace(/\*\*/g,'')
      .replace(/`/g,'')
      .trim();
    if(!line){
      if(out.length&&out[out.length-1]!=='')out.push('');
      continue;
    }
    out.push(line);
    if(out.join('\n').length>700)break;
  }
  var text=out.join('\n').trim();
  if(text.length>700)text=text.slice(0,697)+'...';
  return text;
}
function renderUpdateStatus(status){
  if(status)APP_UPDATE_STATUS=status;
  var s=APP_UPDATE_STATUS||{};
  var current=s.current||(window.__pf_pending&&window.__pf_pending.app_version)||'';
  setText('update-current-version',current?('v'+String(current).replace(/^v/,'')):'—');
  setText('update-latest-version',s.latest?('v'+String(s.latest).replace(/^v/,'')):'—');
  var result=document.getElementById('update-check-result');
  var link=document.getElementById('update-release-link');
  var notesCard=document.getElementById('update-release-notes-card');
  var notesBody=document.getElementById('update-release-notes');
  var copy=document.getElementById('rail-update-copy');
  var pillText=LANG==='ko'?'준비':'Ready';
  var pillTone='info';
  var msg=LANG==='ko'?'업데이트 확인을 실행할 수 있습니다.':'PeterFan is ready to check for updates.';
  if(notesCard){
    notesCard.style.display=s.notes?'':'none';
    if(notesBody)notesBody.textContent=s.notes||'';
  }
  if(s.checking){
    msg=LANG==='ko'?'GitHub 최신 릴리즈를 확인 중입니다.':'Checking the latest GitHub release.';
    pillText=LANG==='ko'?'확인 중':'Checking';
    pillTone='info';
    setText('update-check-result',LANG==='ko'?'확인 중…':'checking…');
  } else if(s.error){
    msg=(LANG==='ko'?'업데이트 확인 실패: ':'Update check failed: ')+s.error;
    pillText=LANG==='ko'?'실패':'Failed';
    pillTone='warn';
    setText('update-check-result',LANG==='ko'?'확인 실패':'check failed');
  } else if(s.install_status==='pending'){
    msg=LANG==='ko'?'새 버전을 설치하고 있습니다. PeterFan이 자동으로 다시 열립니다.':(s.install_message||'Installing the new PeterFan version. The app will reopen automatically.');
    pillText=LANG==='ko'?'설치 중':'Installing';
    pillTone='info';
    setText('update-check-result',LANG==='ko'?'설치 진행 중':'installation in progress');
  } else if(s.install_status==='installed'){
    msg=LANG==='ko'?('PeterFan v'+String(s.latest||current).replace(/^v/,'')+' 설치를 완료했습니다.'):(s.install_message||'PeterFan was installed successfully.');
    pillText=LANG==='ko'?'완료':'Installed';
    pillTone='ok';
    setText('update-check-result',LANG==='ko'?'설치 완료':'installed successfully');
  } else if(s.install_status==='rolled_back'){
    msg=LANG==='ko'?'업데이트에 실패해 이전 PeterFan 버전을 자동으로 복원했습니다.':(s.install_message||'The update failed and the previous PeterFan version was restored.');
    pillText=LANG==='ko'?'복원됨':'Restored';
    pillTone='warn';
    setText('update-check-result',LANG==='ko'?'이전 버전 복원':'previous version restored');
  } else if(s.install_status==='failed'){
    msg=(LANG==='ko'?'업데이트를 완료하지 못했습니다. 기존 앱은 변경하지 않았습니다.':(s.install_message||'The update could not be completed.'));
    pillText=LANG==='ko'?'실패':'Failed';
    pillTone='warn';
    setText('update-check-result',LANG==='ko'?'설치 실패':'installation failed');
  } else if(s.latest){
    var newer=compareVersions(current,s.latest)<0;
    msg=newer
      ?(LANG==='ko'?'새 버전을 사용할 수 있습니다.':'A newer PeterFan release is available.')
      :(LANG==='ko'?'최신 버전을 사용 중입니다.':'You are up to date.');
    pillText=newer?(LANG==='ko'?'업데이트 있음':'Update'):(LANG==='ko'?'최신':'Current');
    pillTone=newer?'warn':'ok';
    setText('update-check-result',newer?(LANG==='ko'?'업데이트 가능':'update available'):(LANG==='ko'?'최신 상태':'up to date'));
  } else {
    setText('update-check-result',LANG==='ko'?'대기 중':'ready');
  }
  if(copy)copy.textContent=msg;
  setPanelPill('rail-update-pill',pillText,pillTone);
  if(link){
    link.style.display=s.url?'':'none';
    link.dataset.url=s.url||'';
    link.textContent=LANG==='ko'?'릴리즈 보기':'View Release';
  }
}
function fetchLatestReleaseStatus(){
  var current=(window.__pf_pending&&window.__pf_pending.app_version)||'';
  renderUpdateStatus({current:current,checking:true});
  return fetch('https://api.github.com/repos/uulab-official/peterfan/releases/latest',{headers:{'Accept':'application/vnd.github+json'}})
    .then(function(r){
      if(!r.ok)throw new Error('HTTP '+r.status);
      return r.json();
    })
    .then(function(j){
      renderUpdateStatus({current:current,latest:String(j.tag_name||'').replace(/^v/,''),tag:j.tag_name||'',url:j.html_url||'',notes:formatReleaseNotes(j.body||'')});
    })
    .catch(function(e){
      renderUpdateStatus({current:current,error:e&&e.message?e.message:String(e)});
    });
}
function openLatestRelease(){
  var link=document.getElementById('update-release-link');
  var url=link&&link.dataset?link.dataset.url:'';
  if(url)window.ipc.postMessage('open:'+url);
}
function runRailAction(action,btn){
  switch(action){
    case 'detail':setRailView('overview');break;
    case 'fan':setRailView('fan');break;
    case 'update':setRailView('update');break;
    case 'settings':setRailView('settings');break;
    case 'more':setRailView('more');break;
  }
}
var RAW_TEMP_OPEN=false;
function toggleRawTemps(){
  RAW_TEMP_OPEN=!RAW_TEMP_OPEN;
  renderRawTempList(window.__pf_pending||{});
}
function renderRawTempList(d){
  var ah=document.getElementById('all-temp-head'),al=document.getElementById('all-temp-list'),all=d.all_temps||[];
  if(ah){
    ah.textContent=(RAW_TEMP_OPEN?'▾ ':'▸ ')+(LANG==='ko'?'전체 센서':'All sensors')+(all.length?' · '+all.length:'');
    ah.style.display=all.length?'':'none';
  }
  if(al){
    al.style.display=RAW_TEMP_OPEN?'':'none';
    al.innerHTML='';
    if(RAW_TEMP_OPEN){
      var groups=[];
      all.forEach(function(t){var name=t.group||'Other',g=groups.find(function(x){return x.name===name;});if(!g){g={name:name,items:[]};groups.push(g);}g.items.push(t);});
      groups.forEach(function(g){
        var h=document.createElement('div');h.className='sensor-group-head';h.textContent=g.name;al.appendChild(h);
        g.items.forEach(function(t){var r=document.createElement('div');r.className='trow'+(t.stale?' stale':'');r.innerHTML='<span class="l"></span><span class="src"></span><span class="v"></span>';r.children[0].textContent=t.l;r.children[0].title=t.l;r.children[1].textContent=(t.source||'')+(t.stale?' · '+(LANG==='ko'?'오래됨 ':'stale ')+Number(t.age_secs||0)+'s':'');r.children[2].textContent=t.c;r.children[2].className='v '+t.cls;al.appendChild(r);});
      });
    }
  }
}
window.__pf={
 update:function(d){
 function cls(p){return p<50?'g':p<80?'y':'r';}
 function bar(id,p,c){var b=document.getElementById(id);if(b){b.style.width=Math.max(0,Math.min(100,p))+'%';b.className='bar-fill '+(c||cls(p));}}
 function set(id,t){var e=document.getElementById(id);if(e)e.textContent=t;}
 function show(id,on){var e=document.getElementById(id);if(e)e.style.display=on?'':'none';}
 window.__pf_pending=d;
 var view=railView();
 CHART_RANGE_LABEL=d.chart_range;
  updateRail(d);
 if(view==='overview'){
   set('cpu-val',d.cpu_text);set('cpu-sub',d.cpu_sub);bar('cpu-bar',d.cpu_pct);
   var cc=document.getElementById('cores');if(cc){cc.innerHTML='';(d.cores||[]).forEach(function(p,i){var s=document.createElement('span');s.className='core '+cls(p);s.style.height=Math.max(8,Math.min(100,p))+'%';s.title='Core '+(i+1)+': '+p.toFixed(1)+'%';cc.appendChild(s);});}
   set('mem-val',d.mem_text);set('mem-sub',d.mem_sub);bar('mem-bar',d.mem_pct);
   show('sec-temp',d.temp_present);if(d.temp_present){set('temp-name',(LANG==='ko'?'온도':'Temperature')+(d.temp_source?' · '+d.temp_source:'')+(d.temp_stale?' · '+(LANG==='ko'?'오래됨 ':'stale ')+Number(d.temp_age_secs||0)+'s':''));set('temp-val',d.temp_text);var tv=document.getElementById('temp-val');if(tv)tv.classList.toggle('stale',!!d.temp_stale);bar('temp-bar',d.temp_stale?0:d.temp_pct,d.temp_cls);
     var tl=document.getElementById('temp-list');if(tl){tl.innerHTML='';(d.temps||[]).forEach(function(t){var r=document.createElement('div');r.className='trow'+(t.stale?' stale':'');r.innerHTML='<span class="l"></span><span class="v"></span>';r.children[0].textContent=t.l;r.children[1].textContent=t.c+(t.stale?' · '+Number(t.age_secs||0)+'s':'');r.children[1].className='v '+t.cls;tl.appendChild(r);});}
     renderRawTempList(d);}
   drawChart('cpu-chart', d.cpu_hist, '#5b9dff', 100, function(v){return v.toFixed(1)+'%';});
   drawChart('mem-chart', d.mem_hist, '#5b9dff', 100, function(v){return v.toFixed(1)+'%';});
   if(d.temp_present)drawChart('temp-chart', d.temp_stale?[]:d.temp_hist, '#ff9f0a', null, function(v){return v.toFixed(0)+'°C';});
   document.querySelectorAll('.range-tabs .range-tab').forEach(function(b){b.classList.toggle('active',b.dataset.range===d.chart_range);});
 } else if(view==='more'){
   set('disk-val',d.disk_text);set('disk-sub',d.disk_sub);bar('disk-bar',d.disk_pct);
   show('disk-io-sub',d.disk_io_present);if(d.disk_io_present)set('disk-io-sub',d.disk_io_sub);
   show('disk-io-chart',d.disk_io_present);show('disk-io-chart-stats',d.disk_io_present);
   if(d.disk_io_present)drawChart('disk-io-chart', d.disk_io_hist, '#ff9f0a', null, fmtBytesPerSec);
   show('sec-batt',d.batt_present);if(d.batt_present){set('batt-val',d.batt_text);set('batt-sub',d.batt_sub);bar('batt-bar',d.batt_pct,d.batt_pct>50?'g':d.batt_pct>20?'y':'r');}
   var battSec=document.getElementById('sec-batt');if(battSec)battSec.dataset.present=d.batt_present?'1':'0';
   set('net-sub',d.net_sub);show('net-ip',!!d.net_ip);if(d.net_ip)set('net-ip',d.net_ip);
   var psCpu=document.getElementById('ps-cpu'),psMem=document.getElementById('ps-mem');
   if(psCpu)psCpu.classList.toggle('active',d.proc_sort!=='mem');
   if(psMem)psMem.classList.toggle('active',d.proc_sort==='mem');
   var pl=document.getElementById('procs-list');
   if(pl){pl.innerHTML='';(d.procs||[]).forEach(function(p){var r=document.createElement('div');r.className='prow';r.innerHTML='<span class="n"></span><span class="c"></span><span class="m"></span><button class="pkill" title="Quit process">×</button>';r.children[0].textContent=p.name;r.children[1].textContent=p.cpu;r.children[2].textContent=p.mem;r.children[3].onclick=function(){quitProcess(p.pid,p.name);};pl.appendChild(r);});}
   drawChart('net-chart', d.net_hist, '#30d158', null, fmtBytesPerSec);
 } else if(view==='settings'){
   updateSetup(d);
   updateHardwareAvailability(d);
 } else if(view==='fan'){
   updateSetup(d);
   var note=document.getElementById('ctl-note');
   if(d.can_control){
     set('ctl-status', d.ctl_status||'');
     updateProfileStrip(d);
     updateFanApplyStatus(d);
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
         note.appendChild(document.createElement('br'));
         note.appendChild(fanControlSetupButton(LANG==='ko'?'팬 제어 재설치':'Reinstall Fan Control'));
       }
     } else if(d.daemon_update_needed){
       note.style.display='';
       note.textContent=LANG==='ko'
         ?'설치된 팬 제어 데몬이 오래되었습니다. 위의 재설치 버튼을 사용하세요.'
         :'The fan-control daemon is out of date. Use the reinstall button above.';
     } else if(!d.daemon_running){
       note.style.display='';
       note.textContent=LANG==='ko'
         ?'팬 제어를 유지하려면 위의 설정 버튼에서 최초 1회 승인이 필요합니다.'
         :'Persistent fan control needs one initial approval from the setup button above.';
     } else {
       note.style.display='none';
     }
     }
     renderFanCards(d.fans);
     updateFanEmptyState(d);
   } else {
     set('ctl-status',LANG==='ko'?'사용 불가':'unavailable');
     updateProfileStrip(d);
     updateFanApplyStatus(d);
     if(note){note.style.display='';note.textContent=LANG==='ko'?'이 Mac에서는 팬 제어를 사용할 수 없습니다. 실시간 RPM만 표시합니다.':'Fan control unavailable on this Mac — showing live RPM only.';}
     var fc=document.getElementById('fan-cards');if(fc)fc.innerHTML='';
     updateFanEmptyState(d);
   }
   if(SHOW_CURVE_EDITOR==='1'&&d.can_control){
     var ces=document.getElementById('curve-editor-section');
     if(ces)ces.style.display='';
     if(d.curve_points){
       CURVE_POINTS_SAVED=d.curve_points.map(function(p){return p.slice();});
       if(CURVE_POINTS===null)CURVE_POINTS=CURVE_POINTS_SAVED.map(function(p){return p.slice();});
     }
     initCurveEditor();drawCurveEditor();syncCurvePointInputs();
   } else {
     var ces2=document.getElementById('curve-editor-section');if(ces2)ces2.style.display='none';
   }
 }
}};
applyPendingUpdate();
if(window.ipc)window.ipc.postMessage('ready');
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
        '<div class="fan-bar"><i></i><span class="fan-target-marker"></span></div>'+
        '<div class="fan-bottom"><span class="fan-rpm-text"></span><span class="fan-seg"><button class="fa-auto"></button><button class="fa-manual"></button></span></div>'+
        '<div class="fan-rpm-row inactive"><span class="fa-min"></span><input type="range"><input type="number" class="fa-num" inputmode="numeric"><span class="fa-max"></span></div>';
      var btnAuto=card.querySelector('.fa-auto');
      var btnManual=card.querySelector('.fa-manual');
      btnAuto.textContent=LANG==='ko'?'자동':'Auto';
      btnManual.textContent=LANG==='ko'?'사용자 지정…':'Custom…';
      btnAuto.onclick=function(){
        if(!markFanPending(card,'auto'))return;
        window.ipc.postMessage('cmd:fanauto:'+f.id);
      };
      btnManual.onclick=function(){
        if(!markFanPending(card,'manual'))return;
        // Pin right where the fan already is instead of jumping to a
        // default — read the live % off the card, not this closure's
        // (potentially stale, first-render-time) copy of `f`.
        var curPct=Math.round(parseFloat(card.dataset.curPct||'50'));
        window.ipc.postMessage('cmd:fanhold:'+f.id+':'+curPct);
        card.querySelector('.fan-rpm-row').classList.remove('inactive');
      };
      var slider=card.querySelector('input[type=range]');
      var numInput=card.querySelector('.fa-num');
      // A drag gesture is too coarse for "I want exactly 2500 RPM" — the
      // number box lets you type it, while the slider stays for quick
      // eyeballed adjustments. Both funnel through `commitFanValue` so they
      // can never send conflicting commands for the same drag/keystroke.
      function commitFanValue(v){
        var min=parseInt(slider.min,10),max=parseInt(slider.max,10);
        v=Math.max(min,Math.min(max,v));
        slider.value=v;
        numInput.value=v;
        var useRpm=slider.dataset.useRpm==='1';
        card.querySelector('.fv').textContent=useRpm?(v+' RPM'):(v+'%');
        var span=max-min;
        var pct=useRpm?(span>0?Math.round((v-min)/span*100):0):v;
        markFanPending(card,'manual',true);
        window.ipc.postMessage('cmd:fanhold:'+f.id+':'+Math.max(0,Math.min(100,pct)));
      }
      slider.addEventListener('input',function(){
        var v=parseInt(slider.value,10);
        numInput.value=v;
        var useRpm=slider.dataset.useRpm==='1';
        card.querySelector('.fv').textContent=useRpm?(v+' RPM'):(v+'%');
      });
      slider.addEventListener('change',function(){
        commitFanValue(parseInt(slider.value,10));
      });
      numInput.addEventListener('input',function(){
        var v=parseInt(numInput.value,10);
        if(isNaN(v))return;
        slider.value=Math.max(parseInt(slider.min,10),Math.min(parseInt(slider.max,10),v));
        var useRpm=slider.dataset.useRpm==='1';
        card.querySelector('.fv').textContent=useRpm?(v+' RPM'):(v+'%');
      });
      numInput.addEventListener('change',function(){
        var v=parseInt(numInput.value,10);
        if(!isNaN(v))commitFanValue(v);
      });
      numInput.addEventListener('keydown',function(e){
        if(e.key==='Enter')numInput.blur();
      });
      container.appendChild(card);
    }
    var manual=!!f.manual;
    var pendingMode=card.dataset.pendingMode||'';
    if(pendingMode){
      var pendingConfirmed=(pendingMode==='manual'&&manual)||(pendingMode==='auto'&&!manual);
      var pendingExpired=Date.now()-parseInt(card.dataset.pendingAt||'0',10)>5000;
      if(pendingConfirmed||pendingExpired){
        delete card.dataset.pendingMode;
        delete card.dataset.pendingAt;
        pendingMode='';
      }
    }
    var displayManual=pendingMode?pendingMode==='manual':manual;
    var useRpm=f.max_rpm>0;
    var targetPct=f.override_pct!=null?f.override_pct:f.pct;
    card.dataset.curPct=targetPct;
    card.querySelector('.fn').textContent=f.l;
    card.querySelector('.fan-bar i').style.width=Math.max(0,Math.min(100,f.pct))+'%';
    var appliedTarget=f.target_pct==null?null:Number(f.target_pct);
    var targetRpm=f.target_rpm==null?null:Number(f.target_rpm);
    var tolerance=useRpm?Math.max(100,Math.round(f.max_rpm*0.025)):3;
    var daemonReadback=f.readback_status||'';
    var stale=daemonReadback==='stale';
    var ramping=daemonReadback==='adjusting'||(!daemonReadback&&targetRpm!=null&&Math.abs(f.cur_rpm-targetRpm)>tolerance);
    var marker=card.querySelector('.fan-target-marker');
    if(marker){
      marker.style.display=appliedTarget==null?'none':'block';
      if(appliedTarget!=null)marker.style.left=Math.max(0,Math.min(100,appliedTarget))+'%';
    }
    card.classList.toggle('ramping',ramping);
    card.classList.toggle('stale',stale);
    card.querySelector('.fan-rpm-text').textContent=useRpm
      ?(targetRpm!=null?(f.cur_rpm+' RPM → '+targetRpm+' RPM'):(f.min_rpm+' — '+f.cur_rpm+' — '+f.max_rpm))
      :(Math.round(f.pct)+'%');
    card.classList.toggle('pending',!!pendingMode);
    card.querySelector('.fa-auto').disabled=!!pendingMode;
    card.querySelector('.fa-manual').disabled=!!pendingMode;
    card.querySelector('.fa-auto').classList.toggle('active',!displayManual);
    card.querySelector('.fa-manual').classList.toggle('active',displayManual);
    card.querySelector('.fa-min').textContent=useRpm?f.min_rpm:'0%';
    card.querySelector('.fa-max').textContent=useRpm?f.max_rpm:'100%';
    // Always occupies the same layout space (opacity/pointer-events toggle
    // only, never display) — hiding it outright used to change the
    // popover's total content height, which used to trigger a full window
    // resize and made every chart below visibly redraw at a new width,
    // which read as "the graphs randomly changed."
    card.querySelector('.fan-rpm-row').classList.toggle('inactive', !displayManual);
    var slider=card.querySelector('input[type=range]');
    var numInput=card.querySelector('.fa-num');
    slider.dataset.useRpm=useRpm?'1':'0';
    slider.min=numInput.min=useRpm?f.min_rpm:0;
    slider.max=numInput.max=useRpm?Math.max(f.max_rpm,f.min_rpm+1):100;
    // Skip the live-tick overwrite while the user is mid-edit in either
    // control — without this, typing into the number box would get
    // clobbered by the next 1s poll before the "change" event even fires.
    if(slider!==document.activeElement&&numInput!==document.activeElement){
      var editRpm=useRpm?Math.round(f.min_rpm+(f.max_rpm-f.min_rpm)*targetPct/100):Math.round(targetPct);
      slider.value=editRpm;
      numInput.value=editRpm;
      var shownTarget=appliedTarget==null?targetPct:appliedTarget;
      card.querySelector('.fv').textContent=appliedTarget!=null
        ?(Math.round(shownTarget)+'% · '+(stale?(LANG==='ko'?'응답 없음':'not responding'):(ramping?(LANG==='ko'?'조정 중':'adjusting'):(LANG==='ko'?'적용됨':'applied'))))
        :(displayManual?(useRpm?(editRpm+' RPM'):(targetPct+'%')):(Math.round(f.pct)+'%'));
    }
  });
  Array.prototype.slice.call(container.children).forEach(function(c){
    if(!seen[c.getAttribute('data-fan-id')])c.remove();
  });
}
function markFanPending(card,mode,refresh){
  if(card.dataset.pendingMode&&!refresh)return false;
  card.dataset.pendingMode=mode;
  card.dataset.pendingAt=Date.now();
  card.classList.add('pending');
  card.querySelector('.fa-auto').disabled=true;
  card.querySelector('.fa-manual').disabled=true;
  card.querySelector('.fa-auto').classList.toggle('active',mode==='auto');
  card.querySelector('.fa-manual').classList.toggle('active',mode==='manual');
  card.querySelector('.fan-rpm-row').classList.toggle('inactive',mode!=='manual');
  return true;
}
function fanControlSetupButton(label){
  var fixBtn=document.createElement('button');
  fixBtn.className='note-fix-btn';
  // The button is rebuilt fresh every tick, so a plain per-click `disabled`
  // would disappear on the next render. Keep the pending state outside the
  // node while the macOS admin-password prompt is in flight.
  if(FAN_CONTROL_FIX_PENDING){
    fixBtn.disabled=true;
    fixBtn.textContent=LANG==='ko'?'설치 중…':'Installing…';
  } else {
    fixBtn.textContent=label;
    fixBtn.onclick=function(){startFanControlSetup(fixBtn);};
  }
  return fixBtn;
}
function startFanControlSetup(btn){
  if(FAN_CONTROL_FIX_PENDING)return;
  FAN_CONTROL_FIX_PENDING=true;
  if(btn){
    btn.disabled=true;
    setButtonLabel(btn,LANG==='ko'?'설치 중…':'Installing…');
  }
  var top=document.getElementById('setup-fan');
  if(top&&top!==btn){
    top.disabled=true;
    setButtonLabel(top,LANG==='ko'?'설치 중…':'Installing…');
  }
  window.ipc.postMessage('cmd:enablefancontrol');
  // No completion callback reaches JS (the result lands as a native macOS
  // notification) — release the guard after a generous timeout so a
  // dismissed/failed prompt doesn't lock the button forever.
  setTimeout(function(){FAN_CONTROL_FIX_PENDING=false;},15000);
}
function runFanDiagnostics(btn){
  if(FAN_DIAGNOSTIC_PENDING)return;
  FAN_DIAGNOSTIC_PENDING=true;
  FAN_DIAGNOSTIC_STARTED_AT=Math.floor(Date.now()/1000);
  if(btn){
    btn.disabled=true;
    btn.textContent=LANG==='ko'?'진단 중…':'Diagnosing…';
  }
  window.ipc.postMessage('cmd:diagnosefan');
  setTimeout(function(){
    FAN_DIAGNOSTIC_PENDING=false;
    var current=document.getElementById('fan-diagnostic-button');
    if(current){
      current.disabled=false;
      current.textContent=LANG==='ko'?'진단 실행':'Run Diagnostics';
    }
  },10000);
}
function renderFanActionLog(d){
  var list=document.getElementById('fan-action-log');
  var button=document.getElementById('fan-diagnostic-button');
  var entries=Array.isArray(d.fan_action_log)?d.fan_action_log:[];
  var diagnosticFinished=entries.some(function(entry){
    return entry&&entry.action==='diagnostic'&&Number(entry.at)>=FAN_DIAGNOSTIC_STARTED_AT-1;
  });
  if(FAN_DIAGNOSTIC_PENDING&&diagnosticFinished)FAN_DIAGNOSTIC_PENDING=false;
  if(button){
    button.disabled=FAN_DIAGNOSTIC_PENDING;
    button.textContent=FAN_DIAGNOSTIC_PENDING
      ?(LANG==='ko'?'진단 중…':'Diagnosing…')
      :(LANG==='ko'?'진단 실행':'Run Diagnostics');
  }
  if(!list)return;
  list.textContent='';
  if(!entries.length){
    var empty=document.createElement('div');
    empty.className='action-log-empty';
    empty.textContent=LANG==='ko'?'팬 제어 이력이 없습니다':'No fan actions yet';
    list.appendChild(empty);
    return;
  }
  entries.forEach(function(entry){
    if(!entry||typeof entry!=='object')return;
    var row=document.createElement('div');
    row.className='action-log-row '+(entry.ok?'ok':'warn');
    var time=document.createElement('div');
    time.className='action-log-time';
    var date=new Date(Number(entry.at||0)*1000);
    time.textContent=isNaN(date.getTime())?'—':date.toLocaleTimeString([], {hour:'2-digit',minute:'2-digit'});
    var main=document.createElement('div');
    main.className='action-log-main';
    var action=document.createElement('div');
    action.className='action-log-action';
    action.textContent=String(entry.action||'—');
    var result=document.createElement('div');
    result.className='action-log-result';
    result.textContent=String(entry.result||'—');
    main.appendChild(action);
    main.appendChild(result);
    row.appendChild(time);
    row.appendChild(main);
    list.appendChild(row);
  });
}
function checkAppUpdates(btn){
  if(APP_UPDATE_CHECK_PENDING)return;
  APP_UPDATE_CHECK_PENDING=true;
  if(btn){
    if(!btn.dataset.defaultLabel){
      var current=btn.querySelector('span');
      btn.dataset.defaultLabel=current?current.textContent:btn.textContent;
    }
    btn.disabled=true;
    setButtonLabel(btn,LANG==='ko'?'확인 중…':'Checking…');
  }
  fetchLatestReleaseStatus();
  window.ipc.postMessage('checkupdates');
  setTimeout(function(){
    APP_UPDATE_CHECK_PENDING=false;
    if(btn){
      btn.disabled=false;
      setButtonLabel(btn,btn.dataset.defaultLabel||(LANG==='ko'?'업데이트':'Updates'));
    }
  },2500);
}
function toggleStartupItem(btn){
  if(LOGIN_ITEM_TOGGLE_PENDING||!window.__pf_pending||!window.__pf_pending.login_item_supported)return;
  LOGIN_ITEM_TOGGLE_PENDING=true;
  if(btn){
    if(!btn.dataset.defaultLabel){
      btn.dataset.defaultLabel=btn.textContent;
    }
    btn.disabled=true;
    btn.textContent=LANG==='ko'?'처리 중…':'Updating…';
    btn.classList.add('active');
  }
  window.ipc.postMessage('toggle-login-item');
  setTimeout(function(){
    LOGIN_ITEM_TOGGLE_PENDING=false;
    if(btn){
      btn.classList.remove('active');
      btn.disabled=false;
      if(window.__pf&&window.__pf_pending)updateSetup(window.__pf_pending);
    }
  },2500);
}
function setSetupMenuOpen(open){
  var menu=document.getElementById('setup-menu');
  var more=document.getElementById('setup-more');
  if(menu)menu.classList.toggle('show',!!open);
  if(more)more.setAttribute('aria-expanded',open?'true':'false');
}
function closeSetupMenu(returnFocus){
  setSetupMenuOpen(false);
  if(returnFocus){
    var more=document.getElementById('setup-more');
    if(more)more.focus();
  }
}
function toggleSetupMenu(ev){
  if(ev&&ev.stopPropagation)ev.stopPropagation();
  var menu=document.getElementById('setup-menu');
  if(menu)setSetupMenuOpen(!menu.classList.contains('show'));
}
function setupMenuItems(){
  return Array.prototype.slice.call(document.querySelectorAll('.setup-menu-item')).filter(function(item){return !item.disabled;});
}
function focusSetupMenuItem(index){
  var items=setupMenuItems();
  if(!items.length)return;
  items[(index+items.length)%items.length].focus();
}
function handleSetupMoreKey(ev){
  if(ev.key==='ArrowDown'||ev.key==='Enter'||ev.key===' '){
    ev.preventDefault();
    setSetupMenuOpen(true);
    focusSetupMenuItem(0);
  } else if(ev.key==='Escape'){
    closeSetupMenu();
  }
}
function handleSetupMenuKey(ev){
  var items=setupMenuItems();
  var idx=items.indexOf(document.activeElement);
  if(ev.key==='Escape'){
    ev.preventDefault();
    closeSetupMenu(true);
  } else if(ev.key==='ArrowDown'){
    ev.preventDefault();
    focusSetupMenuItem(idx+1);
  } else if(ev.key==='ArrowUp'){
    ev.preventDefault();
    focusSetupMenuItem(idx-1);
  } else if(ev.key==='Home'){
    ev.preventDefault();
    focusSetupMenuItem(0);
  } else if(ev.key==='End'){
    ev.preventDefault();
    focusSetupMenuItem(items.length-1);
  }
}
document.addEventListener('click',function(ev){
  var wrap=document.querySelector('.setup-menu-wrap');
  if(wrap&&!wrap.contains(ev.target))closeSetupMenu();
});
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
// Dragging a point on the canvas is inherently approximate (mouse pixels,
// not degrees/percent) — these two number inputs mirror whichever point was
// last touched so an exact temp/duty pair can be typed instead of dragged.
function syncCurvePointInputs(){
  var row=document.getElementById('curve-point-row');
  var tIn=document.getElementById('cp-temp'), dIn=document.getElementById('cp-duty');
  if(!row||!tIn||!dIn)return;
  if(!CURVE_POINTS||CURVE_LAST<0||CURVE_LAST>=CURVE_POINTS.length){
    row.style.display='none';
    return;
  }
  row.style.display='';
  // Don't clobber an in-progress keystroke with the same values it already has.
  if(tIn!==document.activeElement)tIn.value=CURVE_POINTS[CURVE_LAST][0];
  if(dIn!==document.activeElement)dIn.value=CURVE_POINTS[CURVE_LAST][1];
}
function commitCurvePointInput(){
  if(!CURVE_POINTS||CURVE_LAST<0||CURVE_LAST>=CURVE_POINTS.length)return;
  var tIn=document.getElementById('cp-temp'), dIn=document.getElementById('cp-duty');
  var t=parseInt(tIn.value,10), d=parseInt(dIn.value,10);
  if(!isNaN(t))CURVE_POINTS[CURVE_LAST][0]=Math.max(CURVE_TMIN,Math.min(CURVE_TMAX,t));
  if(!isNaN(d))CURVE_POINTS[CURVE_LAST][1]=Math.max(0,Math.min(100,d));
  drawCurveEditor();
  syncCurvePointInputs();
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
    syncCurvePointInputs();
  });
  cv.addEventListener('mousemove',function(e){
    if(CURVE_DRAG<0)return;
    CURVE_POINTS[CURVE_DRAG]=curveEventToPoint(cv,e);
    drawCurveEditor();
    syncCurvePointInputs();
  });
  window.addEventListener('mouseup',function(){CURVE_DRAG=-1;});
  var tIn=document.getElementById('cp-temp'), dIn=document.getElementById('cp-duty');
  [tIn,dIn].forEach(function(inp){
    if(!inp)return;
    inp.addEventListener('change',commitCurvePointInput);
    inp.addEventListener('keydown',function(e){if(e.key==='Enter')inp.blur();});
  });
}
function resetCurve(){
  if(CURVE_POINTS_SAVED)CURVE_POINTS=CURVE_POINTS_SAVED.map(function(p){return p.slice();});
  CURVE_LAST=-1;
  drawCurveEditor();
  syncCurvePointInputs();
}
function removeCurvePoint(){
  if(!CURVE_POINTS||CURVE_POINTS.length<=2)return;
  var idx=(CURVE_LAST>=0&&CURVE_LAST<CURVE_POINTS.length)?CURVE_LAST:CURVE_POINTS.length-1;
  CURVE_POINTS.splice(idx,1);
  CURVE_LAST=-1;
  drawCurveEditor();
  syncCurvePointInputs();
}
function saveCurve(){
  if(!CURVE_POINTS||CURVE_POINTS.length<2)return;
  window.ipc.postMessage('savecurve:'+JSON.stringify(CURVE_POINTS));
}
function focusFanControl(){
  var el=document.getElementById('fan-control-section');
  if(el){
    el.scrollIntoView({block:'nearest',behavior:'auto'});
    el.classList.remove('focus-pulse');
    void el.offsetWidth;
    el.classList.add('focus-pulse');
    setTimeout(function(){el.classList.remove('focus-pulse');},900);
  }
}
function setChartRange(r){
  document.querySelectorAll('.range-tabs .range-tab').forEach(function(b){b.classList.toggle('active',b.dataset.range===r);});
  window.ipc.postMessage('range:'+r);
}
function setProfile(profile){
  if(!beginFanControl('profile',profile))return;
  window.ipc.postMessage('cmd:profile:'+profile);
}
function setAuto(){
  if(!beginFanControl('auto',''))return;
  window.ipc.postMessage('cmd:auto');
}
function beginFanControl(mode,profile){
  if(FAN_CONTROL_PENDING)return false;
  var current=window.__pf_pending||{};
  var wantedProfile=profile||'';
  if(current.active_control_mode===mode&&(mode!=='profile'||current.active_profile===wantedProfile))return false;
  FAN_CONTROL_RESULT=null;
  FAN_CONTROL_PENDING={mode:mode,profile:wantedProfile,startedAt:Date.now(),statusBefore:current.last_cmd_status||'',revisionBefore:Number(current.applied_control_revision||0)};
  updateProfileStrip(window.__pf_pending||{can_control:true,active_control_mode:'',active_profile:'',last_cmd_status:''});
  return true;
}
function fanControlStatusFailed(status){
  return /error|failed|invalid|cancel|unavailable|not ready/i.test(status||'');
}
function updateProfileStrip(d){
  var strip=document.getElementById('profile-strip');
  if(!strip)return;
  var enabled=!!d.can_control;
  var activeMode=d.active_control_mode||'';
  var activeProfile=d.active_profile||'';
  if(FAN_CONTROL_PENDING){
    var revisionApplied=Number(d.applied_control_revision||0)>FAN_CONTROL_PENDING.revisionBefore;
    var matches=revisionApplied&&FAN_CONTROL_PENDING.mode===activeMode&&(activeMode!=='profile'||FAN_CONTROL_PENDING.profile===activeProfile);
    var status=d.last_cmd_status||'';
    var failed=status!==FAN_CONTROL_PENDING.statusBefore&&status!=='applying…'&&fanControlStatusFailed(status);
    var expired=Date.now()-FAN_CONTROL_PENDING.startedAt>8000;
    if(matches){
      FAN_CONTROL_RESULT={ok:true,mode:FAN_CONTROL_PENDING.mode,profile:FAN_CONTROL_PENDING.profile,at:Date.now(),message:''};
      FAN_CONTROL_PENDING=null;
    } else if(failed||expired){
      FAN_CONTROL_RESULT={ok:false,mode:FAN_CONTROL_PENDING.mode,profile:FAN_CONTROL_PENDING.profile,at:Date.now(),message:failed?status:(LANG==='ko'?'하드웨어 적용 확인 시간 초과':'hardware confirmation timed out')};
      FAN_CONTROL_PENDING=null;
    }
  }
  if(FAN_CONTROL_PENDING){
    activeMode=FAN_CONTROL_PENDING.mode;
    activeProfile=FAN_CONTROL_PENDING.profile;
  }
  var pending=!!FAN_CONTROL_PENDING;
  strip.classList.toggle('disabled',!enabled);
  strip.classList.toggle('pending',pending);
  strip.setAttribute('aria-busy',pending?'true':'false');
  Array.prototype.slice.call(strip.querySelectorAll('button')).forEach(function(b){
    b.disabled=!enabled||pending;
    var isAuto=b.dataset.mode==='auto'&&activeMode==='auto';
    var isProfile=b.dataset.mode==='profile'&&activeMode==='profile'&&b.dataset.profile===activeProfile;
    b.classList.toggle('active',enabled&&(isAuto||isProfile));
  });
}
function fanModeLabel(mode,profile){
  if(mode==='auto')return LANG==='ko'?'macOS 자동':'macOS Auto';
  var labels=LANG==='ko'
    ?{silent:'저소음',balanced:'균형',gaming:'게임',performance:'고성능',maximum:'최대'}
    :{silent:'Silent',balanced:'Balanced',gaming:'Gaming',performance:'Performance',maximum:'Maximum'};
  return labels[profile]||profile||(LANG==='ko'?'수동':'Manual');
}
function updateFanApplyStatus(d){
  var el=document.getElementById('fan-apply-status');
  if(!el)return;
  var mode=d.active_control_mode||'',profile=d.active_profile||'';
  var tone='';
  if(FAN_CONTROL_PENDING){
    mode=FAN_CONTROL_PENDING.mode;profile=FAN_CONTROL_PENDING.profile;tone='pending';
  } else if(FAN_CONTROL_RESULT&&Date.now()-FAN_CONTROL_RESULT.at<12000){
    mode=FAN_CONTROL_RESULT.mode;profile=FAN_CONTROL_RESULT.profile;tone=FAN_CONTROL_RESULT.ok?'ok':'error';
  }
  var parts=[fanModeLabel(mode,profile)];
  if(FAN_CONTROL_PENDING)parts.push(LANG==='ko'?'적용 확인 중…':'confirming…');
  else if(FAN_CONTROL_RESULT&&Date.now()-FAN_CONTROL_RESULT.at<12000){
    parts.push(FAN_CONTROL_RESULT.ok?(LANG==='ko'?'적용 완료':'applied'):(LANG==='ko'?'적용 실패':'failed'));
    if(!FAN_CONTROL_RESULT.ok&&FAN_CONTROL_RESULT.message)parts.push(FAN_CONTROL_RESULT.message);
  } else if(mode)parts.push(LANG==='ko'?'현재 모드':'current');
  if(mode!=='auto'&&typeof d.fan_curve_input_temp_c==='number')parts.push((LANG==='ko'?'입력 ':'input ')+Math.round(d.fan_curve_input_temp_c)+'°C');
  var targets=(d.fans||[]).filter(function(f){return typeof f.target_pct==='number';}).map(function(f){return Number(f.target_pct);}).filter(function(v){return isFinite(v);});
  if(mode!=='auto'&&targets.length)parts.push((LANG==='ko'?'목표 ':'target ')+Math.max.apply(null,targets)+'%');
  el.textContent=parts.filter(Boolean).join(' · ');
  el.className='fan-apply-status '+tone;
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
function updateSetup(d){
  var title=document.getElementById('setup-title');
  if(title)title.textContent=d.setup_title||'Ready';
  var detail=document.getElementById('setup-detail');
  if(detail)detail.textContent=d.setup_detail||('v'+(d.app_version||''));
  var dot=document.getElementById('setup-dot');
  if(dot)dot.className='setup-dot '+(d.setup_tone||'info');
  var fan=document.getElementById('setup-fan');
  if(fan){
    fan.style.display=d.fan_setup_needed?'':'none';
    fan.disabled=FAN_CONTROL_FIX_PENDING;
    fan.title=d.daemon_update_needed
      ?(LANG==='ko'?'팬 제어 재설치':'Reinstall fan control')
      :(LANG==='ko'?'팬 제어 설정':'Set up fan control');
    fan.textContent=FAN_CONTROL_FIX_PENDING
      ?(LANG==='ko'?'설치 중…':'Installing…')
      :(d.daemon_update_needed?(LANG==='ko'?'재설치':'Reinstall'):(LANG==='ko'?'팬':'Fan'));
  }
  var more=document.getElementById('setup-more');
  if(more){
    more.title=LANG==='ko'?'설정 작업':'Setup actions';
    more.setAttribute('aria-label',more.title);
  }
  var update=document.getElementById('setup-update');
  if(update){
    update.disabled=APP_UPDATE_CHECK_PENDING;
    update.textContent=APP_UPDATE_CHECK_PENDING?(LANG==='ko'?'확인 중…':'Checking…'):(LANG==='ko'?'업데이트 확인':'Check Updates');
    update.title=LANG==='ko'?'앱 업데이트 확인':'Check for app updates';
  }
  var startupMenu=document.getElementById('setup-startup');
  if(startupMenu){
    startupMenu.style.display=d.login_item_supported?'':'none';
    startupMenu.disabled=LOGIN_ITEM_TOGGLE_PENDING||APP_UPDATE_CHECK_PENDING;
    startupMenu.textContent=(d.login_item_installed
      ?(LANG==='ko'?'부팅 시 자동 실행 끄기':'Disable startup')
      :(LANG==='ko'?'부팅 시 자동 실행 켜기':'Enable startup'));
  }
  var startup=document.getElementById('startup-toggle');
  if(startup){
    startup.classList.toggle('primary',!d.login_item_installed);
    startup.style.display=d.login_item_supported?'':'none';
    startup.disabled=LOGIN_ITEM_TOGGLE_PENDING||APP_UPDATE_CHECK_PENDING;
    startup.textContent=LOGIN_ITEM_TOGGLE_PENDING
      ?(LANG==='ko'?'처리 중…':'Updating…')
      :(d.login_item_installed
        ?(LANG==='ko'?'끄기':'Disable')
        :(LANG==='ko'?'켜기':'Enable'));
    startup.title=d.login_item_installed
      ?(LANG==='ko'?'부팅 시 자동 실행 끄기':'Disable startup on login')
      :(LANG==='ko'?'부팅 시 자동 실행 켜기':'Enable startup on login');
  }
}
var RAIL_NAV_READY=false;
function updateRail(d){
  if(!RAIL_NAV_READY){
    var detail=document.getElementById('railDetail');
    if(detail){setButtonLabel(detail,LANG==='ko'?'상태':'Status');detail.title=LANG==='ko'?'상태 요약':'Status overview';}
    var fan=document.getElementById('railFan');
    if(fan){setButtonLabel(fan,LANG==='ko'?'팬 제어':'Fans');fan.title=LANG==='ko'?'팬 제어로 이동':'Jump to fan control';}
    var upd=document.getElementById('railUpdate');
    if(upd){setButtonLabel(upd,LANG==='ko'?'업데이트':'Update');upd.title=LANG==='ko'?'업데이트 확인':'Check for updates';}
    var settings=document.getElementById('railSettings');
    if(settings){setButtonLabel(settings,LANG==='ko'?'설정':'Settings');settings.title=LANG==='ko'?'설정 열기':'Open settings';}
    RAIL_NAV_READY=true;
  }
  var view=railView();
  if(view==='update'){
    var persisted=d.update_install_result;
    if(persisted&&(!APP_UPDATE_STATUS||APP_UPDATE_STATUS.persisted)){
      var persistedKey=[persisted.status||'',persisted.version||'',persisted.updated_at_unix||''].join(':');
      if(persistedKey!==APP_PERSISTED_UPDATE_KEY){
        APP_PERSISTED_UPDATE_KEY=persistedKey;
        APP_UPDATE_STATUS={
          current:d.app_version||'',
          latest:persisted.version||'',
          install_status:persisted.status||'',
          install_message:persisted.message||'',
          persisted:true
        };
      }
    }
    if(APP_UPDATE_STATUS){APP_UPDATE_STATUS.current=d.app_version||APP_UPDATE_STATUS.current;renderUpdateStatus();}
    else renderUpdateStatus({current:d.app_version||''});
    var updCheck=document.getElementById('rail-update-check');
    if(updCheck&&!APP_UPDATE_CHECK_PENDING)updCheck.textContent=LANG==='ko'?'업데이트 확인':'Check Updates';
  } else if(view==='settings'){
    updateHealthPanel(d);
  } else if(view==='more'){
    setPanelPill('rail-more-pill',LANG==='ko'?'실시간':'Live','info');
  }
}
function setHealthValue(id,text,tone){
  var el=document.getElementById(id);
  if(!el)return;
  el.textContent=text||'—';
  el.className='health-value '+(tone||'');
}
function tempValue(value){
  return typeof value==='number'&&isFinite(value)?Math.round(value)+'°C':'—';
}
function updateHealthPanel(d){
  var health=d.control_health||{},failsafe=!!health.failsafe_active;
  var tone=failsafe?'warn':(d.daemon_update_needed?'warn':(d.daemon_running?'ok':(d.can_control?'warn':'info')));
  var pill=failsafe
    ?(LANG==='ko'?'OS 자동 복귀':'OS fallback')
    :(d.daemon_update_needed
    ?(LANG==='ko'?'재설치 필요':'Reinstall')
    :(d.daemon_running?(LANG==='ko'?'정상':'OK'):(d.can_control?(LANG==='ko'?'설정 필요':'Setup'):(LANG==='ko'?'읽기 전용':'Read-only'))));
  setPanelPill('rail-settings-pill',pill,tone);
  setPanelPill('health-pill',pill,tone);
  var daemonText=d.daemon_running
    ?('v'+(d.daemon_version||'unknown')+(d.daemon_update_needed?' → v'+(d.daemon_required_version||''):''))
    :(LANG==='ko'?'실행 안 됨':'not running');
  setHealthValue('health-daemon',daemonText,d.daemon_update_needed?'warn':(d.daemon_running?'ok':'warn'));
  setHealthValue('health-helper',d.daemon_binary_installed?(d.daemon_path||'installed'):(LANG==='ko'?'설치 안 됨':'not installed'),d.daemon_binary_installed?'ok':'warn');
  setHealthValue('health-launch-daemon',d.launch_daemon_installed?(LANG==='ko'?'설치됨':'installed'):(LANG==='ko'?'설치 안 됨':'not installed'),d.launch_daemon_installed?'ok':'warn');
  setHealthValue('health-team-id',d.team_id||'—',d.team_id?'ok':'warn');
  var path=d.daemon_running
    ?(LANG==='ko'?'root 데몬':'root daemon')
    :(d.can_control?(LANG==='ko'?'앱 직접 제어':'app direct'):(LANG==='ko'?'읽기 전용':'read-only'));
  setHealthValue('health-control-path',path,d.daemon_running?'ok':(d.can_control?'info':'warn'));
  var last=d.last_cmd_status||d.ctl_status||'';
  var lastTone=/error|invalid|unknown|failed|needs root|needs at least/i.test(last)?'warn':'info';
  setHealthValue('health-last-command',last||'—',lastTone);
  setHealthValue('health-safety-state',
    failsafe?(LANG==='ko'?'OS 자동 제어':'OS automatic'):(LANG==='ko'?'정상':'normal'),
    failsafe?'warn':'ok');
  setHealthValue('health-fans',(d.controllable_fan_count||0)+' / '+(d.fan_count||0),d.controllable_fan_count?'ok':'info');
  setHealthValue('health-curve-input',tempValue(d.fan_curve_input_temp_c),'info');
  setHealthValue('health-core-hottest',tempValue(d.fan_core_hottest_temp_c),'info');
  setHealthValue('health-safety-hottest',tempValue(d.fan_safety_temp_c),d.fan_safety_temp_c>=d.fan_critical_temp_c?'warn':'info');
  setHealthValue('health-critical-limit',tempValue(d.fan_critical_temp_c),'info');
  var sensorFailures=Number(health.sensor_failure_count||0),consecutive=Number(health.consecutive_sensor_failures||0);
  setHealthValue('health-sensor-failures',sensorFailures+(consecutive?' ('+consecutive+' active)':''),consecutive?'warn':'info');
  setHealthValue('health-write-failures',String(Number(health.fan_write_failure_count||0)),health.fan_write_failure_count?'warn':'info');
  var staleFans=health.stale_fan_ids||[],readbackFailures=Number(health.fan_readback_failure_count||0);
  var readbackText=staleFans.length
    ?((LANG==='ko'?'응답 없음: ':'not responding: ')+staleFans.join(', '))
    :(d.active_control_mode==='auto'?(LANG==='ko'?'macOS 자동':'macOS automatic'):(LANG==='ko'?'정상':'verified'));
  if(readbackFailures&&!staleFans.length)readbackText+=' · '+readbackFailures;
  setHealthValue('health-readback',readbackText,staleFans.length?'warn':'ok');
  var retrySeconds=Math.max(0,Number(health.retry_after_unix||0)-Math.floor(Date.now()/1000));
  setHealthValue('health-control-retry',retrySeconds?(retrySeconds+'s'):(LANG==='ko'?'대기 없음':'ready'),retrySeconds?'warn':'info');
  setHealthValue('health-control-error',health.last_error||'—',health.last_error?'warn':'info');
  setText('fan-curve-input',tempValue(d.fan_curve_input_temp_c));
  setText('fan-safety-hottest',tempValue(d.fan_safety_temp_c));
  setText('fan-critical-limit',tempValue(d.fan_critical_temp_c));
  var approval=d.daemon_running&&!d.daemon_update_needed
    ?(LANG==='ko'?'추가 승인 없음':'no extra prompt')
    :(d.daemon_update_needed?(LANG==='ko'?'재설치 때 1회':'one prompt to reinstall'):(d.can_control?(LANG==='ko'?'최초 설정 때 1회':'one prompt for setup'):(LANG==='ko'?'필요 없음':'not needed')));
  setHealthValue('health-approval',approval,d.daemon_running&&!d.daemon_update_needed?'ok':(d.can_control?'warn':'info'));
  setHealthValue('health-app','v'+(d.app_version||''),'info');
  renderFanActionLog(d);
}
function updateHardwareAvailability(d){
  var fanCount=d.fan_count||0, controllable=d.controllable_fan_count||0;
  var battery=!!d.batt_present;
  var networkCount=d.network_count||0, networkActive=!!d.network_active;
  var allGood=(controllable>0)&&battery&&networkActive;
  setPanelPill('hardware-pill',allGood?(LANG==='ko'?'감지됨':'Detected'):(LANG==='ko'?'확인 필요':'Check'),allGood?'ok':'info');
  setHealthValue('hardware-fans',
    controllable>0
      ?(controllable+' / '+fanCount)
      :(fanCount>0?(LANG==='ko'?'읽기 전용 팬 '+fanCount+'개':fanCount+' read-only fans'):(LANG==='ko'?'감지 안 됨':'not detected')),
    controllable>0?'ok':'info');
  setHealthValue('hardware-battery',battery?(LANG==='ko'?'감지됨':'detected'):(LANG==='ko'?'배터리 없음':'not present'),battery?'ok':'info');
  setHealthValue('hardware-network',
    networkActive
      ?(LANG==='ko'?'활성':'active')
      :(networkCount>0?(LANG==='ko'?'비활성':'idle'):(LANG==='ko'?'감지 안 됨':'not detected')),
    networkActive?'ok':'info');
}
function updateFanEmptyState(d){
  var empty=document.getElementById('fan-empty-state');
  if(!empty)return;
  var fans=d.fans||[];
  var controllable=fans.filter(function(f){return !!f.controllable;}).length;
  var show=controllable===0;
  empty.style.display=show?'':'none';
  if(show){
    empty.textContent=fans.length>0
      ?(LANG==='ko'?'이 Mac의 팬은 감지됐지만 앱에서 직접 제어할 수 없습니다. 실시간 RPM은 계속 확인할 수 있습니다.':'Fans are detected, but this Mac does not expose controllable fan writes. Live RPM is still monitored.')
      :(LANG==='ko'?'제어 가능한 팬을 찾지 못했습니다. PeterFan은 CPU, 메모리, 온도, 네트워크 상태는 계속 모니터링합니다.':'No controllable fans detected. PeterFan can still monitor CPU, memory, temperature, and network activity.');
  }
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
    var avgLabel=LANG==='ko'?'기간 평균':'range avg';
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
  window.ipc.postMessage('h:520');
}
// The popover stays fixed-height; overflow belongs inside `.main-pane`.
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
    fn left_click_opens_on_down_and_consumes_the_matching_up() {
        let mut down_seen = false;
        assert!(route_left_click(MouseButtonState::Down, &mut down_seen));
        assert!(down_seen);
        assert!(!route_left_click(MouseButtonState::Up, &mut down_seen));
        assert!(!down_seen);

        // Preserve compatibility if a platform ever emits only mouse-up.
        assert!(route_left_click(MouseButtonState::Up, &mut down_seen));
    }

    #[test]
    fn background_read_never_starts_duplicate_hardware_calls() {
        let state = Arc::new(BackgroundRead::<u8>::default());
        let (release_tx, release_rx) = std::sync::mpsc::sync_channel(0);
        assert!(state.start(move || {
            release_rx.recv().ok()?;
            Some(42)
        }));
        assert!(!state.start(|| Some(7)));
        release_tx.send(()).unwrap();

        let value = (0..100).find_map(|_| {
            let value = state.take();
            if value.is_none() {
                std::thread::sleep(Duration::from_millis(5));
            }
            value
        });
        assert_eq!(value, Some(42));
    }

    fn tray_rect(x: f64, y: f64, width: u32, height: u32) -> Rect {
        Rect {
            position: tray_icon::dpi::PhysicalPosition::new(x, y),
            size: tray_icon::dpi::PhysicalSize::new(width, height),
        }
    }

    #[test]
    fn popover_position_stays_on_left_external_display() {
        let displays = [
            DisplayBounds {
                x: -1920.0,
                y: 0.0,
                width: 1920.0,
                height: 1080.0,
            },
            DisplayBounds {
                x: 0.0,
                y: 0.0,
                width: 1728.0,
                height: 1117.0,
            },
        ];
        let pos = popover_position_for_rect(tray_rect(-40.0, 0.0, 24, 24), 440.0, &displays);

        assert!(pos.x < 0.0, "popover should remain on the clicked display");
        assert!(pos.x >= displays[0].x + 8.0);
        assert!(pos.x + 440.0 <= displays[0].right() - 8.0);
        assert_eq!(pos.y, 24.0);
    }

    #[test]
    fn popover_position_stays_on_right_external_display() {
        let displays = [
            DisplayBounds {
                x: 0.0,
                y: 0.0,
                width: 1728.0,
                height: 1117.0,
            },
            DisplayBounds {
                x: 1728.0,
                y: 0.0,
                width: 2560.0,
                height: 1440.0,
            },
        ];
        let pos = popover_position_for_rect(tray_rect(4260.0, 0.0, 24, 24), 440.0, &displays);

        assert!(
            pos.x >= displays[1].x,
            "popover should stay on the clicked display"
        );
        assert!(pos.x + 440.0 <= displays[1].right() - 8.0);
        assert_eq!(pos.y, 24.0);
    }

    #[test]
    fn logical_popover_anchor_ignores_previous_window_scale_on_mixed_dpi_display() {
        let anchor = LogicalPopoverAnchor {
            x: 4240.0,
            y: 24.0,
            display: DisplayBounds {
                x: 1728.0,
                y: 0.0,
                width: 2560.0,
                height: 1440.0,
            },
            scale: 1.0,
        };
        let pos = popover_position_for_anchor(anchor, POPOVER_W);

        assert_eq!(pos.x, 3800.0);
        assert_eq!(pos.y, 24.0);
        assert!(pos.x >= anchor.display.x);
        assert!(pos.x + POPOVER_W <= anchor.display.right());
    }

    #[test]
    fn logical_popover_anchor_stays_on_left_retina_display() {
        let anchor = LogicalPopoverAnchor {
            x: -20.0,
            y: -876.0,
            display: DisplayBounds {
                x: -1512.0,
                y: -900.0,
                width: 1512.0,
                height: 982.0,
            },
            scale: 2.0,
        };
        let pos = popover_position_for_anchor(anchor, POPOVER_W);

        assert!(pos.x < 0.0);
        assert!(pos.x >= anchor.display.x + 8.0);
        assert_eq!(popover_height_for_anchor(anchor), 520.0);
    }

    #[test]
    fn max_popover_height_respects_display_y_offset() {
        let upper_display = DisplayBounds {
            x: 0.0,
            y: -900.0,
            width: 1440.0,
            height: 900.0,
        };
        let lower_display = DisplayBounds {
            x: 0.0,
            y: 0.0,
            width: 1440.0,
            height: 900.0,
        };

        assert_eq!(max_popover_height_for_bounds(upper_display, -876.0), 864.0);
        assert_eq!(max_popover_height_for_bounds(lower_display, 24.0), 864.0);
    }

    #[test]
    fn popover_height_is_fixed_unless_display_is_short() {
        assert_eq!(POPOVER_H, 520.0);
        let displays = [DisplayBounds {
            x: 0.0,
            y: 0.0,
            width: 1440.0,
            height: 900.0,
        }];
        assert_eq!(
            popover_height_for_rect(tray_rect(700.0, 0.0, 24, 24), 1.0, &displays),
            520.0
        );
        let short_display = [DisplayBounds {
            x: 0.0,
            y: 0.0,
            width: 1440.0,
            height: 480.0,
        }];
        assert_eq!(
            popover_height_for_rect(tray_rect(700.0, 0.0, 24, 24), 1.0, &short_display),
            444.0
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
        assert!(ko.contains(">시스템<"));
        assert!(ko.contains(">상세 창 열기<"));
        assert!(ko.contains("저장공간, 배터리, 네트워크"));
        assert!(ko.contains(">자동<"));
        assert!(ko.contains(">균형<"));
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
        assert!(ko.contains(">선택한 점<"));
        assert!(ko.contains(r#"id="cp-temp""#) && ko.contains(r#"id="cp-duty""#));
        // Both languages must still be well-formed enough to contain the
        // dynamic element IDs the JS `update()` function looks up — a typo'd
        // replacement (e.g. matching too broadly) would silently break these.
        for html in [&en, &ko] {
            assert!(html.contains(r#"id="cpu-val""#));
            assert!(html.contains(r#"id="temp-name""#));
            assert!(html.contains("d.temp_source"));
            assert!(html.contains("기간 평균"));
            assert!(html.contains("range avg"));
            assert!(html.contains(r#"id="ctl-status""#));
            assert!(html.contains(r#"id="disk-io-chart-stats""#));
            assert!(html.contains(r#"id="net-ip""#));
            assert!(html.contains(r#"id="ps-cpu""#));
            assert!(html.contains("quitProcess"));
            assert!(html.contains("renderFanCards"));
            assert!(html.contains("fanControlSetupButton"));
            assert!(html.contains("startFanControlSetup"));
            assert!(html.contains(r#"id="fan-cards""#));
            assert!(html.contains(r#"id="profile-strip""#));
            assert!(html.contains("setProfile"));
            assert!(html.contains("setAuto"));
            assert!(html.contains("updateProfileStrip"));
            assert!(html.contains("cmd:auto"));
            assert!(html.contains("cmd:profile:"));
            assert!(html.contains(r#"id="setup-row""#));
            assert!(html.contains(r#"id="setup-more""#));
            assert!(html.contains(r#"aria-haspopup="menu""#));
            assert!(html.contains(r#"aria-expanded="false""#));
            assert!(html.contains(r#"id="setup-menu""#));
            assert!(html.contains(r#"role="menu""#));
            assert!(html.contains("toggleSetupMenu"));
            assert!(html.contains("handleSetupMenuKey"));
            assert!(html.contains("focusSetupMenuItem"));
            assert!(html.contains("setup-menu-item"));
            assert!(html.contains(r#"role="menuitem""#));
            assert!(html.contains("checkupdates"));
            assert!(html.contains("checkAppUpdates"));
            assert!(html.contains("updateSetup"));
            assert!(html.contains("daemon_update_needed"));
            assert!(html.contains("Reinstall fan control"));
            assert!(html.contains("Reinstall Fan Control"));
            assert!(html.contains("Check for app updates"));
            assert!(html.contains("cmd:fanhold:"));
            assert!(html.contains("cmd:fanauto:"));
            assert!(html.contains("savecurve:"));
        }
    }

    #[test]
    fn dashboard_html_has_runcat_style_action_rail() {
        let en = dashboard_html(ResolvedLanguage::En, false);
        let ko = dashboard_html(ResolvedLanguage::Ko, false);

        for html in [&en, &ko] {
            assert!(html.contains(r#"class="dashboard-shell""#));
            assert!(html.contains(r#"class="action-rail""#));
            assert!(html.contains("railDetail"));
            assert!(html.contains("railFan"));
            assert!(html.contains("railUpdate"));
            assert!(html.contains("railSettings"));
            assert!(html.contains("railMore"));
            assert!(html.contains("focusFanControl"));
            assert!(html.contains("rail-settings-panel"));
            assert!(html.contains("rail-more-panel"));
            assert!(!html.contains("railLicense"));
            assert!(!html.contains("rail-license-panel"));
            assert!(html.contains(r#"class="rail-btn active" id="railDetail""#));
            assert!(!html.contains(r#"class="rail-btn primary""#));
            assert!(!html.contains("flashRailButton"));
            assert!(!html.contains("rail-btn.pulse"));
            assert!(html.contains("html,body{background:var(--panel-bg)"));
            assert_eq!(html.matches("data-range=\"2m\"").count(), 1);
        }

        assert!(en.contains(">Status<"));
        assert!(en.contains(">Fan<"));
        assert!(en.contains(">Updates<"));
        assert!(en.contains(">Settings<"));
        assert!(en.contains(">System<"));
        assert!(ko.contains(">상태<"));
        assert!(ko.contains(">팬<"));
        assert!(ko.contains(">업데이트<"));
        assert!(ko.contains(">설정<"));
        assert!(ko.contains(">시스템<"));
    }

    #[test]
    fn dashboard_keeps_runner_character_out_of_popover() {
        let en = dashboard_html(ResolvedLanguage::En, false);
        let ko = dashboard_html(ResolvedLanguage::Ko, false);

        for html in [&en, &ko] {
            assert!(!html.contains(r#"id="runner-strip""#));
            assert!(!html.contains(r#"id="runner-load""#));
            assert!(!html.contains(r#"class="runner-character""#));
            assert!(!html.contains("runner-head"));
            assert!(!html.contains("runner-torso"));
            assert!(!html.contains("runner-arm a"));
            assert!(!html.contains("runner-leg b"));
            assert!(!html.contains("@keyframes runnerTravel"));
            assert!(!html.contains("@keyframes runnerStride"));
            assert!(!html.contains("function updateRunner(cpuPct)"));
            assert!(!html.contains("--runner-speed"));
            assert!(!html.contains("updateRunner(d.cpu_pct);"));
            assert!(!html.contains(">Runner<"));
            assert!(!html.contains(">러너<"));
        }
    }

    #[test]
    fn menu_bar_uses_cpu_driven_runner_icon() {
        let _idle = menubar_runner_icon(8.0, 0);
        let _busy = make_runner_icon(92.0, 3);
        assert_ne!(make_cat_runner_rgba(8.0, 0), make_cat_runner_rgba(92.0, 0));
        assert_ne!(make_cat_runner_rgba(50.0, 0), make_cat_runner_rgba(50.0, 1));
    }

    #[test]
    fn menu_bar_runner_is_cat_like_and_animated() {
        let idle = make_cat_runner_rgba(12.0, 0);
        let active = make_cat_runner_rgba(72.0, 1);

        assert!(idle.chunks_exact(4).any(|px| px[3] > 0));
        assert!(active.chunks_exact(4).any(|px| px[3] > 0));
        assert_ne!(idle, active);
    }

    #[test]
    fn runner_animation_speed_tracks_cpu_load() {
        let idle = runner_frame_interval(5.0);
        let normal = runner_frame_interval(45.0);
        let busy = runner_frame_interval(95.0);

        assert!(idle > normal);
        assert!(normal > busy);
        assert!(idle <= RUNNER_MAX_INTERVAL);
        assert!(busy >= RUNNER_MIN_INTERVAL);
        assert!(idle.as_millis() >= 900);
        assert!((300..=450).contains(&normal.as_millis()));
        assert!(busy.as_millis() <= 160);

        assert!(!runner_enabled(MenubarDisplay::Number));
        assert!(runner_enabled(MenubarDisplay::Graph));
        assert!(runner_enabled(MenubarDisplay::Both));
        assert_eq!(make_runner_icons().len(), 16);
        assert_eq!(runner_load_band(10.0), 0);
        assert_eq!(runner_load_band(90.0), 3);
    }

    #[test]
    fn dashboard_has_no_license_entry_points() {
        let en = dashboard_html(ResolvedLanguage::En, false);
        let ko = dashboard_html(ResolvedLanguage::Ko, false);

        for html in [&en, &ko] {
            assert!(!html.contains("railLicense"));
            assert!(!html.contains("rail-license-panel"));
            assert!(!html.contains("rail-license-form"));
            assert!(!html.contains("lic-row"));
            assert!(!html.contains("lic-form"));
            assert!(!html.contains("Buy License"));
            assert!(!html.contains("Activate"));
            assert!(!html.contains("License"));
            assert!(!html.contains("라이선스"));
            assert!(!html.contains("license:"));
            assert!(!html.contains("submitLicense"));
        }
    }

    #[test]
    fn dashboard_html_supports_compact_popover_mode() {
        let en = dashboard_html(ResolvedLanguage::En, false);

        assert!(en.contains("compact-extra"));
        assert!(en.contains(r#"data-compact-extra="storage""#));
        assert!(en.contains(r#"data-compact-extra="battery""#));
        assert!(en.contains(r#"data-compact-extra="network""#));
        assert!(en.contains(r#"data-compact-extra="processes""#));
        assert!(en.contains("applyPopoverMode"));
        assert!(en.contains("setPopoverExpanded"));
        assert!(en.contains("pf.compact"));
    }

    #[test]
    fn dashboard_main_pane_scrolls_without_visible_scrollbar() {
        let en = dashboard_html(ResolvedLanguage::En, false);

        assert!(en.contains(".panel{"));
        assert!(en.contains("overflow:hidden"));
        assert!(en.contains(".dashboard-shell{"));
        assert!(en.contains("height:100vh"));
        assert!(en.contains(".main-pane{"));
        assert!(en.contains("overflow-y:auto"));
        assert!(en.contains("scrollbar-gutter:stable"));
        assert!(en.contains("scrollbar-width:none"));
        assert!(en.contains(".main-pane::-webkit-scrollbar{display:none;}"));
        assert!(en.contains(".action-rail{"));
        assert!(!en.contains("position:sticky;top:7px"));
    }

    #[test]
    fn action_rail_buttons_route_through_clickable_handlers() {
        let en = dashboard_html(ResolvedLanguage::En, false);

        for (id, action) in [
            ("railDetail", "detail"),
            ("railFan", "fan"),
            ("railUpdate", "update"),
            ("railSettings", "settings"),
            ("railMore", "more"),
        ] {
            assert!(en.contains(&format!(r#"id="{id}""#)));
            assert!(en.contains(&format!(r#"data-rail-action="{action}""#)));
        }

        assert!(en.contains("function runRailAction(action,btn)"));
        assert!(!en.contains("flashRailButton(btn)"));
        assert!(en.contains("case 'detail':setRailView('overview');break;"));
        assert!(en.contains("case 'fan':setRailView('fan');break;"));
        assert!(en.contains("case 'update':setRailView('update');break;"));
        assert!(en.contains("case 'settings':setRailView('settings');break;"));
        assert!(!en.contains(
            "case 'login':setRailView('login');window.ipc.postMessage('togglelogin');break;"
        ));
        assert!(!en.contains("case 'license':setRailView('license');break;"));
        assert!(en.contains("case 'more':setRailView('more');break;"));
        assert!(en.contains("function setButtonLabel(btn,label)"));
        assert!(en.contains("btn.querySelector('span')"));
        assert!(en.contains("btn.dataset.defaultLabel"));
        assert!(en.contains("el.classList.add('focus-pulse')"));
    }

    #[test]
    fn update_rail_keeps_status_button_as_navigation_not_detail_command() {
        let en = dashboard_html(ResolvedLanguage::En, false);
        let ko = dashboard_html(ResolvedLanguage::Ko, false);

        assert!(en.contains(r#"title="Status overview""#));
        assert!(en.contains("setButtonLabel(detail,LANG==='ko'?'상태':'Status');"));
        assert!(en.contains("detail.title=LANG==='ko'?'상태 요약':'Status overview';"));
        assert!(ko.contains(">상태<"));
        assert!(!en.contains("setButtonLabel(detail,LANG==='ko'?'상세':'Detail');"));
        assert!(!en.contains("detail.title=LANG==='ko'?'상세 창 열기':'Open detailed window';"));
    }

    #[test]
    fn rail_panels_show_live_status_pills() {
        let en = dashboard_html(ResolvedLanguage::En, false);

        for id in ["rail-update-pill", "rail-settings-pill", "rail-more-pill"] {
            assert!(en.contains(&format!(r#"id="{id}""#)));
        }

        assert!(en.contains(".panel-title-row{"));
        assert!(en.contains(".panel-pill{"));
        assert!(en.contains("setPanelPill('rail-update-pill'"));
        assert!(en.contains("setPanelPill('rail-settings-pill'"));
        assert!(en.contains("setPanelPill('rail-more-pill'"));
        assert!(en.contains("function setPanelPill(id,text,tone)"));
    }

    #[test]
    fn settings_panel_contains_fan_control_health_card() {
        let en = dashboard_html(ResolvedLanguage::En, false);
        let ko = dashboard_html(ResolvedLanguage::Ko, false);

        assert!(en.contains(r#"id="fan-health-card""#));
        assert!(en.contains(">Fan Control Health<"));
        assert!(en.contains(r#"id="health-daemon""#));
        assert!(en.contains(r#"id="health-helper""#));
        assert!(en.contains(r#"id="health-launch-daemon""#));
        assert!(en.contains(r#"id="health-team-id""#));
        assert!(en.contains(r#"id="health-control-path""#));
        assert!(en.contains(r#"id="health-last-command""#));
        assert!(en.contains(r#"id="health-safety-state""#));
        assert!(en.contains(r#"id="health-curve-input""#));
        assert!(en.contains(r#"id="health-core-hottest""#));
        assert!(en.contains(r#"id="health-safety-hottest""#));
        assert!(en.contains(r#"id="health-critical-limit""#));
        assert!(en.contains(r#"id="health-sensor-failures""#));
        assert!(en.contains(r#"id="health-write-failures""#));
        assert!(en.contains(r#"id="health-readback""#));
        assert!(en.contains(r#"id="health-control-retry""#));
        assert!(en.contains(r#"id="health-control-error""#));
        assert!(en.contains(r#"id="fan-curve-input""#));
        assert!(en.contains(r#"id="fan-safety-hottest""#));
        assert!(en.contains(r#"id="fan-critical-limit""#));
        assert!(en.contains(r#"id="health-approval""#));
        assert!(en.contains(r#"id="fan-action-log-card""#));
        assert!(en.contains(r#"id="fan-diagnostic-button""#));
        assert!(en.contains(r#"id="fan-action-log""#));
        assert!(en.contains("function updateHealthPanel(d)"));
        assert!(en.contains("function runFanDiagnostics(btn)"));
        assert!(en.contains("function renderFanActionLog(d)"));
        assert!(en.contains("cmd:diagnosefan"));
        assert!(en.contains("d.fan_action_log"));
        assert!(en.contains("updateHealthPanel(d);"));
        assert!(en.contains("d.daemon_required_version"));
        assert!(en.contains("d.daemon_binary_installed"));
        assert!(en.contains("d.launch_daemon_installed"));
        assert!(en.contains("d.team_id"));
        assert!(en.contains("d.controllable_fan_count"));
        assert!(en.contains("d.fan_curve_input_temp_c"));
        assert!(en.contains("d.fan_core_hottest_temp_c"));
        assert!(en.contains("d.control_health"));
        assert!(en.contains("health.failsafe_active"));
        assert!(ko.contains(">안전 상태<"));
        assert!(ko.contains(">센서 실패<"));
        assert!(ko.contains(">제어 재시도<"));
        assert!(en.contains("d.fan_safety_temp_c"));
        assert!(en.contains("d.fan_critical_temp_c"));

        assert!(ko.contains(">팬 제어 상태<"));
        assert!(ko.contains(">팬 RPM 검증<"));
        assert!(ko.contains(">데몬<"));
        assert!(ko.contains(">도우미<"));
        assert!(ko.contains(">제어 경로<"));
        assert!(ko.contains(">마지막 명령<"));
        assert!(ko.contains(">커브 입력<"));
        assert!(ko.contains(">코어 최고<"));
        assert!(ko.contains(">안전 최고<"));
        assert!(ko.contains(">임계값<"));
        assert!(ko.contains(">관리자 승인<"));
        assert!(ko.contains(">최근 팬 제어 이력<"));
        assert!(ko.contains(">진단 실행<"));
    }

    #[test]
    fn fan_diagnostic_reports_ready_hardware_without_writing() {
        let (ok, status) = format_fan_diagnostic(FanDiagnosticInput {
            fan_count: 2,
            controllable_count: 2,
            average_c: Some(67.4),
            safety_c: Some(74.1),
            critical_c: 95.0,
            daemon_version: Some(peterfan_platform::MIN_REQUIRED_DAEMON_VERSION),
            daemon_reachable: true,
            readback_stale: false,
        });

        assert!(ok);
        assert!(status.contains("fans 2/2"));
        assert!(status.contains("CPU avg 67°C"));
        assert!(status.contains("safety 74°C"));
        assert!(status.contains("daemon v"));
        assert!(status.contains("ready"));
    }

    #[test]
    fn fan_diagnostic_rejects_stale_or_unreachable_daemon() {
        let base = |daemon_version, daemon_reachable, readback_stale| FanDiagnosticInput {
            fan_count: 2,
            controllable_count: 2,
            average_c: Some(70.0),
            safety_c: Some(75.0),
            critical_c: 95.0,
            daemon_version,
            daemon_reachable,
            readback_stale,
        };
        let (stale_ok, stale) = format_fan_diagnostic(base(Some("0.1.0"), true, false));
        let (offline_ok, offline) = format_fan_diagnostic(base(None, false, false));
        let (readback_ok, readback) = format_fan_diagnostic(base(
            Some(peterfan_platform::MIN_REQUIRED_DAEMON_VERSION),
            true,
            true,
        ));

        assert!(!stale_ok);
        assert!(stale.contains("needs update"));
        assert!(!offline_ok);
        assert!(offline.contains("not running"));
        assert!(!readback_ok);
        assert!(readback.contains("RPM readback stale"));
    }

    #[test]
    fn fan_control_actions_have_readable_log_labels() {
        assert_eq!(
            control_action_label("profile:performance"),
            "profile performance"
        );
        assert_eq!(control_action_label("fanhold:fan.cpu:72"), "fan fan.cpu:72");
        assert_eq!(control_action_label("fanauto:fan.cpu"), "fan fan.cpu auto");
        assert_eq!(control_action_label("auto"), "auto");
    }

    #[test]
    fn system_panel_contains_hardware_empty_states() {
        let en = dashboard_html(ResolvedLanguage::En, false);
        let ko = dashboard_html(ResolvedLanguage::Ko, false);

        assert!(en.contains(r#"id="hardware-availability-card""#));
        assert!(en.contains(">Hardware Availability<"));
        assert!(en.contains(r#"id="hardware-fans""#));
        assert!(en.contains(r#"id="hardware-battery""#));
        assert!(en.contains(r#"id="hardware-network""#));
        assert!(en.contains(r#"id="fan-empty-state""#));
        assert!(en.contains("function updateHardwareAvailability(d)"));
        assert!(en.contains("updateHardwareAvailability(d);"));
        assert!(en.contains("d.network_count"));
        assert!(en.contains("d.network_active"));
        assert!(en.contains("No controllable fans detected"));

        assert!(ko.contains(">하드웨어 감지 상태<"));
        assert!(ko.contains(">배터리<"));
        assert!(ko.contains(">네트워크<"));
        assert!(ko.contains("제어 가능한 팬을 찾지 못했습니다"));
    }

    #[test]
    fn settings_panel_contains_startup_toggle() {
        let en = dashboard_html(ResolvedLanguage::En, false);
        let ko = dashboard_html(ResolvedLanguage::Ko, false);

        for html in [en, ko] {
            assert!(html.contains(r#"id="startup-setting"#));
            assert!(html.contains(">Run on startup<") || html.contains("부팅 시 자동 실행"));
            assert!(html.contains(r#"id="startup-toggle"#));
            assert!(html.contains("toggleStartupItem(this)"));
        }
    }

    #[test]
    fn update_panel_shows_current_latest_and_check_result() {
        let en = dashboard_html(ResolvedLanguage::En, false);

        assert!(en.contains(r#"id="update-current-version""#));
        assert!(en.contains(r#"id="update-latest-version""#));
        assert!(en.contains(r#"id="update-check-result""#));
        assert!(en.contains(r#"id="update-release-link""#));
        assert!(en.contains(r#"id="update-release-notes-card""#));
        assert!(en.contains(r#"id="update-release-notes""#));
        assert!(en.contains("function compareVersions(a,b)"));
        assert!(en.contains("function formatReleaseNotes(body)"));
        assert!(en.contains("function fetchLatestReleaseStatus()"));
        assert!(en.contains("function renderUpdateStatus(status)"));
        assert!(en.contains("https://api.github.com/repos/uulab-official/peterfan/releases/latest"));
        assert!(en.contains("notes:formatReleaseNotes(j.body||'')"));
        assert!(en.contains("fetchLatestReleaseStatus();"));
        assert!(en.contains("APP_UPDATE_STATUS.current=d.app_version||APP_UPDATE_STATUS.current;"));
        assert!(en.contains("d.update_install_result"));
        assert!(en.contains("s.install_status==='installed'"));
        assert!(en.contains("s.install_status==='rolled_back'"));
        assert!(en.contains("installed successfully"));
        assert!(en.contains("d.app_version"));
    }

    #[test]
    fn temperature_section_contains_raw_sensor_inventory() {
        let en = dashboard_html(ResolvedLanguage::En, false);

        assert!(en.contains(r#"id="all-temp-head""#));
        assert!(en.contains(r#"id="all-temp-list""#));
        assert!(en.contains("function toggleRawTemps()"));
        assert!(en.contains("function renderRawTempList(d)"));
        assert!(en.contains("d.all_temps||[]"));
        assert!(en.contains("All sensors"));
        assert!(en.contains("className='sensor-group-head'"));
        assert!(en.contains("t.source||''"));
        assert!(en.contains("<span class=\"src\"></span>"));
    }

    #[test]
    fn sensor_group_labels_cover_every_sensor_kind() {
        for (kind, en, ko) in [
            (SensorKind::Cpu, "CPU", "CPU"),
            (SensorKind::Gpu, "GPU", "GPU"),
            (SensorKind::Memory, "Memory", "메모리"),
            (SensorKind::Storage, "Storage", "저장장치"),
            (SensorKind::Mainboard, "Mainboard", "메인보드"),
            (SensorKind::Battery, "Battery", "배터리"),
            (SensorKind::Other, "Other", "기타"),
        ] {
            assert_eq!(sensor_group_label(ResolvedLanguage::En, kind), en);
            assert_eq!(sensor_group_label(ResolvedLanguage::Ko, kind), ko);
        }
    }

    #[test]
    fn rail_view_switch_resets_scroll_and_marks_pressed_state() {
        let en = dashboard_html(ResolvedLanguage::En, false);

        assert!(en.contains(r#"id="railDetail" data-rail-action="detail" aria-pressed="true""#));
        assert!(en.contains(r#"id="railFan" data-rail-action="fan" aria-pressed="false""#));
        assert!(en.contains("function resetRailPaneScroll()"));
        assert!(en.contains("pane.scrollTop=0;"));
        assert!(en.contains("function setRailView(view)"));
        assert!(en.contains("applyRailView(true);"));
        assert!(en.contains("function applyRailView(resetScroll)"));
        assert!(en.contains("if(resetScroll)resetRailPaneScroll();"));
        assert!(en.contains("el.setAttribute('aria-pressed',on?'true':'false');"));
        assert!(!en.contains("function applyRailView(){"));
        assert_eq!(
            en.matches("note.appendChild(fanControlSetupButton").count(),
            1
        );
    }

    #[test]
    fn settings_keep_technical_health_details_collapsed() {
        let en = dashboard_html(ResolvedLanguage::En, false);
        let ko = dashboard_html(ResolvedLanguage::Ko, false);

        assert!(
            en.contains(r#"<details class="health-details"><summary>Technical details</summary>"#)
        );
        assert!(ko.contains(r#"<details class="health-details"><summary>기술 정보</summary>"#));
        assert!(!en.contains(r#"<details class="health-details" open>"#));
    }

    #[test]
    fn action_rail_buttons_switch_left_pane_views() {
        let en = dashboard_html(ResolvedLanguage::En, false);

        assert!(en.contains(r#"<body class="compact" data-rail-view="overview">"#));
        assert!(en.contains("var RAIL_VIEW=storageGet('pf.rail.view')||'overview';"));
        assert!(en.contains("function setRailView(view)"));
        assert!(en.contains("function applyRailView(resetScroll)"));
        assert!(en.contains("function railView(){"));
        assert!(en.contains("return RAIL_VIEW||'overview';"));
        assert!(en.contains("RAIL_VIEW=view;"));
        assert!(en.contains(r#"id="rail-update-panel""#));
        assert!(en.contains(r#"id="rail-settings-panel""#));
        assert!(!en.contains(r#"id="rail-license-panel""#));
        assert!(!en.contains("rail-license-form"));
        assert!(!en.contains("submitLicenseInput('rail-lic-input')"));
        assert!(en.contains(r#"id="sec-cpu""#));
        assert!(en.contains(r#"id="sec-mem""#));
        assert!(en.contains(r#"id="sec-storage""#));
        assert!(en.contains(r#"id="sec-network""#));
        assert!(en.contains("case 'fan':setRailView('fan');break;"));
        assert!(en.contains("case 'update':setRailView('update');break;"));
        assert!(en.contains("case 'settings':setRailView('settings');break;"));
        assert!(!en.contains(
            "case 'login':setRailView('login');window.ipc.postMessage('togglelogin');break;"
        ));
        assert!(!en.contains("case 'license':setRailView('license');break;"));
        assert!(en.contains("case 'more':setRailView('more');break;"));
        assert!(!en.contains("case 'license':toggleLicForm();break;"));
    }

    #[test]
    fn fan_setup_prompt_is_scoped_to_fan_and_settings_views() {
        let en = dashboard_html(ResolvedLanguage::En, false);

        assert!(en.contains("if(view==='fan'){\n    ['setup-row','fan-control-section'].forEach"));
        assert!(en.contains("} else {\n    ['range-tabs','sec-cpu','sec-mem','sec-temp'].forEach"));
        assert!(!en.contains("['range-tabs','setup-row','sec-cpu','sec-mem','sec-temp'].forEach"));
        assert!(en.contains("setButtonLabel(fan,LANG==='ko'?'팬 제어':'Fans');"));
        assert!(!en.contains("setButtonLabel(fan,d.fan_setup_needed"));
        assert!(!en.contains("fan.classList.toggle('active',!!d.can_control);"));
    }

    #[test]
    fn system_view_keeps_metrics_and_detail_action_together() {
        let en = dashboard_html(ResolvedLanguage::En, false);

        assert!(en.contains(r#"id="rail-more-panel""#));
        assert!(en.contains("case 'detail':setRailView('overview');break;"));
        assert!(en.contains("case 'more':setRailView('more');break;"));
        assert!(en.contains(r#"onclick="window.ipc.postMessage('open_detail')""#));
        assert!(en.contains(r#"onclick="window.ipc.postMessage('quit')""#));
        assert_eq!(en.matches(">Open Detail Window<").count(), 1);
        assert!(en.contains(
            "['rail-more-panel','sec-storage','sec-batt','sec-network','sec-procs','foot'].forEach"
        ));
        assert!(!en.contains("case 'detail':window.ipc.postMessage('open_detail');break;"));
        assert!(!en.contains("if(view==='more')setPopoverExpanded(true);"));
    }

    #[test]
    fn more_view_reveals_low_priority_metric_sections() {
        let en = dashboard_html(ResolvedLanguage::En, false);

        assert!(en.contains(r#"body.compact[data-rail-view="more"] .compact-extra"#));
        assert!(en.contains(
            "['rail-more-panel','sec-storage','sec-batt','sec-network','sec-procs','foot'].forEach"
        ));
        assert!(en.contains(r#"data-compact-extra="storage""#));
        assert!(en.contains(r#"data-compact-extra="battery""#));
        assert!(en.contains(r#"data-compact-extra="network""#));
        assert!(en.contains(r#"data-compact-extra="processes""#));
        assert!(en.contains("System Metrics"));
        assert!(en.contains("Storage, battery, network, and active processes."));
    }

    #[test]
    fn action_rail_does_not_resize_popover_to_content_height() {
        let en = dashboard_html(ResolvedLanguage::En, false);
        let source = include_str!("main.rs");

        assert!(en.contains("function reportHeight()"));
        assert!(en.contains("window.ipc.postMessage('h:520');"));
        assert!(en.contains("overflow belongs inside `.main-pane`"));
        assert!(source.contains("const POPOVER_H: f64 = 520.0;"));
        assert!(source.contains("const DASHBOARD_BACKGROUND: RGBA = (27, 27, 29, 255);"));
        assert!(
            source
                .matches(".with_background_color(DASHBOARD_BACKGROUND)")
                .count()
                >= 2
        );
        assert!(source.contains("fn popover_height_for_rect("));
        assert!(source.contains("const POPOVER_SHOW_DELAY: Duration = Duration::from_millis(35);"));
        assert!(source.contains("app.popover_show_at = Some(Instant::now() + POPOVER_SHOW_DELAY);"));
        assert!(source.contains("body.starts_with(\"h:\")"));
        assert!(!en.contains("document.body.scrollHeight"));
        assert!(!en.contains("document.documentElement.scrollHeight"));
        assert!(!en.contains("main?main.scrollHeight:0"));
        assert!(!en.contains("contentH+shellPad"));
    }

    #[test]
    fn dashboard_slow_sections_are_not_recomputed_every_tick() {
        let source = include_str!("main.rs");

        assert!(source.contains("const DASHBOARD_SLOW_REFRESH: Duration = Duration::from_secs(3);"));
        assert!(source.contains("struct DashboardSlowCache"));
        assert!(source.contains("fn refresh_dashboard_slow_cache("));
        assert!(source.contains("now >= app.next_dashboard_slow_refresh"));
        assert!(source.contains("app.dashboard_slow_cache.proc_sort != proc_sort"));
        assert!(source.contains("app.next_dashboard_slow_refresh = now + DASHBOARD_SLOW_REFRESH;"));
    }

    #[test]
    fn rail_view_panels_override_default_hidden_css() {
        let en = dashboard_html(ResolvedLanguage::En, false);

        assert!(en.contains(".rail-panel{display:none"));
        assert!(en.contains("if(on&&el.classList.contains('rail-panel'))el.style.display='block';"));
        assert!(!en.contains("if(el)el.style.display=on?'':'none';"));
    }

    #[test]
    fn dashboard_sections_share_spacing_without_forced_empty_height() {
        let en = dashboard_html(ResolvedLanguage::En, false);

        assert!(en.contains("--content-x:15px;--section-y:11px;--panel-pad:14px;"));
        assert!(en.contains("padding:var(--section-y) var(--content-x)"));
        assert!(en.contains("padding:var(--panel-pad) var(--content-x)"));
        assert!(en.contains(".range-tabs{display:flex;gap:4px;padding:10px var(--content-x);"));
        assert!(en.contains(
            "#sec-mem,#sec-temp,#sec-batt,#sec-network,#sec-procs{border-top:1px solid var(--line);}"
        ));
        assert!(!en.contains(".row + .row"));
        assert!(en.contains(".main-pane{") && en.contains("contain:layout paint;"));
        assert!(en.contains(".action-rail{") && en.matches("contain:layout paint;").count() >= 2);
        assert!(!en.contains("min-height:220px"));
    }

    #[test]
    fn dashboard_tick_renders_only_the_active_view() {
        let en = dashboard_html(ResolvedLanguage::En, false);
        let update = en
            .split("window.__pf={")
            .nth(1)
            .expect("dashboard update object")
            .split("applyPendingUpdate();")
            .next()
            .expect("dashboard update body");

        for guard in [
            "if(view==='overview')",
            "else if(view==='more')",
            "else if(view==='settings')",
            "else if(view==='fan')",
        ] {
            assert!(update.contains(guard));
        }
        assert!(!update.contains("applyRailView("));
        assert!(!update.contains("reportHeight("));
        assert!(en.contains(
            "if(window.__pf&&window.__pf.update&&window.__pf_pending)window.__pf.update(window.__pf_pending);"
        ));
        assert_eq!(en.matches("applyRailView(").count(), 2);
    }

    #[test]
    fn popover_left_dashboard_has_room_for_real_controls() {
        let popover_w = std::hint::black_box(POPOVER_W);
        assert!(
            popover_w >= 430.0,
            "the dashboard pane needs enough width for fan controls and sensor rows"
        );

        let en = dashboard_html(ResolvedLanguage::En, false);
        assert!(en.contains("grid-template-columns:minmax(0,1fr) 78px"));
        assert!(en.contains("function updateRail(d)"));
        assert!(en.contains("railSettings"));
        assert!(en.contains("setTimeout(function(){"));
        assert!(en.contains("},2500);"));
    }

    #[test]
    fn fan_cards_show_manual_override_target_immediately() {
        let en = dashboard_html(ResolvedLanguage::En, false);

        assert!(en.contains("var targetPct=f.override_pct!=null?f.override_pct:f.pct;"));
        assert!(en.contains("card.dataset.curPct=targetPct;"));
        assert!(en.contains("var editRpm=useRpm?Math.round(f.min_rpm+(f.max_rpm-f.min_rpm)*targetPct/100):Math.round(targetPct);"));
        assert!(en.contains("var appliedTarget=f.target_pct==null?null:Number(f.target_pct);"));
        assert!(en.contains("f.cur_rpm+' RPM → '+targetRpm+' RPM'"));
        assert!(en.contains("var displayManual=pendingMode?pendingMode==='manual':manual;"));
        assert!(en.contains("card.querySelector('.fv').textContent=appliedTarget!=null"));
        assert!(en.contains("(LANG==='ko'?'조정 중':'adjusting')"));
        assert!(en.contains("(LANG==='ko'?'적용됨':'applied')"));
        assert!(en.contains("var daemonReadback=f.readback_status||'';"));
        assert!(en.contains("daemonReadback==='stale'"));
        assert!(en.contains("(LANG==='ko'?'응답 없음':'not responding')"));
        assert!(en.contains("if(!markFanPending(card,'auto'))return;"));
        assert!(en.contains("if(!markFanPending(card,'manual'))return;"));
    }

    #[test]
    fn fan_control_commands_show_optimistic_pending_state() {
        let en = dashboard_html(ResolvedLanguage::En, false);
        let source = include_str!("main.rs");

        assert!(en.contains("var FAN_CONTROL_PENDING=null;"));
        assert!(en.contains("var FAN_CONTROL_RESULT=null;"));
        assert!(en.contains(r#"id="fan-apply-status""#));
        assert!(en.contains("if(FAN_CONTROL_PENDING)return false;"));
        assert!(en.contains("if(!beginFanControl('profile',profile))return;"));
        assert!(en.contains("if(!beginFanControl('auto',''))return;"));
        assert!(en.contains("revisionBefore:Number(current.applied_control_revision||0)"));
        assert!(en.contains("var revisionApplied=Number(d.applied_control_revision||0)>FAN_CONTROL_PENDING.revisionBefore;"));
        assert!(en.contains("FAN_CONTROL_RESULT={ok:true"));
        assert!(en.contains("hardware confirmation timed out"));
        assert!(en.contains("Date.now()-FAN_CONTROL_PENDING.startedAt>8000"));
        assert!(en.contains("function updateFanApplyStatus(d)"));
        assert!(en.contains("typeof f.target_pct==='number'"));
        assert!(en.contains("strip.setAttribute('aria-busy',pending?'true':'false');"));
        assert!(en.contains("b.disabled=!enabled||pending;"));
        assert!(source.contains("static CONTROL_REFRESH_REQUESTED: AtomicBool"));
        assert!(source.contains("CONTROL_REFRESH_REQUESTED.store(true, Ordering::Relaxed);"));
        assert!(source.contains("CONTROL_REFRESH_REQUESTED.swap(false, Ordering::Relaxed)"));
    }

    #[test]
    fn dashboard_requests_refresh_when_webview_becomes_ready() {
        let en = dashboard_html(ResolvedLanguage::En, false);

        assert!(en.contains("window.__pf_pending"));
        assert!(en.contains("function applyPendingUpdate()"));
        assert!(en.contains("window.ipc.postMessage('ready')"));
        assert!(en.contains("applyPendingUpdate();"));
    }

    #[test]
    fn popover_click_path_defers_heavy_dashboard_refresh() {
        let source = include_str!("main.rs");

        assert!(source.contains("POPOVER_PREWARM_DELAY"));
        assert!(source.contains(".with_focused(false)"));
        assert!(source.contains(".with_accept_first_mouse(true)"));
        assert!(source.contains("defer_dashboard_io_after_open(app);"));
        assert!(source.contains("configure_native_popover_window(w, position, POPOVER_W, height)"));
        assert!(source.contains("native_window.setFrameTopLeftPoint"));
        assert!(source.contains("#[cfg(not(target_os = \"macos\"))]"));
        assert!(source.contains("Let the normal tick deliver data"));
        assert!(source.contains("next_metric_at = show_at;"));
        assert!(source.contains("*control_flow = ControlFlow::WaitUntil(show_at);"));
        assert!(!source.contains("app.popover_visible = true;\n    update(app);"));
    }

    #[test]
    fn health_panel_uses_globally_visible_text_helper() {
        let en = dashboard_html(ResolvedLanguage::En, false);

        assert!(en.contains("setText('fan-curve-input',tempValue(d.fan_curve_input_temp_c));"));
        assert!(en.contains("setText('fan-safety-hottest',tempValue(d.fan_safety_temp_c));"));
        assert!(en.contains("setText('fan-critical-limit',tempValue(d.fan_critical_temp_c));"));
        assert!(!en.contains("set('fan-curve-input'"));
        assert!(!en.contains("set('fan-safety-hottest'"));
        assert!(!en.contains("set('fan-critical-limit'"));
    }

    #[test]
    fn active_profile_from_daemon_mode_handles_known_modes() {
        assert_eq!(
            active_profile_from_mode("manual:balanced"),
            Some("balanced")
        );
        assert_eq!(
            active_profile_from_mode("rules:performance (smc)"),
            Some("performance")
        );
        assert_eq!(active_profile_from_mode("profile:silent"), Some("silent"));
        assert_eq!(active_profile_from_mode("auto"), None);
        assert_eq!(active_profile_from_mode("hold:45%"), None);
    }

    #[test]
    fn active_control_mode_from_daemon_mode_handles_known_modes() {
        assert_eq!(active_control_mode_from_mode("auto"), "auto");
        assert_eq!(active_control_mode_from_mode("manual:balanced"), "profile");
        assert_eq!(
            active_control_mode_from_mode("rules:silent (smc)"),
            "profile"
        );
        assert_eq!(active_control_mode_from_mode("hold:45%"), "hold");
        assert_eq!(active_control_mode_from_mode(""), "");
    }

    #[test]
    fn default_fan_control_selection_is_auto() {
        assert_eq!(resolved_active_control_mode(Some("auto"), false), "auto");
        assert_eq!(resolved_active_control_mode(None, false), "auto");
        assert_eq!(resolved_active_control_mode(Some(""), false), "auto");
        assert_eq!(resolved_active_control_mode(None, true), "hold");
        assert_eq!(
            resolved_active_control_mode(Some("manual:balanced"), false),
            "profile"
        );
    }

    #[test]
    fn daemon_update_uses_min_required_version_not_app_version() {
        assert!(peterfan_platform::daemon_update_required("1.27.10"));
        assert!(!peterfan_platform::daemon_update_required(
            peterfan_platform::MIN_REQUIRED_DAEMON_VERSION
        ));
        assert!(peterfan_platform::daemon_update_required("1.27.13"));
        assert!(peterfan_platform::daemon_update_required("1.27.15"));
        assert!(!peterfan_platform::daemon_update_required("1.27.22"));
    }

    #[test]
    fn menubar_rejects_cli_subcommands_before_launching_gui() {
        let ok_args = vec![
            "PeterFan".to_string(),
            "--mock".to_string(),
            "--metric".to_string(),
            "temp".to_string(),
            "--display".to_string(),
            "graph".to_string(),
            "-psn_0_12345".to_string(),
        ];
        assert_eq!(unsupported_menubar_arg(&ok_args), None);

        let cli_args = vec!["PeterFan".to_string(), "doctor".to_string()];
        assert_eq!(unsupported_menubar_arg(&cli_args), Some("doctor"));

        let missing_value = vec!["PeterFan".to_string(), "--metric".to_string()];
        assert_eq!(unsupported_menubar_arg(&missing_value), Some("--metric"));
    }

    #[test]
    fn single_instance_lock_rejects_second_process() {
        let path = std::env::temp_dir().join(format!(
            "{}.test.{}.lock",
            SINGLE_INSTANCE_LOCK_BASENAME,
            std::process::id()
        ));
        let first = acquire_single_instance_lock_at(&path).expect("first lock should work");
        let second = acquire_single_instance_lock_at(&path);
        assert!(second.is_err());
        drop(first);
        let third = acquire_single_instance_lock_at(&path).expect("released lock should work");
        drop(third);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn mock_mode_uses_a_separate_single_instance_lock() {
        assert_ne!(
            single_instance_lock_path(false),
            single_instance_lock_path(true)
        );
        assert!(single_instance_lock_path(false)
            .to_string_lossy()
            .contains(".app."));
        assert!(single_instance_lock_path(true)
            .to_string_lossy()
            .contains(".mock."));
    }

    fn temp(id: &str, kind: SensorKind, value: f32) -> TempSensor {
        TempSensor {
            id: id.to_string(),
            label: id.to_string(),
            kind,
            source: SensorSource::Unknown,
            value: Celsius(value),
        }
    }

    #[cfg(test)]
    fn selected_temp(
        id: &str,
        value: f32,
        label_hint: Option<&'static str>,
    ) -> SelectedTemperature {
        SelectedTemperature {
            id: id.to_string(),
            value,
            label_hint,
        }
    }

    #[test]
    fn display_temperature_prefers_cpu_average_over_hottest() {
        let temps = vec![
            temp("cpu.die", SensorKind::Cpu, 52.0),
            temp("cpu.die.hot", SensorKind::Cpu, 67.0),
            temp("ssd", SensorKind::Storage, 70.0),
        ];

        assert_eq!(
            display_temperature(&temps).map(|t| t.id.as_str()),
            Some("cpu.die")
        );
        assert_eq!(
            hottest_temperature(&temps).map(|t| t.id.as_str()),
            Some("ssd")
        );
    }

    #[test]
    fn display_temperature_falls_back_to_hottest_without_cpu_average() {
        let temps = vec![
            temp("battery", SensorKind::Battery, 33.0),
            temp("airport", SensorKind::Other, 45.0),
        ];

        assert_eq!(
            display_temperature(&temps).map(|t| t.id.as_str()),
            Some("airport")
        );
    }

    #[test]
    fn primary_menu_temperature_prefers_calibrated_cpu_headline_over_raw_hottest() {
        let temps = vec![
            temp("cpu.die", SensorKind::Cpu, 87.0),
            temp("cpu.die.hot", SensorKind::Cpu, 97.0),
            temp("ssd", SensorKind::Storage, 41.0),
        ];

        assert_eq!(
            primary_menu_temperature(&temps, TemperatureSource::CoreAverage)
                .map(|t| (t.id, t.value)),
            Some(("cpu.die".to_string(), 87.0))
        );
    }

    #[test]
    fn primary_menu_temperature_prefers_cpu_headline_for_average_mode() {
        let temps = vec![
            temp("cpu.die", SensorKind::Cpu, 85.0),
            temp("cpu.smc.die", SensorKind::Cpu, 83.0),
            temp("cpu.smc.aggregate", SensorKind::Cpu, 74.0),
            temp("cpu.smc.summary", SensorKind::Cpu, 63.0),
            temp("cpu.iohid.tdie", SensorKind::Cpu, 50.0),
            temp("cpu.die.hot", SensorKind::Cpu, 86.0),
        ];

        assert_eq!(
            primary_menu_temperature(&temps, TemperatureSource::CoreAverage)
                .map(|t| (t.id, (t.value * 10.0).round() / 10.0)),
            Some(("cpu.die".to_string(), 85.0))
        );
    }

    #[test]
    fn primary_menu_temperature_uses_hottest_stable_average_without_cpu_die() {
        let temps = vec![
            temp("cpu.smc.aggregate", SensorKind::Cpu, 72.0),
            temp("cpu.smc.summary", SensorKind::Cpu, 85.0),
            temp("cpu.iohid.tdie", SensorKind::Cpu, 50.0),
            temp("cpu.die.hot", SensorKind::Cpu, 101.0),
        ];

        assert_eq!(
            primary_menu_temperature(&temps, TemperatureSource::CoreAverage)
                .map(|t| (t.id, t.value)),
            Some(("cpu.smc.summary".to_string(), 85.0))
        );
    }

    #[test]
    fn primary_menu_temperature_honors_configured_source() {
        let temps = vec![
            temp("cpu.die", SensorKind::Cpu, 58.0),
            temp("cpu.die.hot", SensorKind::Cpu, 76.0),
            temp("cpu.iohid.cpu", SensorKind::Cpu, 79.0),
            temp("cpu.iohid.tdie", SensorKind::Cpu, 55.0),
            temp("cpu.smc.summary", SensorKind::Cpu, 63.0),
            temp("cpu.smc.aggregate", SensorKind::Cpu, 74.0),
        ];

        assert_eq!(
            primary_menu_temperature(&temps, TemperatureSource::IohidTdie).map(|t| t.id),
            Some("cpu.iohid.tdie".to_string())
        );
        assert_eq!(
            primary_menu_temperature(
                &[
                    temp("cpu.die", SensorKind::Cpu, 58.0),
                    temp("cpu.die.hot", SensorKind::Cpu, 76.0),
                    temp("cpu.iohid.cpu", SensorKind::Cpu, 79.0),
                    temp("cpu.smc.summary", SensorKind::Cpu, 63.0),
                    temp("cpu.smc.aggregate", SensorKind::Cpu, 74.0),
                ],
                TemperatureSource::IohidTdie,
            )
            .map(|t| t.id),
            Some("cpu.iohid.cpu".to_string())
        );
        let source_fallback = vec![
            temp("cpu.die", SensorKind::Cpu, 58.0),
            temp("cpu.iohid.cpu", SensorKind::Cpu, 79.0),
            temp("cpu.smc.summary", SensorKind::Cpu, 63.0),
            temp("cpu.smc.aggregate", SensorKind::Cpu, 74.0),
        ];
        assert_eq!(
            primary_menu_temperature(&source_fallback, TemperatureSource::IohidTdie).map(|t| t.id),
            Some("cpu.iohid.cpu".to_string())
        );
        assert_eq!(
            primary_menu_temperature(&temps, TemperatureSource::SmcAggregate).map(|t| t.id),
            Some("cpu.smc.aggregate".to_string())
        );
        assert_eq!(
            primary_menu_temperature(&temps, TemperatureSource::Hottest).map(|t| t.id),
            Some("cpu.die.hot".to_string())
        );
    }

    #[test]
    fn display_temperature_source_labels_cpu_average() {
        let cpu = selected_temp("cpu.smc.die", 72.0, None);
        let iohide = selected_temp("cpu.iohid.tdie", 70.0, None);
        let iohide_cpu = selected_temp("cpu.iohid.cpu", 78.0, None);
        let aggregate = selected_temp("cpu.smc.aggregate", 74.0, None);
        let hot = selected_temp("cpu.die.hot", 67.0, None);
        let airport = selected_temp("airport", 45.0, None);

        assert_eq!(
            display_temperature_source(ResolvedLanguage::Ko, Some(&cpu)),
            "CPU 다이"
        );
        assert_eq!(
            display_temperature_source(ResolvedLanguage::En, Some(&cpu)),
            "CPU die"
        );
        assert_eq!(
            display_temperature_source(ResolvedLanguage::Ko, Some(&iohide)),
            "CPU 다이"
        );
        assert_eq!(
            display_temperature_source(ResolvedLanguage::En, Some(&iohide)),
            "CPU Tdie"
        );
        assert_eq!(
            display_temperature_source(ResolvedLanguage::Ko, Some(&iohide_cpu)),
            "CPU 다이"
        );
        assert_eq!(
            display_temperature_source(ResolvedLanguage::En, Some(&iohide_cpu)),
            "CPU IOHID CPU"
        );
        assert_eq!(
            display_temperature_source(ResolvedLanguage::Ko, Some(&aggregate)),
            "SMC 집계"
        );
        assert_eq!(
            display_temperature_source(ResolvedLanguage::En, Some(&aggregate)),
            "SMC aggregate"
        );
        assert_eq!(
            display_temperature_source(ResolvedLanguage::Ko, Some(&hot)),
            "최고"
        );
        assert_eq!(
            display_temperature_source(ResolvedLanguage::En, Some(&airport)),
            "airport"
        );
        assert!(display_temperature_source(ResolvedLanguage::Ko, None).is_empty());
    }

    #[test]
    fn display_temperature_source_labels_raw_cpu_sensor_set_as_average() {
        let temps = vec![
            temp("cpu.core.1", SensorKind::Cpu, 40.0),
            temp("cpu.core.2", SensorKind::Cpu, 60.0),
            temp("ssd", SensorKind::Storage, 80.0),
        ];
        let display = selected_temp("ssd", 80.0, None);

        assert_eq!(
            display_temperature_source_for_temps(ResolvedLanguage::Ko, &temps, Some(&display)),
            "ssd"
        );
        assert_eq!(
            display_temperature_source_for_temps(ResolvedLanguage::En, &temps, Some(&display)),
            "ssd"
        );
    }

    #[test]
    fn temperature_row_labels_call_out_average_and_hottest() {
        let cpu = temp("cpu.smc.die", SensorKind::Cpu, 70.0);
        let cpu_average = temp("cpu.die", SensorKind::Cpu, 52.0);
        let hot = temp("cpu.die.hot", SensorKind::Cpu, 67.0);
        let airport = temp("airport", SensorKind::Other, 45.0);

        assert_eq!(
            temperature_row_label(ResolvedLanguage::Ko, &cpu),
            "CPU 다이"
        );
        assert_eq!(temperature_row_label(ResolvedLanguage::En, &cpu), "CPU die");
        assert_eq!(
            temperature_row_label(ResolvedLanguage::Ko, &cpu_average),
            "CPU Core Average"
        );
        assert_eq!(
            temperature_row_label(ResolvedLanguage::En, &cpu_average),
            "CPU Core Average"
        );
        assert_eq!(
            temperature_row_label(ResolvedLanguage::Ko, &hot),
            "CPU Core Hottest"
        );
        assert_eq!(
            temperature_row_label(ResolvedLanguage::En, &hot),
            "CPU Core Hottest"
        );
        assert_eq!(
            temperature_row_label(ResolvedLanguage::Ko, &airport),
            "airport"
        );
    }

    #[test]
    fn raw_temperature_row_label_keeps_sensor_identity_visible() {
        let mut sensor = temp("smc.raw.TVD0", SensorKind::Cpu, 84.0);
        sensor.label = "SMC TVD0".to_string();

        assert_eq!(
            raw_temperature_row_label(&sensor),
            "SMC TVD0 · smc.raw.TVD0"
        );
    }

    #[test]
    fn menu_bar_display_style_labels_match_cat_runner() {
        assert_eq!(strings(ResolvedLanguage::En).style_graph, "Cat");
        assert_eq!(strings(ResolvedLanguage::En).style_both, "Number + Cat");
        assert_eq!(strings(ResolvedLanguage::Ko).style_graph, "고양이");
        assert_eq!(strings(ResolvedLanguage::Ko).style_both, "숫자 + 고양이");

        let help = help_text();
        assert!(help.contains("--display <number|cat|both>"));
        assert!(help.contains("cat also accepts legacy graph"));
        assert!(!help.contains("--metric"));
    }

    #[test]
    fn setup_copy_calls_out_stale_daemon() {
        assert_eq!(
            setup_title(ResolvedLanguage::En, true, true),
            "Reinstall Fan Control"
        );
        assert_eq!(
            setup_title(ResolvedLanguage::Ko, true, true),
            "팬 제어 재설치"
        );
        assert!(
            setup_detail(ResolvedLanguage::En, true, true, Some("1.26.8"))
                .contains("daemon v1.26.8 → v")
        );
        assert!(
            setup_detail(ResolvedLanguage::Ko, true, true, Some("1.26.8"))
                .contains("데몬 v1.26.8 → v")
        );
        assert!(
            setup_detail(ResolvedLanguage::En, true, true, Some("1.26.8"))
                .ends_with("one approval this time")
        );
        assert!(
            setup_detail(ResolvedLanguage::Ko, true, true, Some("1.26.8"))
                .ends_with("이번 한 번 승인 필요")
        );
    }

    #[test]
    fn reinstall_hint_distinguishes_legacy_and_self_reinstalling_daemons() {
        assert_eq!(
            daemon_reinstall_hint(ResolvedLanguage::Ko, Some("1.26.24")),
            "이번 한 번 승인 필요"
        );
        assert_eq!(
            daemon_reinstall_hint(ResolvedLanguage::En, Some("1.26.24")),
            "one approval this time"
        );
        assert_eq!(
            daemon_reinstall_hint(ResolvedLanguage::Ko, Some("1.26.37")),
            "조용히 가능"
        );
        assert_eq!(
            daemon_reinstall_hint(ResolvedLanguage::En, Some("1.26.37")),
            "no prompt"
        );
    }

    #[test]
    fn stale_daemon_is_not_usable_for_cached_state() {
        assert!(!daemon_control_usable(Some("1.26.24")));
        assert!(daemon_control_usable(Some(
            peterfan_platform::MIN_REQUIRED_DAEMON_VERSION
        )));
        assert!(!daemon_control_usable(None));
    }

    #[test]
    fn setup_detail_shows_daemon_version_when_ready() {
        let en = setup_detail(ResolvedLanguage::En, true, false, Some("1.26.18"));
        assert_eq!(en, "daemon v1.26.18 · no additional approval");
        assert!(!en.contains("login"));

        let ko = setup_detail(ResolvedLanguage::Ko, true, false, Some("1.26.18"));
        assert_eq!(ko, "데몬 v1.26.18 · 추가 승인 없음");
        assert!(!ko.contains("자동 실행"));
    }

    #[test]
    fn setup_detail_reassures_when_daemon_is_compatible_but_older_than_app() {
        assert!(!peterfan_platform::daemon_update_required(
            peterfan_platform::MIN_REQUIRED_DAEMON_VERSION
        ));

        let en = setup_detail(
            ResolvedLanguage::En,
            true,
            false,
            Some(peterfan_platform::MIN_REQUIRED_DAEMON_VERSION),
        );
        assert!(en.contains(&format!(
            "daemon v{}",
            peterfan_platform::MIN_REQUIRED_DAEMON_VERSION
        )));
        assert!(en.contains("no additional approval"));

        let ko = setup_detail(
            ResolvedLanguage::Ko,
            true,
            false,
            Some(peterfan_platform::MIN_REQUIRED_DAEMON_VERSION),
        );
        assert!(ko.contains(&format!(
            "데몬 v{}",
            peterfan_platform::MIN_REQUIRED_DAEMON_VERSION
        )));
        assert!(ko.contains("추가 승인 없음"));
    }

    #[test]
    fn stale_daemon_update_never_auto_prompts() {
        let mut cfg = peterfan_core::config::Config::default();
        assert!(!should_prompt_stale_daemon_update(&cfg, "1.2.3", 1_000));

        cfg.menubar.daemon_update_prompt_snoozed_until_unix = Some(1_500);
        assert!(!should_prompt_stale_daemon_update(&cfg, "1.2.3", 1_000));
        assert!(!should_prompt_stale_daemon_update(&cfg, "1.2.3", 1_501));

        cfg.menubar.daemon_update_prompt_dismissed_for = Some("1.2.3".to_string());
        assert!(!should_prompt_stale_daemon_update(&cfg, "1.2.3", 1_501));
        assert!(!should_prompt_stale_daemon_update(&cfg, "1.2.4", 1_501));
    }

    #[test]
    fn launch_policy_keeps_daemon_update_prompt_user_initiated() {
        assert!(!should_auto_prompt_stale_daemon_update_on_launch());
    }

    #[test]
    fn launch_policy_keeps_first_run_setup_user_initiated() {
        assert!(!should_auto_prompt_first_run_setup_on_launch());
    }

    #[test]
    fn clearing_daemon_prompt_state_removes_dismiss_and_snooze() {
        let mut cfg = peterfan_core::config::Config::default();
        cfg.menubar.daemon_update_prompt_dismissed_for = Some("1.2.3".to_string());
        cfg.menubar.daemon_update_prompt_snoozed_until_unix = Some(1_500);

        clear_daemon_update_prompt_state(&mut cfg);

        assert!(cfg.menubar.daemon_update_prompt_dismissed_for.is_none());
        assert!(cfg
            .menubar
            .daemon_update_prompt_snoozed_until_unix
            .is_none());
    }

    #[test]
    fn fan_speed_percent_uses_the_controllable_rpm_range() {
        let fan_at = |rpm| Fan {
            id: "fan.test".into(),
            label: "Test Fan".into(),
            rpm,
            min_rpm: Some(2_000),
            max_rpm: Some(6_000),
            duty_percent: None,
            controllable: true,
        };

        assert_eq!(normalized_fan_speed_percent(&fan_at(2_000)), Some(0.0));
        assert_eq!(normalized_fan_speed_percent(&fan_at(4_000)), Some(50.0));
        assert_eq!(normalized_fan_speed_percent(&fan_at(6_000)), Some(100.0));
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

    // `apply_local` mutates the process-wide `LOCAL_FAN_OVERRIDES` static as
    // a side effect for global-mode commands (auto/profile/hold clear it).
    // Cargo runs tests in this file concurrently on multiple threads, so any
    // test asserting on that shadow state must serialize against the others
    // via this lock, or their clears/inserts can interleave and flake.
    static FAN_OVERRIDE_TEST_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn apply_local_handles_hold_preset() {
        let _guard = FAN_OVERRIDE_TEST_LOCK.lock().unwrap();
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
        let _guard = FAN_OVERRIDE_TEST_LOCK.lock().unwrap();
        let provider = peterfan_platform::mock();
        assert!(apply_local(provider.as_ref(), "auto").contains("applied locally"));
        assert!(apply_local(provider.as_ref(), "profile:balanced").contains("applied locally"));
        assert_eq!(apply_local(provider.as_ref(), "bogus"), "unknown command");
    }

    #[test]
    fn apply_local_fanhold_remembers_pin_without_a_daemon() {
        let _guard = FAN_OVERRIDE_TEST_LOCK.lock().unwrap();
        clear_local_fan_overrides();
        let provider = peterfan_platform::mock();
        let fan_id = provider.fans().unwrap()[0].id.clone();

        let result = apply_local(provider.as_ref(), &format!("fanhold:{fan_id}:30"));
        assert!(result.contains("applied locally"), "unexpected: {result}");
        assert_eq!(local_fan_overrides().get(&fan_id), Some(&30));

        // The per-fan "Auto" toggle must clear just that fan's pin, and a
        // global command must clear all of them — matching the daemon.
        apply_local(provider.as_ref(), &format!("fanauto:{fan_id}"));
        assert!(!local_fan_overrides().contains_key(&fan_id));

        apply_local(provider.as_ref(), &format!("fanhold:{fan_id}:30"));
        apply_local(provider.as_ref(), "auto");
        assert!(local_fan_overrides().is_empty());
    }

    #[test]
    fn control_result_is_ok_rejects_daemon_error_replies() {
        // An older/incompatible daemon still replies with a "daemon:" prefix
        // even when the command itself failed — that must not read as success.
        assert!(!control_result_is_ok("daemon: error: unknown command"));
        assert!(!control_result_is_ok("daemon: invalid percent"));
        assert!(control_result_is_ok("daemon: ok auto (mock)"));
        assert!(control_result_is_ok("applied locally"));
    }

    #[test]
    fn temperature_sample_becomes_stale_only_after_the_limit() {
        let now = Instant::now();
        let sampled = now - Duration::from_secs(6);
        assert!(!sample_is_stale(
            Some(sampled),
            now,
            TEMPERATURE_STALE_AFTER
        ));
        assert!(sample_is_stale(
            Some(sampled - Duration::from_millis(1)),
            now,
            TEMPERATURE_STALE_AFTER
        ));
        assert!(sample_is_stale(None, now, TEMPERATURE_STALE_AFTER));
    }

    #[test]
    fn temperature_ui_marks_cached_readings_as_stale() {
        let html = dashboard_html(ResolvedLanguage::En, false);
        assert!(html.contains("d.temp_stale?' · '"));
        assert!(html.contains("t.stale?' stale':'"));
        assert!(html.contains("d.temp_stale?[]:d.temp_hist"));
        assert!(html.contains(".trow.stale"));
    }
}
