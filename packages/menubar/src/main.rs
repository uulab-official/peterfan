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
#![recursion_limit = "512"]
#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

#[cfg(target_os = "macos")]
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
#[cfg(target_os = "macos")]
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
#[cfg(target_os = "macos")]
use std::sync::atomic::{AtomicU32, AtomicUsize};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[cfg(any(target_os = "macos", test))]
use tao::dpi::LogicalPosition;
use tao::dpi::LogicalSize;
#[cfg(any(not(target_os = "macos"), test))]
use tao::dpi::PhysicalPosition;
#[cfg(not(target_os = "macos"))]
use tao::dpi::PhysicalSize;
use tao::event::{Event, StartCause, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy, EventLoopWindowTarget};
#[cfg(not(target_os = "macos"))]
use tao::monitor::MonitorHandle;
use tao::window::{Window, WindowBuilder};

#[cfg(target_os = "macos")]
use tao::platform::macos::{ActivationPolicy, EventLoopExtMacOS, WindowExtMacOS};

mod cpu_widget;
mod cpu_widget_app;
mod menu_items_app;
#[cfg(target_os = "macos")]
mod native_cpu_widget;
#[cfg(target_os = "macos")]
mod native_runner;
mod pause_clock;
mod random_runner;
mod runner;
mod ui_events;
mod update_history;
mod update_schedule;
use random_runner::RandomRunner;
use update_schedule::{UpdateLease, UpdateSchedule};

#[cfg(target_os = "macos")]
use block2::RcBlock;
#[cfg(target_os = "macos")]
use objc2::rc::Retained;
#[cfg(target_os = "macos")]
use objc2::AllocAnyThread;
#[cfg(target_os = "macos")]
use objc2_app_kit::{
    NSBitmapFormat, NSBitmapImageRep, NSCellImagePosition, NSDeviceRGBColorSpace, NSEvent, NSImage,
    NSImageCacheMode, NSScreen, NSWindow, NSWorkspace,
};
#[cfg(target_os = "macos")]
use objc2_foundation::{
    MainThreadMarker, NSDate, NSPoint, NSRect, NSRunLoop, NSRunLoopCommonModes, NSSize, NSString,
    NSTimer,
};

use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use tray_icon::{
    Icon, MouseButton, MouseButtonState, Rect, TrayIcon, TrayIconAttributes, TrayIconEvent,
};
use wry::{WebView, WebViewBuilder, RGBA};

use peterfan_core::config::{
    Config, CpuWidgetConfig, CustomCurveConfig, Language, MenubarDisplay, NotificationConfig,
    ResolvedLanguage, Rule, RunnerCharacter, TemperatureSource, UpdateChannel,
};
use peterfan_core::error::CoreError;
use peterfan_core::metrics::{DiskInfo, ProcSort, ProcessInfo};
use peterfan_core::profile::Profile;
use peterfan_core::thermals::{representative_temperature_c, safety_temperature_c};
#[cfg(test)]
use peterfan_core::types::SensorSource;
use peterfan_core::types::{Celsius, Fan, TempSensor};
use peterfan_core::types::{SensorGroup, SensorKind};
use peterfan_core::{HardwareProvider, SystemMonitor};
#[cfg(any(target_os = "macos", test))]
use runner::ease_motion_cpu as ease_runner_motion_cpu;
#[cfg(target_os = "macos")]
use runner::{advance_phase as advance_runner_phase, NATIVE_TICK as RUNNER_NATIVE_TICK};
use runner::{
    enabled as runner_enabled, frame_interval_for_character as runner_character_interval,
    frame_interval_for_pose as runner_frame_interval_for_pose, load_band as runner_load_band,
    load_band_with_hysteresis as runner_load_band_with_hysteresis,
    should_animate as runner_should_animate, smooth_cpu as smooth_runner_cpu,
    uses_event_loop_timer as runner_uses_event_loop_timer, RunnerBuildError, RunnerPack,
    FRAME_COUNT as RUNNER_FRAME_COUNT, PIXEL_HEIGHT as RUNNER_PIXEL_HEIGHT,
    PIXEL_WIDTH as RUNNER_PIXEL_WIDTH,
};
#[cfg(test)]
use runner::{
    frame_interval as runner_frame_interval, phase_step as runner_phase_step, render_raw_frame,
};
#[cfg(target_os = "macos")]
use runner::{
    native_next_delay as runner_native_next_delay, NATIVE_TOLERANCE as RUNNER_NATIVE_TOLERANCE,
};
#[cfg(test)]
use runner::{
    LOAD_BANDS as RUNNER_LOAD_BANDS, MAX_INTERVAL as RUNNER_MAX_INTERVAL,
    MIN_INTERVAL as RUNNER_MIN_INTERVAL,
};

const REFRESH: Duration = Duration::from_secs(1);
const RUNNER_CPU_REFRESH: Duration = Duration::from_millis(250);
const TEMPERATURE_REFRESH_VISIBLE: Duration = Duration::from_secs(2);
const TEMPERATURE_REFRESH_BACKGROUND: Duration = Duration::from_secs(3);
const FAN_REFRESH_VISIBLE: Duration = Duration::from_secs(1);
const FAN_REFRESH_BACKGROUND: Duration = Duration::from_secs(3);
const FAN_STALE_AFTER: Duration = Duration::from_secs(8);
const FAN_EMPTY_CONFIRMATIONS: u8 = 3;
const RUNNER_PREVIEW_BAND: usize = 1;
const RUNNER_PREVIEW_POSE: u8 = 1;
const POPOVER_PREWARM_DELAY: Duration = Duration::from_millis(1200);
const POPOVER_SHOW_DELAY: Duration = Duration::from_millis(35);
const DASHBOARD_OPEN_GRACE: Duration = Duration::from_millis(900);
const DASHBOARD_SLOW_REFRESH: Duration = Duration::from_secs(3);
const DASHBOARD_SLOW_OPEN_GRACE: Duration = Duration::from_millis(450);
const ALL_TEMP_REFRESH: Duration = Duration::from_secs(10);
const DAEMON_REFRESH: Duration = Duration::from_secs(2);
const DAEMON_STALE_AFTER: Duration = Duration::from_secs(8);
const RESUME_RECOVERY_GAP: Duration = Duration::from_secs(8);
const TEMPERATURE_STALE_AFTER: Duration = Duration::from_secs(8);
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
/// Active compact dashboard view (0 = overview, 1 = fan, 2 = settings,
/// 3 = system metrics).
/// The WebView reports navigation changes so the native updater can avoid
/// collecting and serializing metrics hidden behind another view.
static ACTIVE_RAIL_VIEW: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);
/// Raw SMC/IOHID sensors are expensive and normally collapsed. Poll them only
/// while the user has explicitly opened the sensor disclosure.
static RAW_TEMPS_OPEN: AtomicBool = AtomicBool::new(false);
static DAEMON_VERSION_CACHE: Mutex<Option<(Instant, Option<String>)>> = Mutex::new(None);
static MENUBAR_LOG_LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, PartialEq)]
struct CpuCoreGroup {
    kind: &'static str,
    start_index: usize,
    usages: Vec<f32>,
}

fn cpu_core_groups_for_layout(
    per_core: &[f32],
    performance_efficiency_counts: Option<(usize, usize)>,
) -> Vec<CpuCoreGroup> {
    if let Some((performance, efficiency)) = performance_efficiency_counts {
        if performance > 0
            && efficiency > 0
            && performance.saturating_add(efficiency) == per_core.len()
        {
            return vec![
                CpuCoreGroup {
                    // Apple Silicon exposes Mach logical CPU indexes with the
                    // efficiency cluster first. Keep that ordering so sysinfo's
                    // per-core samples retain their real logical CPU indexes.
                    kind: "efficiency",
                    start_index: 0,
                    usages: per_core[..efficiency].to_vec(),
                },
                CpuCoreGroup {
                    kind: "performance",
                    start_index: efficiency,
                    usages: per_core[efficiency..].to_vec(),
                },
            ];
        }
    }

    vec![CpuCoreGroup {
        kind: "logical",
        start_index: 0,
        usages: per_core.to_vec(),
    }]
}

#[cfg(target_os = "macos")]
fn apple_cpu_cluster_counts() -> Option<(usize, usize)> {
    static COUNTS: std::sync::OnceLock<Option<(usize, usize)>> = std::sync::OnceLock::new();
    *COUNTS.get_or_init(|| {
        let output = std::process::Command::new("/usr/sbin/sysctl")
            .args([
                "-n",
                "hw.perflevel0.name",
                "hw.perflevel0.logicalcpu",
                "hw.perflevel1.name",
                "hw.perflevel1.logicalcpu",
            ])
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        let values = String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .collect::<Vec<_>>();
        if values.len() != 4 {
            return None;
        }

        let clusters = [
            (
                values[0].to_ascii_lowercase(),
                values[1].parse::<usize>().ok()?,
            ),
            (
                values[2].to_ascii_lowercase(),
                values[3].parse::<usize>().ok()?,
            ),
        ];
        let performance = clusters
            .iter()
            .find(|(name, _)| name.contains("performance"))
            .map(|(_, count)| *count)?;
        let efficiency = clusters
            .iter()
            .find(|(name, _)| name.contains("efficiency"))
            .map(|(_, count)| *count)?;
        Some((performance, efficiency))
    })
}

#[cfg(not(target_os = "macos"))]
fn apple_cpu_cluster_counts() -> Option<(usize, usize)> {
    None
}

fn dashboard_cpu_core_groups(per_core: &[f32]) -> Vec<serde_json::Value> {
    cpu_core_groups_for_layout(per_core, apple_cpu_cluster_counts())
        .into_iter()
        .map(|group| {
            let prefix = match group.kind {
                "performance" => "P",
                "efficiency" => "E",
                _ => "C",
            };
            let cores = group
                .usages
                .iter()
                .enumerate()
                .map(|(offset, usage)| {
                    serde_json::json!({
                        "index": group.start_index + offset,
                        "label": format!("{prefix}{}", offset + 1),
                        "usage": usage,
                    })
                })
                .collect::<Vec<_>>();
            let average = if group.usages.is_empty() {
                0.0
            } else {
                group.usages.iter().sum::<f32>() / group.usages.len() as f32
            };
            let peak = group.usages.iter().copied().fold(0.0_f32, f32::max);
            serde_json::json!({
                "kind": group.kind,
                "average": average,
                "peak": peak,
                "cores": cores,
            })
        })
        .collect()
}

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

    fn clear(&mut self) {
        self.minute.clear();
        self.hour.clear();
        self.day.clear();
        self.minute_acc.clear();
        self.hour_acc.clear();
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
/// Keep hardware writes serialized even when the popover and context menu are
/// used at nearly the same time. The event loop stays responsive while the
/// background workers take turns talking to the daemon/SMC.
static FAN_COMMAND_LOCK: Mutex<()> = Mutex::new(());
/// Last control result, shown in the popover.
static STATUS: Mutex<String> = Mutex::new(String::new());
/// Keep recent settings acknowledgements per request so one window cannot
/// mistake another window's save (or an older status message) for its own.
static SETTINGS_RESULTS: Mutex<VecDeque<serde_json::Value>> = Mutex::new(VecDeque::new());
const SETTINGS_RESULTS_MAX: usize = 32;
/// One native update pipeline shared by the popover, detail window, and
/// right-click menu. GitHub checks from inside the WebView were unreliable on
/// some Macs and could race the native installer dialog.
static APP_UPDATE_IN_FLIGHT: AtomicBool = AtomicBool::new(false);
// Mock sessions must never contact release services, even from manual actions.
static APP_UPDATE_NETWORK_ENABLED: AtomicBool = AtomicBool::new(false);
static APP_UPDATE_NOTIFIED: Mutex<VecDeque<String>> = Mutex::new(VecDeque::new());
static APP_UPDATE_STATE: Mutex<AppUpdateState> = Mutex::new(AppUpdateState {
    phase: "idle",
    latest: None,
    release_url: None,
    notes: None,
    message: None,
    install_ready: false,
    channel: UpdateChannel::Stable,
    schedule: UpdateSchedule::new(),
    check_error: None,
});
/// A completed fan command invalidates the cached daemon snapshot. The event
/// loop consumes this on its next wake so confirmed UI state does not wait for
/// the normal two-second daemon refresh interval.
static CONTROL_REFRESH_REQUESTED: AtomicBool = AtomicBool::new(false);
const FAN_ACTION_LOG_MAX: usize = 12;
static FAN_ACTION_LOG: Mutex<VecDeque<serde_json::Value>> = Mutex::new(VecDeque::new());
/// Privacy-safe support snapshot prepared from the latest dashboard sample.
/// Clipboard writes happen on a worker because launching the platform helper
/// must never delay a popover click or animation frame.
static SUPPORT_REPORT: Mutex<String> = Mutex::new(String::new());
static SUPPORT_REPORT_COPY_IN_FLIGHT: AtomicBool = AtomicBool::new(false);
static SUPPORT_REPORT_COPY_REVISION: AtomicU64 = AtomicU64::new(0);
static SUPPORT_REPORT_COPY_OK: AtomicBool = AtomicBool::new(false);
/// Monotonic completion signal for copying the current TOML configuration.
static CONFIG_COPY_IN_FLIGHT: AtomicBool = AtomicBool::new(false);
static CONFIG_COPY_REVISION: AtomicU64 = AtomicU64::new(0);
static CONFIG_COPY_OK: AtomicBool = AtomicBool::new(false);
/// Guards `install_fan_control()` process-wide. The popover and Detail
/// Window each track their own "installing…" button state in per-webview JS
/// (`FAN_CONTROL_FIX_PENDING`), which doesn't stop both windows from firing
/// the install thread within the same tick and stacking two macOS
/// admin-password dialogs.
static INSTALL_FAN_CONTROL_IN_FLIGHT: AtomicBool = AtomicBool::new(false);
/// Stage milestones only; 100 is reserved for a confirmed healthy daemon.
static FAN_CONTROL_INSTALL_PROGRESS: Mutex<(&str, u8)> = Mutex::new(("idle", 0));

#[cfg(target_os = "macos")]
fn report_fan_control_install_stage(stage: peterfan_platform::daemon_install::InstallStage) {
    use peterfan_platform::daemon_install::InstallStage;
    let mut progress = FAN_CONTROL_INSTALL_PROGRESS
        .lock()
        .expect("install progress poisoned");
    *progress = match stage {
        InstallStage::Preflight => ("preflight", 10),
        InstallStage::Installing => ("installing", 30),
        InstallStage::Verifying => ("verifying", 85),
        InstallStage::Complete => ("complete", 100),
        InstallStage::Failed => ("failed", progress.1.min(99)),
    };
    CONTROL_REFRESH_REQUESTED.store(true, Ordering::Release);
}

fn fan_control_install_progress_snapshot() -> serde_json::Value {
    let progress = FAN_CONTROL_INSTALL_PROGRESS
        .lock()
        .expect("install progress poisoned");
    serde_json::json!({"phase": progress.0, "percent": progress.1})
}

/// Monotonic completion signal for fan-control setup. WebViews compare this
/// with the value captured when setup started, so success, failure, and
/// cancellation all leave the "Installing..." state immediately.
static INSTALL_FAN_CONTROL_REVISION: AtomicU64 = AtomicU64::new(0);
#[cfg(any(target_os = "macos", target_os = "windows"))]
static LOGIN_ITEM_TOGGLE_IN_FLIGHT: AtomicBool = AtomicBool::new(false);
/// Shadow of `apply_local`'s per-fan pins, consulted only when no daemon is
/// reachable (`daemon_temps_json()` returns `None` in that case, since it's
/// a daemon IPC query). Without this, pinning a fan via a direct SMC write
/// leaves the UI reporting "Auto" on the very next tick even though the fan
/// is genuinely still pinned in hardware.
static LOCAL_FAN_OVERRIDES: Mutex<Option<std::collections::HashMap<String, u8>>> = Mutex::new(None);

fn pending_command_key(cmd: &str) -> Option<String> {
    if matches!(cmd, "ready:popover" | "ready:detail") {
        return Some(cmd.to_string());
    }
    if matches!(cmd, "auto") || cmd.starts_with("profile:") {
        return Some("global-fan-control".to_string());
    }
    if cmd.starts_with("display:") {
        return Some("menubar-display".to_string());
    }
    if cmd.starts_with("character:") {
        return Some("runner-character".to_string());
    }
    if cmd.starts_with("processfilter:") {
        return Some("process-filter".to_string());
    }
    if let Some(setting) = cmd
        .strip_prefix("notifications:")
        .and_then(|value| value.split(':').next())
    {
        return Some(format!("notifications:{setting}"));
    }
    if let Some(rest) = cmd.strip_prefix("fanhold:") {
        return rest
            .rsplit_once(':')
            .map(|(fan_id, _)| format!("fan:{fan_id}"));
    }
    cmd.strip_prefix("fanauto:")
        .map(|fan_id| format!("fan:{fan_id}"))
}

/// Collapse commands that cannot usefully be applied twice. In particular,
/// rapid slider updates keep only the latest target for that fan, while
/// commands for different fans remain independent.
fn queue_pending_command(queue: &mut Vec<String>, cmd: String) {
    if let Some(key) = pending_command_key(&cmd) {
        queue.retain(|queued| pending_command_key(queued).as_deref() != Some(key.as_str()));
    } else if queue.iter().any(|queued| queued == &cmd) {
        return;
    }
    queue.push(cmd);
}

fn enqueue_pending(cmd: impl Into<String>) {
    let mut queue = PENDING.lock().expect("pending poisoned");
    queue_pending_command(&mut queue, cmd.into());
}

fn parse_process_quit_command(
    command: &str,
) -> Option<peterfan_platform::process_control::ProcessQuitRequest> {
    let (pid, start_time) = command.strip_prefix("quitprocess:")?.split_once(':')?;
    let pid = pid.parse::<u32>().ok()?;
    let expected_start_time_unix_secs = start_time.parse::<u64>().ok()?;
    if pid <= 1 || expected_start_time_unix_secs == 0 {
        return None;
    }
    Some(peterfan_platform::process_control::ProcessQuitRequest {
        pid,
        expected_start_time_unix_secs,
    })
}

fn is_native_preference_command(command: &str) -> bool {
    matches!(command, "cpu-core-graph:0" | "cpu-core-graph:1")
        || command.starts_with("display:")
        || command.starts_with("character:")
        || command.starts_with("settingsrequest:")
}

fn is_native_process_command(command: &str) -> bool {
    command.starts_with("processfilter:") || parse_process_quit_command(command).is_some()
}

#[derive(Clone)]
struct ProcessActionState {
    revision: u64,
    phase: &'static str,
    pid: Option<u32>,
    name: Option<String>,
    code: Option<&'static str>,
    message: Option<String>,
}

static PROCESS_ACTION: Mutex<ProcessActionState> = Mutex::new(ProcessActionState {
    revision: 0,
    phase: "idle",
    pid: None,
    name: None,
    code: None,
    message: None,
});
static PROCESS_ACTION_IN_FLIGHT: AtomicBool = AtomicBool::new(false);
static PROCESS_REFRESH_REQUESTED: AtomicBool = AtomicBool::new(false);

fn process_action_json(state: &ProcessActionState) -> serde_json::Value {
    serde_json::json!({
        "revision": state.revision,
        "phase": state.phase,
        "pid": state.pid,
        "name": state.name,
        "code": state.code,
        "message": state.message,
    })
}

fn process_action_snapshot() -> serde_json::Value {
    let state = PROCESS_ACTION.lock().expect("process action poisoned");
    process_action_json(&state)
}

#[derive(Clone)]
struct AppUpdateState {
    phase: &'static str,
    latest: Option<String>,
    release_url: Option<String>,
    notes: Option<String>,
    message: Option<String>,
    install_ready: bool,
    channel: UpdateChannel,
    schedule: UpdateSchedule,
    check_error: Option<String>,
}

fn set_app_update_state(
    phase: &'static str,
    release: Option<&peterfan_platform::updater::ReleaseInfo>,
    message: impl Into<Option<String>>,
) {
    let mut state = APP_UPDATE_STATE.lock().expect("app update state poisoned");
    state.phase = phase;
    state.latest = release.map(|value| value.version.clone());
    state.release_url = release.map(|value| value.html_url.clone());
    state.notes = release
        .filter(|value| !value.notes.trim().is_empty())
        .map(|value| value.notes.clone());
    state.message = message.into();
    state.install_ready = release.is_some_and(peterfan_platform::updater::is_installable_release);
}

fn app_update_state_snapshot() -> serde_json::Value {
    let state = APP_UPDATE_STATE
        .lock()
        .expect("app update state poisoned")
        .clone();
    serde_json::json!({
        "phase": state.phase,
        "latest": state.latest,
        "url": state.release_url,
        "notes": state.notes,
        "message": state.message,
        "install_ready": state.install_ready,
        "channel": state.channel.as_str(),
        "last_attempt_unix": state.schedule.last_attempt_unix,
        "last_success_unix": state.schedule.last_success_unix,
        "next_check_unix": state.schedule.next_check_unix,
        "check_error": state.check_error,
        "auto_check_enabled": APP_UPDATE_NETWORK_ENABLED.load(Ordering::Acquire),
    })
}

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
    #[cfg(target_os = "macos")]
    menu_items: native_cpu_widget::MenuItemsMenu,
    /// CPU runner character, independent of number/runner display mode.
    character_items: Vec<(RunnerCharacter, tray_icon::menu::CheckMenuItem)>,
    random_character_item: tray_icon::menu::CheckMenuItem,
    /// "CPU Temperature Source" submenu — which sensor family feeds the
    /// headline/menu-bar temperature.
    temperature_source_items: Vec<(TemperatureSource, tray_icon::menu::CheckMenuItem)>,
    /// "Fan Speed" submenu — direct RPM presets, mapped to the same command
    /// strings `execute_control` already understands (`auto`, `hold:<pct>`).
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
    #[cfg(target_os = "macos")]
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
    runner_character: &'static str,
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
            #[cfg(target_os = "macos")]
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
            check_updates: "Update Now…",
            quit: "Quit PeterFan",
            menu_bar_style: "Menu Bar Style",
            runner_character: "Runner Character",
            temperature_source: "Dashboard Temperature",
            fan_speed: "Fan Speed",
            language: "Language",
            style_number: "Number",
            style_graph: "Runner",
            style_both: "Number + Runner",
        },
        ResolvedLanguage::Ko => L10n {
            #[cfg(target_os = "macos")]
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
            check_updates: "지금 업데이트…",
            quit: "PeterFan 종료",
            menu_bar_style: "메뉴 막대 스타일",
            runner_character: "러너 캐릭터",
            temperature_source: "대시보드 온도",
            fan_speed: "팬 속도",
            language: "언어",
            style_number: "숫자",
            style_graph: "러너",
            style_both: "숫자 + 러너",
        },
    }
}

#[derive(Debug, Clone)]
struct RunnerAssets {
    pack: RunnerPack,
    icons: Vec<Icon>,
    #[cfg(any(target_os = "macos", test))]
    native_images: Vec<NativeRunnerImage>,
}

#[cfg(target_os = "macos")]
type NativeRunnerImage = Retained<NSImage>;
#[cfg(not(target_os = "macos"))]
type NativeRunnerImage = Vec<u8>;

#[derive(Debug, Clone)]
struct RunnerStartupState {
    character: RunnerCharacter,
    assets: RunnerAssets,
    original_error: Option<RunnerBuildError>,
}

#[derive(Debug, Clone)]
struct RunnerStartupFailure {
    requested: RunnerCharacter,
    original_error: Option<RunnerBuildError>,
    fatal_error: RunnerBuildError,
}

impl std::fmt::Display for RunnerStartupFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(original_error) = &self.original_error {
            write!(
                f,
                "runner '{}' failed, fallback cache build also failed: original={}, fallback={}",
                self.requested.as_str(),
                original_error,
                self.fatal_error
            )
        } else {
            write!(
                f,
                "runner '{}' startup failed: {}",
                self.requested.as_str(),
                self.fatal_error
            )
        }
    }
}

struct App {
    monitor: Box<dyn SystemMonitor>,
    /// Shared (not owned) so control actions can run on a background thread
    /// without blocking the event loop — SMC calls take tens to hundreds of
    /// ms, especially when they're failing (no daemon, no root).
    provider: std::sync::Arc<dyn HardwareProvider>,
    display: MenubarDisplay,
    primary_visible: bool,
    runner_character: RunnerCharacter,
    random_runner: RandomRunner,
    temperature_source: TemperatureSource,
    update_channel: UpdateChannel,
    critical_temp_c: f32,
    notifications: NotificationConfig,
    notification_runtime: NotificationRuntime,
    language: Language,
    show_cpu_core_graph: bool,
    cpu_widget_config: CpuWidgetConfig,
    cpu_widget_history: cpu_widget::CpuWidgetHistory,
    cpu_widget_error: Option<String>,
    #[cfg(target_os = "macos")]
    cpu_widget_native: Option<native_cpu_widget::NativeCpuWidget>,
    tray: Option<TrayIcon>,
    tray_menu: Option<TrayMenu>,
    window: Option<Window>,
    webview: Option<WebView>,
    webview_ready: bool,
    dashboard_script: Option<String>,
    popover_visible: bool,
    popover_show_at: Option<Instant>,
    popover_tray_focus_loss_at: Option<Instant>,
    popover_anchor_tray: Option<String>,
    /// `tray-icon` normally sends both Down and Up. Open on Down for macOS
    /// menu bars that swallow Up, then consume the matching Up event.
    left_button_down_seen: bool,
    /// A persistent, resizable, normal-chrome window with the same
    /// dashboard content — for "leave it open while I work" use, unlike the
    /// dropdown popover which hides the moment focus moves elsewhere.
    /// Created lazily on first request.
    detail_window: Option<Window>,
    detail_webview: Option<WebView>,
    detail_webview_ready: bool,
    /// Short-term (2-minute) history for the menu-bar runner icon only — the
    /// icon always shows the recent trend, independent of the popover's
    /// chart range selector.
    fan_hist: VecDeque<f32>,
    /// Multi-range history (2m/1h/1d) for the popover's own charts.
    cpu_h: RangedHistory,
    /// Per-core short history used by the compact overview graph.
    per_core_h: Vec<RangedHistory>,
    mem_h: RangedHistory,
    temp_h: RangedHistory,
    /// Combined rx+tx throughput (bytes/sec) — the chart only ever shows the
    /// total, so there's no need to keep rx/tx as separate series.
    net_h: RangedHistory,
    /// Combined disk read+write throughput (bytes/sec), same reasoning.
    disk_io_h: RangedHistory,
    process_filter: String,
    dashboard_slow_cache: DashboardSlowCache,
    next_dashboard_slow_refresh: Instant,
    /// Small animation frame for the RunCat-style menu-bar character. It
    /// advances on each refresh, with bigger CPU load taking larger strides.
    runner_frame: u8,
    runner_cpu_pct: f32,
    runner_has_sample: bool,
    reduce_motion: bool,
    runner_assets: RunnerAssets,
    #[cfg(not(target_os = "macos"))]
    runner_band: usize,
    #[cfg(target_os = "macos")]
    runner_native_timer: Option<Retained<NSTimer>>,
    #[cfg(target_os = "macos")]
    runner_native_painter: Rc<RefCell<native_runner::NativeRunnerPainter>>,
    #[cfg(target_os = "macos")]
    runner_native_cpu_bits: Arc<AtomicU32>,
    #[cfg(target_os = "macos")]
    runner_native_motion_cpu_bits: Arc<AtomicU32>,
    #[cfg(target_os = "macos")]
    runner_native_phase_us: Arc<AtomicU64>,
    #[cfg(target_os = "macos")]
    runner_native_tick_us: Arc<AtomicU64>,
    #[cfg(target_os = "macos")]
    runner_native_band: Arc<AtomicUsize>,
    #[cfg(target_os = "macos")]
    runner_native_frame: Arc<AtomicU64>,
    #[cfg(target_os = "macos")]
    runner_native_applied_icon: Arc<AtomicUsize>,
    last_runner_icon: Option<usize>,
    last_menubar_text: Option<String>,
    last_menubar_tooltip: Option<String>,
    temperature_cache: Vec<TempSensor>,
    temperature_sampled_at: Option<Instant>,
    temperature_sampled_at_unix_ms: Option<u64>,
    next_temperature_refresh: Instant,
    temperature_read: Arc<BackgroundRead<TimedSample<Vec<TempSensor>>>>,
    fan_cache: Vec<Fan>,
    fan_sampled_at: Option<Instant>,
    fan_empty_samples: u8,
    next_fan_refresh: Instant,
    fan_read: Arc<BackgroundRead<Vec<Fan>>>,
    all_temp_rows_cache: Vec<serde_json::Value>,
    all_temp_sampled_at: Option<Instant>,
    all_temp_sampled_at_unix_ms: Option<u64>,
    next_all_temp_refresh: Instant,
    all_temp_read: Arc<BackgroundRead<TimedSample<Vec<TempSensor>>>>,
    daemon_json_cache: Option<serde_json::Value>,
    daemon_json_sampled_at: Option<Instant>,
    daemon_probe_completed: bool,
    next_daemon_refresh: Instant,
    daemon_read: Arc<BackgroundRead<Option<serde_json::Value>>>,
    update_install_result: Option<peterfan_platform::updater::UpdateInstallResult>,
    next_update_result_refresh: Instant,
    control_confirm_until: Option<Instant>,
}

#[derive(Default)]
struct NotificationRuntime {
    temperature_warning_active: bool,
    fan_failure_baseline: Option<u64>,
    fan_failure_warning_active: bool,
}

#[derive(Debug, PartialEq)]
struct NotificationNotice {
    title: String,
    body: String,
}

fn evaluate_notification_rules(
    settings: &NotificationConfig,
    runtime: &mut NotificationRuntime,
    cpu_average_c: Option<f32>,
    control_health: &serde_json::Value,
) -> Vec<NotificationNotice> {
    let mut notices = Vec::new();

    match (settings.temperature_c, cpu_average_c) {
        (Some(threshold), Some(value)) if value >= threshold => {
            if !runtime.temperature_warning_active {
                notices.push(NotificationNotice {
                    title: "PeterFan — CPU temperature".to_string(),
                    body: format!(
                        "CPU Core Average is {value:.0}°C (warning at {threshold:.0}°C)."
                    ),
                });
            }
            runtime.temperature_warning_active = true;
        }
        (Some(threshold), Some(value)) if value < threshold - 3.0 => {
            runtime.temperature_warning_active = false;
        }
        (None, _) => runtime.temperature_warning_active = false,
        _ => {}
    }

    let write_failures = control_health
        .get("fan_write_failure_count")
        .and_then(serde_json::Value::as_u64);
    let readback_failures = control_health
        .get("fan_readback_failure_count")
        .and_then(serde_json::Value::as_u64);
    if write_failures.is_some() || readback_failures.is_some() {
        let failures = write_failures
            .unwrap_or(0)
            .saturating_add(readback_failures.unwrap_or(0));
        let failure_count_increased = runtime
            .fan_failure_baseline
            .is_some_and(|previous| failures > previous);
        if let Some(previous) = runtime.fan_failure_baseline {
            if settings.fan_failures && failures > previous && !runtime.fan_failure_warning_active {
                let detail = control_health
                    .get("last_error")
                    .and_then(serde_json::Value::as_str)
                    .filter(|value| !value.trim().is_empty())
                    .unwrap_or("Fan control verification failed; check Fan Control Health.");
                notices.push(NotificationNotice {
                    title: "PeterFan — fan control needs attention".to_string(),
                    body: detail.chars().take(180).collect(),
                });
            }
        }

        let failsafe_active = control_health
            .get("failsafe_active")
            .and_then(serde_json::Value::as_bool);
        let consecutive_writes = control_health
            .get("consecutive_fan_write_failures")
            .and_then(serde_json::Value::as_u64);
        let consecutive_readbacks = control_health
            .get("consecutive_fan_readback_failures")
            .and_then(serde_json::Value::as_u64);
        let stale_fans = control_health
            .get("stale_fan_ids")
            .and_then(serde_json::Value::as_array);
        let has_incident_state = failsafe_active.is_some()
            || consecutive_writes.is_some()
            || consecutive_readbacks.is_some()
            || stale_fans.is_some();
        let incident_unhealthy = failsafe_active.unwrap_or(false)
            || consecutive_writes.unwrap_or(0) > 0
            || consecutive_readbacks.unwrap_or(0) > 0
            || stale_fans.is_some_and(|fans| !fans.is_empty());

        if failure_count_increased || incident_unhealthy {
            // A retry may increment the lifetime counters repeatedly while the
            // same hardware problem is active. Treat that as one incident.
            runtime.fan_failure_warning_active = true;
        } else if has_incident_state {
            // Explicit healthy state rearms notifications for the next fault.
            runtime.fan_failure_warning_active = false;
        }
        runtime.fan_failure_baseline = Some(failures);
    }

    notices
}

struct DashboardSlowCache {
    sampled_at: Option<Instant>,
    proc_sort: ProcSort,
    proc_filter: String,
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
            sampled_at: None,
            proc_sort: ProcSort::Cpu,
            proc_filter: String::new(),
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

fn save_cpu_core_graph(show: bool) -> std::io::Result<PathBuf> {
    peterfan_platform::config::update(|cfg| {
        cfg.menubar.show_cpu_core_graph = show;
        Ok(())
    })
}

fn save_runner_character(character: RunnerCharacter, random: bool) -> std::io::Result<PathBuf> {
    peterfan_platform::config::update(|cfg| {
        cfg.menubar.character = character;
        cfg.menubar.random_character = random;
        Ok(())
    })
}

fn note_runner_build_failure(message: String) {
    *STATUS.lock().expect("status poisoned") = message.clone();
    log_menubar_event(&message);
}

fn runner_startup_failure_message(
    requested: RunnerCharacter,
    fallback: RunnerCharacter,
    error: &RunnerBuildError,
) -> String {
    bounded_runner_message(
        format!(
            "runner '{}' failed; using '{}': {}",
            requested.as_str(),
            fallback.as_str(),
            error
        ),
        180,
    )
}

fn runner_startup_fatal_message(error: &RunnerStartupFailure) -> String {
    bounded_runner_message(error.to_string(), 180)
}

fn runner_switch_failure_message(requested: RunnerCharacter, error: &RunnerBuildError) -> String {
    bounded_runner_message(
        format!(
            "runner '{}' unavailable; keeping current runner: {}",
            requested.as_str(),
            error
        ),
        180,
    )
}

fn runner_preview_failure_message(requested: RunnerCharacter, error: &RunnerBuildError) -> String {
    bounded_runner_message(
        format!(
            "runner '{}' preview fallback: {}",
            requested.as_str(),
            error
        ),
        180,
    )
}

fn bounded_runner_message(message: String, limit: usize) -> String {
    let mut bounded = message.chars().take(limit).collect::<String>();
    if message.chars().count() > limit && limit > 0 {
        bounded.pop();
        bounded.push('…');
    }
    bounded
}

fn runner_character_label(lang: ResolvedLanguage, character: RunnerCharacter) -> &'static str {
    match (lang, character) {
        (ResolvedLanguage::En, RunnerCharacter::Cat) => "Cat",
        (ResolvedLanguage::En, RunnerCharacter::Dog) => "Dog",
        (ResolvedLanguage::En, RunnerCharacter::Rabbit) => "Rabbit",
        (ResolvedLanguage::En, RunnerCharacter::Fox) => "Fox",
        (ResolvedLanguage::En, RunnerCharacter::Penguin) => "Penguin",
        (ResolvedLanguage::En, RunnerCharacter::Dinosaur) => "Dinosaur",
        (ResolvedLanguage::En, RunnerCharacter::Robot) => "Robot",
        (ResolvedLanguage::En, RunnerCharacter::Ghost) => "Ghost",
        (ResolvedLanguage::En, RunnerCharacter::Bird) => "Bird",
        (ResolvedLanguage::En, RunnerCharacter::Duck) => "Duck",
        (ResolvedLanguage::En, RunnerCharacter::Sheep) => "Sheep",
        (ResolvedLanguage::En, RunnerCharacter::Fish) => "Fish",
        (ResolvedLanguage::En, RunnerCharacter::Slime) => "Slime",
        (ResolvedLanguage::En, RunnerCharacter::Human) => "Human",
        (ResolvedLanguage::En, RunnerCharacter::Pushup) => "Push-up",
        (ResolvedLanguage::En, RunnerCharacter::Orbit) => "Orbit",
        (ResolvedLanguage::En, RunnerCharacter::Sushi) => "Sushi",
        (ResolvedLanguage::En, RunnerCharacter::City) => "City",
        (ResolvedLanguage::En, RunnerCharacter::Sausage) => "Sausage",
        (ResolvedLanguage::En, RunnerCharacter::Party) => "Party",
        (ResolvedLanguage::Ko, RunnerCharacter::Cat) => "고양이",
        (ResolvedLanguage::Ko, RunnerCharacter::Dog) => "강아지",
        (ResolvedLanguage::Ko, RunnerCharacter::Rabbit) => "토끼",
        (ResolvedLanguage::Ko, RunnerCharacter::Fox) => "여우",
        (ResolvedLanguage::Ko, RunnerCharacter::Penguin) => "펭귄",
        (ResolvedLanguage::Ko, RunnerCharacter::Dinosaur) => "공룡",
        (ResolvedLanguage::Ko, RunnerCharacter::Robot) => "로봇",
        (ResolvedLanguage::Ko, RunnerCharacter::Ghost) => "유령",
        (ResolvedLanguage::Ko, RunnerCharacter::Bird) => "새",
        (ResolvedLanguage::Ko, RunnerCharacter::Duck) => "오리",
        (ResolvedLanguage::Ko, RunnerCharacter::Sheep) => "양",
        (ResolvedLanguage::Ko, RunnerCharacter::Fish) => "물고기",
        (ResolvedLanguage::Ko, RunnerCharacter::Slime) => "슬라임",
        (ResolvedLanguage::Ko, RunnerCharacter::Human) => "달리는 사람",
        (ResolvedLanguage::Ko, RunnerCharacter::Pushup) => "푸시업",
        (ResolvedLanguage::Ko, RunnerCharacter::Orbit) => "궤도",
        (ResolvedLanguage::Ko, RunnerCharacter::Sushi) => "회전초밥",
        (ResolvedLanguage::Ko, RunnerCharacter::City) => "도시",
        (ResolvedLanguage::Ko, RunnerCharacter::Sausage) => "소시지",
        (ResolvedLanguage::Ko, RunnerCharacter::Party) => "파티",
    }
}

fn save_temperature_source(source: TemperatureSource) -> std::io::Result<PathBuf> {
    peterfan_platform::config::update(|cfg| {
        cfg.menubar.temperature_source = source;
        Ok(())
    })
}

fn save_update_channel(channel: UpdateChannel) -> std::io::Result<PathBuf> {
    peterfan_platform::config::update(|cfg| {
        cfg.menubar.update_channel = channel;
        Ok(())
    })
}

fn configured_update_channel() -> UpdateChannel {
    peterfan_platform::config::load().menubar.update_channel
}

/// Persist the UI language choice so it survives a relaunch.
fn save_language(language: Language) -> std::io::Result<PathBuf> {
    peterfan_platform::config::update(|cfg| {
        cfg.menubar.language = language;
        Ok(())
    })
}

fn apply_notification_command(
    settings: &mut NotificationConfig,
    command: &str,
) -> Result<(), String> {
    if let Some(value) = command.strip_prefix("notifications:temperature:") {
        settings.temperature_c = if value == "off" {
            None
        } else {
            let threshold = value
                .parse::<f32>()
                .map_err(|_| "temperature warning must be a number".to_string())?;
            if !(50.0..=110.0).contains(&threshold) {
                return Err("temperature warning must be between 50°C and 110°C".to_string());
            }
            Some(threshold)
        };
    } else if let Some(value) = command.strip_prefix("notifications:fan-failures:") {
        settings.fan_failures = match value {
            "1" => true,
            "0" => false,
            _ => return Err("fan failure notification must be on or off".to_string()),
        };
    } else if let Some(value) = command.strip_prefix("notifications:updates:") {
        settings.updates = match value {
            "1" => true,
            "0" => false,
            _ => return Err("update notification must be on or off".to_string()),
        };
    } else {
        return Err("unknown notification setting".to_string());
    }
    Ok(())
}

/// Stage settings locally; a failed write must leave the active selection intact.
fn persist_preference<T, E>(
    current: &mut T,
    candidate: T,
    persist: impl FnOnce(&T) -> Result<(), E>,
) -> Result<(), E> {
    persist(&candidate)?;
    *current = candidate;
    Ok(())
}

fn save_notification_command(command: &str) -> Result<(), String> {
    peterfan_platform::config::update(|cfg| {
        apply_notification_command(&mut cfg.notifications, command)
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidInput, error))
    })
    .map(|_| ())
    .map_err(|error| format!("could not save notifications: {error}"))
}

fn report_preference_failure(setting: &str, error: impl std::fmt::Display) {
    let message = format!("Couldn't save {setting}: {error}");
    *STATUS.lock().expect("status poisoned") = message.clone();
    log_menubar_event(&message);
}

fn preference_saved(setting: &str, result: std::io::Result<PathBuf>) -> bool {
    match result {
        Ok(_) => true,
        Err(error) => {
            report_preference_failure(setting, error);
            false
        }
    }
}

/// Parse, validate, persist, and reload the automation rules edited in the
/// detail window. The daemon validates the same config again before applying
/// it, but validating here gives the user an immediate error without touching
/// the on-disk file.
fn parse_automation_rules(rules_json: &str) -> Result<Vec<Rule>, String> {
    let rules: Vec<Rule> = match serde_json::from_str(rules_json) {
        Ok(rules) => rules,
        Err(error) => return Err(format!("invalid rule data ({error})")),
    };
    let validation = Config {
        rules: rules.clone(),
        ..Config::default()
    };
    validation.validate_for_control()?;
    Ok(rules)
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
enum SettingsOperation {
    Rules,
    ImportConfig,
    CpuWidget,
    MenuItems,
}

#[derive(serde::Deserialize)]
struct SettingsRequest {
    id: String,
    operation: SettingsOperation,
    payload: String,
}

fn parse_settings_request(request_json: &str) -> Option<SettingsRequest> {
    let request: SettingsRequest = serde_json::from_str(request_json).ok()?;
    if request.id.is_empty()
        || request.id.len() > 128
        || !request
            .id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return None;
    }
    Some(request)
}

struct SettingsSaveResult {
    ok: bool,
    daemon_reload: &'static str,
    message: String,
}

impl SettingsSaveResult {
    fn failed(message: String) -> Self {
        Self {
            ok: false,
            daemon_reload: "not_attempted",
            message,
        }
    }
}

fn settings_saved_result(
    message: String,
    reply: Option<&str>,
    daemon_supported: bool,
) -> SettingsSaveResult {
    let (daemon_reload, suffix) = if !daemon_supported {
        ("not_applicable", String::new())
    } else {
        match reply {
            Some(reply) if reply.starts_with("ok reloaded (") => ("reloaded", String::new()),
            Some(reply) => ("failed", format!("; daemon reload failed: {reply}")),
            None => ("deferred", "; daemon reload pending".to_string()),
        }
    };
    SettingsSaveResult {
        ok: true,
        daemon_reload,
        message: format!("{message}{suffix}"),
    }
}

fn reload_after_settings_save(message: String) -> SettingsSaveResult {
    #[cfg(unix)]
    let reply = peterfan_platform::ipc::send_command("reload");
    #[cfg(not(unix))]
    let reply: Option<String> = None;
    settings_saved_result(message, reply.as_deref(), cfg!(unix))
}

fn save_automation_rules(rules_json: &str) -> SettingsSaveResult {
    let rules = match parse_automation_rules(rules_json) {
        Ok(rules) => rules,
        Err(error) => return SettingsSaveResult::failed(format!("rules save failed: {error}")),
    };
    let count = rules.len();
    if let Err(error) = peterfan_platform::config::update(|cfg| {
        cfg.rules = rules;
        Ok(())
    }) {
        return SettingsSaveResult::failed(format!("rules save failed: {error}"));
    }
    reload_after_settings_save(format!("rules saved ({count})"))
}

fn parse_imported_config(config_text: &str) -> Result<Config, String> {
    let config = match Config::from_toml(config_text) {
        Ok(config) => config,
        Err(error) => return Err(format!("invalid TOML ({error})")),
    };
    config.validate_for_control()?;
    Ok(config)
}

fn import_config(config_text: &str) -> SettingsSaveResult {
    let config = match parse_imported_config(config_text) {
        Ok(config) => config,
        Err(error) => return SettingsSaveResult::failed(format!("config import failed: {error}")),
    };
    // Import can change the update channel. Hold the same lease through save
    // and reconciliation so it cannot race a check or an installer.
    let Some(_lease) = UpdateLease::acquire(&APP_UPDATE_IN_FLIGHT) else {
        return SettingsSaveResult::failed(
            "config import failed: an update is in progress; retry after it finishes".into(),
        );
    };
    let rules = config.rules.len();
    if let Err(error) = peterfan_platform::config::save(&config) {
        return SettingsSaveResult::failed(format!("config import failed: {error}"));
    }
    reconcile_update_channel(
        &mut APP_UPDATE_STATE.lock().expect("app update state poisoned"),
        config.menubar.update_channel,
        now_unix(),
        APP_UPDATE_NETWORK_ENABLED.load(Ordering::Acquire),
    );
    reload_after_settings_save(format!("config imported ({rules} rules)"))
}

fn run_settings_request(request_json: &str) -> Option<String> {
    let request = parse_settings_request(request_json)?;
    let result = match request.operation {
        SettingsOperation::Rules => save_automation_rules(&request.payload),
        SettingsOperation::ImportConfig => import_config(&request.payload),
        SettingsOperation::CpuWidget | SettingsOperation::MenuItems => {
            SettingsSaveResult::failed("CPU widget requires the GUI event loop".into())
        }
    };
    let mut results = SETTINGS_RESULTS.lock().expect("settings results poisoned");
    results.push_back(serde_json::json!({
        "id": request.id,
        "operation": request.operation,
        "ok": result.ok,
        "daemon_reload": result.daemon_reload,
        "message": result.message,
    }));
    while results.len() > SETTINGS_RESULTS_MAX {
        results.pop_front();
    }
    Some(result.message)
}

fn copy_config_export() {
    if CONFIG_COPY_IN_FLIGHT
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return;
    }
    let config = peterfan_platform::config::load().to_toml();
    std::thread::spawn(move || {
        let result = write_support_report_to_clipboard(&config);
        CONFIG_COPY_OK.store(result.is_ok(), Ordering::Release);
        CONFIG_COPY_REVISION.fetch_add(1, Ordering::AcqRel);
        CONFIG_COPY_IN_FLIGHT.store(false, Ordering::Release);
        match result {
            Ok(()) => log_menubar_event("configuration copied"),
            Err(error) => log_menubar_event(&format!("configuration copy failed: {error}")),
        }
    });
}

#[cfg(any(target_os = "macos", test))]
fn daemon_supports_user_config(reply: Option<&str>) -> bool {
    reply.is_some_and(|reply| reply.starts_with("ok logged-in-user"))
}

#[cfg(any(target_os = "macos", test))]
fn daemon_self_reinstall_is_secure(installed: Option<&str>, resident: Option<&str>) -> bool {
    installed.is_some_and(peterfan_platform::daemon_self_reinstall_supported)
        && resident.is_some_and(peterfan_platform::daemon_self_reinstall_supported)
}

#[cfg(target_os = "macos")]
fn ensure_daemon_user_config_sync() {
    let probe = peterfan_platform::ipc::send_command("config-source");
    if !daemon_supports_user_config(probe.as_deref()) {
        let installed = peterfan_platform::installed_daemon_version();
        let resident = peterfan_platform::running_daemon_version();
        // A helper that is already too old for general control is handled by
        // the regular stale-daemon updater running alongside this task. Only
        // self-reinstall here when the sole missing capability is config sync.
        if installed
            .as_deref()
            .is_some_and(|version| !peterfan_platform::daemon_update_required(version))
            && daemon_self_reinstall_is_secure(installed.as_deref(), resident.as_deref())
        {
            match peterfan_platform::daemon_install::reinstall_via_running_daemon(false) {
                Ok(_) => log_menubar_event("daemon updated for user config sync"),
                Err(error) => {
                    log_menubar_event(&format!("daemon config sync update skipped: {error}"))
                }
            }
        }

        let deadline = Instant::now() + Duration::from_secs(12);
        while Instant::now() < deadline {
            let probe = peterfan_platform::ipc::send_command("config-source");
            if daemon_supports_user_config(probe.as_deref()) {
                break;
            }
            std::thread::sleep(Duration::from_millis(250));
        }
    }
    if daemon_control_usable(
        peterfan_platform::installed_daemon_version().as_deref(),
        peterfan_platform::running_daemon_version().as_deref(),
    ) {
        if let Some(reply) = peterfan_platform::ipc::send_command("reload") {
            log_menubar_event(&format!("daemon config sync: {reply}"));
        }
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
fn ensure_daemon_user_config_sync() {
    let _ = peterfan_platform::ipc::send_command("reload");
}

#[cfg(not(unix))]
fn ensure_daemon_user_config_sync() {}

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
            id: t.id.clone(),
            value: t.value.0,
            label_hint: Some(if t.kind == SensorKind::Cpu {
                "hottest"
            } else {
                "system"
            }),
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
    } else if sensor.id.starts_with("system.acpi.thermal_zone.") {
        match lang {
            ResolvedLanguage::Ko => "시스템 열 영역".to_string(),
            ResolvedLanguage::En => "System thermal zone".to_string(),
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

#[cfg(test)]
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

fn sensor_group_label_for_sensor(lang: ResolvedLanguage, sensor: &TempSensor) -> &'static str {
    match (lang, SensorGroup::from_sensor(sensor)) {
        (_, SensorGroup::CpuAverage) => "CPU average",
        (_, SensorGroup::CpuHottest) => "CPU hottest",
        (_, SensorGroup::CpuCore) => "CPU cores",
        (ResolvedLanguage::Ko, SensorGroup::CpuDie) => "CPU 다이",
        (ResolvedLanguage::En, SensorGroup::CpuDie) => "CPU die",
        (ResolvedLanguage::Ko, SensorGroup::GpuDie) => "GPU 다이",
        (ResolvedLanguage::En, SensorGroup::GpuDie) => "GPU die",
        (_, SensorGroup::Gpu) => "GPU",
        (ResolvedLanguage::Ko, SensorGroup::Memory) => "메모리",
        (ResolvedLanguage::En, SensorGroup::Memory) => "Memory",
        (ResolvedLanguage::Ko, SensorGroup::Storage) => "저장장치",
        (ResolvedLanguage::En, SensorGroup::Storage) => "Storage",
        (ResolvedLanguage::Ko, SensorGroup::Battery) => "배터리",
        (ResolvedLanguage::En, SensorGroup::Battery) => "Battery",
        (ResolvedLanguage::Ko, SensorGroup::Mainboard) => "메인보드",
        (ResolvedLanguage::En, SensorGroup::Mainboard) => "Mainboard",
        (ResolvedLanguage::Ko, SensorGroup::Other) => "기타",
        (ResolvedLanguage::En, SensorGroup::Other) => "Other",
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

#[cfg(target_os = "macos")]
fn clear_daemon_version_cache() {
    *DAEMON_VERSION_CACHE
        .lock()
        .expect("daemon version cache poisoned") = None;
}

fn daemon_control_usable(installed: Option<&str>, resident: Option<&str>) -> bool {
    installed.is_some_and(|version| !peterfan_platform::daemon_update_required(version))
        && resident.is_some_and(|version| !peterfan_platform::daemon_update_required(version))
}

#[cfg(any(target_os = "macos", test))]
fn clear_daemon_update_prompt_state(cfg: &mut peterfan_core::config::Config) {
    cfg.menubar.daemon_update_prompt_dismissed_for = None;
    cfg.menubar.daemon_update_prompt_snoozed_until_unix = None;
}

#[cfg(target_os = "macos")]
fn persist_clear_daemon_update_prompt_state() {
    preference_saved(
        "daemon update prompt",
        peterfan_platform::config::update(|cfg| {
            clear_daemon_update_prompt_state(cfg);
            Ok(())
        }),
    );
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
    if let Err(error) = peterfan_platform::config::update(|cfg| {
        cfg.custom_curve = Some(curve);
        Ok(())
    }) {
        return format!("failed to save curve: {error}");
    }
    // Prefer the daemon (it re-applies continuously as temps change); fall
    // back to one direct write so the change is felt immediately even
    // without a daemon, same "best effort, no persistent loop" contract as
    // every other local-fallback path in this file.
    #[cfg(unix)]
    if daemon_control_usable(
        cached_installed_daemon_version().as_deref(),
        peterfan_platform::running_daemon_version().as_deref(),
    ) && peterfan_platform::ipc::send_command("reload").is_some()
    {
        let _ = peterfan_platform::ipc::send_command("profile custom");
        return "custom curve saved".into();
    }
    if provider.capabilities().control_fans {
        let temps = provider.temperatures().unwrap_or_default();
        let Some(temp) = representative_temperature_c(&temps) else {
            return "custom curve saved; not applied: no trustworthy temperature".into();
        };
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

fn copy_support_report() {
    if SUPPORT_REPORT_COPY_IN_FLIGHT
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return;
    }
    let report = SUPPORT_REPORT
        .lock()
        .expect("support report poisoned")
        .clone();
    std::thread::spawn(move || {
        let result = write_support_report_to_clipboard(&report);
        SUPPORT_REPORT_COPY_OK.store(result.is_ok(), Ordering::Release);
        SUPPORT_REPORT_COPY_REVISION.fetch_add(1, Ordering::AcqRel);
        SUPPORT_REPORT_COPY_IN_FLIGHT.store(false, Ordering::Release);
        match result {
            Ok(()) => log_menubar_event("privacy-safe support report copied"),
            Err(error) => log_menubar_event(&format!("support report copy failed: {error}")),
        }
    });
}

fn write_support_report_to_clipboard(report: &str) -> Result<(), String> {
    if report.trim().is_empty() {
        return Err("no dashboard sample is ready".to_string());
    }
    #[cfg(target_os = "macos")]
    let mut command = std::process::Command::new("/usr/bin/pbcopy");
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut command = std::process::Command::new("cmd.exe");
        command.args(["/C", "clip"]);
        command
    };
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    return Err("clipboard export is supported on macOS and Windows".to_string());

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    {
        let mut child = command
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|error| error.to_string())?;
        child
            .stdin
            .take()
            .ok_or_else(|| "clipboard helper stdin unavailable".to_string())?
            .write_all(report.as_bytes())
            .map_err(|error| error.to_string())?;
        let status = child.wait().map_err(|error| error.to_string())?;
        status
            .success()
            .then_some(())
            .ok_or_else(|| format!("clipboard helper exited with {status}"))
    }
}

struct SupportReportInput<'a> {
    cpu_pct: f32,
    logical_cores: usize,
    selected_average_c: Option<f32>,
    core_hottest_c: Option<f32>,
    safety_hottest_c: Option<f32>,
    temperatures: &'a [TempSensor],
    temperature_sampled_at_unix_ms: Option<u64>,
    temperature_stale: bool,
    fans: &'a [serde_json::Value],
    daemon_version: Option<&'a str>,
    daemon: Option<&'a serde_json::Value>,
}

fn support_report_json(input: SupportReportInput<'_>) -> serde_json::Value {
    let daemon_field = |key: &str| input.daemon.and_then(|value| value.get(key)).cloned();
    serde_json::json!({
        "schema_version": 1,
        "generated_at_unix_ms": now_unix_ms(),
        "privacy": "no hostname, username, serial number, IP address, or file paths",
        "app": {
            "name": "PeterFan",
            "version": env!("CARGO_PKG_VERSION"),
            "bundle_id": "kr.co.uulab.peterfan",
        },
        "platform": {
            "os": std::env::consts::OS,
            "arch": std::env::consts::ARCH,
            "logical_cores": input.logical_cores,
        },
        "cpu": {
            "usage_percent": input.cpu_pct,
            "core_average_c": input.selected_average_c,
            "core_hottest_c": input.core_hottest_c,
            "safety_hottest_c": input.safety_hottest_c,
        },
        "temperature_sample": {
            "sampled_at_unix_ms": input.temperature_sampled_at_unix_ms,
            "stale": input.temperature_stale,
            "sensors": input.temperatures.iter().map(|sensor| serde_json::json!({
                "id": sensor.id,
                "label": sensor.label,
                "kind": sensor.kind,
                "group": SensorGroup::from_sensor(sensor),
                "source": sensor.source,
                "value_c": sensor.value.0,
            })).collect::<Vec<_>>(),
        },
        "fans": input.fans,
        "daemon": {
            "version": input.daemon_version,
            "mode": daemon_field("mode"),
            "backend": daemon_field("backend"),
            "control_revision": daemon_field("control_revision"),
            "applied_control_revision": daemon_field("applied_control_revision"),
            "last_control_apply_unix_ms": daemon_field("last_control_apply_unix_ms"),
            "fan_targets": daemon_field("fan_targets"),
            "fan_readbacks": daemon_field("fan_readbacks"),
            "control_health": daemon_field("control_health"),
        },
    })
}

fn sample_age(sampled_at: Option<Instant>, now: Instant) -> Option<Duration> {
    sampled_at.map(|sampled| now.saturating_duration_since(sampled))
}

fn sample_is_stale(sampled_at: Option<Instant>, now: Instant, limit: Duration) -> bool {
    sample_age(sampled_at, now)
        .map(|age| age > limit)
        .unwrap_or(true)
}

#[cfg(target_os = "macos")]
fn suspend_inclusive_time() -> Option<Duration> {
    // Darwin Instant uses uptime, which pauses during suspend. This public
    // clock includes sleep and is unaffected by wall-clock/NTP adjustments.
    let mut time = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: the fixed supported clock ID writes into one valid timespec.
    if unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC_RAW, &mut time) } != 0 {
        return None;
    }
    let seconds = u64::try_from(time.tv_sec).ok()?;
    let nanos = u32::try_from(time.tv_nsec)
        .ok()
        .filter(|nanos| *nanos < 1_000_000_000)?;
    Some(Duration::new(seconds, nanos))
}

#[cfg(not(target_os = "macos"))]
fn suspend_inclusive_time() -> Option<Duration> {
    None
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ResumeRecoveryPlan {
    hide_popover: bool,
    invalidate_hardware_caches: bool,
    clear_dashboard_histories: bool,
    discard_completed_reads: bool,
    schedule_refreshes_immediately: bool,
    clear_control_confirmation: bool,
}

const fn resume_recovery_plan() -> ResumeRecoveryPlan {
    ResumeRecoveryPlan {
        hide_popover: true,
        invalidate_hardware_caches: true,
        clear_dashboard_histories: true,
        discard_completed_reads: true,
        schedule_refreshes_immediately: true,
        clear_control_confirmation: true,
    }
}

fn recover_after_pause(app: &mut App, now: Instant) {
    let plan = resume_recovery_plan();
    log_menubar_event("resume recovery: invalidating hardware and dashboard caches");
    if plan.hide_popover {
        hide_popover(app);
    }
    if plan.invalidate_hardware_caches {
        app.temperature_cache.clear();
        app.temperature_sampled_at = None;
        app.temperature_sampled_at_unix_ms = None;
        // Keep the last known fan identities across sleep so the UI does not
        // briefly claim that built-in fans disappeared. Readings remain gated by
        // `fan_sampled_at` until a fresh post-wake sample arrives.
        app.fan_sampled_at = None;
        app.fan_empty_samples = 0;
        app.all_temp_rows_cache.clear();
        app.all_temp_sampled_at = None;
        app.all_temp_sampled_at_unix_ms = None;
        app.daemon_json_cache = None;
        app.daemon_json_sampled_at = None;
        app.daemon_probe_completed = false;
        app.dashboard_slow_cache = DashboardSlowCache::default();
    }
    if plan.clear_dashboard_histories {
        app.cpu_widget_history = cpu_widget::CpuWidgetHistory::default();
        app.fan_hist.clear();
        app.cpu_h.clear();
        app.per_core_h.clear();
        app.mem_h.clear();
        app.temp_h.clear();
        app.net_h.clear();
        app.disk_io_h.clear();
    }
    // A completed read from before sleep must not be mistaken for the first
    // post-resume sample. An in-flight hardware call is left alone; its
    // result is still valid if it finishes after wake.
    if plan.discard_completed_reads {
        let _ = app.temperature_read.take();
        let _ = app.fan_read.take();
        let _ = app.all_temp_read.take();
        let _ = app.daemon_read.take();
    }
    if plan.schedule_refreshes_immediately {
        app.next_temperature_refresh = now;
        app.next_fan_refresh = now;
        app.next_all_temp_refresh = now;
        app.next_daemon_refresh = now;
        app.next_dashboard_slow_refresh = now;
        app.next_update_result_refresh = now;
    }
    if plan.clear_control_confirmation {
        app.control_confirm_until = None;
    }
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

#[cfg(target_os = "windows")]
fn acquire_single_instance_lock_at(path: &Path) -> Result<File, String> {
    use std::os::windows::fs::OpenOptionsExt;

    OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .share_mode(0)
        .open(path)
        .map_err(|e| format!("could not lock {}: {e}", path.display()))
}

#[cfg(all(not(unix), not(target_os = "windows")))]
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
    let health_result_path = peterfan_platform::updater::update_install_result_path();
    let mut update_health = match peterfan_platform::update_health::Pending::from_args(
        &args,
        health_result_path.as_deref(),
        use_mock,
    ) {
        Ok(request) => request,
        Err(error) => {
            eprintln!("Invalid update launch: {error}");
            std::process::exit(2);
        }
    };
    let _single_instance = match acquire_single_instance_lock(use_mock) {
        Ok(lock) => lock,
        Err(e) => {
            log_menubar_event(&format!("startup rejected: single instance lock: {e}"));
            eprintln!("PeterFan is already running ({e}).");
            return;
        }
    };
    APP_UPDATE_NETWORK_ENABLED.store(!use_mock, Ordering::Release);
    let channel = configured_update_channel();
    let now = now_unix();
    // Mock launches never resolve or open the history sidecar.
    let history = if use_mock {
        None
    } else {
        load_update_history(channel, now)
    };
    {
        let mut state = APP_UPDATE_STATE.lock().expect("app update state poisoned");
        if use_mock {
            state.channel = channel;
        } else {
            initialize_update_discovery(&mut state, channel, now, history);
        }
    }
    log_menubar_event(&format!("startup mock={use_mock}"));
    load_fan_action_log();

    let saved_config = peterfan_platform::config::load();
    let critical_temp_c = saved_config.critical_temp_c;
    let notifications = saved_config.notifications.clone();
    let saved = saved_config.menubar;
    let display = args
        .iter()
        .position(|a| a == "--display")
        .and_then(|i| args.get(i + 1))
        .and_then(|v| MenubarDisplay::parse(v))
        .unwrap_or(saved.display);
    let requested_runner_character = args
        .iter()
        .position(|a| a == "--character")
        .and_then(|i| args.get(i + 1))
        .and_then(|v| RunnerCharacter::parse(v))
        .unwrap_or(saved.character);
    let requested_runner_character = if requested_runner_character.is_featured() {
        requested_runner_character
    } else {
        RunnerCharacter::Cat
    };
    let runner_startup = match initialize_runner_assets(requested_runner_character) {
        Ok(state) => state,
        Err(error) => {
            let message = runner_startup_fatal_message(&error);
            note_runner_build_failure(message.clone());
            eprintln!("{message}");
            return;
        }
    };
    let random_character = args
        .iter()
        .position(|a| a == "--character")
        .and_then(|i| args.get(i + 1))
        .map(|value| value == "random")
        .unwrap_or(saved.random_character);
    let runner_character = runner_startup.character;
    let temperature_source = saved.temperature_source;
    let update_channel = saved.update_channel;
    let language = saved.language;
    let show_cpu_core_graph = saved.show_cpu_core_graph;
    let cpu_widget_config = saved.cpu_widget;

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
    #[allow(unused_mut)]
    let mut event_loop = EventLoopBuilder::<()>::new().build();
    #[cfg(target_os = "macos")]
    event_loop.set_activation_policy(ActivationPolicy::Accessory);
    let event_proxy = event_loop.create_proxy();
    // Native callbacks otherwise fill a receiver without waking Tao. Forward
    // before waking so an idle loop handles the click immediately, not at the
    // next metrics refresh. Installing a handler disables the library receiver.
    let tray_proxy = event_proxy.clone();
    let (forward_tray, tray_events) = ui_events::channel(move || {
        let _ = tray_proxy.send_event(());
    });
    TrayIconEvent::set_event_handler(Some(move |event| {
        if matches!(event, TrayIconEvent::Click { .. }) {
            log_menubar_event(&format!("tray native callback {event:?}"));
            forward_tray(event);
        }
    }));
    let menu_proxy = event_proxy.clone();
    let (forward_menu, menu_events) = ui_events::channel(move || {
        let _ = menu_proxy.send_event(());
    });
    MenuEvent::set_event_handler(Some(forward_menu));

    let mut app = App {
        monitor,
        provider,
        display,
        primary_visible: true,
        runner_character,
        random_runner: RandomRunner::new(random_character, Instant::now()),
        temperature_source,
        update_channel,
        critical_temp_c,
        notifications,
        notification_runtime: NotificationRuntime::default(),
        language,
        show_cpu_core_graph,
        cpu_widget_config,
        cpu_widget_history: cpu_widget::CpuWidgetHistory::default(),
        cpu_widget_error: None,
        #[cfg(target_os = "macos")]
        cpu_widget_native: None,
        tray: None,
        tray_menu: None,
        window: None,
        webview: None,
        webview_ready: false,
        dashboard_script: None,
        popover_visible: false,
        popover_show_at: None,
        popover_tray_focus_loss_at: None,
        popover_anchor_tray: None,
        left_button_down_seen: false,
        detail_window: None,
        detail_webview: None,
        detail_webview_ready: false,
        fan_hist: VecDeque::with_capacity(HIST_CAP),
        cpu_h: RangedHistory::new(),
        per_core_h: Vec::new(),
        mem_h: RangedHistory::new(),
        temp_h: RangedHistory::new(),
        net_h: RangedHistory::new(),
        disk_io_h: RangedHistory::new(),
        process_filter: String::new(),
        dashboard_slow_cache: DashboardSlowCache::default(),
        next_dashboard_slow_refresh: Instant::now() + DASHBOARD_SLOW_REFRESH,
        runner_frame: 0,
        runner_cpu_pct: 0.0,
        runner_has_sample: false,
        reduce_motion: system_reduce_motion(),
        runner_assets: runner_startup.assets,
        #[cfg(not(target_os = "macos"))]
        runner_band: 0,
        #[cfg(target_os = "macos")]
        runner_native_timer: None,
        #[cfg(target_os = "macos")]
        runner_native_painter: Rc::new(RefCell::new(native_runner::NativeRunnerPainter::default())),
        #[cfg(target_os = "macos")]
        runner_native_cpu_bits: Arc::new(AtomicU32::new(0.0_f32.to_bits())),
        #[cfg(target_os = "macos")]
        runner_native_motion_cpu_bits: Arc::new(AtomicU32::new(0.0_f32.to_bits())),
        #[cfg(target_os = "macos")]
        runner_native_phase_us: Arc::new(AtomicU64::new(0)),
        #[cfg(target_os = "macos")]
        runner_native_tick_us: Arc::new(AtomicU64::new(RUNNER_NATIVE_TICK.as_micros() as u64)),
        #[cfg(target_os = "macos")]
        runner_native_band: Arc::new(AtomicUsize::new(0)),
        #[cfg(target_os = "macos")]
        runner_native_frame: Arc::new(AtomicU64::new(0)),
        #[cfg(target_os = "macos")]
        runner_native_applied_icon: Arc::new(AtomicUsize::new(usize::MAX)),
        last_runner_icon: None,
        last_menubar_text: None,
        last_menubar_tooltip: None,
        temperature_cache: Vec::new(),
        temperature_sampled_at: None,
        temperature_sampled_at_unix_ms: None,
        next_temperature_refresh: Instant::now(),
        temperature_read: Arc::new(BackgroundRead::default()),
        fan_cache: Vec::new(),
        fan_sampled_at: None,
        fan_empty_samples: 0,
        next_fan_refresh: Instant::now(),
        fan_read: Arc::new(BackgroundRead::default()),
        all_temp_rows_cache: Vec::new(),
        all_temp_sampled_at: None,
        all_temp_sampled_at_unix_ms: None,
        next_all_temp_refresh: Instant::now() + ALL_TEMP_REFRESH,
        all_temp_read: Arc::new(BackgroundRead::default()),
        daemon_json_cache: None,
        daemon_json_sampled_at: None,
        daemon_probe_completed: false,
        next_daemon_refresh: Instant::now(),
        daemon_read: Arc::new(BackgroundRead::default()),
        update_install_result: peterfan_platform::updater::read_update_install_result(),
        next_update_result_refresh: Instant::now() + Duration::from_secs(1),
        control_confirm_until: None,
    };
    if let Some(error) = runner_startup.original_error {
        note_runner_build_failure(runner_startup_failure_message(
            requested_runner_character,
            app.runner_character,
            &error,
        ));
    }

    let mut next_metric_at = Instant::now();
    let mut next_runner_at = Instant::now();
    let mut next_runner_cpu_at = Instant::now() + RUNNER_CPU_REFRESH;
    let mut prewarm_popover_at = Some(Instant::now() + POPOVER_PREWARM_DELAY);
    let mut pause_clock = pause_clock::PauseClock::new(RESUME_RECOVERY_GAP);
    event_loop.run(move |event, target, control_flow| {
        if QUIT.load(Ordering::Relaxed) {
            *control_flow = ControlFlow::Exit;
            return;
        }

        let now = Instant::now();
        if pause_clock.observe(now, suspend_inclusive_time()) {
            recover_after_pause(&mut app, now);
            next_metric_at = now;
            next_runner_at = now;
            next_runner_cpu_at = now + RUNNER_CPU_REFRESH;
            prewarm_popover_at = None;
        }

        if OPEN_DETAIL.swap(false, Ordering::Relaxed) {
            hide_popover(&mut app);
            open_detail_window(&mut app, target, &event_proxy);
        }

        match event {
            Event::NewEvents(StartCause::Init) => {
                build_tray(&mut app);
                #[cfg(target_os = "macos")]
                if let Err(error) = menu_items_app::apply(&mut app, saved.item_visibility(), false) {
                    app.cpu_widget_error = Some(error.clone());report_preference_failure("CPU widget", error);
                }
                #[cfg(target_os = "macos")]
                sync_native_runner_timer(&mut app);
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
                    // Compatible root helpers can verify and install the
                    // newly bundled daemon themselves. Keep required helper
                    // updates silent after the one initial macOS approval.
                    std::thread::spawn(maybe_silently_update_stale_daemon);
                    // A system LaunchDaemon can start before any GUI user has
                    // logged in. Reload now that the menu-bar app knows which
                    // user's settings should drive rules and custom curves.
                    std::thread::spawn(ensure_daemon_user_config_sync);
                    std::thread::spawn(check_for_updates_on_launch);
                }
            }
            Event::NewEvents(StartCause::ResumeTimeReached { .. }) => {}
            Event::WindowEvent {
                event: WindowEvent::Focused(false),
                window_id,
                ..
            } => {
                // Ordering out an old anchor can deliver a delayed blur after
                // its replacement has been scheduled but is still hidden.
                if app.popover_visible && app.window.as_ref().is_some_and(|w| w.id() == window_id) {
                    #[cfg(target_os = "macos")]
                    let tray_press = app.popover_visible
                        && (app.tray.as_ref().is_some_and(|tray| app.popover_anchor_tray.as_deref() == Some(tray.id().as_ref()) && native_tray_left_press(tray))
                            || app.cpu_widget_native.as_ref().is_some_and(|widget| app.popover_anchor_tray.as_deref() == Some(widget.tray().id().as_ref()) && native_tray_left_press(widget.tray())));
                    #[cfg(not(target_os = "macos"))]
                    let tray_press = false;
                    log_menubar_event(&format!("popover focus lost tray_press={tray_press}"));
                    let owner = app.popover_anchor_tray.clone();
                    hide_popover(&mut app);
                    // AppKit can blur the popover before delivering the same
                    // status-item press. Preserve that click's close intent.
                    if tray_press {
                        app.popover_tray_focus_loss_at = Some(now);
                        app.popover_anchor_tray = owner;
                    }
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

        if app.popover_show_at.is_some_and(|at| now >= at) {
            if let Some(w) = &app.window {
                w.set_visible(true);
                // Keep the prewarmed window unfocused during startup, but
                // focus it when the user explicitly opens the popover so
                // WebView buttons and scrolling receive mouse events.
                w.set_focus();
                app.popover_visible = true;
                log_menubar_event("popover shown");
                if let Some(wv) = &app.webview {
                    evaluate_dashboard_script(
                        wv,
                        "window.setRunnerPreviewVisibility&&window.setRunnerPreviewVisibility(true);",
                        "popover",
                    );
                }
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
        if PROCESS_REFRESH_REQUESTED.swap(false, Ordering::AcqRel) {
            app.next_dashboard_slow_refresh = now;
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
            let reduce_motion = system_reduce_motion();
            if app.reduce_motion != reduce_motion {
                app.reduce_motion = reduce_motion;
                app.runner_frame = 0;
                #[cfg(target_os = "macos")]
                {
                    // A static accessibility presentation uses the neutral
                    // pose rather than freezing halfway through an exercise.
                    stop_native_runner_timer(&mut app);
                    app.runner_native_frame.store(0, Ordering::Relaxed);
                    app.runner_native_applied_icon
                        .store(usize::MAX, Ordering::Relaxed);
                }
                app.last_runner_icon = None;
                apply_runner_icon(&mut app);
            }
            update(&mut app);
            next_runner_cpu_at = now + RUNNER_CPU_REFRESH;
            #[cfg(target_os = "macos")]
            sync_native_runner_timer(&mut app);
            next_metric_at = now
                + if confirming_control {
                    CONTROL_CONFIRM_REFRESH
                } else {
                    REFRESH
                };
            if app.primary_visible && runner_uses_event_loop_timer(app.display, app.reduce_motion) {
                // A CPU spike must accelerate the runner immediately instead
                // of waiting for the old idle-speed deadline to expire.
                next_runner_at = next_runner_at.min(
                    now + runner_frame_interval_for_pose(app.runner_character, app.runner_cpu_pct, app.runner_frame),
                );
            }
        }
        if let Some(character) = app.random_runner.next_due(
            now, app.runner_character, runner_visibility_should_animate(app.primary_visible, app.display, app.reduce_motion),
        ) {
            switch_runner_character(&mut app, character);
            next_runner_at = now;
            next_metric_at = now;
        }
        if runner_visibility_should_animate(app.primary_visible, app.display, app.reduce_motion) && now >= next_runner_cpu_at {
            let cpu = app.monitor.sample_cpu_usage();
            app.runner_cpu_pct = smooth_runner_cpu(app.runner_cpu_pct, cpu, app.runner_has_sample);
            app.runner_has_sample = true;
            next_runner_cpu_at = now + RUNNER_CPU_REFRESH;
            #[cfg(target_os = "macos")]
            sync_native_runner_timer(&mut app);
            #[cfg(not(target_os = "macos"))]
            {
                next_runner_at = next_runner_at.min(
                    now + runner_frame_interval_for_pose(app.runner_character, app.runner_cpu_pct, app.runner_frame),
                );
            }
        }
        if (app.primary_visible && runner_uses_event_loop_timer(app.display, app.reduce_motion)) && now >= next_runner_at {
            animate_runner(&mut app);
            next_runner_at =
                now + runner_frame_interval_for_pose(app.runner_character, app.runner_cpu_pct, app.runner_frame);
        }
        if let Some(request) = update_health.as_mut() {
            match request.poll(now, app.tray.is_some(), app.webview_ready, env!("CARGO_PKG_VERSION")) {
                Ok(true) => {log_menubar_event("update launch health acknowledged");update_health = None;}
                Ok(false) => {}
                Err(error) => {
                    log_menubar_event(&format!("update launch health failed: {error}"));
                    update_health = None;
                }
            }
        }
        let next_tick = [
            Some(next_metric_at),
            app.random_runner.deadline(runner_visibility_should_animate(app.primary_visible, app.display, app.reduce_motion)),
            runner_visibility_should_animate(app.primary_visible, app.display, app.reduce_motion).then_some(next_runner_cpu_at),
            (app.primary_visible && runner_uses_event_loop_timer(app.display, app.reduce_motion)).then_some(next_runner_at),
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
                if let Some(value) = c.strip_prefix("processfilter:") {
                    app.process_filter = peterfan_core::metrics::normalize_process_query(value);
                    app.next_dashboard_slow_refresh = now;
                    next_metric_at = now;
                    refresh_after_pending = true;
                } else if let Some(request) = parse_process_quit_command(c) {
                    if !PROCESS_ACTION_IN_FLIGHT.swap(true, Ordering::AcqRel) {
                        let revision = {
                            let mut state = PROCESS_ACTION.lock().expect("process action poisoned");
                            let revision = state.revision.saturating_add(1);
                            *state = ProcessActionState {
                                revision,
                                phase: "pending",
                                pid: Some(request.pid),
                                name: None,
                                code: None,
                                message: None,
                            };
                            revision
                        };
                        let proxy = event_proxy.clone();
                        std::thread::spawn(move || {
                            let result = peterfan_platform::process_control::quit_process(request);
                            let state = completed_process_action(revision, request, result);
                            log_menubar_event(&process_action_log_entry(&state));
                            *PROCESS_ACTION.lock().expect("process action poisoned") = state;
                            PROCESS_REFRESH_REQUESTED.store(true, Ordering::Release);
                            PROCESS_ACTION_IN_FLIGHT.store(false, Ordering::Release);
                            let _ = proxy.send_event(());
                        });
                    }
                    refresh_after_pending = true;
                } else if let Some(json) = c.strip_prefix("savecurve:") {
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
                    let proxy = event_proxy.clone();
                    std::thread::spawn(move || {
                        install_fan_control();
                        let _ = proxy.send_event(());
                    });
                    refresh_after_pending = true;
                } else if c == "checkupdates" {
                    std::thread::spawn(check_for_updates_interactive);
                    refresh_after_pending = true;
                } else if c == "installupdate" {
                    std::thread::spawn(install_update_interactive);
                    refresh_after_pending = true;
                } else if let Some(value) = c.strip_prefix("updatechannel:") {
                    if let Some(channel) = UpdateChannel::parse(value) {
                        if let Some(_lease) = UpdateLease::acquire(&APP_UPDATE_IN_FLIGHT) {
                            match save_update_channel(channel) {
                                Ok(_) => {
                                    app.update_channel = channel;
                                    let mut state = APP_UPDATE_STATE.lock().expect("app update state poisoned");
                                    reconcile_update_channel(
                                        &mut state, channel, now_unix(),
                                        APP_UPDATE_NETWORK_ENABLED.load(Ordering::Acquire),
                                    );
                                    *STATUS.lock().expect("status poisoned") =
                                        format!("update channel set to {}", channel.as_str());
                                }
                                Err(error) => {
                                    let message = format!("Couldn't save update channel: {error}");
                                    let mut state = APP_UPDATE_STATE.lock().expect("app update state poisoned");
                                    state.phase = "channel_failed";
                                    state.message = Some(message.clone());
                                    *STATUS.lock().expect("status poisoned") = message;
                                }
                            }
                        } else {
                            *STATUS.lock().expect("status poisoned") =
                                "Update in progress; change the channel after it finishes.".into();
                        }
                    }
                    refresh_after_pending = true;
                } else if c == "toggle-login-item" || c == "togglelogin" {
                    std::thread::spawn(move || {
                        let status = toggle_login_item();
                        *STATUS.lock().expect("status poisoned") = status;
                    });
                    refresh_after_pending = true;
                } else if let Some(value) = c.strip_prefix("display:") {
                    if let Some(display) = MenubarDisplay::parse(value) {
                        if let Err(error) = menu_items_app::apply_display(&mut app, display) { report_preference_failure("menu bar display", error); }
                        next_runner_at = now;
                        refresh_after_pending = true;
                    }
                } else if let Some(value) = c.strip_prefix("cpu-core-graph:") {
                    let show = value == "1";
                    if preference_saved("CPU core graph", save_cpu_core_graph(show)) {
                        app.show_cpu_core_graph = show;
                    }
                    refresh_after_pending = true;
                } else if let Some(value) = c.strip_prefix("character:") {
                    if value == "random" {
                        if set_runner_random(&mut app) {
                            next_runner_at = now;
                        }
                        refresh_after_pending = true;
                    } else if let Some(character) = RunnerCharacter::parse(value) {
                        if set_runner_character(&mut app, character) {
                            next_runner_at = now;
                        }
                        refresh_after_pending = true;
                    }
                } else if let Some(request) = c.strip_prefix("settingsrequest:") {
                    let parsed = parse_settings_request(request);
                    let operation = parsed.as_ref().map(|r| r.operation);
                    let status = if operation == Some(SettingsOperation::CpuWidget) {
                        parsed.map(|request| cpu_widget_app::request(&mut app, request))
                    } else if operation == Some(SettingsOperation::MenuItems) {
                        parsed.map(|request| menu_items_app::request(&mut app, request))
                    } else { run_settings_request(request) };
                    if let Some(status) = status { *STATUS.lock().expect("status poisoned") = status; }
                    if operation == Some(SettingsOperation::ImportConfig) {
                        menu_items_app::reload(&mut app, &peterfan_platform::config::load().menubar);
                    }
                    refresh_after_pending = true;
                } else if let Some(json) = c.strip_prefix("rules:") {
                    let status = save_automation_rules(json).message;
                    *STATUS.lock().expect("status poisoned") = status;
                    refresh_after_pending = true;
                } else if let Some(config_text) = c.strip_prefix("importconfig:") {
                    let status = import_config(config_text).message;
                    *STATUS.lock().expect("status poisoned") = status;
                    refresh_after_pending = true;
                } else if c.starts_with("notifications:") {
                    let mut candidate = app.notifications.clone();
                    let result = apply_notification_command(&mut candidate, c).and_then(|()| {
                        persist_preference(&mut app.notifications, candidate, |_| save_notification_command(c))
                    });
                    if let Err(error) = result {
                        report_preference_failure("notifications", error);
                    }
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
                } else if c == "ready:popover" {
                    app.webview_ready = true;
                    log_menubar_event("popover webview ready");
                    // A hidden prewarmed WebView can finish after the first
                    // native payload was built. Re-send that payload now so
                    // the first visible frame cannot remain at empty defaults.
                    if let (Some(wv), Some(script)) =
                        (app.webview.as_ref(), app.dashboard_script.as_deref())
                    {
                        evaluate_dashboard_script(wv, script, "popover");
                    }
                    next_metric_at = now;
                } else if c == "ready:detail" {
                    app.detail_webview_ready = true;
                    log_menubar_event("detail webview ready");
                    if let (Some(wv), Some(script)) =
                        (app.detail_webview.as_ref(), app.dashboard_script.as_deref())
                    {
                        evaluate_dashboard_script(wv, script, "detail");
                    }
                    next_metric_at = now;
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
                        let status = execute_control_serial(provider.as_ref(), &cmd);
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
        while let Ok(ev) = menu_events.try_recv() {
            let id = &ev.id;
            #[cfg(target_os = "macos")]
            if let Some(action) = app.cpu_widget_native.as_ref().and_then(|widget| widget.menu_action(id)) {
                use native_cpu_widget::CpuWidgetAction;
                match action {
                    CpuWidgetAction::OpenDashboard => {
                        if let Some((tray_id, rect)) = app.cpu_widget_native.as_ref().and_then(|widget| widget.tray().rect().map(|rect| (widget.tray().id().as_ref().to_string(), rect))) {
                            hide_popover(&mut app);
                            toggle_popover(&mut app, target, &tray_id, rect, &event_proxy);
                        }
                    }
                    CpuWidgetAction::Visibility(visibility) => {
                        if let Err(error) = menu_items_app::apply(&mut app, visibility, true) { report_preference_failure("Menu items", error); }
                    }
                    preference => {
                        let mut config = app.cpu_widget_config;
                        match preference {

                            CpuWidgetAction::Style(style) => config = config.select_style(style),
                            CpuWidgetAction::Percentage(show) => config.show_percentage = show,
                            CpuWidgetAction::OpenDashboard | CpuWidgetAction::Visibility(_) => unreachable!(),
                        }
                        if let Err(error) = cpu_widget_app::apply(&mut app, config, true) {
                            report_preference_failure("CPU widget", error);
                        }
                    }
                }
                next_metric_at = now;*control_flow = ControlFlow::WaitUntil(now);
                continue;
            }
            #[cfg(target_os = "macos")]
            if let Some(visibility) = app.tray_menu.as_ref().and_then(|menu| menu.menu_items.action(id)) {
                if let Err(error) = menu_items_app::apply(&mut app, visibility, true) { report_preference_failure("Menu items", error); }
                next_metric_at = now;
                continue;
            }
            let mut matched_display: Option<MenubarDisplay> = None;
            let mut matched_character: Option<RunnerCharacter> = None;
            let mut matched_random = false;
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
                } else if id == tm.random_character_item.id() {
                    matched_random = true;
                    None
                } else if let Some((character, _)) =
                    tm.character_items.iter().find(|(_, item)| item.id() == id)
                {
                    matched_character = Some(*character);
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
                    let proxy = event_proxy.clone();
                    std::thread::spawn(move || {
                        install_fan_control();
                        let _ = proxy.send_event(());
                    });
                    None
                } else if tm.check_updates == *id {
                    std::thread::spawn(install_update_interactive);
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
                if let Err(error) = menu_items_app::apply_display(&mut app, d) { report_preference_failure("menu bar display", error); }
                next_runner_at = Instant::now();
                if let Some(ref tm) = app.tray_menu {
                    for (candidate, item) in &tm.display_items { item.set_checked(*candidate == app.display); }
                }
                update(&mut app);
            }
            if matched_random {
                if set_runner_random(&mut app) {
                    next_runner_at = Instant::now();
                }
                update(&mut app);
            }
            if let Some(character) = matched_character {
                if set_runner_character(&mut app, character) {
                    next_runner_at = Instant::now();
                }
                update(&mut app);
            }
            if let Some(source) = matched_temperature_source {
              if preference_saved("temperature source", save_temperature_source(source)) {
                app.temperature_source = source;
              }
                if let Some(ref tm) = app.tray_menu {
                    for (candidate, item) in &tm.temperature_source_items {
                        item.set_checked(*candidate == app.temperature_source);
                    }
                }
                update(&mut app);
            }
            if let Some(l) = matched_language {
              if preference_saved("language", save_language(l)) {
                app.language = l;
                // Rebuild everything that bakes labels in at construction
                // time — the native menu (labels are set once, at build
                // time) and both webviews (the dashboard HTML is generated
                // per-language, not re-translated live) — so the change is
                // visible immediately instead of needing a relaunch.
                build_tray(&mut app);
                let was_visible = app.popover_visible;
                let popover_geometry = app
                    .window
                    .as_ref()
                    .and_then(capture_window_geometry);
                if app.window.is_some() {
                    if let Some(window) = app.window.as_ref() {
                        window.set_visible(false);
                    }
                    app.window = None;
                    app.webview = None;
                    app.webview_ready = false;
                    let _ = build_popover(&mut app, target, &event_proxy);
                    if let (Some(window), Some(geometry)) = (app.window.as_ref(), popover_geometry)
                    {
                        restore_window_geometry(window, geometry);
                    }
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
                    let detail_geometry = app
                        .detail_window
                        .as_ref()
                        .and_then(capture_window_geometry);
                    if let Some(window) = app.detail_window.as_ref() {
                        window.set_visible(false);
                    }
                    app.detail_window = None;
                    app.detail_webview = None;
                    app.detail_webview_ready = false;
                    if was_detail_visible {
                        open_detail_window(&mut app, target, &event_proxy);
                        if let (Some(window), Some(geometry)) =
                            (app.detail_window.as_ref(), detail_geometry)
                        {
                            restore_window_geometry(window, geometry);
                        }
                    }
                }
              }
                if let Some(ref tm) = app.tray_menu {
                    for (candidate, item) in &tm.language_items {
                        item.set_checked(*candidate == app.language);
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
                    let status = execute_control_serial(provider.as_ref(), &cmd);
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

        while let Ok(ev) = tray_events.try_recv() {
            // Open on mouse-down. Some macOS menu-bar configurations swallow
            // mouse-up while the bar is auto-hiding or changing displays.
            // Consume the matching Up event so a normal click toggles once.
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state,
                rect,
                id,
                ..
            } = ev
            {
                let runner = app.tray.as_ref().map(|tray| (tray.id().as_ref(), tray.rect()));
                #[cfg(target_os = "macos")]
                let cpu = app.cpu_widget_native.as_ref().map(|widget| (widget.tray().id().as_ref(), widget.tray().rect()));
                #[cfg(not(target_os = "macos"))]
                let cpu = None;
                let Some(live_rect) = clicked_tray_anchor(id.as_ref(), runner, cpu, rect) else { continue; };
                let should_toggle = route_left_click(button_state, &mut app.left_button_down_seen);
                log_menubar_event(&format!(
                    "tray left-click state={button_state:?} toggle={should_toggle} rect={rect:?}"
                ));
                if should_toggle {
                    // The event rectangle can be one animation frame behind
                    // the native status item. Re-read it at click time so a
                    // mixed-DPI display switch cannot anchor the popover to a
                    // stale menu-bar position.
                    toggle_popover(&mut app, target, id.as_ref(), live_rect, &event_proxy);
                    if app.popover_show_at.is_some() {
                        // Tray events are drained after the normal wake deadline is
                        // calculated. Wake once immediately to inject a payload
                        // into the still-hidden prewarmed WebView; the existing
                        // placement deadline then reveals an already-rendered UI.
                        next_metric_at = now;
                        *control_flow = ControlFlow::WaitUntil(now);
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
        --character <name>                      CPU runner character (20 choices)\n    \
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
            "--metric" | "--display" | "--character" | "--update-health-token" => {
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

    // "Display" — number only / runner only / both.
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

    let character_submenu = Submenu::new(s.runner_character, true);
    let random_character_item = CheckMenuItem::new(
        if app.language.resolve() == ResolvedLanguage::Ko {
            "랜덤 · 1분마다"
        } else {
            "Random · every minute"
        },
        true,
        app.random_runner.enabled(),
        None,
    );
    let _ = character_submenu.append(&random_character_item);
    let _ = character_submenu.append(&PredefinedMenuItem::separator());
    let character_items = RunnerCharacter::FEATURED
        .into_iter()
        .map(|character| {
            let item = CheckMenuItem::new(
                runner_character_label(app.language.resolve(), character),
                true,
                !app.random_runner.enabled() && app.runner_character == character,
                None,
            );
            let _ = character_submenu.append(&item);
            (character, item)
        })
        .collect::<Vec<_>>();

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
    #[cfg(not(target_os = "macos"))]
    let _ = menu.append(&display_submenu);
    #[cfg(target_os = "macos")]
    let menu_items = native_cpu_widget::MenuItemsMenu::new(
        app.language.resolve(),
        menu_items_app::visibility(app),
    );
    #[cfg(target_os = "macos")]
    let _ = menu.append(menu_items.submenu());
    let _ = menu.append(&character_submenu);
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
        #[cfg(target_os = "macos")]
        menu_items,
        character_items,
        random_character_item,
        temperature_source_items,
        fan_speed_items,
        #[cfg(target_os = "macos")]
        enable_fan_control: enable_fan_control_item.id().clone(),
        check_updates: check_updates_item.id().clone(),
        open_detail: open_detail_item.id().clone(),
        open_diagnostics: open_diagnostics_item.id().clone(),
        language_items,
    };

    let initial_runner_icon = (app.primary_visible && runner_enabled(app.display))
        .then(|| runner_icon_index(app.runner_cpu_pct, app.runner_frame));
    let initial_icon =
        initial_runner_icon.and_then(|index| app.runner_assets.icons.get(index).cloned());
    match TrayIcon::new(tray_attributes(initial_icon, Box::new(menu))) {
        Ok(tray) => {
            #[cfg(target_os = "macos")]
            configure_native_status_item(&tray, app.display);
            app.tray = Some(tray);
            app.tray_menu = Some(tray_menu);
            app.last_runner_icon = initial_runner_icon;
            #[cfg(target_os = "macos")]
            app.runner_native_applied_icon
                .store(initial_runner_icon.unwrap_or(usize::MAX), Ordering::Relaxed);
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
        .with_html(dashboard_html_with_runner_pack(
            app.language.resolve(),
            false,
            &app.runner_assets.pack,
        ))
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
            } else if let Some(error) = body.strip_prefix("js-error:") {
                log_menubar_event(&format!("popover webview javascript error: {error}"));
            } else if body == "ready" {
                enqueue_pending("ready:popover");
            } else if body == "refresh" {
                CONTROL_REFRESH_REQUESTED.store(true, Ordering::Release);
            } else if body == "copydiagnostics" {
                copy_support_report();
            } else if body == "copyconfig" {
                copy_config_export();
            } else if body == "checkupdates"
                || body == "installupdate"
                || body == "toggle-login-item"
                || body == "togglelogin"
                || body.starts_with("updatechannel:")
                || is_native_preference_command(body)
                || body.starts_with("rules:")
                || body.starts_with("importconfig:")
                || body.starts_with("notifications:")
                || is_native_process_command(body)
            {
                enqueue_pending(body);
            } else if body.starts_with("h:") {
                // Kept for compatibility with older dashboard HTML. The
                // popover is intentionally fixed-height now; content scrolls
                // inside `.main-pane` instead of resizing the native window.
            } else if let Some(cmd) = body.strip_prefix("cmd:") {
                enqueue_pending(cmd);
            } else if body.starts_with("savecurve:") {
                // Keep the prefix so the drain loop can tell these apart
                // from a daemon control command.
                enqueue_pending(body);
            } else if let Some(r) = body.strip_prefix("range:") {
                let v = match r {
                    "1h" => 1,
                    "1d" => 2,
                    _ => 0,
                };
                CHART_RANGE.store(v, Ordering::Relaxed);
            } else if let Some(s) = body.strip_prefix("procsort:") {
                PROC_SORT.store(if s == "mem" { 1 } else { 0 }, Ordering::Relaxed);
            } else if let Some(view) = body.strip_prefix("view:") {
                log_menubar_event(&format!("popover view requested view={view}"));
                ACTIVE_RAIL_VIEW.store(
                    match view {
                        "fan" => 1,
                        "settings" => 2,
                        "system" | "more" => 3,
                        _ => 0,
                    },
                    Ordering::Relaxed,
                );
            } else if let Some(open) = body.strip_prefix("rawtemps:") {
                RAW_TEMPS_OPEN.store(open == "1", Ordering::Relaxed);
            }
            let _ = ipc_proxy.send_event(());
        })
        .build(&window)
    {
        Ok(webview) => {
            app.window = Some(window);
            app.webview = Some(webview);
            app.webview_ready = false;
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

#[derive(Clone, Copy, Debug)]
struct WindowGeometry {
    #[cfg(target_os = "macos")]
    frame: NSRect,
    #[cfg(not(target_os = "macos"))]
    position: PhysicalPosition<i32>,
    #[cfg(not(target_os = "macos"))]
    size: PhysicalSize<u32>,
}

fn capture_window_geometry(window: &Window) -> Option<WindowGeometry> {
    #[cfg(target_os = "macos")]
    {
        // The Tao position is not reliable across mixed-DPI macOS displays;
        // retain the actual AppKit frame so a WebView rebuild cannot move the
        // popover to the window manager's default (usually screen center).
        let raw = window.ns_window().cast::<NSWindow>();
        if raw.is_null() {
            return None;
        }
        let native_window = unsafe { &*raw };
        Some(WindowGeometry {
            frame: native_window.frame(),
        })
    }

    #[cfg(not(target_os = "macos"))]
    {
        Some(WindowGeometry {
            position: window.outer_position().ok()?,
            size: window.inner_size(),
        })
    }
}

fn restore_window_geometry(window: &Window, geometry: WindowGeometry) {
    #[cfg(target_os = "macos")]
    {
        let raw = window.ns_window().cast::<NSWindow>();
        if raw.is_null() {
            return;
        }
        let native_window = unsafe { &*raw };
        native_window.setContentSize(NSSize::new(
            geometry.frame.size.width,
            geometry.frame.size.height,
        ));
        native_window.setFrameTopLeftPoint(NSPoint::new(
            geometry.frame.origin.x,
            geometry.frame.origin.y + geometry.frame.size.height,
        ));
    }

    #[cfg(not(target_os = "macos"))]
    {
        let scale = window.scale_factor();
        window.set_inner_size(LogicalSize::new(
            geometry.size.width as f64 / scale,
            geometry.size.height as f64 / scale,
        ));
        window.set_outer_position(geometry.position);
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
        .with_html(dashboard_html_with_runner_pack(
            app.language.resolve(),
            true,
            &app.runner_assets.pack,
        ))
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
            } else if let Some(error) = body.strip_prefix("js-error:") {
                log_menubar_event(&format!("detail webview javascript error: {error}"));
            } else if body == "ready" {
                enqueue_pending("ready:detail");
            } else if body == "refresh" {
                CONTROL_REFRESH_REQUESTED.store(true, Ordering::Release);
            } else if body == "copydiagnostics" {
                copy_support_report();
            } else if body == "copyconfig" {
                copy_config_export();
            } else if body == "checkupdates"
                || body == "installupdate"
                || body == "toggle-login-item"
                || body == "togglelogin"
                || body.starts_with("updatechannel:")
                || is_native_preference_command(body)
                || body.starts_with("rules:")
                || body.starts_with("importconfig:")
                || body.starts_with("notifications:")
                || is_native_process_command(body)
            {
                enqueue_pending(body);
            } else if let Some(cmd) = body.strip_prefix("cmd:") {
                enqueue_pending(cmd);
            } else if body.starts_with("savecurve:") {
                enqueue_pending(body);
            } else if let Some(r) = body.strip_prefix("range:") {
                let v = match r {
                    "1h" => 1,
                    "1d" => 2,
                    _ => 0,
                };
                CHART_RANGE.store(v, Ordering::Relaxed);
            } else if let Some(s) = body.strip_prefix("procsort:") {
                PROC_SORT.store(if s == "mem" { 1 } else { 0 }, Ordering::Relaxed);
            } else if let Some(view) = body.strip_prefix("view:") {
                log_menubar_event(&format!("detail view requested view={view}"));
                ACTIVE_RAIL_VIEW.store(
                    match view {
                        "fan" => 1,
                        "settings" => 2,
                        "system" | "more" => 3,
                        _ => 0,
                    },
                    Ordering::Relaxed,
                );
            } else if let Some(open) = body.strip_prefix("rawtemps:") {
                RAW_TEMPS_OPEN.store(open == "1", Ordering::Relaxed);
            }
            let _ = ipc_proxy.send_event(());
        })
        .build(&window)
    {
        Ok(webview) => {
            window.set_visible(true);
            app.detail_window = Some(window);
            app.detail_webview = Some(webview);
            app.detail_webview_ready = false;
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

#[cfg(any(target_os = "macos", test))]
#[derive(Clone, Copy, Debug, PartialEq)]
struct LogicalPopoverAnchor {
    x: f64,
    y: f64,
    display: DisplayBounds,
    scale: f64,
}

#[cfg(any(target_os = "macos", test))]
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

#[cfg(any(target_os = "macos", test))]
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
    let (x, y) = appkit_window_top_left(position, primary_top);
    native_window.setContentSize(NSSize::new(width, height));
    native_window.setFrameTopLeftPoint(NSPoint::new(x, y));
    Some(native_window.frame())
}

#[cfg(any(target_os = "macos", test))]
fn appkit_window_top_left(position: LogicalPosition<f64>, primary_top: f64) -> (f64, f64) {
    (position.x, primary_top - position.y)
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

#[cfg(target_os = "macos")]
fn native_tray_left_press(tray: &TrayIcon) -> bool {
    if NSEvent::pressedMouseButtons() & 1 == 0 {
        return false;
    }
    let mtm = unsafe { MainThreadMarker::new_unchecked() };
    let Some(window) = tray
        .ns_status_item()
        .and_then(|item| item.button(mtm))
        .and_then(|button| button.window())
    else {
        return false;
    };
    let frame = window.frame();
    let point = NSEvent::mouseLocation();
    point.x >= frame.origin.x
        && point.x < frame.origin.x + frame.size.width
        && point.y >= frame.origin.y
        && point.y < frame.origin.y + frame.size.height
}

fn clicked_tray_anchor(
    id: &str,
    runner: Option<(&str, Option<Rect>)>,
    cpu: Option<(&str, Option<Rect>)>,
    event_rect: Rect,
) -> Option<Rect> {
    let valid = |rect: &Rect| {
        rect.size.width > 0
            && rect.size.height > 0
            && rect.position.x.is_finite()
            && rect.position.y.is_finite()
    };
    runner
        .into_iter()
        .chain(cpu)
        .find(|(candidate, _)| *candidate == id)
        .and_then(|(_, live)| {
            live.filter(valid)
                .or_else(|| valid(&event_rect).then_some(event_rect))
        })
}

fn consume_tray_focus_dismissal(dismissed_at: &mut Option<Instant>, now: Instant) -> bool {
    // Bound stale state if AppKit cancels the original mouse gesture entirely.
    dismissed_at
        .take()
        .is_some_and(|at| now.saturating_duration_since(at) <= Duration::from_millis(500))
}

fn tray_press_closes_popover(
    owner: Option<&str>,
    clicked: &str,
    open: bool,
    dismissed: bool,
) -> bool {
    owner == Some(clicked) && (open || dismissed)
}

fn toggle_popover(
    app: &mut App,
    target: &EventLoopWindowTarget<()>,
    tray_id: &str,
    rect: Rect,
    event_proxy: &EventLoopProxy<()>,
) {
    let closed_by_this_press =
        consume_tray_focus_dismissal(&mut app.popover_tray_focus_loss_at, Instant::now());
    if tray_press_closes_popover(
        app.popover_anchor_tray.as_deref(),
        tray_id,
        app.popover_visible || app.popover_show_at.is_some(),
        closed_by_this_press,
    ) {
        log_menubar_event("popover closed by tray click");
        hide_popover(app);
        return;
    }
    hide_popover(app);
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
    app.popover_anchor_tray = Some(tray_id.to_string());
}

fn hide_popover(app: &mut App) {
    app.popover_show_at = None;
    app.popover_tray_focus_loss_at = None;
    app.popover_anchor_tray = None;
    if let Some(wv) = &app.webview {
        evaluate_dashboard_script(
            wv,
            "window.setRunnerPreviewVisibility&&window.setRunnerPreviewVisibility(false);",
            "popover",
        );
    }
    if let Some(w) = &app.window {
        w.set_visible(false);
    }
    app.popover_visible = false;
}

fn defer_dashboard_io_after_open(app: &mut App) {
    let now = Instant::now();
    app.next_temperature_refresh = now;
    app.next_fan_refresh = now;
    app.next_daemon_refresh = now;
    app.next_dashboard_slow_refresh = now + DASHBOARD_SLOW_OPEN_GRACE;
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

fn dashboard_process_limit(filter: &str) -> usize {
    if filter.is_empty() {
        5
    } else {
        20
    }
}

fn dashboard_process_row(process: &ProcessInfo) -> serde_json::Value {
    let can_quit = cfg!(target_os = "macos")
        && process.owned_by_current_user == Some(true)
        && process.start_time_unix_secs.is_some()
        && !peterfan_platform::process_control::is_protected_process(process.pid, &process.name);
    serde_json::json!({
        "name": process.name,
        "cpu": format!("{:.1}%", process.cpu_percent),
        "mem": bytes(process.memory),
        "pid": process.pid,
        "start_time": process.start_time_unix_secs,
        "can_quit": can_quit,
    })
}

fn process_action_error_code(
    error: &peterfan_platform::process_control::ProcessQuitError,
) -> &'static str {
    use peterfan_platform::process_control::ProcessQuitError;

    match error {
        ProcessQuitError::InvalidPid => "invalid_pid",
        ProcessQuitError::NotFound => "not_found",
        ProcessQuitError::IdentityChanged => "identity_changed",
        ProcessQuitError::OwnershipUnknown | ProcessQuitError::NotOwned => "not_owned",
        ProcessQuitError::Protected => "protected",
        ProcessQuitError::Unsupported => "unsupported",
        ProcessQuitError::SignalFailed(_) => "signal_failed",
    }
}

fn completed_process_action(
    revision: u64,
    request: peterfan_platform::process_control::ProcessQuitRequest,
    result: Result<String, peterfan_platform::process_control::ProcessQuitError>,
) -> ProcessActionState {
    match result {
        Ok(name) => ProcessActionState {
            revision,
            phase: "complete",
            pid: Some(request.pid),
            name: Some(name),
            code: Some("requested"),
            message: Some("Termination requested".to_string()),
        },
        Err(error) => ProcessActionState {
            revision,
            phase: "complete",
            pid: Some(request.pid),
            name: None,
            code: Some(process_action_error_code(&error)),
            message: Some(error.to_string()),
        },
    }
}

fn process_action_log_entry(state: &ProcessActionState) -> String {
    let name: String = state
        .name
        .as_deref()
        .unwrap_or("unknown")
        .chars()
        .filter(|character| !character.is_control())
        .take(80)
        .collect();
    format!(
        "process termination completed pid={} name={} code={}",
        state.pid.unwrap_or(0),
        name,
        state.code.unwrap_or("unknown")
    )
}

fn refresh_dashboard_slow_cache(app: &mut App, proc_sort: ProcSort) {
    let config = peterfan_platform::config::load();
    let disks = app.monitor.disks();
    let disk = primary_disk(&disks);
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

    let process_limit = dashboard_process_limit(&app.process_filter);
    let procs: Vec<_> = app
        .monitor
        .processes_matching(process_limit, proc_sort, &app.process_filter)
        .iter()
        .map(dashboard_process_row)
        .collect();

    // Discovery can transiently fail at launch after sleep or during a Windows
    // power-state transition. Retry with each slow dashboard sample.
    let battery = app.monitor.battery();
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

    let curve_points = config
        .custom_curve
        .as_ref()
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
        sampled_at: Some(Instant::now()),
        proc_sort,
        proc_filter: app.process_filter.clone(),
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

fn temperature_refresh_interval(dashboard_visible: bool) -> Duration {
    if dashboard_visible {
        TEMPERATURE_REFRESH_VISIBLE
    } else {
        TEMPERATURE_REFRESH_BACKGROUND
    }
}

fn fan_refresh_interval(dashboard_visible: bool) -> Duration {
    if dashboard_visible {
        FAN_REFRESH_VISIBLE
    } else {
        FAN_REFRESH_BACKGROUND
    }
}

fn refresh_temperature_cache(app: &mut App, now: Instant, dashboard_visible: bool) {
    if let Some(sample) = app.temperature_read.take() {
        if !sample.values.is_empty() {
            app.temperature_cache = sample.values;
            app.temperature_sampled_at = Some(sample.sampled_at);
            app.temperature_sampled_at_unix_ms = Some(sample.sampled_at_unix_ms);
        }
    }
    if app.temperature_cache.is_empty() || now >= app.next_temperature_refresh {
        app.next_temperature_refresh = now + temperature_refresh_interval(dashboard_visible);
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

fn merge_fan_sample(cache: &mut Vec<Fan>, empty_samples: &mut u8, fans: Vec<Fan>) {
    if fans.is_empty() && !cache.is_empty() {
        *empty_samples = empty_samples.saturating_add(1);
        if *empty_samples >= FAN_EMPTY_CONFIRMATIONS {
            cache.clear();
        }
    } else {
        *cache = fans;
        *empty_samples = 0;
    }
}

fn refresh_fan_cache(app: &mut App, now: Instant, dashboard_visible: bool) {
    if let Some(fans) = app.fan_read.take() {
        app.fan_sampled_at = Some(now);
        merge_fan_sample(&mut app.fan_cache, &mut app.fan_empty_samples, fans);
    }
    if now >= app.next_fan_refresh {
        app.next_fan_refresh = now + fan_refresh_interval(dashboard_visible);
        let provider = Arc::clone(&app.provider);
        app.fan_read.start(move || provider.fans().ok());
    }
}

fn refresh_daemon_cache(app: &mut App, now: Instant) {
    if let Some(snapshot) = app.daemon_read.take() {
        app.daemon_probe_completed = true;
        if let Some(snapshot) = snapshot {
            app.daemon_json_cache = Some(snapshot);
            app.daemon_json_sampled_at = Some(now);
        } else if app
            .daemon_json_sampled_at
            .is_some_and(|sampled_at| now.duration_since(sampled_at) > DAEMON_STALE_AFTER)
        {
            app.daemon_json_cache = None;
            app.daemon_json_sampled_at = None;
        }
    }
    if now >= app.next_daemon_refresh {
        app.next_daemon_refresh = now + DAEMON_REFRESH;
        app.daemon_read.start(|| Some(daemon_temps_json()));
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
                    "group": sensor_group_label_for_sensor(app.language.resolve(), temp),
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

fn evaluate_dashboard_script(webview: &WebView, script: &str, target: &str) {
    if let Err(error) = webview.evaluate_script(script) {
        log_menubar_event(&format!(
            "{target} dashboard payload evaluation failed: {error:?}"
        ));
    }
}

fn dashboard_render_requested(
    popover_visible: bool,
    popover_scheduled: bool,
    detail_visible: bool,
) -> bool {
    popover_visible || popover_scheduled || detail_visible
}

fn update(app: &mut App) {
    let now = Instant::now();
    // Re-read the small user config so CLI edits and the detail-window rule
    // editor converge without an app restart. The privileged daemon still
    // validates the same file before applying rules.
    let config = peterfan_platform::config::load();
    app.update_channel = config.menubar.update_channel;
    menu_items_app::reload(app, &config.menubar);
    app.monitor.refresh_quick();
    let detail_visible = app.detail_window.as_ref().is_some_and(Window::is_visible);
    let popover_scheduled = app.popover_show_at.is_some();
    let popover_render_requested = app.popover_visible || popover_scheduled;
    let dashboard_visible =
        dashboard_render_requested(app.popover_visible, popover_scheduled, detail_visible);
    let active_view = ACTIVE_RAIL_VIEW.load(Ordering::Relaxed);
    let overview_visible = dashboard_visible && active_view == 0;
    let settings_visible = dashboard_visible && active_view == 2;
    let system_visible = dashboard_visible && active_view == 3;
    let proc_sort = if PROC_SORT.load(Ordering::Relaxed) == 1 {
        ProcSort::Memory
    } else {
        ProcSort::Cpu
    };
    let refresh_slow_metrics = (settings_visible || system_visible)
        && (now >= app.next_dashboard_slow_refresh
            || app.dashboard_slow_cache.proc_sort != proc_sort
            || app.dashboard_slow_cache.proc_filter != app.process_filter);
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
    refresh_temperature_cache(app, now, dashboard_visible);
    refresh_fan_cache(app, now, dashboard_visible);
    refresh_daemon_cache(app, now);
    let temperature_stale =
        sample_is_stale(app.temperature_sampled_at, now, TEMPERATURE_STALE_AFTER);
    let temperature_age_secs = sample_age(app.temperature_sampled_at, now)
        .map(|age| age.as_secs())
        .unwrap_or(0);
    let temps = app.temperature_cache.clone();
    let fans = app.fan_cache.clone();
    let fan_data_stale = sample_is_stale(app.fan_sampled_at, now, FAN_STALE_AFTER);
    let fan_data_ready = !fan_data_stale;
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
    cpu_widget_app::refresh(app, cpu.usage_percent, &cpu.per_core);
    if app.per_core_h.len() != cpu.per_core.len() {
        app.per_core_h = (0..cpu.per_core.len())
            .map(|_| RangedHistory::new())
            .collect();
    }
    for (history, usage) in app.per_core_h.iter_mut().zip(&cpu.per_core) {
        history.push(*usage);
    }
    app.mem_h.push(mem.used_percent);
    if let Some(temp) = display_temp {
        app.temp_h.push(temp);
    }
    app.net_h.push((rx + tx) as f32);
    app.runner_cpu_pct =
        smooth_runner_cpu(app.runner_cpu_pct, cpu.usage_percent, app.runner_has_sample);
    app.runner_has_sample = true;

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
        let menubar_text = if show_number { &title } else { "" };
        if app.last_menubar_text.as_deref() != Some(menubar_text) {
            set_menubar_text(tray, menubar_text);
            app.last_menubar_text = Some(menubar_text.to_string());
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
            "CPU {:.0}%   {mem_label} {:.0}%",
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
        let tooltip = tip_parts.join("   ·   ");
        if app.last_menubar_tooltip.as_deref() != Some(tooltip.as_str())
            && tray.set_tooltip(Some(&tooltip)).is_ok()
        {
            app.last_menubar_tooltip = Some(tooltip);
        }
    }

    let notification_health = app
        .daemon_json_cache
        .as_ref()
        .and_then(|value| value.get("control_health").cloned())
        .unwrap_or_else(|| serde_json::json!({}));
    for notice in evaluate_notification_rules(
        &app.notifications,
        &mut app.notification_runtime,
        display_temp,
        &notification_health,
    ) {
        log_menubar_event(&format!("native notification: {}", notice.title));
        post_native_notification(notice.title, notice.body);
    }

    if !popover_render_requested && !detail_visible {
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
    let system_info = app.monitor.system_info();
    let ghz = cpu.frequency_mhz as f64 / 1000.0;
    let load_str = cpu
        .load_avg
        .map(|l| format!("load {:.2} {:.2} {:.2}", l.one, l.five, l.fifteen))
        .unwrap_or_default();
    let load_avg_text = cpu
        .load_avg
        .map(|l| format!("{:.2} · {:.2} · {:.2}", l.one, l.five, l.fifteen))
        .unwrap_or_else(|| "—".to_string());
    let power_text = app
        .dashboard_slow_cache
        .power_w
        .map(|watts| format!("{watts:.1} W"))
        .unwrap_or_else(|| "—".to_string());
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
    let raw_temps_visible = overview_visible && RAW_TEMPS_OPEN.load(Ordering::Relaxed);
    refresh_all_temperature_cache(app, now, raw_temps_visible);
    let all_temp_stale = sample_is_stale(app.all_temp_sampled_at, now, ALL_TEMPERATURE_STALE_AFTER);
    let all_temp_age_secs = sample_age(app.all_temp_sampled_at, now)
        .map(|age| age.as_secs())
        .unwrap_or(0);
    let all_temp_rows: Vec<_> = if raw_temps_visible {
        app.all_temp_rows_cache
            .iter()
            .cloned()
            .map(|mut row| {
                row["sampled_at_unix_ms"] = serde_json::json!(app.all_temp_sampled_at_unix_ms);
                row["age_secs"] = serde_json::json!(all_temp_age_secs);
                row["stale"] = serde_json::json!(all_temp_stale);
                row
            })
            .collect()
    } else {
        Vec::new()
    };

    // Fans and daemon state are prefetched continuously on background threads,
    // so opening this view only renders cached values and never waits on SMC
    // or a Unix socket.
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
    let resident_version = daemon_json
        .as_ref()
        .and_then(|value| value.get("daemon_version"))
        .and_then(serde_json::Value::as_str);
    let daemon_usable =
        daemon_running && daemon_control_usable(daemon_version.as_deref(), resident_version);
    let daemon_update_needed = app.provider.name() != "mock" && daemon_running && !daemon_usable;
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

    let fan_control_supported = app.provider.capabilities().control_fans || daemon_usable;
    let can_control = fan_data_ready
        && fan_control_access(
            app.provider.capabilities().control_fans,
            daemon_usable,
            direct_fan_control_allowed(),
            app.provider.name() == "mock",
        );
    let ctl_status = if daemon_update_needed {
        match app.language.resolve() {
            ResolvedLanguage::Ko => "팬 제어 재설치 필요".to_string(),
            ResolvedLanguage::En => "reinstall fan control".to_string(),
        }
    } else if daemon_usable {
        daemon_st.clone()
    } else if fan_control_supported && !can_control {
        match app.language.resolve() {
            ResolvedLanguage::Ko => "팬 제어 설정 필요".to_string(),
            ResolvedLanguage::En => "fan control setup required".to_string(),
        }
    } else {
        STATUS.lock().expect("status poisoned").clone()
    };
    let chart_range = ChartRange::from_u8(CHART_RANGE.load(Ordering::Relaxed));
    let (daemon_binary_installed, daemon_path, launch_daemon_installed) = daemon_install_metadata();
    let app_update_status = app_update_state_snapshot();
    let fan_rpm_values: Vec<u32> = fans
        .iter()
        .map(|fan| fan.rpm)
        .filter(|rpm| *rpm > 0)
        .collect();
    let fan_avg_rpm = if fan_rpm_values.is_empty() {
        0
    } else {
        fan_rpm_values.iter().sum::<u32>() / fan_rpm_values.len() as u32
    };
    let fan_avg_rpm_text = if fan_data_stale {
        match app.language.resolve() {
            ResolvedLanguage::Ko => "읽는 중…".to_string(),
            ResolvedLanguage::En => "Reading…".to_string(),
        }
    } else if fan_avg_rpm > 0 {
        format!("{fan_avg_rpm} RPM")
    } else if fans.is_empty() {
        match app.language.resolve() {
            ResolvedLanguage::Ko => "팬 없음".to_string(),
            ResolvedLanguage::En => "No fans".to_string(),
        }
    } else {
        match app.language.resolve() {
            ResolvedLanguage::Ko => "읽는 중…".to_string(),
            ResolvedLanguage::En => "Reading…".to_string(),
        }
    };
    let cpu_core_groups = dashboard_cpu_core_groups(&cpu.per_core);
    let core_hist: Vec<Vec<f32>> = app
        .per_core_h
        .iter()
        .map(|history| to_vec(history.range(ChartRange::TwoMin)))
        .collect();

    let mut payload = serde_json::json!({
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
        "core_groups": cpu_core_groups,
        "core_hist": core_hist,
        "mem_pct": mem.used_percent,
        "mem_text": format!("{:.1}%", mem.used_percent),
        "mem_sub": format!(
            "{} / {}   swap {} / {}",
            bytes(mem.used), bytes(mem.total), bytes(mem.swap_used), bytes(mem.swap_total)
        ),
        "disk_pct": app.dashboard_slow_cache.disk_pct,
        "slow_data_ready": app.dashboard_slow_cache.sampled_at.is_some(),
        "disk_text": &app.dashboard_slow_cache.disk_text,
        "disk_sub": &app.dashboard_slow_cache.disk_sub,
        "disk_io_present": app.dashboard_slow_cache.disk_io_present,
        "disk_io_sub": &app.dashboard_slow_cache.disk_io_sub,
        "procs": &app.dashboard_slow_cache.procs,
        "proc_sort": if matches!(proc_sort, ProcSort::Memory) { "mem" } else { "cpu" },
        "process_filter": &app.process_filter,
        "process_action": process_action_snapshot(),
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
        "fan_avg_rpm": fan_avg_rpm,
        "fan_avg_rpm_text": fan_avg_rpm_text,
        "fan_data_stale": fan_data_stale,
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
        "fan_control_supported": fan_control_supported,
        "ctl_status": ctl_status,
        "daemon_running": !daemon_st.is_empty(),
        "daemon_version": daemon_version.clone(),
        "daemon_runtime_version": resident_version,
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
        "fan_setup_needed": fan_control_supported && !can_control,
        "fan_control_installing": INSTALL_FAN_CONTROL_IN_FLIGHT.load(Ordering::Acquire),
        "fan_control_install_progress": fan_control_install_progress_snapshot(),
        "fan_control_install_revision": INSTALL_FAN_CONTROL_REVISION.load(Ordering::Acquire),
        "fan_control_state_ready": !fan_control_supported || (fan_data_ready && app.daemon_probe_completed),
        "fan_count": fans.len(),
        "controllable_fan_count": fans.iter().filter(|f| f.controllable).count(),
        "fan_curve_input_temp_c": display_temp,
        "fan_core_hottest_temp_c": core_hottest,
        "fan_safety_temp_c": safety_temp,
        "fan_critical_temp_c": app.critical_temp_c,
        "control_health": control_health,
        "fan_action_log": fan_action_log_snapshot(),
        "notifications": {
            "temperature_c": app.notifications.temperature_c,
            "fan_failures": app.notifications.fan_failures,
            "updates": app.notifications.updates,
        },
        "rules": &config.rules,
        "app_version": env!("CARGO_PKG_VERSION"),
        "update_channel": app.update_channel.as_str(),
        "show_cpu_core_graph": app.show_cpu_core_graph,
        "app_update_status": app_update_status,
        "update_install_result": &app.update_install_result,
        "setup_tone": setup_tone(!daemon_st.is_empty(), daemon_update_needed),
        "setup_title": setup_title(app.language.resolve(), !daemon_st.is_empty(), daemon_update_needed),
        "setup_detail": setup_detail(app.language.resolve(), !daemon_st.is_empty(), daemon_update_needed, daemon_version.as_deref()),
        "curve_points": &app.dashboard_slow_cache.curve_points,
        "last_cmd_status": STATUS.lock().expect("status poisoned").clone(),
        "settings_results": SETTINGS_RESULTS.lock().expect("settings results poisoned").clone(),
    });
    payload["config_toml"] = serde_json::json!(config.to_toml());
    payload["menu_items"] = serde_json::json!(menu_items_app::visibility(app));
    payload["menu_items_supported"] = serde_json::json!(cfg!(target_os = "macos"));
    payload["cpu_widget"] = serde_json::json!(app.cpu_widget_config);
    payload["cpu_widget_supported"] = serde_json::json!(cfg!(target_os = "macos"));
    payload["cpu_widget_error"] = serde_json::json!(app.cpu_widget_error);
    payload["cpu_widget_history"] = serde_json::json!(app.cpu_widget_history.values());
    payload["cpu_widget_cores"] = serde_json::json!(app.cpu_widget_history.core_values());
    payload["menubar_display"] = serde_json::json!(app.display.as_str());
    payload["runner_character"] = serde_json::json!(app.runner_character.as_str());
    payload["runner_random"] = serde_json::json!(app.random_runner.enabled());
    payload["runner_selected_preview"] =
        serde_json::json!(serde_json::from_str::<serde_json::Value>(
            &app.runner_assets.selected_preview_frames_json()
        )
        .expect("runner preview JSON is serialized from canonical frames"));
    payload["runner_selected_preview_character"] =
        serde_json::json!(app.runner_assets.pack.character().as_str());
    payload["runner_cpu_pct"] = serde_json::json!(app.runner_cpu_pct);
    payload["runner_reduce_motion"] = serde_json::json!(app.reduce_motion);
    payload["runner_interval_ms"] = serde_json::json!((!app.reduce_motion).then(|| {
        runner_character_interval(app.runner_character, app.runner_cpu_pct).as_millis()
    }));
    payload["network_rate_text"] = serde_json::json!(format!("{}/s", bytes((rx + tx) as u64)));
    payload["load_avg_text"] = serde_json::json!(load_avg_text);
    payload["power_text"] = serde_json::json!(power_text);
    payload["uptime_text"] = serde_json::json!(format_uptime(system_info.uptime_secs));
    payload["logical_cores"] = serde_json::json!(system_info.logical_cores);
    let support_report = support_report_json(SupportReportInput {
        cpu_pct: cpu.usage_percent,
        logical_cores: system_info.logical_cores,
        selected_average_c: display_temp,
        core_hottest_c: core_hottest,
        safety_hottest_c: safety_temp,
        temperatures: &temps,
        temperature_sampled_at_unix_ms: app.temperature_sampled_at_unix_ms,
        temperature_stale,
        fans: &fan_rows,
        daemon_version: daemon_version.as_deref(),
        daemon: daemon_json.as_ref(),
    });
    if let Ok(report) = serde_json::to_string_pretty(&support_report) {
        *SUPPORT_REPORT.lock().expect("support report poisoned") = report;
    }
    payload["support_report_copy_revision"] =
        serde_json::json!(SUPPORT_REPORT_COPY_REVISION.load(Ordering::Acquire));
    payload["support_report_copy_ok"] =
        serde_json::json!(SUPPORT_REPORT_COPY_OK.load(Ordering::Acquire));
    payload["config_copy_revision"] =
        serde_json::json!(CONFIG_COPY_REVISION.load(Ordering::Acquire));
    payload["config_copy_ok"] = serde_json::json!(CONFIG_COPY_OK.load(Ordering::Acquire));
    let script = format!(
        "window.__pf_pending={payload};window.__pf&&window.__pf.update(window.__pf_pending)"
    );
    app.dashboard_script = Some(script);
    if popover_scheduled && !app.popover_visible {
        log_menubar_event("popover payload prepared before show");
    }
    if popover_render_requested && app.webview_ready {
        if let (Some(wv), Some(script)) = (app.webview.as_ref(), app.dashboard_script.as_deref()) {
            evaluate_dashboard_script(wv, script, "popover");
        }
    }
    if detail_visible && app.detail_webview_ready {
        if let (Some(wv), Some(script)) =
            (app.detail_webview.as_ref(), app.dashboard_script.as_deref())
        {
            evaluate_dashboard_script(wv, script, "detail");
        }
    }
}

fn animate_runner(app: &mut App) {
    if !(app.primary_visible && runner_enabled(app.display)) {
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
    if provider.name() == "mock" {
        return apply_local(provider, cmd);
    }
    #[cfg(unix)]
    if !daemon_control_usable(
        cached_installed_daemon_version().as_deref(),
        peterfan_platform::running_daemon_version().as_deref(),
    ) {
        if direct_fan_control_allowed() {
            return apply_local(provider, cmd);
        }
        return "error: fan control requires a verified current daemon; reinstall fan control"
            .into();
    }
    #[cfg(unix)]
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
            return format!(
                "daemon: {}",
                if reply.is_empty() {
                    "error: no acknowledgement"
                } else {
                    &reply
                }
            );
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

fn execute_control_serial(provider: &dyn HardwareProvider, cmd: &str) -> String {
    let _guard = FAN_COMMAND_LOCK.lock().expect("fan command lock poisoned");
    execute_control_logged(provider, cmd)
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
                let Some(temp) = representative_temperature_c(&temps) else {
                    return "profile not applied: no trustworthy temperature".into();
                };
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
    let Some(status_item) = tray.ns_status_item() else {
        return;
    };
    let mtm = unsafe { MainThreadMarker::new_unchecked() };
    if let Some(button) = status_item.button(mtm) {
        button.setTitle(&NSString::from_str(text));
    }
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

#[cfg(target_os = "windows")]
fn login_item_installed() -> bool {
    peterfan_platform::windows_login_item::is_installed()
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn login_item_installed() -> bool {
    false
}

#[cfg(target_os = "macos")]
fn login_item_supported() -> bool {
    true
}

#[cfg(target_os = "windows")]
fn login_item_supported() -> bool {
    true
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
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

    #[cfg(target_os = "windows")]
    {
        if LOGIN_ITEM_TOGGLE_IN_FLIGHT
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return "login item already updating".into();
        }

        let result = if peterfan_platform::windows_login_item::is_installed() {
            match peterfan_platform::windows_login_item::remove() {
                Ok(true) => "startup disabled".to_string(),
                Ok(false) => "startup already disabled".to_string(),
                Err(error) => format!("startup disable failed: {error}"),
            }
        } else {
            match peterfan_platform::windows_login_item::install(None) {
                Ok(_) => "startup enabled".to_string(),
                Err(error) => format!("startup enable failed: {error}"),
            }
        };

        LOGIN_ITEM_TOGGLE_IN_FLIGHT.store(false, Ordering::SeqCst);
        result
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        "startup toggle is not available on this platform".to_string()
    }
}

#[cfg(target_os = "macos")]
fn send_native_notification(title: &str, body: &str) {
    let script = format!(
        "display notification {} with title {}",
        applescript_quote(body),
        applescript_quote(title)
    );
    let _ = std::process::Command::new("/usr/bin/osascript")
        .arg("-e")
        .arg(script)
        .status();
}

#[cfg(target_os = "windows")]
fn send_native_notification(title: &str, body: &str) {
    let quote = |value: &str| value.replace('\'', "''");
    let script = format!(
        "[void][Windows.UI.Notifications.ToastNotificationManager, Windows.UI.Notifications, ContentType=WindowsRuntime];\
         $t=[Windows.UI.Notifications.ToastNotificationManager]::GetTemplateContent([Windows.UI.Notifications.ToastTemplateType]::ToastText02);\
         $t.SelectSingleNode('//text[@id=\"1\"]').InnerText='{}';\
         $t.SelectSingleNode('//text[@id=\"2\"]').InnerText='{}';\
         [Windows.UI.Notifications.ToastNotificationManager]::CreateToastNotifier('PeterFan').Show([Windows.UI.Notifications.ToastNotification]::new($t))",
        quote(title),
        quote(body)
    );
    let _ = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .status();
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn send_native_notification(_title: &str, _body: &str) {}

fn post_native_notification(title: impl Into<String>, body: impl Into<String>) {
    let title = title.into();
    let body = body.into();
    std::thread::spawn(move || send_native_notification(&title, &body));
}

/// Show a desktop notification for a control action triggered from the
/// right-click menu — those aren't visible in the popover unless it's open.
fn notify_control_result(action: &str, ok: bool, result: &str) {
    let title = if ok {
        "PeterFan"
    } else {
        "PeterFan — action needed"
    };
    post_native_notification(format!("{title} · {action}"), result.to_string());
}

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
#[cfg(any(target_os = "macos", test))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FanControlInstallPlan {
    AlreadyReady,
    SelfReinstall,
    PrivilegedInstall,
}

#[cfg(any(target_os = "macos", test))]
fn fan_control_install_plan(
    installed_version: Option<&str>,
    running_version: Option<&str>,
    daemon_reachable: bool,
) -> FanControlInstallPlan {
    match (installed_version, daemon_reachable) {
        (Some(version), true)
            if !peterfan_platform::daemon_update_required(version)
                && running_version == Some(version) =>
        {
            FanControlInstallPlan::AlreadyReady
        }
        (Some(version), true)
            if daemon_self_reinstall_is_secure(Some(version), running_version) =>
        {
            FanControlInstallPlan::SelfReinstall
        }
        _ => FanControlInstallPlan::PrivilegedInstall,
    }
}

#[cfg(target_os = "macos")]
fn reinstall_or_privileged_install<F, G>(
    self_reinstall: F,
    privileged_install: G,
) -> Result<peterfan_platform::daemon_install::InstallOutcome, String>
where
    F: FnOnce() -> Result<peterfan_platform::daemon_install::InstallOutcome, String>,
    G: FnOnce() -> Result<peterfan_platform::daemon_install::InstallOutcome, String>,
{
    match self_reinstall() {
        Ok(outcome) => Ok(outcome),
        Err(self_reinstall_error) => privileged_install().map_err(|fallback_error| {
            format!(
                "passwordless daemon reinstall failed: {self_reinstall_error}; administrator reinstall also failed: {fallback_error}"
            )
        }),
    }
}

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
    report_fan_control_install_stage(peterfan_platform::daemon_install::InstallStage::Preflight);
    let old_version = peterfan_platform::installed_daemon_version();
    let daemon_reachable = peterfan_platform::daemon_reachable();
    let resident_version = peterfan_platform::running_daemon_version();
    let plan = fan_control_install_plan(
        old_version.as_deref(),
        resident_version.as_deref(),
        daemon_reachable,
    );
    let updating_existing = old_version.is_some();
    clear_daemon_version_cache();
    let install_result = match plan {
        FanControlInstallPlan::AlreadyReady => {
            peterfan_platform::daemon_install::verify_existing_with_progress(
                report_fan_control_install_stage,
            )
            .map(|()| InstallOutcome::Installed)
        }
        FanControlInstallPlan::SelfReinstall => reinstall_or_privileged_install(
            || {
                peterfan_platform::daemon_install::reinstall_via_running_daemon_with_progress(
                    false,
                    report_fan_control_install_stage,
                )
            },
            || {
                peterfan_platform::daemon_install::install_with_progress(
                    false,
                    report_fan_control_install_stage,
                )
            },
        ),
        FanControlInstallPlan::PrivilegedInstall => {
            peterfan_platform::daemon_install::install_with_progress(
                false,
                report_fan_control_install_stage,
            )
        }
    };
    let (ok, message) = match install_result {
        Ok(InstallOutcome::Installed) if plan == FanControlInstallPlan::AlreadyReady => (
            true,
            format!(
                "Fan control is already enabled — daemon v{} is ready.",
                old_version.as_deref().unwrap_or("unknown")
            ),
        ),
        Ok(InstallOutcome::Installed) => {
            clear_daemon_version_cache();
            persist_clear_daemon_update_prompt_state();
            let installed_version = cached_installed_daemon_version()
                .unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string());
            if plan == FanControlInstallPlan::SelfReinstall {
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
    log_menubar_event(&format!(
        "fan control install plan={plan:?} ok={ok} result={message}"
    ));
    let (action_label, notification_title) = match plan {
        FanControlInstallPlan::AlreadyReady => ("check fan control", "Fan Control"),
        FanControlInstallPlan::SelfReinstall => ("reinstall fan control", "Reinstall Fan Control"),
        FanControlInstallPlan::PrivilegedInstall if updating_existing => {
            ("reinstall fan control", "Reinstall Fan Control")
        }
        FanControlInstallPlan::PrivilegedInstall => ("enable fan control", "Enable Fan Control"),
    };
    record_fan_action(action_label, &message, ok);
    *STATUS.lock().expect("status poisoned") = message.clone();
    report_fan_control_install_stage(if ok {
        peterfan_platform::daemon_install::InstallStage::Complete
    } else {
        peterfan_platform::daemon_install::InstallStage::Failed
    });
    INSTALL_FAN_CONTROL_IN_FLIGHT.store(false, Ordering::Release);
    INSTALL_FAN_CONTROL_REVISION.fetch_add(1, Ordering::AcqRel);
    CONTROL_REFRESH_REQUESTED.store(true, Ordering::Release);
    notify_control_result(notification_title, ok, &message);
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
    process_is_elevated()
}

fn fan_control_access(
    hardware_supported: bool,
    daemon_usable: bool,
    elevated: bool,
    mock: bool,
) -> bool {
    daemon_usable || (hardware_supported && (elevated || mock))
}

#[cfg(target_os = "macos")]
fn process_is_elevated() -> bool {
    // SAFETY: geteuid() is always safe and has no preconditions.
    unsafe { libc::geteuid() == 0 }
}

#[cfg(target_os = "macos")]
fn direct_fan_control_allowed() -> bool {
    process_is_elevated()
}

#[cfg(not(target_os = "macos"))]
fn direct_fan_control_allowed() -> bool {
    true
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
        preference_saved(
            "setup prompt",
            peterfan_platform::config::update(|cfg| {
                cfg.menubar.setup_prompt_dismissed = true;
                Ok(())
            }),
        );
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

#[cfg(any(target_os = "macos", test))]
fn daemon_can_self_update_silently(
    installed_version: &str,
    resident_version: Option<&str>,
) -> bool {
    peterfan_platform::daemon_update_required(installed_version)
        && daemon_self_reinstall_is_secure(Some(installed_version), resident_version)
}

#[cfg(target_os = "macos")]
fn maybe_silently_update_stale_daemon() {
    use peterfan_platform::daemon_install::InstallOutcome;

    let Some(old_version) = stale_daemon_version() else {
        return;
    };
    if !daemon_can_self_update_silently(
        &old_version,
        peterfan_platform::running_daemon_version().as_deref(),
    ) || INSTALL_FAN_CONTROL_IN_FLIGHT
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return;
    }

    log_menubar_event(&format!(
        "silently updating fan daemon v{old_version} -> v{}",
        peterfan_platform::MIN_REQUIRED_DAEMON_VERSION
    ));
    clear_daemon_version_cache();
    let result = peterfan_platform::daemon_install::reinstall_via_running_daemon_with_progress(
        false,
        report_fan_control_install_stage,
    );
    let (ok, message) = match result {
        Ok(InstallOutcome::Installed) => {
            clear_daemon_version_cache();
            persist_clear_daemon_update_prompt_state();
            let version = cached_installed_daemon_version()
                .unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string());
            (true, format!("Fan control updated silently to v{version}."))
        }
        Ok(InstallOutcome::InstalledButUnreachable) => (
            false,
            "Fan control updated, but the daemon is still restarting.".to_string(),
        ),
        Ok(InstallOutcome::DryRun(_)) => unreachable!("automatic update never uses dry-run"),
        Err(error) => (false, format!("Silent fan-control update failed: {error}")),
    };
    INSTALL_FAN_CONTROL_IN_FLIGHT.store(false, Ordering::SeqCst);
    INSTALL_FAN_CONTROL_REVISION.fetch_add(1, Ordering::AcqRel);
    record_fan_action("silent daemon update", &message, ok);
    *STATUS.lock().expect("status poisoned") = message.clone();
    CONTROL_REFRESH_REQUESTED.store(true, Ordering::Release);
    log_menubar_event(&message);
}

#[cfg(not(target_os = "macos"))]
fn maybe_silently_update_stale_daemon() {}

#[cfg(any(target_os = "macos", test))]
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
        preference_saved(
            "daemon update prompt",
            peterfan_platform::config::update(|cfg| {
                cfg.menubar.daemon_update_prompt_dismissed_for =
                    Some(peterfan_platform::MIN_REQUIRED_DAEMON_VERSION.to_string());
                Ok(())
            }),
        );
    } else if stdout.contains(not_now) {
        preference_saved(
            "daemon update prompt",
            peterfan_platform::config::update(|cfg| {
                cfg.menubar.daemon_update_prompt_snoozed_until_unix =
                    Some(now_unix() + 24 * 60 * 60);
                Ok(())
            }),
        );
    }
}
#[cfg(not(target_os = "macos"))]
fn maybe_prompt_stale_daemon_update() {}

/// Quiet recurring discovery. Wall-clock deadlines plus a bounded poll notice
/// overdue work on wake without issuing a burst of missed checks.
#[cfg(any(target_os = "macos", target_os = "windows"))]
fn check_for_updates_on_launch() {
    {
        let state = APP_UPDATE_STATE.lock().expect("app update state poisoned");
        log_menubar_event(&format!(
            "app update scheduler started channel={} next_check={:?} last_attempt={:?} last_success={:?} phase={} install_ready={}",
            state.channel.as_str(),
            state.schedule.next_check_unix,
            state.schedule.last_attempt_unix,
            state.schedule.last_success_unix,
            state.phase,
            state.install_ready,
        ));
    }
    loop {
        if QUIT.load(Ordering::Acquire) || !APP_UPDATE_NETWORK_ENABLED.load(Ordering::Acquire) {
            return;
        }
        if let Some(_lease) = UpdateLease::acquire(&APP_UPDATE_IN_FLIGHT) {
            reconcile_update_channel(
                &mut APP_UPDATE_STATE.lock().expect("app update state poisoned"),
                configured_update_channel(),
                now_unix(),
                true,
            );
        }
        let due = APP_UPDATE_STATE
            .lock()
            .expect("app update state poisoned")
            .schedule
            .due(now_unix());
        if due {
            run_app_update(false, true);
        }
        let delay = APP_UPDATE_STATE
            .lock()
            .expect("app update state poisoned")
            .schedule
            .poll_delay(now_unix());
        std::thread::sleep(delay);
    }
}
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn check_for_updates_on_launch() {}

fn initialize_update_discovery(
    state: &mut AppUpdateState,
    channel: UpdateChannel,
    now: u64,
    history: Option<update_history::UpdateHistory>,
) {
    state.channel = channel;
    state.schedule = UpdateSchedule::new();
    // Establish fresh discovery before restoring history: an interrupted attempt
    // has no deadline and must never prevent this launch's 20-second check.
    state.schedule.start(now);
    state.phase = "idle";
    state.latest = None;
    state.release_url = None;
    state.notes = None;
    state.message = None;
    state.install_ready = false;
    state.check_error = None;
    if let Some(history) = history.filter(|history| history.channel == channel) {
        state.schedule.last_attempt_unix = history.last_attempt_unix;
        state.schedule.last_success_unix = history.last_success_unix;
        state.check_error = history.check_error;
    }
}

fn update_history_path() -> Option<PathBuf> {
    peterfan_platform::updater::update_install_result_path()
        .map(|path| path.with_file_name("update-check-history.json"))
}

fn load_update_history(channel: UpdateChannel, now: u64) -> Option<update_history::UpdateHistory> {
    let path = update_history_path()?;
    match update_history::load(&path, channel, now, true) {
        Ok(history) => history,
        Err(error) => {
            log_menubar_event(&format!("app update history read failed: {error}"));
            None
        }
    }
}

fn capture_update_history(state: &AppUpdateState) -> update_history::UpdateHistory {
    update_history::UpdateHistory::capture(
        state.channel,
        state.schedule.last_attempt_unix,
        state.schedule.last_success_unix,
        state.check_error.clone(),
    )
}

fn persist_update_history(history: &update_history::UpdateHistory) {
    if !APP_UPDATE_NETWORK_ENABLED.load(Ordering::Acquire) {
        return;
    }
    let result = update_history_path()
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "missing update history directory",
            )
        })
        .and_then(|path| update_history::save(&path, history, true));
    if let Err(error) = result {
        log_menubar_event(&format!("app update history save failed: {error}"));
    }
}

fn finish_update_discovery(success: bool, pending_assets: bool, error: Option<String>) {
    let history = {
        let mut state = APP_UPDATE_STATE.lock().expect("app update state poisoned");
        if pending_assets {
            state.schedule.finish_pending_assets(now_unix());
        } else {
            state.schedule.finish(now_unix(), success);
        }
        state.check_error = error;
        capture_update_history(&state)
    };
    persist_update_history(&history);
}

/// External editors do not take the update lease; reject their stale response.
fn discard_stale_update_channel(
    state: &mut AppUpdateState,
    requested: UpdateChannel,
    configured: UpdateChannel,
    now: u64,
) -> bool {
    if requested == configured {
        return false;
    }
    reset_update_channel(state, configured, now, true);
    true
}

fn reconcile_update_channel(
    state: &mut AppUpdateState,
    configured: UpdateChannel,
    now: u64,
    checks_enabled: bool,
) -> bool {
    if state.channel == configured {
        return false;
    }
    reset_update_channel(state, configured, now, checks_enabled);
    true
}

fn reset_update_channel(
    state: &mut AppUpdateState,
    configured: UpdateChannel,
    now: u64,
    checks_enabled: bool,
) {
    state.phase = "idle";
    state.latest = None;
    state.release_url = None;
    state.notes = None;
    state.install_ready = false;
    state.channel = configured;
    state.schedule = UpdateSchedule::new();
    if checks_enabled {
        state.schedule.next_check_unix = Some(now);
    }
    state.message = Some("Update channel changed; checking the selected channel next.".into());
    state.check_error = None;
}

/// Keep notification deduplication separate from discovery so repeated checks
/// still refresh metadata. Bounded per-process history also covers channel flips.
fn claim_update_notification(channel: UpdateChannel, version: &str) -> bool {
    let key = format!("{}:{version}", channel.as_str());
    let mut notified = APP_UPDATE_NOTIFIED
        .lock()
        .expect("update notifications poisoned");
    if notified.contains(&key) {
        return false;
    }
    notified.push_back(key);
    while notified.len() > 32 {
        notified.pop_front();
    }
    true
}

/// Keep discovery and installation as explicit actions in the dashboard.
/// Both use the native updater so WebView networking never sits in the trust
/// path. The tray menu and compact setup menu call the install variant for a
/// genuine one-click update; Settings exposes both buttons.
#[cfg(any(target_os = "macos", target_os = "windows"))]
fn run_update_interactive(install: bool) {
    run_app_update(install, false);
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn run_app_update(install: bool, automatic: bool) {
    if !APP_UPDATE_NETWORK_ENABLED.load(Ordering::Acquire) {
        set_app_update_state(
            "disabled",
            None,
            Some("Update checks are disabled in mock mode.".into()),
        );
        return;
    }
    let Some(_lease) = UpdateLease::acquire(&APP_UPDATE_IN_FLIGHT) else {
        return;
    };
    let channel = configured_update_channel();
    let history = {
        let mut state = APP_UPDATE_STATE.lock().expect("app update state poisoned");
        if automatic && !state.schedule.due(now_unix()) {
            return;
        }
        if state.channel != channel {
            state.schedule = UpdateSchedule::new();
        }
        state.channel = channel;
        state.schedule.begin(now_unix());
        state.check_error = None;
        capture_update_history(&state)
    };
    persist_update_history(&history);
    set_app_update_state("checking", None, None);
    log_menubar_event(if install {
        "app update install requested"
    } else {
        "app update check started"
    });
    let fetched = peterfan_platform::updater::fetch_release_for_channel(channel);
    // Config imports or external edits can change channel without this lease.
    // Such a response must never be offered or installed for the new channel.
    if discard_stale_update_channel(
        &mut APP_UPDATE_STATE.lock().expect("app update state poisoned"),
        channel,
        configured_update_channel(),
        now_unix(),
    ) {
        log_menubar_event("app update result discarded: update channel changed");
        return;
    }
    let pending_assets = fetched.as_ref().is_ok_and(|release| {
        peterfan_platform::updater::is_newer(env!("CARGO_PKG_VERSION"), &release.version)
            && !peterfan_platform::updater::is_installable_release(release)
    });
    finish_update_discovery(
        fetched.is_ok(),
        pending_assets,
        fetched.as_ref().err().cloned(),
    );
    match fetched {
        Ok(release)
            if !peterfan_platform::updater::is_newer(
                env!("CARGO_PKG_VERSION"),
                &release.version,
            ) =>
        {
            let message = format!("PeterFan v{} is current.", env!("CARGO_PKG_VERSION"));
            set_app_update_state("current", Some(&release), Some(message.clone()));
            *STATUS.lock().expect("status poisoned") = message.clone();
            log_menubar_event(&message);
        }
        Ok(release) if !peterfan_platform::updater::is_installable_release(&release) => {
            let message = format!(
                "PeterFan v{} is available, but its verified files are not ready.",
                release.version
            );
            set_app_update_state("available", Some(&release), Some(message.clone()));
            *STATUS.lock().expect("status poisoned") = message.clone();
            log_menubar_event(&message);
        }
        Ok(release) if !install => {
            let message = format!(
                "PeterFan v{} is available. Choose Install Update to continue.",
                release.version
            );
            set_app_update_state("available", Some(&release), Some(message.clone()));
            *STATUS.lock().expect("status poisoned") = message.clone();
            log_menubar_event(&message);
            if automatic
                && peterfan_platform::config::load().notifications.updates
                && claim_update_notification(channel, &release.version)
            {
                post_native_notification(
                    "PeterFan — update available",
                    format!(
                        "PeterFan v{} is ready. Open Settings to install it.",
                        release.version
                    ),
                );
            }
        }
        Ok(release) => {
            set_app_update_state(
                "downloading",
                Some(&release),
                Some(format!(
                    "Downloading and verifying PeterFan v{}.",
                    release.version
                )),
            );
            log_menubar_event(&format!(
                "app update download started target=v{} asset={}",
                release.version,
                release.asset_name.as_deref().unwrap_or("unknown")
            ));
            match peterfan_platform::updater::download_and_install_release(&release) {
                Ok(()) => {
                    let message = format!(
                        "PeterFan v{} is verified and ready to relaunch.",
                        release.version
                    );
                    set_app_update_state("queued", Some(&release), Some(message.clone()));
                    *STATUS.lock().expect("status poisoned") = message.clone();
                    log_menubar_event(&message);
                    QUIT.store(true, Ordering::Release);
                }
                Err(error) => {
                    let message = format!("Update failed: {error}");
                    set_app_update_state("failed", Some(&release), Some(message.clone()));
                    *STATUS.lock().expect("status poisoned") = message.clone();
                    log_menubar_event(&message);
                }
            }
        }
        Err(error) => {
            let message = format!("Couldn't check for updates: {error}");
            set_app_update_state("failed", None, Some(message.clone()));
            *STATUS.lock().expect("status poisoned") = message.clone();
            log_menubar_event(&message);
        }
    }
    let state = APP_UPDATE_STATE.lock().expect("app update state poisoned");
    log_menubar_event(&format!(
        "app update completed automatic={automatic} phase={} channel={} last_attempt={:?} last_success={:?} next_check={:?}",
        state.phase,
        state.channel.as_str(),
        state.schedule.last_attempt_unix,
        state.schedule.last_success_unix,
        state.schedule.next_check_unix
    ));
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn check_for_updates_interactive() {
    run_update_interactive(false);
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn install_update_interactive() {
    run_update_interactive(true);
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn check_for_updates_interactive() {}
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn install_update_interactive() {}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn primary_disk(disks: &[DiskInfo]) -> Option<&DiskInfo> {
    let home = dirs::home_dir();
    let executable = std::env::current_exe().ok();
    disks.iter().max_by_key(|disk| {
        let score = home
            .iter()
            .chain(executable.iter())
            .map(|target| mount_match_score(&disk.mount, &target.display().to_string()))
            .max()
            .unwrap_or(0);
        (score, !disk.removable, disk.total)
    })
}

fn mount_match_score(mount: &str, target: &str) -> usize {
    let normalize = |value: &str| {
        let value = value.replace('\\', "/");
        if cfg!(target_os = "windows") {
            value.to_ascii_lowercase()
        } else {
            value
        }
    };
    let mount = normalize(mount);
    let target = normalize(target);
    if target.starts_with(&mount) {
        mount.len()
    } else {
        0
    }
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

fn format_uptime(total_secs: u64) -> String {
    let days = total_secs / 86_400;
    let hours = (total_secs % 86_400) / 3_600;
    let minutes = (total_secs % 3_600) / 60;
    if days > 0 {
        format!("{days}d {hours}h")
    } else if hours > 0 {
        format!("{hours}h {minutes}m")
    } else {
        format!("{minutes}m")
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

#[cfg(target_os = "macos")]
fn system_reduce_motion() -> bool {
    NSWorkspace::sharedWorkspace().accessibilityDisplayShouldReduceMotion()
}

#[cfg(not(target_os = "macos"))]
fn system_reduce_motion() -> bool {
    false
}

fn runner_icon_index(cpu_pct: f32, frame: u8) -> usize {
    runner_load_band(cpu_pct) * usize::from(RUNNER_FRAME_COUNT)
        + usize::from(frame % RUNNER_FRAME_COUNT)
}

#[cfg(any(not(target_os = "macos"), test))]
fn runner_icon_index_with_hysteresis(cpu_pct: f32, frame: u8, band: &mut usize) -> usize {
    *band = runner_load_band_with_hysteresis(cpu_pct, *band);
    *band * usize::from(RUNNER_FRAME_COUNT) + usize::from(frame % RUNNER_FRAME_COUNT)
}

impl RunnerAssets {
    fn from_pack(pack: RunnerPack) -> Result<Self, RunnerBuildError> {
        Self::from_pack_with(
            pack,
            runner_icon_from_rgba,
            make_runner_native_image_from_rgba,
        )
    }

    fn from_pack_with<FI, FN>(
        pack: RunnerPack,
        mut icon_builder: FI,
        _native_builder: FN,
    ) -> Result<Self, RunnerBuildError>
    where
        FI: FnMut(RunnerCharacter, usize, &[u8]) -> Result<Icon, RunnerBuildError>,
        FN: FnMut(RunnerCharacter, usize, &[u8]) -> Result<NativeRunnerImage, RunnerBuildError>,
    {
        let character = pack.character();
        let icons = pack
            .frames()
            .iter()
            .enumerate()
            .map(|(index, rgba)| icon_builder(character, index, rgba))
            .collect::<Result<Vec<_>, _>>()?;
        #[cfg(any(target_os = "macos", test))]
        let mut native_builder = _native_builder;
        #[cfg(any(target_os = "macos", test))]
        let native_images = pack
            .frames()
            .iter()
            .enumerate()
            .map(|(index, rgba)| native_builder(character, index, rgba))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            pack,
            icons,
            #[cfg(any(target_os = "macos", test))]
            native_images,
        })
    }

    fn selected_preview_frames_json(&self) -> String {
        runner_selected_preview_frames_json(&self.pack)
    }
}

fn build_runner_assets(character: RunnerCharacter) -> Result<RunnerAssets, RunnerBuildError> {
    RunnerAssets::from_pack(RunnerPack::build(character)?)
}

#[cfg(test)]
fn build_runner_assets_with<FP, FI, FN>(
    character: RunnerCharacter,
    pack_builder: FP,
    icon_builder: FI,
    native_builder: FN,
) -> Result<RunnerAssets, RunnerBuildError>
where
    FP: FnOnce(RunnerCharacter) -> Result<RunnerPack, RunnerBuildError>,
    FI: FnMut(RunnerCharacter, usize, &[u8]) -> Result<Icon, RunnerBuildError>,
    FN: FnMut(RunnerCharacter, usize, &[u8]) -> Result<NativeRunnerImage, RunnerBuildError>,
{
    RunnerAssets::from_pack_with(pack_builder(character)?, icon_builder, native_builder)
}

fn initialize_runner_assets(
    requested: RunnerCharacter,
) -> Result<RunnerStartupState, RunnerStartupFailure> {
    initialize_runner_assets_with(requested, build_runner_assets)
}

fn initialize_runner_assets_with<F>(
    character: RunnerCharacter,
    builder: F,
) -> Result<RunnerStartupState, RunnerStartupFailure>
where
    F: FnMut(RunnerCharacter) -> Result<RunnerAssets, RunnerBuildError>,
{
    initialize_runner_assets_impl(character, builder)
}

fn initialize_runner_assets_impl<F>(
    requested: RunnerCharacter,
    mut builder: F,
) -> Result<RunnerStartupState, RunnerStartupFailure>
where
    F: FnMut(RunnerCharacter) -> Result<RunnerAssets, RunnerBuildError>,
{
    match builder(requested) {
        Ok(assets) => Ok(RunnerStartupState {
            character: requested,
            assets,
            original_error: None,
        }),
        Err(original_error) if requested != RunnerCharacter::Cat => {
            match builder(RunnerCharacter::Cat) {
                Ok(assets) => Ok(RunnerStartupState {
                    character: RunnerCharacter::Cat,
                    assets,
                    original_error: Some(original_error),
                }),
                Err(fatal_error) => Err(RunnerStartupFailure {
                    requested,
                    original_error: Some(original_error),
                    fatal_error,
                }),
            }
        }
        Err(fatal_error) => Err(RunnerStartupFailure {
            requested,
            original_error: None,
            fatal_error,
        }),
    }
}

fn replace_runner_assets_with<F>(
    current_character: &mut RunnerCharacter,
    current_assets: &mut RunnerAssets,
    requested: RunnerCharacter,
    builder: F,
) -> Result<(), RunnerBuildError>
where
    F: FnOnce(RunnerCharacter) -> Result<RunnerAssets, RunnerBuildError>,
{
    if *current_character == requested {
        return Ok(());
    }
    let next_assets = builder(requested)?;
    *current_character = requested;
    *current_assets = next_assets;
    Ok(())
}

#[cfg(test)]
fn make_runner_icons(character: RunnerCharacter) -> Vec<Icon> {
    build_runner_assets(character).expect("runner assets").icons
}

#[cfg(target_os = "macos")]
fn runner_icon_from_rgba(
    character: RunnerCharacter,
    frame_index: usize,
    rgba: &[u8],
) -> Result<Icon, RunnerBuildError> {
    validate_runner_rgba_frame(character, &format!("icon cache frame {frame_index}"), rgba)?;
    Icon::from_rgba(rgba.to_vec(), RUNNER_PIXEL_WIDTH, RUNNER_PIXEL_HEIGHT).map_err(|error| {
        RunnerBuildError::cache(
            character,
            format!("icon cache frame {frame_index}"),
            error.to_string(),
        )
    })
}

#[cfg(target_os = "macos")]
fn make_runner_native_image_from_rgba(
    character: RunnerCharacter,
    frame_index: usize,
    rgba: &[u8],
) -> Result<NativeRunnerImage, RunnerBuildError> {
    validate_runner_rgba_frame(
        character,
        &format!("native cache frame {frame_index}"),
        rgba,
    )?;
    // Allocate the final raster eagerly. NSImage(data:) retains an encoded
    // ImageIO representation: AppKit status-item snapshot draws can revisit
    // its PNG decode and purgeable pixel cache on every animation cycle.
    // A null planes pointer asks the bitmap representation to own its storage.
    let bitmap = unsafe {
        NSBitmapImageRep::initWithBitmapDataPlanes_pixelsWide_pixelsHigh_bitsPerSample_samplesPerPixel_hasAlpha_isPlanar_colorSpaceName_bitmapFormat_bytesPerRow_bitsPerPixel(
            NSBitmapImageRep::alloc(),
            std::ptr::null_mut(),
            RUNNER_PIXEL_WIDTH as isize,
            RUNNER_PIXEL_HEIGHT as isize,
            8,
            4,
            true,
            false,
            NSDeviceRGBColorSpace,
            NSBitmapFormat::AlphaNonpremultiplied,
            (RUNNER_PIXEL_WIDTH * 4) as isize,
            32,
        )
    }
    .ok_or_else(|| {
        RunnerBuildError::cache(
            character,
            format!("native cache frame {frame_index}"),
            "bitmap allocation returned nil",
        )
    })?;
    let pixels = bitmap.bitmapData();
    if pixels.is_null()
        || bitmap.bytesPerRow() != (RUNNER_PIXEL_WIDTH * 4) as isize
        || bitmap.pixelsHigh() != RUNNER_PIXEL_HEIGHT as isize
    {
        return Err(RunnerBuildError::cache(
            character,
            format!("native cache frame {frame_index}"),
            "bitmap allocation returned invalid storage",
        ));
    }
    // SAFETY: AppKit owns this non-planar RGBA8 allocation, its requested
    // stride/height were checked above, and validate_runner_rgba_frame checked
    // the source length. No references to the bitmap pixels escape this copy.
    unsafe { std::ptr::copy_nonoverlapping(rgba.as_ptr(), pixels, rgba.len()) };
    let logical_size = NSSize::new(27.0, 18.0);
    bitmap.setSize(logical_size);
    let image = NSImage::initWithSize(NSImage::alloc(), logical_size);
    image.addRepresentation(&bitmap);
    // Keep a raster cache for repeated status-item snapshot draws. The native
    // timer reuses these tiny images without changing the shared CPU cadence.
    image.setCacheMode(NSImageCacheMode::Always);
    // Template mode keeps every runner as the same white status-item glyph on
    // both light and dark menu bars.
    image.setTemplate(true);
    Ok(image)
}

#[cfg(not(target_os = "macos"))]
fn runner_icon_from_rgba(
    character: RunnerCharacter,
    frame_index: usize,
    rgba: &[u8],
) -> Result<Icon, RunnerBuildError> {
    validate_runner_rgba_frame(character, &format!("icon cache frame {frame_index}"), rgba)?;
    Icon::from_rgba(rgba.to_vec(), RUNNER_PIXEL_WIDTH, RUNNER_PIXEL_HEIGHT).map_err(|error| {
        RunnerBuildError::cache(
            character,
            format!("icon cache frame {frame_index}"),
            error.to_string(),
        )
    })
}

#[cfg(not(target_os = "macos"))]
fn make_runner_native_image_from_rgba(
    character: RunnerCharacter,
    frame_index: usize,
    rgba: &[u8],
) -> Result<NativeRunnerImage, RunnerBuildError> {
    validate_runner_rgba_frame(
        character,
        &format!("native cache frame {frame_index}"),
        rgba,
    )?;
    Ok(rgba.to_vec())
}

fn validate_runner_rgba_frame(
    character: RunnerCharacter,
    stage: &str,
    rgba: &[u8],
) -> Result<(), RunnerBuildError> {
    let expected_len = (RUNNER_PIXEL_WIDTH * RUNNER_PIXEL_HEIGHT * 4) as usize;
    if rgba.len() != expected_len {
        return Err(RunnerBuildError::cache(
            character,
            stage.to_string(),
            format!("expected {expected_len} rgba bytes, got {}", rgba.len()),
        ));
    }
    Ok(())
}

#[cfg(all(target_os = "macos", test))]
fn native_status_item_width(display: MenubarDisplay) -> f64 {
    native_cpu_widget::primary_width(display)
}

#[cfg(target_os = "macos")]
fn configure_native_status_item(tray: &TrayIcon, display: MenubarDisplay) {
    if let Ok(primary) = native_cpu_widget::PreparedPrimaryItem::new(tray) {
        primary.configure(tray, display);
    }
}

#[cfg(target_os = "macos")]
fn native_status_item_image_position(display: MenubarDisplay) -> NSCellImagePosition {
    native_cpu_widget::primary_image_position(display)
}

fn runner_visibility_should_animate(
    primary_visible: bool,
    display: MenubarDisplay,
    reduce_motion: bool,
) -> bool {
    primary_visible && runner_should_animate(display, reduce_motion)
}

fn invalidate_runner_icon(last_runner_icon: &mut Option<usize>) {
    // `None` is the valid cached state for number-only mode. Use an impossible
    // frame index so the next update removes a previously visible runner
    // instead of mistaking the cache for an already-hidden icon.
    *last_runner_icon = Some(usize::MAX);
}

#[cfg(target_os = "macos")]
fn stop_native_runner_timer(app: &mut App) {
    if let Some(timer) = app.runner_native_timer.take() {
        timer.invalidate();
    }
    app.runner_native_phase_us.store(0, Ordering::Relaxed);
    app.runner_native_tick_us
        .store(RUNNER_NATIVE_TICK.as_micros() as u64, Ordering::Relaxed);
}

#[cfg(target_os = "macos")]
fn sync_native_runner_timer(app: &mut App) {
    app.runner_native_cpu_bits
        .store(app.runner_cpu_pct.to_bits(), Ordering::Relaxed);
    if !runner_visibility_should_animate(app.primary_visible, app.display, app.reduce_motion) {
        stop_native_runner_timer(app);
        return;
    }

    if let Some(timer) = app
        .runner_native_timer
        .as_ref()
        .filter(|timer| timer.isValid())
    {
        let motion_cpu = f32::from_bits(app.runner_native_motion_cpu_bits.load(Ordering::Relaxed));
        if (app.runner_cpu_pct - motion_cpu).abs() >= 0.1 {
            // Bring a distant idle deadline forward without postponing a pose
            // that is already due sooner. Elapsed time comes from Instant.
            if timer.fireDate().timeIntervalSinceNow() > RUNNER_NATIVE_TICK.as_secs_f64() {
                let fire_date =
                    NSDate::dateWithTimeIntervalSinceNow(RUNNER_NATIVE_TICK.as_secs_f64());
                timer.setFireDate(&fire_date);
            }
        }
        return;
    }

    stop_native_runner_timer(app);
    let Some(tray) = &app.tray else {
        return;
    };
    let Some(status_item) = tray.ns_status_item() else {
        return;
    };
    let mtm = unsafe { MainThreadMarker::new_unchecked() };
    if status_item.button(mtm).is_none() {
        return;
    }

    let character = app.runner_character;
    let images = app.runner_assets.native_images.clone();
    let frame = Arc::clone(&app.runner_native_frame);
    let cpu_bits = Arc::clone(&app.runner_native_cpu_bits);
    let motion_cpu_bits = Arc::clone(&app.runner_native_motion_cpu_bits);
    let phase_us = Arc::clone(&app.runner_native_phase_us);
    let scheduled_tick_us = Arc::clone(&app.runner_native_tick_us);
    let band = Arc::clone(&app.runner_native_band);
    let applied_icon = Arc::clone(&app.runner_native_applied_icon);
    let painter = Rc::clone(&app.runner_native_painter);
    phase_us.store(0, Ordering::Relaxed);
    band.store(runner_load_band(app.runner_cpu_pct), Ordering::Relaxed);
    scheduled_tick_us.store(RUNNER_NATIVE_TICK.as_micros() as u64, Ordering::Relaxed);
    let last_tick = Cell::new(Instant::now());
    let block = RcBlock::new(move |timer: std::ptr::NonNull<NSTimer>| {
        let target_cpu = f32::from_bits(cpu_bits.load(Ordering::Relaxed));
        let current_cpu = f32::from_bits(motion_cpu_bits.load(Ordering::Relaxed));
        let now = Instant::now();
        let elapsed = now.saturating_duration_since(last_tick.replace(now));
        let elapsed_us = elapsed.as_micros().min(u128::from(u64::MAX)) as u64;
        let cpu_pct = ease_runner_motion_cpu(current_cpu, target_cpu, elapsed);
        motion_cpu_bits.store(cpu_pct.to_bits(), Ordering::Relaxed);
        let current_frame = frame.load(Ordering::Relaxed) as u8 % RUNNER_FRAME_COUNT;
        let (next_phase, next_frame) = advance_runner_phase(
            character,
            phase_us.load(Ordering::Relaxed),
            elapsed_us,
            cpu_pct,
            current_frame,
        );
        phase_us.store(next_phase, Ordering::Relaxed);
        if next_frame != current_frame {
            let current_band = band.load(Ordering::Relaxed);
            let next_band = runner_load_band_with_hysteresis(cpu_pct, current_band);
            band.store(next_band, Ordering::Relaxed);
            frame.store(u64::from(next_frame), Ordering::Relaxed);
            let index = next_band * usize::from(RUNNER_FRAME_COUNT) + usize::from(next_frame);
            if let Some(image) = images.get(index) {
                painter.borrow().paint(image);
                applied_icon.store(index, Ordering::Relaxed);
            }
        }

        let next_delay =
            runner_native_next_delay(character, next_phase, cpu_pct, target_cpu, next_frame);
        scheduled_tick_us.store(next_delay.as_micros() as u64, Ordering::Relaxed);
        let fire_date = NSDate::dateWithTimeIntervalSinceNow(next_delay.as_secs_f64());
        unsafe { timer.as_ref() }.setFireDate(&fire_date);
    });
    let timer = unsafe {
        NSTimer::timerWithTimeInterval_repeats_block(RUNNER_NATIVE_TICK.as_secs_f64(), true, &block)
    };
    timer.setTolerance(RUNNER_NATIVE_TOLERANCE.as_secs_f64());
    unsafe {
        NSRunLoop::mainRunLoop().addTimer_forMode(&timer, NSRunLoopCommonModes);
    }
    app.runner_native_timer = Some(timer);
}

fn sync_runner_selection(app: &App) {
    if let Some(ref tm) = app.tray_menu {
        tm.random_character_item
            .set_checked(app.random_runner.enabled());
        for (candidate, item) in &tm.character_items {
            item.set_checked(!app.random_runner.enabled() && *candidate == app.runner_character);
        }
    }
}

/// Build first, persist second, and return prepared assets without changing live
/// animation state. Both error paths leave the current runner and timer intact.
fn prepare_runner_preference<A>(
    current: RunnerCharacter,
    requested: RunnerCharacter,
    build: impl FnOnce(RunnerCharacter) -> Result<A, String>,
    persist: impl FnOnce(RunnerCharacter) -> Result<(), String>,
) -> Result<Option<A>, String> {
    let assets = if current == requested {
        None
    } else {
        Some(build(requested)?)
    };
    persist(requested)?;
    Ok(assets)
}

fn set_runner_random(app: &mut App) -> bool {
    if app.random_runner.enabled() {
        let saved = preference_saved(
            "random runner",
            save_runner_character(app.runner_character, true),
        );
        sync_runner_selection(app);
        return saved;
    }
    let now = Instant::now();
    let mut candidate = RandomRunner::new(true, now);
    let character = candidate
        .next_due(
            now,
            app.runner_character,
            runner_visibility_should_animate(app.primary_visible, app.display, app.reduce_motion),
        )
        .unwrap_or(app.runner_character);
    if select_runner_preference(app, character, true) {
        app.random_runner = candidate;
        sync_runner_selection(app);
        log_menubar_event("runner selection=random interval=60s");
        true
    } else {
        false
    }
}

fn set_runner_character(app: &mut App, character: RunnerCharacter) -> bool {
    if select_runner_preference(app, character, false) {
        app.random_runner.set_enabled(false, Instant::now());
        sync_runner_selection(app);
        log_menubar_event(&format!(
            "runner selection=fixed character={}",
            character.as_str()
        ));
        true
    } else {
        false
    }
}

fn select_runner_preference(app: &mut App, character: RunnerCharacter, random: bool) -> bool {
    match prepare_runner_preference(
        app.runner_character,
        character,
        |requested| {
            build_runner_assets(requested)
                .map_err(|error| runner_switch_failure_message(requested, &error))
        },
        |requested| {
            save_runner_character(requested, random)
                .map(|_| ())
                .map_err(|error| format!("Couldn't save runner selection: {error}"))
        },
    ) {
        Ok(Some(assets)) => {
            app.runner_assets = assets;
            app.runner_character = character;
            finish_runner_switch(app);
            true
        }
        Ok(None) => true,
        Err(error) => {
            note_runner_build_failure(error);
            sync_runner_selection(app);
            false
        }
    }
}

fn switch_runner_character(app: &mut App, character: RunnerCharacter) -> bool {
    if app.runner_character == character {
        return true;
    }

    if let Err(error) = replace_runner_assets_with(
        &mut app.runner_character,
        &mut app.runner_assets,
        character,
        build_runner_assets,
    ) {
        note_runner_build_failure(runner_switch_failure_message(character, &error));
        return false;
    }

    finish_runner_switch(app);
    true
}

fn finish_runner_switch(app: &mut App) {
    #[cfg(target_os = "macos")]
    stop_native_runner_timer(app);
    #[cfg(target_os = "macos")]
    app.runner_native_applied_icon
        .store(usize::MAX, Ordering::Relaxed);
    app.last_runner_icon = None;
    app.runner_frame = 0;
    #[cfg(target_os = "macos")]
    app.runner_native_frame.store(0, Ordering::Relaxed);
    sync_runner_selection(app);
    log_menubar_event(&format!("runner active={}", app.runner_character.as_str()));
    apply_runner_icon(app);
    #[cfg(target_os = "macos")]
    sync_native_runner_timer(app);
}

fn apply_runner_icon(app: &mut App) {
    #[cfg(target_os = "macos")]
    sync_native_runner_painter(app);
    #[cfg(target_os = "macos")]
    let desired = (app.primary_visible && runner_enabled(app.display)).then(|| {
        app.runner_native_band.load(Ordering::Relaxed) * usize::from(RUNNER_FRAME_COUNT)
            + app.runner_native_frame.load(Ordering::Relaxed) as usize
                % usize::from(RUNNER_FRAME_COUNT)
    });
    #[cfg(not(target_os = "macos"))]
    let desired = (app.primary_visible && runner_enabled(app.display)).then(|| {
        runner_icon_index_with_hysteresis(
            app.runner_cpu_pct,
            app.runner_frame,
            &mut app.runner_band,
        )
    });
    #[cfg(target_os = "macos")]
    let already_applied =
        app.runner_native_applied_icon.load(Ordering::Relaxed) == desired.unwrap_or(usize::MAX);
    #[cfg(not(target_os = "macos"))]
    let already_applied = desired == app.last_runner_icon;
    if already_applied {
        return;
    }

    if let Some(tray) = &app.tray {
        #[cfg(target_os = "macos")]
        {
            let Some(status_item) = tray.ns_status_item() else {
                return;
            };
            let mtm = unsafe { MainThreadMarker::new_unchecked() };
            let Some(button) = status_item.button(mtm) else {
                return;
            };
            button.setImagePosition(native_status_item_image_position(app.display));
            let image = desired
                .and_then(|index| app.runner_assets.native_images.get(index))
                .map(|image| &**image);
            if let Some(image) = image {
                app.runner_native_painter.borrow().paint(image);
            } else {
                button.setImage(None);
            }
            app.last_runner_icon = desired;
            app.runner_native_applied_icon
                .store(desired.unwrap_or(usize::MAX), Ordering::Relaxed);
        }

        #[cfg(not(target_os = "macos"))]
        let icon = desired.and_then(|index| app.runner_assets.icons.get(index).cloned());
        #[cfg(not(target_os = "macos"))]
        if tray.set_icon_with_as_template(icon, false).is_ok() {
            app.last_runner_icon = desired;
        }
    }
}

#[cfg(target_os = "macos")]
fn sync_native_runner_painter(app: &mut App) {
    let Some(status_item) = app.tray.as_ref().and_then(TrayIcon::ns_status_item) else {
        return;
    };
    // This function is reached only by the main event loop's tray updates.
    let mtm = unsafe { MainThreadMarker::new_unchecked() };
    let Some(button) = status_item.button(mtm) else {
        return;
    };
    let screens = NSScreen::screens(mtm).len();
    let eligible = native_runner::eligible(app.display, app.reduce_motion, screens);
    let mut painter = app.runner_native_painter.borrow_mut();
    let was_active = painter.active();
    let button_changed = painter.bind(&button);
    if eligible && !painter.active() {
        let transparent = vec![0; (RUNNER_PIXEL_WIDTH * RUNNER_PIXEL_HEIGHT * 4) as usize];
        if let Ok(placeholder) =
            make_runner_native_image_from_rgba(app.runner_character, 0, &transparent)
        {
            painter.attach(&placeholder);
        }
    } else if !eligible && was_active {
        painter.detach();
    }
    painter.sync();
    if button_changed || was_active != painter.active() {
        // The timer observes this shared painter on its next tick, so monitor
        // attachment does not restart the current pose, phase or CPU easing.
        app.runner_native_applied_icon
            .store(usize::MAX, Ordering::Relaxed);
        let renderer = if painter.active() { "layer" } else { "image" };
        log_menubar_event(&format!(
            "runner renderer={renderer} screens={screens} geometry={:?} phase_us={} frame={} button_bounds={:?} click_regions={:?}",
            painter.attachment_geometry(),
            app.runner_native_phase_us.load(Ordering::Relaxed),
            app.runner_native_frame.load(Ordering::Relaxed),
            button.bounds(),
            button.subviews().iter().map(|view| view.frame()).collect::<Vec<_>>(),
        ));
    }
}

#[cfg(test)]
fn menubar_runner_icon(cpu_pct: f32, frame: u8) -> Icon {
    make_runner_icon(RunnerCharacter::Cat, cpu_pct, frame)
}

#[cfg(test)]
fn make_runner_icon(character: RunnerCharacter, cpu_pct: f32, frame: u8) -> Icon {
    runner_icon_from_rgba(
        character,
        usize::from(frame),
        &make_runner_rgba(character, cpu_pct, frame),
    )
    .expect("valid icon")
}

#[cfg(test)]
fn make_runner_rgba(character: RunnerCharacter, cpu_pct: f32, frame: u8) -> Vec<u8> {
    apply_test_runner_style(
        render_raw_frame(character, cpu_pct, frame).expect("runner frame must render as RGBA"),
    )
}

#[cfg(test)]
struct RunnerQaArtifacts {
    sheet_path: PathBuf,
    report_path: PathBuf,
}

#[cfg(test)]
const RUNNER_QA_SCALE: u32 = 4;
#[cfg(test)]
const RUNNER_QA_PADDING: u32 = 4;

#[cfg(test)]
fn write_runner_quality_artifacts() -> Result<RunnerQaArtifacts, RunnerBuildError> {
    let target = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("target");
    std::fs::create_dir_all(&target).map_err(|error| {
        RunnerBuildError::cache(RunnerCharacter::Cat, "runner QA target", error.to_string())
    })?;

    let packs = RunnerCharacter::FEATURED
        .into_iter()
        .map(|character| RunnerPack::build(character).map(|pack| (character, pack)))
        .collect::<Result<Vec<_>, _>>()?;
    let frame_payload: serde_json::Map<String, serde_json::Value> = packs
        .iter()
        .map(|(character, pack)| {
            (
                character.as_str().to_string(),
                serde_json::json!(pack
                    .frames()
                    .iter()
                    .map(|frame| runner::alpha_hex(frame))
                    .collect::<Vec<_>>()),
            )
        })
        .collect();
    std::fs::write(
        target.join("runner-motion-frames.json"),
        serde_json::to_vec(&frame_payload).expect("serializable frame payload"),
    )
    .map_err(|error| {
        RunnerBuildError::cache(RunnerCharacter::Cat, "motion QA frames", error.to_string())
    })?;
    let cadence: serde_json::Map<String, serde_json::Value> = RunnerCharacter::FEATURED
        .into_iter()
        .map(|character| {
            (
                character.as_str().to_string(),
                serde_json::json!((0..=100)
                    .map(|cpu| (0..RUNNER_FRAME_COUNT)
                        .map(|pose| runner::timing::frame_interval_for_pose(
                            character, cpu as f32, pose
                        )
                        .as_millis())
                        .collect::<Vec<_>>())
                    .collect::<Vec<_>>()),
            )
        })
        .collect();
    std::fs::write(
        target.join("runner-motion-cadence.json"),
        serde_json::to_vec(&cadence).expect("serializable cadence"),
    )
    .map_err(|error| {
        RunnerBuildError::cache(RunnerCharacter::Cat, "motion QA cadence", error.to_string())
    })?;
    let sheet_path = target.join("runner-sprite-sheet.png");
    let report_path = target.join("runner-quality-report.json");

    let (width, height) = runner_qa_sheet_dimensions();
    let mut sheet = vec![0u8; (width * height * 4) as usize];
    for pixel in sheet.as_chunks_mut::<4>().0.iter_mut() {
        pixel.copy_from_slice(&[28, 28, 30, 255]);
    }

    for (character_index, (_, pack)) in packs.iter().enumerate() {
        for band in 0..RUNNER_LOAD_BANDS {
            let row = character_index * RUNNER_LOAD_BANDS + band;
            for frame in 0..RUNNER_FRAME_COUNT {
                let rgba = pack
                    .frame_by_index(band, frame)
                    .expect("QA sheet frame must exist");
                draw_runner_qa_frame(&mut sheet, width, row as u32, frame as u32, rgba);
            }
        }
    }

    let file = std::fs::File::create(&sheet_path).map_err(|error| {
        RunnerBuildError::cache(
            RunnerCharacter::Cat,
            "runner QA sheet create",
            error.to_string(),
        )
    })?;
    let mut encoder = png::Encoder::new(file, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder
        .write_header()
        .and_then(|mut writer| writer.write_image_data(&sheet))
        .map_err(|error| {
            RunnerBuildError::cache(
                RunnerCharacter::Cat,
                "runner QA sheet write",
                error.to_string(),
            )
        })?;

    std::fs::write(&report_path, runner::build_quality_report_json()?).map_err(|error| {
        RunnerBuildError::cache(
            RunnerCharacter::Cat,
            "runner QA report write",
            error.to_string(),
        )
    })?;

    Ok(RunnerQaArtifacts {
        sheet_path,
        report_path,
    })
}

#[cfg(test)]
fn runner_qa_sheet_dimensions() -> (u32, u32) {
    let cell_width = RUNNER_PIXEL_WIDTH * (RUNNER_QA_SCALE + 1) + RUNNER_QA_PADDING * 3;
    let cell_height = RUNNER_PIXEL_HEIGHT * RUNNER_QA_SCALE + RUNNER_QA_PADDING * 2;
    (
        cell_width * u32::from(RUNNER_FRAME_COUNT),
        cell_height * RunnerCharacter::FEATURED.len() as u32 * RUNNER_LOAD_BANDS as u32,
    )
}

#[cfg(test)]
fn draw_runner_qa_frame(sheet: &mut [u8], sheet_width: u32, row: u32, column: u32, rgba: &[u8]) {
    let cell_width = RUNNER_PIXEL_WIDTH * (RUNNER_QA_SCALE + 1) + RUNNER_QA_PADDING * 3;
    let cell_height = RUNNER_PIXEL_HEIGHT * RUNNER_QA_SCALE + RUNNER_QA_PADDING * 2;
    let cell_origin_x = column * cell_width;
    let cell_origin_y = row * cell_height;
    let native_origin_y = cell_origin_y + (cell_height - RUNNER_PIXEL_HEIGHT) / 2;
    draw_runner_qa_scaled(
        sheet,
        sheet_width,
        cell_origin_x + RUNNER_QA_PADDING,
        native_origin_y,
        rgba,
        1,
    );
    draw_runner_qa_scaled(
        sheet,
        sheet_width,
        cell_origin_x + RUNNER_QA_PADDING * 2 + RUNNER_PIXEL_WIDTH,
        cell_origin_y + RUNNER_QA_PADDING,
        rgba,
        RUNNER_QA_SCALE,
    );
}

#[cfg(test)]
fn draw_runner_qa_scaled(
    sheet: &mut [u8],
    sheet_width: u32,
    origin_x: u32,
    origin_y: u32,
    rgba: &[u8],
    scale: u32,
) {
    for source_y in 0..RUNNER_PIXEL_HEIGHT {
        for source_x in 0..RUNNER_PIXEL_WIDTH {
            let source_index = ((source_y * RUNNER_PIXEL_WIDTH + source_x) * 4) as usize;
            let alpha = f32::from(rgba[source_index + 3]) / 255.0;
            for scale_y in 0..scale {
                for scale_x in 0..scale {
                    let target_x = origin_x + source_x * scale + scale_x;
                    let target_y = origin_y + source_y * scale + scale_y;
                    let target_index = ((target_y * sheet_width + target_x) * 4) as usize;
                    for channel in 0..3 {
                        let foreground = f32::from(rgba[source_index + channel]);
                        sheet[target_index + channel] =
                            (foreground * alpha + 28.0 * (1.0 - alpha)).round() as u8;
                    }
                    sheet[target_index + 3] = 255;
                }
            }
        }
    }
}

fn runner_cached_packs() -> &'static Vec<(RunnerCharacter, RunnerPack)> {
    static PACKS: std::sync::OnceLock<Vec<(RunnerCharacter, RunnerPack)>> =
        std::sync::OnceLock::new();
    PACKS.get_or_init(|| runner_preview_packs_with(RunnerPack::build))
}

fn runner_preview_packs_with<F>(mut builder: F) -> Vec<(RunnerCharacter, RunnerPack)>
where
    F: FnMut(RunnerCharacter) -> Result<RunnerPack, RunnerBuildError>,
{
    let cat_fallback = builder(RunnerCharacter::Cat);
    RunnerCharacter::FEATURED
        .into_iter()
        .filter_map(|character| {
            match if character == RunnerCharacter::Cat {
                cat_fallback.clone()
            } else {
                builder(character)
            } {
                Ok(pack) => Some((character, pack)),
                Err(error) if character != RunnerCharacter::Cat => match &cat_fallback {
                    Ok(pack) => {
                        log_menubar_event(&runner_preview_failure_message(character, &error));
                        Some((character, pack.clone()))
                    }
                    Err(fallback_error) => {
                        log_menubar_event(&runner_preview_failure_message(
                            character,
                            fallback_error,
                        ));
                        None
                    }
                },
                Err(error) => {
                    log_menubar_event(&runner_preview_failure_message(character, &error));
                    None
                }
            }
        })
        .collect()
}

#[cfg(test)]
fn apply_test_runner_style(mut rgba: Vec<u8>) -> Vec<u8> {
    for pixel in rgba.as_chunks_mut::<4>().0.iter_mut() {
        if pixel[3] != 0 {
            pixel[0] = 255;
            pixel[1] = 255;
            pixel[2] = 255;
        }
    }
    rgba
}

// ---------------------------------------------------------------------------
// Popover dashboard (self-contained HTML/CSS/JS).
// ---------------------------------------------------------------------------

fn runner_character_values_json() -> String {
    serde_json::to_string(&RunnerCharacter::FEATURED.map(|character| character.as_str()))
        .expect("runner character values must serialize")
}

fn runner_preview_alpha_json() -> &'static str {
    static PREVIEWS: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    PREVIEWS.get_or_init(|| runner_preview_alpha_json_for(runner_cached_packs()))
}

fn runner_preview_frames_json() -> &'static str {
    static PREVIEWS: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    PREVIEWS.get_or_init(|| runner_preview_frames_json_for(runner_cached_packs()))
}

fn runner_preview_alpha_json_for(packs: &[(RunnerCharacter, RunnerPack)]) -> String {
    let entries = packs
        .iter()
        .map(|(character, pack)| {
            format!(
                r#""{}":"{}""#,
                character.as_str(),
                pack.preview_alpha_hex(RUNNER_PREVIEW_BAND, RUNNER_PREVIEW_POSE)
                    .unwrap_or_default()
            )
        })
        .collect::<Vec<_>>();
    format!("{{{}}}", entries.join(","))
}

fn runner_preview_frames_json_for(packs: &[(RunnerCharacter, RunnerPack)]) -> String {
    let entries = packs
        .iter()
        .map(|(character, pack)| {
            let frames = (0..RUNNER_FRAME_COUNT)
                .map(|frame| {
                    format!(
                        r#""{}""#,
                        pack.preview_alpha_hex(RUNNER_PREVIEW_BAND, frame)
                            .unwrap_or_default()
                    )
                })
                .collect::<Vec<_>>()
                .join(",");
            format!(r#""{}":[{}]"#, character.as_str(), frames)
        })
        .collect::<Vec<_>>();
    format!("{{{}}}", entries.join(","))
}

fn runner_selected_preview_frames_json(pack: &RunnerPack) -> String {
    let frames = pack
        .frames()
        .iter()
        .map(|frame| format!(r#""{}""#, runner::alpha_hex(frame)))
        .collect::<Vec<_>>();
    format!("[{}]", frames.join(","))
}

fn runner_picker_options(lang: ResolvedLanguage) -> String {
    let label = if lang == ResolvedLanguage::Ko {
        "랜덤 · 1분마다"
    } else {
        "Random · every minute"
    };
    let random = format!(
        r#"<button type="button" class="character-option" role="option" aria-selected="false" data-character="random" onclick="chooseRunnerCharacter('random')" onkeydown="handleRunnerOptionKey(event)"><span class="runner-preview random-runner-icon" aria-hidden="true">⇄</span><span class="character-option-label">{label}</span><span class="character-option-check" aria-hidden="true"></span></button>"#
    );
    random + &RunnerCharacter::FEATURED
        .map(|character| {
            let value = character.as_str();
            let label = runner_character_label(lang, character);
            format!(
                r#"<button type="button" class="character-option" role="option" aria-selected="false" data-character="{value}" onclick="chooseRunnerCharacter('{value}')" onkeydown="handleRunnerOptionKey(event)"><canvas class="runner-preview" width="{RUNNER_PIXEL_WIDTH}" height="{RUNNER_PIXEL_HEIGHT}" data-runner-preview="{value}" aria-hidden="true"></canvas><span class="character-option-label">{label}</span><span class="character-option-check" aria-hidden="true"></span></button>"#
            )
        })
        .join("")
}

/// Build the popover/detail-window HTML for the given language. The template
/// itself is authored in English and Korean labels are substituted in by
/// exact `>Label<`/string match — cheap, and safe because each source string
/// only ever appears where a translation actually belongs (verified by hand,
/// covered by `dashboard_html_translates_known_labels` below).
#[cfg(test)]
fn dashboard_html(lang: ResolvedLanguage, show_curve_editor: bool) -> String {
    dashboard_html_with_runner_pack(lang, show_curve_editor, &runner_cached_packs()[0].1)
}

fn dashboard_html_with_runner_pack(
    lang: ResolvedLanguage,
    show_curve_editor: bool,
    selected_pack: &RunnerPack,
) -> String {
    let lang_tag = match lang {
        ResolvedLanguage::En => "en",
        ResolvedLanguage::Ko => "ko",
    };
    let html = DASHBOARD_HTML_EN
        .replace("__LANG__", lang_tag)
        .replace("__SHOWCURVE__", if show_curve_editor { "1" } else { "0" });
    let html = match lang {
        ResolvedLanguage::En => html,
        ResolvedLanguage::Ko => html
            .replace(">Fan control<", ">팬 제어<")
            .replace(">Status<", ">상태<")
            .replace(">Memory<", ">메모리<")
            .replace(">Storage<", ">저장공간<")
            .replace(">Temperature<", ">온도<")
            .replace(">CPU temperature<", ">CPU 온도<")
            .replace(">CPU temp<", ">CPU 온도<")
            .replace(">Fan average<", ">팬 평균 RPM<")
            .replace(">Fan<", ">팬<")
            .replace(">Fans<", ">팬<")
            .replace(">Battery<", ">배터리<")
            .replace(">Network<", ">네트워크<")
            .replace(">Top Processes<", ">실행 중 프로세스<")
            .replace(">MEM<", ">메모리<")
            .replace("Search name or PID", "이름 또는 PID 검색")
            .replace("Search processes", "프로세스 검색")
            .replace("No matching processes.", "일치하는 프로세스가 없습니다.")
            .replace(">Ready<", ">준비 완료<")
            .replace(">Set Up<", ">설정<")
            .replace(">Settings<", ">설정<")
            .replace(">General Settings<", ">일반 설정<")
            .replace(">General<", ">일반<")
            .replace(">App Preferences<", ">앱 설정<")
            .replace(">Menu bar<", ">메뉴 막대<")
            .replace(">Character<", ">캐릭터<")
            .replace(">Update channel<", ">업데이트 채널<")
            .replace(">Fan RPM<", ">팬 RPM<")
            .replace("Choose a character, or shuffle every minute.", "캐릭터를 고르거나 1분마다 랜덤으로 바꾸세요.")
            .replace(">CPU core graph<", ">코어별 CPU 그래프<")
            .replace(
                "Show live per-core CPU activity in the overview.",
                "상단에 코어별 CPU 사용률을 표시합니다.",
            )
            .replace(">On<", ">켜짐<")
            .replace(">Off<", ">꺼짐<")
            .replace(">Runner<", ">러너<")
            .replace(">Number<", ">숫자<")
            .replace(">Cat<", ">고양이<")
            .replace(">Dog<", ">강아지<")
            .replace(">Rabbit<", ">토끼<")
            .replace(">Fox<", ">여우<")
            .replace(">Penguin<", ">펭귄<")
            .replace(">Dinosaur<", ">공룡<")
            .replace(">Robot<", ">로봇<")
            .replace(">Ghost<", ">유령<")
            .replace(">Bird<", ">새<")
            .replace(">Duck<", ">오리<")
            .replace(">Sheep<", ">양<")
            .replace(">Fish<", ">물고기<")
            .replace(">Slime<", ">슬라임<")
            .replace(">Human<", ">달리는 사람<")
            .replace(">Push-up<", ">푸시업<")
            .replace(">Orbit<", ">궤도<")
            .replace(">Sushi<", ">회전초밥<")
            .replace(">City<", ">도시<")
            .replace(">Sausage<", ">소시지<")
            .replace(">Party<", ">파티<")
            .replace(">Both<", ">둘 다<")
            .replace("CPU runner · waiting", "CPU 러너 · 대기 중")
            .replace(">Notifications<", ">알림<")
            .replace(">Automation Rules<", ">자동화 규칙<")
            .replace(
                "Rules run in order; the first matching condition selects its fan profile.",
                "규칙은 위에서부터 평가되며, 처음 일치한 조건의 팬 프로파일을 적용합니다.",
            )
            .replace(">Add Rule<", ">규칙 추가<")
            .replace(">Quiet Battery / Performance AC<", ">배터리 저소음 / AC 고성능<")
            .replace(">Save Rules<", ">규칙 저장<")
            .replace(">Configuration Transfer<", ">설정 가져오기 및 내보내기<")
            .replace(
                "Copy the current TOML or paste a previously exported PeterFan config, then import it after validation.",
                "현재 TOML 설정을 복사하거나 이전에 내보낸 PeterFan 설정을 붙여넣은 뒤 검증하고 가져옵니다.",
            )
            .replace(">Copy Current Config<", ">현재 설정 복사<")
            .replace(">Import Config<", ">설정 가져오기<")
            .replace(">CPU temperature warning<", ">CPU 온도 경고<")
            .replace(
                "CPU Core Average · separate from the 90°C safety alert",
                "CPU 코어 평균 · 90°C 안전 알림과 별도",
            )
            .replace(">Fan control failures<", ">팬 제어 실패<")
            .replace(
                "Notify when write or RPM verification fails",
                "쓰기 또는 RPM 검증 실패 시 알림",
            )
            .replace(">App updates<", ">앱 업데이트<")
            .replace(
                "Notify once per version when a verified update is ready",
                "검증된 새 버전이 준비되면 버전당 한 번 알림",
            )
            .replace(">Load average<", ">로드 평균<")
            .replace(">Power<", ">소비 전력<")
            .replace(">Network rate<", ">네트워크 속도<")
            .replace(">Uptime<", ">가동 시간<")
            .replace(">Fan Control Health<", ">팬 제어 상태<")
            .replace(">Technical details<", ">기술 정보<")
            .replace(">Hardware Availability<", ">하드웨어 감지 상태<")
            .replace(">Hardware<", ">하드웨어<")
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
            .replace(">Installed app<", ">설치된 앱<")
            .replace(">Latest signed<", ">최신 서명 릴리스<")
            .replace(">Last attempt<", ">마지막 확인 시도<")
            .replace(">Last successful check<", ">마지막 확인 성공<")
            .replace(">Next automatic check<", ">다음 자동 확인<")
            .replace(">Core details<", ">코어 상세<")
            .replace(">Check for Updates<", ">업데이트 확인<")
            .replace(">Install Update<", ">지금 업데이트<")
            .replace(">View Release<", ">릴리즈 보기<")
            .replace(
                "Automatically checks every 6 hours. Failed checks retry; installation is your choice.",
                "6시간마다 자동 확인하며, 실패하면 재시도합니다. 설치는 직접 선택합니다.",
            )
            .replace("Checking your Mac…", "Mac 상태 확인 중…")
            .replace(
                "Waiting for the first sensor sample.",
                "첫 센서 값을 기다리는 중입니다.",
            )
            .replace(
                "macOS manages fan speed for the current workload.",
                "현재 작업에 맞춰 macOS가 팬 속도를 관리합니다.",
            )
            .replace(">Release Notes<", ">릴리즈 노트<")
            .replace(">Auto<", ">자동<")
            .replace(">Silent<", ">저소음<")
            .replace(">Quiet<", ">저소음<")
            .replace(">Balanced<", ">균형<")
            .replace(">Balance<", ">균형<")
            .replace(">Gaming<", ">게임<")
            .replace(">Game<", ">게임<")
            .replace(">Performance<", ">성능<")
            .replace(">Fast<", ">성능<")
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
            .replace(">Copy Report<", ">리포트 복사<")
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
                "Reading system sensors…",
                "시스템 센서를 읽는 중…",
            )
            .replace(
                "Reading system metrics…",
                "시스템 지표를 읽는 중…",
            )
            .replace(
                "CPU temperature sensors are unavailable.",
                "CPU 온도 센서를 읽을 수 없습니다.",
            )
            .replace(
                ">No fan sensors<",
                ">팬 센서 없음<",
            )
            .replace(
                "No fan sensors were reported. CPU, memory, and network monitoring remain available; temperature appears only when this system exposes a supported sensor.",
                "팬 센서를 찾지 못했습니다. CPU, 메모리와 네트워크는 계속 표시되며, 온도는 이 시스템이 지원 센서를 제공할 때만 표시됩니다.",
            )
            .replace(
                "Manage startup and fan-control safety.",
                "시작 동작과 팬 제어 안전 상태를 관리합니다.",
            )
            .replace(
                "One click checks, verifies, installs, and relaunches PeterFan.",
                "한 번에 최신 버전을 확인하고 검증·설치한 뒤 PeterFan을 다시 실행합니다.",
            )
            .replace(">Startup<", ">시작 설정<")
            .replace("Start on login", "부팅 시 자동 실행")
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
    };
    let mut html = html;
    if lang == ResolvedLanguage::Ko {
        for (english, korean) in [
            ("Status", "상태"),
            ("Fans", "팬 제어"),
            ("Settings", "설정"),
            ("System", "시스템"),
            ("Live summary", "실시간 요약"),
            ("Per-core CPU usage", "코어별 CPU 사용률"),
            ("Menu bar style", "메뉴 막대 표시"),
            ("Quick actions", "빠른 작업"),
            ("Runner character", "러너 캐릭터"),
            ("Update channel", "업데이트 채널"),
            ("CPU temperature warning", "CPU 온도 경고"),
            ("Fan control failures", "팬 제어 오류 알림"),
            ("App updates", "앱 업데이트 알림"),
            ("PeterFan configuration", "PeterFan 설정"),
            ("System quick facts", "시스템 요약"),
        ] {
            html = html.replace(
                &format!("aria-label=\"{english}\""),
                &format!("aria-label=\"{korean}\""),
            );
        }
    }
    html.replace("__MENU_ITEMS_SETTINGS__", include_str!("menu_items.html"))
        .replace("__MENU_ITEMS_SCRIPT__", include_str!("menu_items.js"))
        .replace("__CPU_WIDGET_SETTINGS__", include_str!("cpu_widget.html"))
        .replace("__CPU_WIDGET_SCRIPT__", include_str!("cpu_widget.js"))
        .replace("__RUNNER_OPTIONS__", &runner_picker_options(lang))
        .replace("__RUNNER_CHARACTERS__", &runner_character_values_json())
        .replace("__RUNNER_PREVIEWS__", runner_preview_alpha_json())
        .replace("__RUNNER_PREVIEW_FRAMES__", runner_preview_frames_json())
        .replace(
            "__RUNNER_SELECTED_PREVIEW__",
            &runner_selected_preview_frames_json(selected_pack),
        )
        .replace(
            "__RUNNER_SELECTED_PREVIEW_CHARACTER__",
            selected_pack.character().as_str(),
        )
}

const DASHBOARD_HTML_EN: &str = r##"<!doctype html><html lang="__LANG__"><head><meta charset="utf-8"><meta name="color-scheme" content="light dark">
<style>
:root{--g:#5dd879;--y:#f4c95d;--r:#ff6b63;--accent:#6ea8ff;--accent-soft:rgba(110,168,255,.13);--accent-medium:rgba(110,168,255,.22);--accent-border:rgba(110,168,255,.42);--ok-soft:rgba(93,216,121,.14);--warn-soft:rgba(244,201,93,.16);--danger-soft:rgba(255,107,99,.15);--danger-border:rgba(255,107,99,.36);--bg:#141517;--text:#f3f4f6;--dim:#969da8;--line:rgba(255,255,255,.085);--panel-bg:#181a1d;--surface:#1e2024;--surface-raised:#25282d;--panel-border:rgba(255,255,255,.11);--chip-bg:rgba(255,255,255,.06);--chip-hover:rgba(110,168,255,.16);--track:rgba(255,255,255,.09);--track-hover:rgba(255,255,255,.075);--shadow:0 18px 46px rgba(0,0,0,.42),0 2px 10px rgba(0,0,0,.26);--content-x:18px;--section-y:16px;--panel-pad:18px;}
@media (prefers-color-scheme: light){
:root{--g:#239f52;--y:#b47a00;--r:#d83b35;--accent:#2567bd;--accent-soft:rgba(37,103,189,.11);--accent-medium:rgba(37,103,189,.17);--accent-border:rgba(37,103,189,.32);--ok-soft:rgba(35,159,82,.12);--warn-soft:rgba(180,122,0,.13);--danger-soft:rgba(216,59,53,.11);--danger-border:rgba(216,59,53,.28);--bg:#eef0f3;--text:#202329;--dim:#69717e;--line:rgba(25,31,40,.10);--panel-bg:#f4f5f7;--surface:#fbfbfc;--surface-raised:#fff;--panel-border:rgba(25,31,40,.12);--chip-bg:rgba(25,31,40,.055);--chip-hover:rgba(37,103,189,.12);--track:rgba(25,31,40,.09);--track-hover:rgba(25,31,40,.065);--shadow:0 18px 44px rgba(27,32,40,.15),0 2px 8px rgba(27,32,40,.07);}
}
*{box-sizing:border-box;margin:0;padding:0;}
html,body{background:var(--panel-bg);font-family:-apple-system,system-ui,sans-serif;color:var(--text);-webkit-user-select:none;cursor:default;-webkit-font-smoothing:antialiased;overflow:hidden;}
.panel{background:var(--panel-bg);border:1px solid var(--panel-border);border-radius:10px;overflow:hidden;box-shadow:var(--shadow);max-height:100vh;}
.dashboard-shell{display:grid;grid-template-columns:minmax(0,1fr) 54px;gap:7px;padding:7px;height:100vh;max-height:100vh;}
.main-pane{position:relative;min-width:0;min-height:0;max-height:calc(100vh - 14px);border:1px solid var(--line);border-radius:8px;overflow-y:auto;overflow-x:hidden;scrollbar-gutter:stable;scrollbar-width:none;background:rgba(255,255,255,.012);contain:layout paint;}
.main-pane::-webkit-scrollbar{display:none;}
.data-loading{position:absolute;z-index:12;top:50px;left:var(--content-x);right:var(--content-x);display:flex;align-items:center;gap:7px;padding:9px 10px;border:1px solid var(--line);border-radius:7px;color:var(--dim);font-size:10.5px;line-height:1.35;box-shadow:0 5px 18px rgba(0,0,0,.16);transition:opacity .12s ease,visibility .12s ease;}
.data-loading-dot{width:7px;height:7px;border-radius:50%;background:var(--accent);box-shadow:0 0 0 3px var(--accent-soft);animation:data-loading-pulse 1.2s ease-in-out infinite;flex:0 0 auto;}
.loading-retry{margin-left:auto;background:var(--chip-bg);border:1px solid var(--accent-border);border-radius:5px;color:var(--accent);font:inherit;font-size:9.5px;font-weight:700;padding:3px 7px;cursor:pointer;}
.loading-retry:hover{background:var(--chip-hover);}
.loading-retry:disabled{opacity:.5;cursor:default;}
@keyframes data-loading-pulse{0%,100%{opacity:.45}50%{opacity:1}}
body.data-ready .data-loading{opacity:0;visibility:hidden;pointer-events:none;}
body.compact .compact-extra{display:none!important;}
body.compact[data-rail-view="system"] .compact-extra{display:grid!important;}
body.compact[data-rail-view="system"] .foot.compact-extra{display:block!important;}
.summary-strip{display:grid;grid-template-columns:repeat(3,minmax(0,1fr));gap:6px;padding:0 var(--content-x) 10px;}
.summary-cell{min-width:0;padding:8px 9px;border:1px solid var(--line);border-radius:7px;background:var(--chip-bg);}
.summary-label{display:block;color:var(--dim);font-size:10px;font-weight:750;letter-spacing:0;white-space:nowrap;overflow:hidden;text-overflow:ellipsis;}
.summary-value{display:block;margin-top:3px;color:var(--text);font-size:13px;font-weight:750;font-variant-numeric:tabular-nums;white-space:nowrap;overflow:hidden;text-overflow:ellipsis;}
.summary-value.g{color:var(--g);}.summary-value.y{color:var(--y);}.summary-value.r{color:var(--r);}.summary-value.info{color:var(--accent);}
.summary-core-chart{display:block;width:100%;height:24px;margin-top:6px;opacity:.92;}
.summary-core-chart[hidden]{display:none;}
.action-rail{display:flex;flex-direction:column;gap:7px;align-self:start;contain:layout paint;}
.rail-btn{height:50px;width:100%;display:flex;align-items:center;justify-content:center;background:transparent;border:1px solid transparent;border-radius:7px;color:var(--dim);font:inherit;cursor:pointer;color-scheme:inherit;}
.rail-btn:hover{background:var(--chip-hover);border-color:var(--accent-border);}
.rail-btn:active{background:var(--accent-medium);}
.rail-btn svg{width:22px;height:22px;fill:none;stroke:currentColor;stroke-width:1.7;stroke-linecap:round;stroke-linejoin:round;}
.rail-btn span{display:none;}
.rail-btn.active{background:var(--accent-soft);border-color:var(--accent-border);color:var(--accent);}
.setup{position:relative;display:flex;justify-content:space-between;align-items:center;gap:10px;padding:10px var(--content-x);border-bottom:1px solid var(--line);}
.setup-main{display:flex;align-items:center;gap:6px;font-size:11px;font-weight:700;}
.setup-dot{width:7px;height:7px;border-radius:50%;background:var(--dim);box-shadow:0 0 0 3px transparent;flex:0 0 auto;}
.setup-dot.ok{background:var(--g);box-shadow:0 0 0 3px var(--ok-soft);}
.setup-dot.info{background:var(--accent);box-shadow:0 0 0 3px var(--accent-soft);}
.setup-dot.warn{background:var(--y);box-shadow:0 0 0 3px var(--warn-soft);}
.setup-copy{min-width:0;}
.setup-sub{font-size:10px;color:var(--dim);margin-top:1px;font-variant-numeric:tabular-nums;white-space:nowrap;overflow:hidden;text-overflow:ellipsis;max-width:215px;}
.setup-progress{margin-top:5px;font-size:9px;color:var(--dim);font-variant-numeric:tabular-nums;}
.setup-progress-label{display:inline;}
.setup-progress-percent{float:right;margin-left:8px;}
.setup-progress-track{height:3px;width:100%;min-width:120px;margin-top:3px;background:var(--track);border-radius:3px;overflow:hidden;}
.setup-progress-fill{height:100%;width:0;background:var(--accent);border-radius:3px;}
.setup-progress.failed .setup-progress-fill{background:var(--y);}
.setup-actions{display:flex;gap:4px;flex:0 0 auto;}
.setup-actions button{background:var(--chip-bg);border:1px solid transparent;color:var(--dim);font:inherit;font-size:10px;font-weight:700;padding:4px 7px;border-radius:6px;cursor:pointer;white-space:nowrap;transition:background .15s,color .15s,border-color .15s;}
.setup-actions button:hover{background:var(--chip-hover);color:var(--text);}
.setup-actions button:disabled{opacity:.45;cursor:default;pointer-events:none;}
.setup-actions button.primary{background:var(--accent-medium);border-color:var(--accent-border);color:var(--accent);}
.setup-actions button.active{color:var(--g);border-color:var(--ok-soft);}
.setup-menu-wrap{position:relative;}
.setup-actions .setup-more{width:24px;height:24px;padding:0;font-size:15px;line-height:20px;color:var(--text);}
.setup-menu{display:none;position:absolute;right:0;top:29px;min-width:142px;padding:5px;background:var(--panel-bg);border:1px solid var(--panel-border);border-radius:8px;box-shadow:var(--shadow);z-index:20;}
.setup-menu.show{display:block;}
.setup-actions .setup-menu-item{display:block;width:100%;padding:6px 8px;border-radius:6px;text-align:left;color:var(--text);font-size:10.5px;font-weight:600;background:transparent;}
.setup-actions .setup-menu-item:hover{background:var(--track-hover);}
.setup-actions .setup-menu-item:focus-visible,.setup-actions .setup-more:focus-visible{outline:2px solid var(--accent);outline-offset:2px;}
.setup-actions .setup-menu-item.active{color:var(--g);border-color:transparent;}
.row{display:grid;grid-template-columns:23px 1fr;gap:13px;padding:var(--section-y) var(--content-x);align-items:center;}
#sec-mem,#sec-temp,#sec-batt,#sec-network,#sec-procs{border-top:1px solid var(--line);}
.ic{width:21px;height:21px;color:var(--dim);}
.ic svg{width:100%;height:100%;fill:none;stroke:currentColor;stroke-width:1.6;stroke-linecap:round;stroke-linejoin:round;}
.content{min-width:0;}
.head{display:flex;justify-content:space-between;align-items:baseline;gap:10px;}
.name{font-size:11.5px;font-weight:650;color:var(--text);}
.val{font-size:15px;font-weight:650;white-space:nowrap;font-variant-numeric:tabular-nums;}
.sub{font-size:10.5px;color:var(--dim);margin-top:2px;line-height:1.45;white-space:nowrap;overflow:hidden;text-overflow:ellipsis;font-variant-numeric:tabular-nums;}
.bar{height:3px;background:var(--track);border-radius:99px;margin-top:7px;overflow:hidden;}
.bar-fill{height:100%;border-radius:99px;width:0;transition:width .35s ease;}
.bar-fill.g{background:var(--g);}.bar-fill.y{background:var(--y);}.bar-fill.r{background:var(--r);}.bar-fill.b{background:var(--accent);}
.cores{display:flex;align-items:flex-end;gap:2.5px;height:22px;margin-top:8px;background:var(--track);border-radius:4px;padding:2px 3px 0;}
.core{flex:1;border-radius:1px 1px 0 0;min-height:2px;transition:height .3s ease;cursor:default;}
.core.g{background:var(--g);}.core.y{background:var(--y);}.core.r{background:var(--r);}
.core-details-head{display:block;width:100%;background:transparent;border:0;font:inherit;font-size:10.5px;font-weight:700;color:var(--dim);margin-top:7px;padding:2px 0;text-align:left;cursor:pointer;}
.core-details-head:hover{color:var(--accent);}
.core-details-list{display:none;margin-top:4px;}
.core-details-list.open{display:block;}
.core-group{padding-top:7px;margin-top:6px;border-top:1px solid var(--line);}
.core-group:first-child{padding-top:2px;margin-top:0;border-top:0;}
.core-group-head{display:flex;align-items:baseline;justify-content:space-between;gap:8px;margin-bottom:5px;font-size:10px;}
.core-group-name{font-weight:750;color:var(--text);}
.core-group-stats{color:var(--dim);font-variant-numeric:tabular-nums;white-space:nowrap;}
.core-detail-grid{display:grid;grid-template-columns:repeat(5,minmax(0,1fr));gap:4px;}
.core-detail{min-width:0;padding:5px 4px;border-radius:5px;background:var(--chip-bg);text-align:center;}
.core-detail-label{display:block;color:var(--dim);font-size:9px;font-weight:750;}
.core-detail-value{display:block;margin-top:2px;color:var(--text);font-size:10px;font-weight:750;font-variant-numeric:tabular-nums;}
.core-detail-meter{display:block;height:2px;margin-top:4px;border-radius:99px;background:var(--track);overflow:hidden;}
.core-detail-meter>span{display:block;height:100%;border-radius:99px;background:var(--g);}
.core-detail-meter>span.y{background:var(--y);}.core-detail-meter>span.r{background:var(--r);}
.trow{display:flex;justify-content:space-between;align-items:baseline;font-size:10.5px;margin-top:5px;}
.trow .l{color:var(--dim);}
.trow .v{font-weight:600;font-variant-numeric:tabular-nums;}
.v.g{color:var(--g);}.v.y{color:var(--y);}.v.r{color:var(--r);}
.trow.stale .l,.trow.stale .src,.trow.stale .v{color:var(--dim);opacity:.72;}
.val.stale{color:var(--dim);}
.all-temp-head{display:block;width:100%;background:transparent;border:0;border-top:1px solid var(--line);font:inherit;font-size:10.5px;font-weight:700;color:var(--dim);margin-top:9px;padding:8px 0 0;text-align:left;cursor:pointer;}
.all-temp-head:hover{color:var(--accent);}
.all-temp-list .trow{font-size:10.5px;margin-top:5px;gap:10px;}
.all-temp-list .trow .l{white-space:nowrap;overflow:hidden;text-overflow:ellipsis;}
.all-temp-list .trow .src{flex:0 0 auto;color:var(--dim);font-size:10px;font-weight:700;}
.sensor-group-head{margin-top:8px;padding-top:6px;border-top:1px solid var(--line);color:var(--text);font-size:10px;font-weight:750;}
.sensor-group-head:first-child{margin-top:3px;padding-top:0;border-top:0;}
.proc-tools{display:flex;align-items:center;gap:6px;margin-top:7px;}
.proc-search{width:100%;min-width:0;height:26px;padding:0 8px;border:1px solid var(--line);border-radius:6px;background:var(--chip-bg);color:var(--text);font:inherit;font-size:10.5px;outline:none;}
.proc-search:focus{border-color:var(--accent-border);box-shadow:0 0 0 2px var(--accent-soft);}
.proc-search::placeholder{color:var(--dim);}
.proc-status{min-height:14px;margin-top:5px;color:var(--dim);font-size:9.5px;line-height:1.35;}
.proc-status.ok{color:var(--g);}.proc-status.error{color:var(--r);}.proc-status.pending{color:var(--accent);}
.prow{display:grid;grid-template-columns:minmax(0,1fr) auto auto auto 52px;gap:7px;align-items:center;min-height:24px;font-size:10.5px;margin-top:3px;}
.pkill{width:52px;height:21px;padding:0 5px;border:1px solid var(--line);border-radius:5px;background:var(--chip-bg);color:var(--dim);font:inherit;font-size:9.5px;font-weight:700;cursor:pointer;transition:background .15s,color .15s,border-color .15s;}
.pkill:hover,.pkill:focus-visible{background:var(--chip-hover);border-color:var(--accent-border);color:var(--text);}
.pkill.confirm{background:var(--warn-soft);border-color:var(--y);color:var(--y);}
.pkill:disabled{opacity:.35;cursor:default;pointer-events:none;}
.prow .n{white-space:nowrap;overflow:hidden;text-overflow:ellipsis;}
.prow .pid{color:var(--dim);font-size:9px;font-variant-numeric:tabular-nums;white-space:nowrap;}
.prow .c{color:var(--accent);font-weight:600;font-variant-numeric:tabular-nums;white-space:nowrap;}
.prow .m{color:var(--dim);font-variant-numeric:tabular-nums;white-space:nowrap;}
.ctl{padding:0 var(--content-x) 13px;border-top:1px solid var(--line);}
.ctl.focus-pulse{background:var(--accent-soft);box-shadow:inset 0 0 0 1px var(--accent-border);}
.ctl-head{display:flex;justify-content:space-between;align-items:baseline;padding:11px 0 8px;margin:0;border-bottom:1px solid var(--line);}
.ctl-head .name{font-size:11.5px;font-weight:700;color:var(--text);}
.ctl-status{display:block;max-width:58%;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;color:var(--dim);font-size:9.5px;font-weight:650;font-variant-numeric:tabular-nums;}
.fan-inputs{display:grid;grid-template-columns:repeat(3,minmax(0,1fr));gap:6px;padding:10px 0 8px;}
.fan-input{min-width:0;padding:5px 6px;border:1px solid var(--line);border-radius:6px;background:rgba(255,255,255,.018);}
.fan-input span{display:block;font-size:10px;font-weight:700;color:var(--dim);white-space:nowrap;overflow:hidden;text-overflow:ellipsis;}
.fan-input b{display:block;margin-top:2px;font-size:10px;font-weight:750;font-variant-numeric:tabular-nums;white-space:nowrap;}
.profile-strip{display:grid;grid-template-columns:repeat(3,minmax(0,1fr));gap:5px;margin:2px 0 9px;}
.profile-strip button{min-width:0;background:var(--chip-bg);border:1px solid transparent;color:var(--dim);font:inherit;font-size:10px;font-weight:700;padding:7px 3px;border-radius:6px;cursor:pointer;white-space:nowrap;overflow:hidden;text-overflow:ellipsis;transition:background .15s,color .15s,border-color .15s,opacity .15s;}
.profile-strip button:disabled{opacity:.42;cursor:default;pointer-events:none;}
.profile-strip button:hover{background:var(--chip-hover);color:var(--text);}
.profile-strip button.active{background:var(--accent-medium);border-color:var(--accent-border);color:var(--accent);}
.profile-strip.disabled button{opacity:.42;pointer-events:none;}
.profile-strip.pending button{cursor:progress;}
.profile-strip.pending button:not(.active){opacity:.48;}
.profile-strip.pending button.active{box-shadow:inset 0 -2px 0 var(--accent);}
.fan-apply-status{min-height:15px;margin:-2px 0 5px;color:var(--dim);font-size:10.5px;line-height:1.45;font-variant-numeric:tabular-nums;white-space:nowrap;overflow:hidden;text-overflow:ellipsis;}
.fan-apply-status.pending{color:var(--accent);}
.fan-apply-status.ok{color:var(--g);}
.fan-apply-status.error{color:var(--r);}
.fan-cards{display:flex;flex-direction:column;}
.fan-card{padding:9px 0;}
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
.fan-seg button{background:var(--chip-bg);border:1px solid transparent;color:var(--dim);font:inherit;font-size:10px;font-weight:600;padding:4px 8px;border-radius:5px;cursor:pointer;white-space:nowrap;transition:background .15s,color .15s;}
.fan-seg button.active{background:var(--panel-bg);color:var(--text);border-color:rgba(91,157,255,.4);}
.fan-card.pending .fan-seg button{cursor:progress;}
.fan-card.pending .fan-seg button:not(.active){opacity:.48;}
.fan-rpm-row{display:grid;grid-template-columns:auto 1fr auto auto;gap:6px;align-items:center;margin-top:5px;transition:opacity .15s;}
.fan-rpm-row.inactive{opacity:.35;pointer-events:none;}
.fan-rpm-row span{font-size:10px;color:var(--dim);font-variant-numeric:tabular-nums;white-space:nowrap;}
.fan-rpm-row input[type=range]{-webkit-appearance:none;height:3px;border-radius:99px;background:var(--track);outline:none;cursor:pointer;}
.fan-rpm-row input[type=range]::-webkit-slider-thumb{-webkit-appearance:none;width:14px;height:14px;border-radius:50%;background:var(--accent);cursor:pointer;}
.fan-rpm-row input[type=number]{width:44px;background:var(--track);border:1px solid transparent;border-radius:4px;color:var(--text);font:inherit;font-size:9px;font-variant-numeric:tabular-nums;text-align:center;padding:3px 0;-moz-appearance:textfield;}
.fan-rpm-row input[type=number]::-webkit-inner-spin-button,.fan-rpm-row input[type=number]::-webkit-outer-spin-button{-webkit-appearance:none;margin:0;}
.fan-rpm-row input[type=number]:focus{border-color:var(--accent);outline:none;}
.empty-state{display:flex;flex-direction:column;gap:3px;padding:10px;border:1px dashed var(--line);border-radius:8px;color:var(--dim);font-size:10.5px;line-height:1.45;background:rgba(255,255,255,.018);}
.empty-state-title{font-size:10.5px;color:var(--text);font-weight:700;}
.empty-state-copy{font-size:9.5px;color:var(--dim);line-height:1.5;}
.metric-empty{margin-top:6px;color:var(--dim);font-size:10px;line-height:1.45;}
.ctl-note{font-size:10.5px;color:var(--dim);line-height:1.5;margin-top:6px;}
.note-fix-btn{margin-top:5px;background:var(--accent-medium);border:1px solid var(--accent-border);color:var(--accent);font:inherit;font-size:10px;font-weight:600;padding:5px 10px;border-radius:6px;cursor:pointer;}
.note-fix-btn:hover{background:var(--chip-hover);}
#curve-canvas{width:100%;height:120px;display:block;border-radius:6px;background:var(--track);cursor:crosshair;touch-action:none;margin-top:8px;}
.curve-point-row{display:flex;align-items:center;gap:5px;margin-top:8px;font-size:9px;color:var(--dim);font-variant-numeric:tabular-nums;}
.curve-point-row .cpr-arrow{color:var(--dim);}
.curve-point-row input[type=number]{width:40px;background:var(--track);border:1px solid transparent;border-radius:4px;color:var(--text);font:inherit;font-size:9px;font-variant-numeric:tabular-nums;text-align:center;padding:3px 0;-moz-appearance:textfield;}
.curve-point-row input[type=number]::-webkit-inner-spin-button,.curve-point-row input[type=number]::-webkit-outer-spin-button{-webkit-appearance:none;margin:0;}
.curve-point-row input[type=number]:focus{border-color:var(--accent);outline:none;}
.curve-actions{display:flex;gap:6px;margin-top:8px;}
.curve-actions button{flex:1;background:var(--chip-bg);border:1px solid transparent;color:var(--text);font:inherit;font-size:10px;font-weight:600;padding:6px 4px;border-radius:7px;cursor:pointer;transition:background .15s;}
.curve-actions button:hover{background:var(--chip-hover);}
.curve-actions button.primary{background:var(--accent-medium);border-color:var(--accent-border);color:var(--accent);}
.chart{width:100%;height:32px;display:block;margin-top:9px;border-radius:4px;cursor:crosshair;}
.chart-tip{position:fixed;pointer-events:none;background:rgba(20,20,22,.92);color:#fff;font-size:10.5px;font-weight:600;padding:3px 7px;border-radius:5px;display:none;z-index:999;white-space:nowrap;font-variant-numeric:tabular-nums;}
.chart-stats{font-size:10px;color:var(--dim);text-align:right;margin-top:3px;font-variant-numeric:tabular-nums;}
.rail-panel{display:none;padding:var(--panel-pad) var(--content-x);border-bottom:1px solid var(--line);}
.panel-title-row{display:flex;align-items:center;justify-content:space-between;gap:10px;margin-bottom:10px;}
.rail-panel .panel-title{font-size:15px;font-weight:700;min-width:0;}
.panel-pill{display:inline-flex;align-items:center;height:20px;padding:0 8px;border-radius:99px;background:var(--chip-bg);color:var(--dim);font-size:9.5px;font-weight:800;white-space:nowrap;font-variant-numeric:tabular-nums;}
.panel-pill.ok{background:var(--ok-soft);color:var(--g);}
.panel-pill.warn{background:var(--warn-soft);color:var(--y);}
.panel-pill.info{background:var(--accent-soft);color:var(--accent);}
.rail-panel .panel-copy{display:none;}
#rail-update-panel .panel-copy{display:block;color:var(--dim);font-size:10.5px;line-height:1.5;margin-bottom:10px;overflow-wrap:anywhere;}
.view-loading{position:absolute;z-index:8;top:48px;left:var(--content-x);right:var(--content-x);display:flex;align-items:center;gap:7px;padding:7px 9px;border:1px solid var(--line);border-radius:6px;color:var(--dim);font-size:10px;line-height:1.35;background:var(--surface-raised);box-shadow:0 5px 16px rgba(0,0,0,.14);}
.view-loading .data-loading-dot{width:6px;height:6px;}
#rail-settings-pill,#rail-more-pill{display:none;}
#rail-more-panel{position:relative;}
.rail-panel .panel-action{min-height:30px;background:var(--accent-medium);border:1px solid var(--accent-border);color:var(--accent);font:inherit;font-size:11px;font-weight:700;padding:6px 10px;border-radius:7px;cursor:pointer;}
.rail-panel .panel-action.secondary{background:var(--chip-bg);border-color:transparent;color:var(--text);}
.rail-panel .panel-action.danger{background:var(--danger-soft);border-color:var(--danger-border);color:var(--r);}
.rail-panel .panel-actions{display:flex;gap:8px;align-items:center;flex-wrap:wrap;}
.release-notes-card{margin-top:10px;padding:9px 10px;border:1px solid var(--line);border-radius:8px;background:rgba(255,255,255,.025);}
.release-notes-title{font-size:11px;font-weight:800;color:var(--text);margin-bottom:5px;}
.release-notes-body{font-size:10.5px;line-height:1.45;color:var(--dim);white-space:pre-wrap;max-height:118px;overflow:hidden;}
.settings-list{display:flex;flex-direction:column;gap:0;}
.settings-item{display:flex;align-items:center;justify-content:space-between;gap:10px;padding:10px 0;border-top:1px solid var(--line);}
.settings-item:first-child{border-top:0;}
.settings-item-title{font-size:11.5px;font-weight:700;color:var(--text);}
.settings-item-copy{display:none;}
.settings-control-stack{display:flex;flex-direction:column;align-items:flex-end;gap:5px;width:184px;min-width:0;}
.display-segment{display:grid;grid-template-columns:repeat(3,minmax(0,1fr));gap:3px;width:100%;padding:3px;border-radius:8px;background:var(--chip-bg);}
.display-segment button{min-width:0;min-height:27px;padding:4px 5px;border:0;border-radius:6px;background:transparent;color:var(--dim);font:inherit;font-size:9.5px;font-weight:750;cursor:pointer;white-space:nowrap;}
.display-segment button:hover{background:var(--track-hover);color:var(--text);}
.display-segment button.active{background:var(--surface-raised);color:var(--text);box-shadow:0 1px 4px rgba(0,0,0,.15);}
.character-picker{position:relative;width:184px;}
.character-select{display:grid;grid-template-columns:38px minmax(0,1fr) 10px;align-items:center;gap:7px;width:100%;height:40px;padding:3px 8px 3px 5px;border:1px solid var(--line);border-radius:7px;background:var(--chip-bg);color:var(--text);font:inherit;font-size:10px;font-weight:700;text-align:left;cursor:pointer;color-scheme:inherit;}
.character-select:hover,.character-select[aria-expanded="true"]{border-color:var(--accent-border);background-color:var(--track-hover);}
.character-select:focus-visible{outline:2px solid var(--accent);outline-offset:1px;}
.runner-preview{display:block;width:27px;height:18px;opacity:.92;image-rendering:auto;pointer-events:none;}
.character-select .runner-preview{width:27px;height:18px;}
.character-label{white-space:nowrap;overflow:hidden;text-overflow:ellipsis;}
.character-chevron{width:7px;height:7px;border-right:1.5px solid currentColor;border-bottom:1.5px solid currentColor;transform:translateY(-2px) rotate(45deg);color:var(--dim);transition:transform .12s ease;}
.character-select[aria-expanded="true"] .character-chevron{transform:translateY(2px) rotate(225deg);}
.random-runner-icon{font-size:21px;text-align:center;line-height:18px;color:var(--dim)}
.character-menu{display:none;position:fixed;z-index:1000;width:224px;max-height:260px;overflow-y:auto;padding:5px;border:1px solid var(--panel-border);border-radius:8px;background:var(--surface-raised);box-shadow:var(--shadow);scrollbar-width:thin;overscroll-behavior:contain;}
.character-menu.show{display:block;}
.character-option{display:grid;grid-template-columns:38px minmax(0,1fr) 14px;align-items:center;gap:7px;width:100%;height:34px;padding:1px 7px 1px 4px;border:0;border-radius:6px;background:transparent;color:var(--text);font:inherit;font-size:10.5px;font-weight:650;text-align:left;cursor:pointer;}
.character-option:hover,.character-option:focus-visible{background:var(--chip-hover);outline:none;}
.character-option[aria-selected="true"]{background:var(--accent-soft);color:var(--accent);}
.character-option-label{white-space:nowrap;overflow:hidden;text-overflow:ellipsis;}
.character-option-check{display:none;width:8px;height:5px;border-left:1.5px solid currentColor;border-bottom:1.5px solid currentColor;transform:translateY(-1px) rotate(-45deg);}
.character-option[aria-selected="true"] .character-option-check{display:block;}
.runner-pace{color:var(--dim);font-size:9.5px;font-variant-numeric:tabular-nums;text-align:right;}
.system-facts{display:grid;grid-template-columns:repeat(2,minmax(0,1fr));margin:0 0 10px;border-top:1px solid var(--line);border-bottom:1px solid var(--line);}
.system-fact{min-width:0;padding:8px 10px;}
.system-fact:nth-child(even){border-left:1px solid var(--line);}
.system-fact:nth-child(n+3){border-top:1px solid var(--line);}
.system-fact-label{display:block;color:var(--dim);font-size:9.5px;font-weight:700;}
.system-fact-value{display:block;margin-top:3px;color:var(--text);font-size:11px;font-weight:750;font-variant-numeric:tabular-nums;white-space:nowrap;overflow:hidden;text-overflow:ellipsis;}
.health-card{padding:10px 0;border-top:1px solid var(--line);}
.settings-details{border-top:1px solid var(--line);padding:10px 0;}
.settings-details>summary{display:flex;align-items:center;justify-content:space-between;gap:10px;color:var(--text);font-size:11.5px;font-weight:700;cursor:pointer;list-style:none;}
.settings-details>summary::-webkit-details-marker{display:none;}
.settings-details>summary:after{content:"›";color:var(--dim);font-size:16px;line-height:1;transform:rotate(0deg);transition:transform .12s ease;}
.settings-details[open]>summary:after{transform:rotate(90deg);}
.settings-details[open]>summary{margin-bottom:10px;}
.settings-details>summary .panel-pill{margin-left:auto;}
.notification-list{display:flex;flex-direction:column;}
.notification-row{display:flex;align-items:center;justify-content:space-between;gap:12px;min-height:38px;padding:7px 0;border-top:1px solid var(--line);}
.notification-row:first-child{border-top:0;}
.notification-title{color:var(--text);font-size:10.5px;font-weight:700;}
.notification-copy{margin-top:2px;color:var(--dim);font-size:9px;line-height:1.35;}
.notification-control{display:flex;align-items:center;justify-content:flex-end;gap:6px;flex:0 0 auto;}
.automation-copy{margin:0 0 8px;color:var(--dim);font-size:10px;line-height:1.4;}
.automation-list{display:flex;flex-direction:column;gap:7px;}
.automation-rule{padding:8px;border:1px solid var(--line);border-radius:8px;background:var(--chip-bg);}
.automation-rule-head{display:flex;align-items:center;justify-content:space-between;gap:8px;margin-bottom:6px;color:var(--dim);font-size:9.5px;font-weight:750;}
.automation-rule-number{font-variant-numeric:tabular-nums;}
.automation-rule-remove{border:0;background:transparent;color:var(--dim);font:inherit;font-size:14px;line-height:1;cursor:pointer;padding:0 2px;}
.automation-rule-remove:hover{color:var(--r);}
.automation-rule-grid{display:grid;grid-template-columns:minmax(0,1fr) minmax(0,1fr);gap:6px;}
.automation-rule-field{display:flex;flex-direction:column;gap:3px;min-width:0;}
.automation-rule-field>span{color:var(--dim);font-size:9px;font-weight:700;}
.automation-rule-field select,.automation-rule-field input{width:100%;min-width:0;box-sizing:border-box;padding:5px 6px;border:1px solid var(--line);border-radius:6px;background:var(--surface-raised);color:var(--text);font:inherit;font-size:10px;font-variant-numeric:tabular-nums;color-scheme:inherit;}
.automation-rule-field select:focus,.automation-rule-field input:focus{border-color:var(--accent);outline:2px solid var(--accent-soft);}
.automation-rule-extra{display:flex;align-items:center;gap:5px;margin-top:6px;color:var(--dim);font-size:9.5px;}
.automation-rule-extra input{width:42px!important;text-align:center;}
.automation-actions{display:flex;gap:7px;margin-top:9px;}
.automation-actions button{flex:1;}
.automation-status{min-height:15px;margin-top:6px;color:var(--dim);font-size:10px;line-height:1.4;}
.automation-status.ok{color:var(--g);}.automation-status.warn{color:var(--y);}
.config-transfer-copy{margin:0 0 8px;color:var(--dim);font-size:10px;line-height:1.4;}
.config-transfer-text{box-sizing:border-box;width:100%;min-height:132px;resize:vertical;border:1px solid var(--line);border-radius:8px;background:var(--input);color:var(--text);padding:8px;font:10px/1.45 ui-monospace,SFMono-Regular,Menlo,monospace;outline:none;}
.config-transfer-text:focus{border-color:var(--accent);}
.config-transfer-actions{display:flex;gap:7px;margin-top:8px;}
.config-transfer-actions button{flex:1;}
.config-transfer-status{min-height:15px;margin-top:6px;color:var(--dim);font-size:10px;line-height:1.4;}
.config-transfer-status.ok{color:var(--g);}.config-transfer-status.warn{color:var(--y);}
.notification-threshold{width:50px;height:25px;padding:3px 4px;border:1px solid var(--line);border-radius:5px;background:var(--chip-bg);color:var(--text);font:inherit;font-size:10px;font-weight:700;text-align:center;font-variant-numeric:tabular-nums;}
.notification-threshold:disabled{opacity:.45;}
.notification-threshold:focus{outline:2px solid var(--accent);outline-offset:1px;}
.notification-unit{color:var(--dim);font-size:9.5px;}
.notification-toggle{-webkit-appearance:none;appearance:none;position:relative;width:32px;height:18px;border:1px solid var(--line);border-radius:99px;background:var(--track);cursor:pointer;transition:background .15s,border-color .15s;}
.notification-toggle:after{content:"";position:absolute;top:2px;left:2px;width:12px;height:12px;border-radius:50%;background:var(--dim);transition:transform .15s,background .15s;}
.notification-toggle:checked{background:var(--accent-medium);border-color:var(--accent-border);}
.notification-toggle:checked:after{transform:translateX(14px);background:var(--accent);}
.notification-toggle:focus-visible{outline:2px solid var(--accent);outline-offset:2px;}
#fan-action-log-card{margin-top:10px;padding-top:10px;border-top:1px solid var(--line);}
.sensor-loading{padding:8px 0;color:var(--dim);font-size:10.5px;}
.health-head{display:flex;align-items:center;justify-content:space-between;gap:10px;margin-bottom:8px;}
.health-title{font-size:11px;font-weight:800;color:var(--text);}
.health-grid{display:grid;grid-template-columns:1fr;gap:6px;}
.health-row{display:flex;align-items:baseline;justify-content:space-between;gap:12px;font-size:10.5px;border-top:1px solid var(--line);padding-top:6px;}
.health-row:first-child{border-top:0;padding-top:0;}
.health-label{color:var(--dim);font-weight:650;}
.health-value{font-weight:650;text-align:right;font-variant-numeric:tabular-nums;white-space:normal;overflow-wrap:anywhere;}
.update-channel-select{max-width:150px;border:1px solid var(--line);border-radius:6px;background:var(--chip-bg);color:var(--text);font:inherit;font-size:10px;padding:4px 6px;color-scheme:inherit;}
.update-channel-select:focus-visible{outline:2px solid var(--accent);outline-offset:1px;}
.health-value.ok{color:var(--g);}.health-value.warn{color:var(--y);}.health-value.info{color:var(--accent);}
.health-action{background:var(--chip-bg);border:1px solid transparent;color:var(--accent);font:inherit;font-size:10px;font-weight:750;padding:4px 7px;border-radius:6px;cursor:pointer;white-space:nowrap;}
.health-action:disabled{opacity:.5;cursor:default;}
.health-actions{display:flex;align-items:center;gap:5px;}
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
.quit{display:block;width:100%;background:transparent;border:0;color:var(--dim);font:inherit;font-size:10.5px;padding:8px;border-radius:8px;cursor:pointer;transition:background .15s,color .15s;}
.quit:hover{background:var(--track-hover);color:var(--text);}
.range-tabs{display:flex;gap:4px;padding:12px var(--content-x) 8px;justify-content:flex-end;align-items:center;min-height:40px;}
.view-title{margin-right:auto;font-size:15px;font-weight:700;color:var(--text);}
.sort-tabs{display:flex;gap:4px;}
.range-tab{background:var(--chip-bg);border:1px solid transparent;color:var(--dim);font:inherit;font-size:9.5px;font-weight:600;padding:3px 9px;border-radius:99px;cursor:pointer;transition:background .15s,color .15s;}
.range-tab:hover{background:var(--chip-hover);}
.range-tab.active{background:rgba(91,157,255,.22);color:var(--accent);}
.health-verdict{display:flex;align-items:center;gap:10px;padding:10px var(--content-x);border-bottom:1px solid var(--line);background:var(--chip-bg);}
.health-verdict-dot{width:9px;height:9px;border-radius:3px;background:var(--dim);box-shadow:0 0 0 4px rgba(150,157,168,.10);flex:0 0 auto;}
.health-verdict-copy{display:flex;align-items:baseline;gap:8px;min-width:0;}
.health-verdict-title{font-size:12px;font-weight:800;white-space:nowrap;}
.health-verdict-detail{min-width:0;color:var(--dim);font-size:10.5px;line-height:1.35;white-space:nowrap;overflow:hidden;text-overflow:ellipsis;font-variant-numeric:tabular-nums;}
.health-verdict.ok .health-verdict-dot{background:var(--g);box-shadow:0 0 0 4px var(--ok-soft);}
.health-verdict.info .health-verdict-dot{background:var(--accent);box-shadow:0 0 0 4px var(--accent-soft);}
.health-verdict.warm .health-verdict-dot{background:var(--y);box-shadow:0 0 0 4px var(--warn-soft);}
.health-verdict.hot .health-verdict-dot{background:var(--r);box-shadow:0 0 0 4px var(--danger-soft);}
.health-verdict.ok .health-verdict-title{color:var(--g);}
.health-verdict.info .health-verdict-title{color:var(--accent);}
.health-verdict.warm .health-verdict-title{color:var(--y);}
.health-verdict.hot .health-verdict-title{color:var(--r);}
/* Product hierarchy: one compact live summary, calm detail bands, and an icon rail. */
.panel{border-radius:12px;background:var(--panel-bg);}
.dashboard-shell{grid-template-columns:minmax(0,1fr) 50px;gap:0;padding:0;}
.main-pane{max-height:100vh;border:0;border-radius:0;background:var(--surface);scrollbar-gutter:auto;}
.range-tabs{margin:0;padding:14px var(--content-x) 12px;min-height:50px;border:0;border-bottom:1px solid var(--line);border-radius:0;background:transparent;}
.view-title{font-size:16px;font-weight:750;letter-spacing:0;}
.range-tab{min-width:32px;min-height:28px;padding:4px 8px;border-radius:6px;font-size:10.5px;font-weight:700;letter-spacing:0;}
.range-tab.active{background:var(--surface-raised);box-shadow:0 1px 4px rgba(0,0,0,.15);color:var(--accent);}
body:not([data-rail-view="overview"]) .range-tabs .range-tab{display:none;}
.summary-strip{grid-template-columns:repeat(4,minmax(0,1fr));gap:0;padding:12px var(--content-x) 14px;border-bottom:1px solid var(--line);}
.summary-cell{position:relative;min-height:50px;padding:0 10px;border:0;border-left:1px solid var(--line);border-radius:0;background:transparent;overflow:hidden;}
.summary-cell:first-child{padding-left:0;border-left:0;}
.summary-cell:last-child{padding-right:0;}
.summary-label{font-size:10px;font-weight:650;letter-spacing:0;}
.summary-value{margin-top:5px;font-size:17px;line-height:1.05;font-weight:750;letter-spacing:0;}
.summary-meter{display:block;height:3px;margin-top:8px;border-radius:99px;background:var(--track);overflow:hidden;}
.summary-meter .bar-fill{display:block;}
.summary-cell.cpu.core-graph-enabled{min-height:82px;}
.row{grid-template-columns:21px minmax(0,1fr);gap:13px;padding:var(--section-y) var(--content-x);}
.ic{opacity:.82;}
.name{font-size:11.5px;font-weight:700;letter-spacing:0;}
.val{font-size:15px;font-weight:700;letter-spacing:0;}
.sub{margin-top:3px;}
.cores{height:24px;margin-top:9px;background:var(--track);}
.chart{height:34px;margin-top:10px;}
.data-loading{background:var(--chip-bg);}
.setup{padding:12px var(--content-x);}
.rail-panel{padding:var(--panel-pad) var(--content-x);}
.panel-title-row{margin-bottom:12px;}
.rail-panel .panel-title{font-size:16px;font-weight:750;letter-spacing:0;}
.settings-item{padding:12px 0;}
.ctl{padding:0 var(--content-x) 16px;}
.ctl-head{padding:14px 0 10px;}
.profile-strip{gap:6px;margin:4px 0 10px;}
.profile-strip button{min-height:31px;border-radius:7px;}
.fan-input{padding:7px 8px;border-radius:7px;background:var(--chip-bg);}
.fan-card{padding:11px 0;}
.fan-inputs{gap:0;padding:11px 0 12px;border-bottom:1px solid var(--line);}
.fan-input{padding:0 10px;border:0;border-left:1px solid var(--line);border-radius:0;background:transparent;}
.fan-input:first-child{padding-left:0;border-left:0;}
.fan-input:last-child{padding-right:0;}
.fan-input span{font-size:10px;font-weight:650;letter-spacing:0;}
.fan-input b{margin-top:4px;font-size:11px;}
.profile-strip{padding:4px;gap:3px;border-radius:9px;background:var(--chip-bg);}
.profile-strip{position:relative;overflow:hidden;}
.profile-strip button{min-height:30px;padding:6px 3px;border:0;border-radius:6px;background:transparent;transform-origin:center;}
.profile-strip button:hover{background:var(--track-hover);}
.profile-strip button.active{border-color:transparent;background:var(--surface-raised);box-shadow:0 1px 4px rgba(0,0,0,.15);color:var(--text);}
.profile-strip button.active:disabled{opacity:.8;}
.profile-strip.pending::after{content:"";position:absolute;left:0;bottom:0;width:34%;height:2px;border-radius:2px;background:var(--accent);transform:translateX(-110%);animation:fan-pending-slide 1.1s ease-in-out infinite;}
.profile-strip.confirmed button.active{color:var(--g);box-shadow:0 1px 4px rgba(0,0,0,.15),inset 0 0 0 1px var(--ok-soft);}
.profile-strip.failed{box-shadow:inset 0 0 0 1px var(--danger-border);}
@keyframes fan-pending-slide{0%{transform:translateX(-110%)}100%{transform:translateX(305%)}}
.profile-guide{display:grid;grid-template-columns:minmax(0,1fr) 64px;gap:10px;align-items:center;margin:0 0 8px;padding:8px 9px;border:1px solid var(--line);border-radius:7px;background:rgba(255,255,255,.018);}
.profile-guide-copy{min-width:0;}
.profile-guide-title{display:block;color:var(--text);font-size:11px;font-weight:750;white-space:nowrap;overflow:hidden;text-overflow:ellipsis;}
.profile-guide-detail{display:block;margin-top:2px;color:var(--dim);font-size:10.5px;line-height:1.35;white-space:nowrap;overflow:hidden;text-overflow:ellipsis;}
.profile-preview-bars{display:flex;align-items:flex-end;justify-content:flex-end;gap:3px;height:28px;}
.profile-preview-bars span{width:8px;min-height:2px;border-radius:2px 2px 1px 1px;background:var(--accent);opacity:.82;transition:height .18s ease;}
.profile-guide[data-mode="auto"] .profile-preview-bars span{background:var(--g);opacity:.6;}
.fan-apply-status{display:flex;align-items:center;gap:6px;min-height:17px;margin:0 0 5px;}
.fan-apply-status::before{content:"";display:block;width:6px;height:6px;border-radius:2px;background:currentColor;opacity:.9;flex:0 0 auto;}
.fan-apply-status:empty::before{display:none;}
.fan-bar{height:4px;margin-bottom:6px;}
.fan-seg{padding:2px;border-radius:7px;background:var(--chip-bg);}
.fan-seg button{padding:3px 7px;border:0;border-radius:5px;background:transparent;transform-origin:center;}
.fan-seg button.active{border-color:transparent;background:var(--surface-raised);box-shadow:0 1px 3px rgba(0,0,0,.14);}
.panel-pill{height:22px;padding:0 8px;border-radius:7px;font-size:9px;}
.rail-panel .panel-title{font-size:12.5px;font-weight:700;}
.rail-panel .panel-action{min-height:31px;border-radius:7px;}
#rail-settings-panel{padding-bottom:6px;}
#rail-update-panel{border-bottom:0;}
#rail-more-panel{padding-bottom:14px;}
.settings-item{min-height:48px;}
.settings-details{padding:12px 0;}
.settings-details>summary{min-height:24px;}
.health-row{min-height:22px;align-items:center;}
.release-notes-card{border-radius:7px;background:var(--chip-bg);}
.profile-strip button:not(:disabled):active,.fan-seg button:not(:disabled):active,.panel-action:not(:disabled):active,.range-tab:not(:disabled):active,.setup-actions button:not(:disabled):active{transform:scale(.97);}
.action-rail{align-self:stretch;gap:4px;padding:10px 5px;border-left:1px solid var(--line);background:var(--panel-bg);}
.rail-btn{height:44px;border:0;border-radius:8px;transition:background .15s,color .15s,transform .1s;}
.rail-btn:hover{border-color:transparent;background:var(--chip-hover);}
.rail-btn:active{transform:scale(.96);}
.rail-btn svg{width:19px;height:19px;}
.rail-btn.active{border-color:transparent;background:var(--accent-soft);box-shadow:none;color:var(--accent);}
button:focus-visible,input:focus-visible,summary:focus-visible{outline:2px solid var(--accent);outline-offset:2px;}
@media (prefers-reduced-motion:reduce){
*,*::before,*::after{animation-duration:.01ms!important;animation-iteration-count:1!important;transition-duration:.01ms!important;scroll-behavior:auto!important;}
.profile-strip.pending::after{width:100%;transform:none;animation:none;opacity:.65;}
}
@media (prefers-color-scheme: light){
.range-tab.active{box-shadow:0 1px 4px rgba(24,32,44,.10);}
.rail-btn.active{background:var(--accent-soft);}
}
</style></head><body class="compact" data-rail-view="overview"><div class="panel"><div class="dashboard-shell"><main class="main-pane">

<div class="range-tabs" id="range-tabs">
<div class="view-title">Status</div>
<button class="range-tab active" data-range="2m" aria-pressed="true" onclick="setChartRange('2m')">2m</button>
<button class="range-tab" data-range="1h" aria-pressed="false" onclick="setChartRange('1h')">1h</button>
<button class="range-tab" data-range="1d" aria-pressed="false" onclick="setChartRange('1d')">1d</button>
</div>

<div class="health-verdict info" id="health-verdict" role="status" aria-live="polite" aria-atomic="true">
<span class="health-verdict-dot" aria-hidden="true"></span>
<div class="health-verdict-copy"><strong class="health-verdict-title" id="health-verdict-title">Checking your Mac…</strong><span class="health-verdict-detail" id="health-verdict-detail">Waiting for the first sensor sample.</span></div>
</div>

<div class="summary-strip" id="summary-strip" aria-label="Live summary">
<div class="summary-cell cpu"><span class="summary-label" id="summary-cpu-label">CPU</span><span class="summary-value" id="summary-cpu">—</span><span class="summary-meter"><span class="bar-fill" id="summary-cpu-bar"></span></span><canvas class="summary-core-chart" id="summary-cpu-core-chart" aria-label="Per-core CPU usage" role="img"></canvas></div>
<div class="summary-cell memory"><span class="summary-label" id="summary-mem-label">Memory</span><span class="summary-value" id="summary-mem">—</span><span class="summary-meter"><span class="bar-fill" id="summary-mem-bar"></span></span></div>
<div class="summary-cell temperature"><span class="summary-label" id="summary-temp-label">CPU temp</span><span class="summary-value" id="summary-temp">—</span><span class="summary-meter"><span class="bar-fill" id="summary-temp-bar"></span></span></div>
<div class="summary-cell fan"><span class="summary-label" id="summary-fan-label">Fan RPM</span><span class="summary-value" id="summary-fan">—</span><span class="summary-meter"><span class="bar-fill info" id="summary-fan-bar"></span></span></div>
</div>

<div class="data-loading" id="data-loading" role="status" aria-live="polite"><span class="data-loading-dot"></span><span id="data-loading-text">Reading system sensors…</span><button class="loading-retry" id="data-loading-retry" style="display:none" onclick="retryDashboard()">Retry</button></div>

<div class="setup" id="setup-row">
<div class="setup-copy"><div class="setup-main"><span class="setup-dot" id="setup-dot"></span><span id="setup-title">Ready</span></div><div class="setup-sub" id="setup-detail"></div><div class="setup-progress" id="setup-progress" style="display:none"><div class="setup-progress-label" id="setup-progress-label" role="status" aria-live="polite"></div><span class="setup-progress-percent" id="setup-progress-percent" aria-hidden="true">0%</span><div class="setup-progress-track" id="setup-progress-track" role="progressbar" aria-label="Installation stages" aria-valuemin="0" aria-valuemax="100"><div class="setup-progress-fill" id="setup-progress-fill"></div></div></div></div>
<div class="setup-actions">
<button id="setup-fan" class="primary" disabled onclick="handleFanSetupAction(this)">Set Up</button>
<div class="setup-menu-wrap">
<button id="setup-more" class="setup-more" disabled onclick="toggleSetupMenu(event)" onkeydown="handleSetupMoreKey(event)" aria-label="Setup actions" aria-haspopup="menu" aria-expanded="false" title="Setup actions">…</button>
<div class="setup-menu" id="setup-menu" role="menu" onkeydown="handleSetupMenuKey(event)">
<button class="setup-menu-item" role="menuitem" id="setup-startup" disabled onclick="closeSetupMenu();toggleStartupItem(this)">Start on login</button>
<button class="setup-menu-item" role="menuitem" id="setup-update" disabled onclick="closeSetupMenu();installAppUpdate(this)">Update</button>
</div>
</div>
</div>
</div>

<div class="rail-panel" id="rail-settings-panel">
<div class="panel-title-row"><div class="panel-title">General</div><span class="panel-pill info" id="rail-settings-pill">PeterFan</span></div>
<div class="panel-copy">Manage startup and fan-control safety.</div>
<div class="settings-list">
<div class="settings-item" id="startup-setting">
<div><div class="settings-item-title">Start on login</div><div class="settings-item-copy">Run PeterFan automatically on startup.</div></div>
<button id="startup-toggle" class="panel-action secondary" disabled onclick="toggleStartupItem(this)">Enable</button>
</div>
<div class="settings-item" id="menubar-display-setting">
<div><div class="settings-item-title">Menu bar</div><div class="settings-item-copy">Choose temperature, CPU runner, or both.</div></div>
<div class="settings-control-stack">
<div class="display-segment" role="group" aria-label="Menu bar style">
<button id="display-number" data-display="number" aria-pressed="false" onclick="setMenubarDisplay('number')">Number</button>
<button id="display-runner" data-display="cat" aria-pressed="false" onclick="setMenubarDisplay('cat')">Runner</button>
<button id="display-both" data-display="both" aria-pressed="true" onclick="setMenubarDisplay('both')">Both</button>
</div>
<div class="runner-pace" id="runner-pace">CPU runner · waiting</div>
</div>
</div>
__MENU_ITEMS_SETTINGS__
__CPU_WIDGET_SETTINGS__
<div class="settings-item" id="show-core-graph-setting">
<div><div class="settings-item-title">CPU core graph</div><div class="settings-item-copy">Show live per-core CPU activity in the overview.</div></div>
<button id="cpu-core-graph-toggle" class="panel-action secondary" aria-pressed="true" onclick="toggleCpuCoreGraph(this)">On</button>
</div>
<div class="settings-item" id="runner-character-setting">
<div><div class="settings-item-title">Character</div><div class="settings-item-copy">Choose a character, or shuffle every minute.</div></div>
<div class="character-picker">
<button type="button" id="runner-character-select" class="character-select" aria-label="Runner character" aria-haspopup="listbox" aria-controls="runner-character-menu" aria-expanded="false" onclick="toggleRunnerCharacterMenu(event)" onkeydown="handleRunnerTriggerKey(event)"><canvas id="runner-character-preview" class="runner-preview" width="48" height="32" data-runner-preview="cat" aria-hidden="true"></canvas><span id="runner-character-label" class="character-label">Cat</span><span class="character-chevron" aria-hidden="true"></span></button>
</div>
</div>
<details class="settings-details" id="notification-settings">
<summary><span>Notifications</span><span class="panel-pill info" id="notification-pill">2 on</span></summary>
<div class="notification-list">
<div class="notification-row">
<div><div class="notification-title">CPU temperature warning</div><div class="notification-copy">CPU Core Average · separate from the 90°C safety alert</div></div>
<div class="notification-control"><input class="notification-threshold" id="notification-temp-threshold" type="number" min="50" max="110" step="1" value="85" disabled onchange="changeTemperatureNotification()"><span class="notification-unit">°C</span><input class="notification-toggle" id="notification-temp-toggle" type="checkbox" aria-label="CPU temperature warning" onchange="toggleTemperatureNotification(this.checked)"></div>
</div>
<div class="notification-row">
<div><div class="notification-title">Fan control failures</div><div class="notification-copy">Notify when write or RPM verification fails</div></div>
<input class="notification-toggle" id="notification-fan-toggle" type="checkbox" aria-label="Fan control failures" checked onchange="setNotificationBoolean('fan-failures',this.checked)">
</div>
<div class="notification-row">
<div><div class="notification-title">App updates</div><div class="notification-copy">Notify once per version when a verified update is ready</div></div>
<input class="notification-toggle" id="notification-update-toggle" type="checkbox" aria-label="App updates" checked onchange="setNotificationBoolean('updates',this.checked)">
</div>
</div>
</details>
<details class="settings-details" id="automation-settings">
<summary><span>Automation Rules</span><span class="panel-pill info" id="automation-pill">0 rules</span></summary>
<div class="automation-copy">Rules run in order; the first matching condition selects its fan profile.</div>
<div class="automation-list" id="automation-rules"></div>
<div class="automation-actions"><button class="panel-action secondary" id="automation-add" onclick="addAutomationRule()">Add Rule</button><button class="panel-action secondary" id="automation-preset" onclick="applyQuietAcPreset()">Quiet Battery / Performance AC</button><button class="panel-action" id="automation-save" onclick="saveAutomationRules()">Save Rules</button></div>
<div class="automation-status" id="automation-status" role="status" aria-live="polite"></div>
</details>
<details class="settings-details" id="config-transfer-settings">
<summary><span>Configuration Transfer</span><span class="panel-pill info" id="config-transfer-pill">Ready</span></summary>
<div class="config-transfer-copy">Copy the current TOML or paste a previously exported PeterFan config, then import it after validation.</div>
<textarea class="config-transfer-text" id="config-transfer-text" spellcheck="false" aria-label="PeterFan configuration" oninput="markConfigImportDirty()"></textarea>
<div class="config-transfer-actions"><button class="panel-action secondary" id="config-copy" onclick="copyConfigExport()">Copy Current Config</button><button class="panel-action" id="config-import" onclick="importConfig()">Import Config</button></div>
<div class="config-transfer-status" id="config-transfer-status" role="status" aria-live="polite"></div>
</details>
<details class="settings-details" id="fan-health-card">
<summary><span>Fan Control Health</span><span class="panel-pill info" id="health-pill">Ready</span></summary>
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
<div id="fan-action-log-card">
<div class="health-head"><div class="health-title">Recent Fan Actions</div><div class="health-actions"><button class="health-action" id="support-report-button" disabled onclick="copySupportReport(this)">Copy Report</button><button class="health-action" id="fan-diagnostic-button" disabled onclick="runFanDiagnostics(this)">Run Diagnostics</button></div></div>
<div class="action-log" id="fan-action-log"><div class="action-log-empty">No fan actions yet</div></div>
</div>
</details>
</div>
</div>

<div class="rail-panel" id="rail-update-panel">
<div class="panel-title-row"><div class="panel-title">Updates</div><span class="panel-pill info" id="rail-update-pill">Ready</span></div>
<div class="panel-copy" id="rail-update-copy">Automatically checks every 6 hours. Failed checks retry; installation is your choice.</div>
<div class="health-grid" id="update-version-grid">
<div class="health-row"><span class="health-label">Installed app</span><span class="health-value" id="update-current-version">—</span></div>
<div class="health-row"><span class="health-label">Latest signed</span><span class="health-value" id="update-latest-version">—</span></div>
<div class="health-row"><span class="health-label">Status</span><span class="health-value" id="update-check-result" role="status" aria-live="polite" aria-atomic="true">—</span></div>
<div class="health-row"><span class="health-label">Last attempt</span><span class="health-value" id="update-last-attempt">—</span></div>
<div class="health-row"><span class="health-label">Last successful check</span><span class="health-value" id="update-last-success">—</span></div>
<div class="health-row"><span class="health-label">Next automatic check</span><span class="health-value" id="update-next-check">—</span></div>
<div class="health-row"><span class="health-label">Update channel</span><select class="update-channel-select" id="update-channel" aria-label="Update channel" onchange="changeUpdateChannel(this.value)"><option value="stable">Stable releases</option><option value="pre-release">Pre-release builds</option></select></div>
</div>
<div class="release-notes-card" id="update-release-notes-card" style="display:none"><div class="release-notes-title">Release Notes</div><div class="release-notes-body" id="update-release-notes">—</div></div>
<div class="panel-actions" style="margin-top:10px"><button class="panel-action secondary" id="rail-update-check" disabled onclick="checkAppUpdates(this)">Check for Updates</button><button class="panel-action" id="rail-update-install" disabled onclick="installAppUpdate(this)">Install Update</button><button class="panel-action secondary" id="update-release-link" onclick="openLatestRelease()" style="display:none">View Release</button></div>
</div>

<div class="rail-panel" id="rail-more-panel">
<div class="panel-title-row"><div class="panel-title">Hardware</div><span class="panel-pill info" id="rail-more-pill">Live</span></div>
<div class="panel-copy">Storage, battery, network, and active processes.</div>
<div class="view-loading" id="system-loading" role="status" aria-live="polite" style="display:none"><span class="data-loading-dot"></span><span>Reading system metrics…</span></div>
<div class="system-facts" aria-label="System quick facts">
<div class="system-fact"><span class="system-fact-label">Load average</span><span class="system-fact-value" id="system-load">—</span></div>
<div class="system-fact"><span class="system-fact-label">Power</span><span class="system-fact-value" id="system-power">—</span></div>
<div class="system-fact"><span class="system-fact-label">Network rate</span><span class="system-fact-value" id="system-network-rate">—</span></div>
<div class="system-fact"><span class="system-fact-label">Uptime</span><span class="system-fact-value" id="system-uptime">—</span></div>
</div>
<div class="health-card" id="hardware-availability-card" style="display:none">
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
<button class="active" disabled data-mode="auto" aria-pressed="true" title="Auto" onclick="setAuto()">Auto</button>
<button disabled data-mode="profile" data-profile="silent" aria-pressed="false" title="Silent" onclick="setProfile('silent')">Quiet</button>
<button disabled data-mode="profile" data-profile="balanced" aria-pressed="false" title="Balanced" onclick="setProfile('balanced')">Balance</button>
<button disabled data-mode="profile" data-profile="gaming" aria-pressed="false" title="Gaming" onclick="setProfile('gaming')">Game</button>
<button disabled data-mode="profile" data-profile="performance" aria-pressed="false" title="Performance" onclick="setProfile('performance')">Fast</button>
<button disabled data-mode="profile" data-profile="maximum" aria-pressed="false" title="Maximum" onclick="setProfile('maximum')">Max</button>
</div>
<div class="profile-guide" id="profile-guide" data-mode="auto" role="status" aria-live="polite" aria-atomic="true">
<div class="profile-guide-copy"><strong class="profile-guide-title" id="profile-guide-title">macOS Auto</strong><span class="profile-guide-detail" id="profile-guide-detail">macOS manages fan speed for the current workload.</span></div>
<div class="profile-preview-bars" id="profile-preview-bars" aria-hidden="true"><span></span><span></span><span></span><span></span><span></span></div>
</div>
<div class="fan-apply-status" id="fan-apply-status" role="status" aria-live="polite" aria-atomic="true"></div>
<div class="fan-cards" id="fan-cards"></div>
<div class="empty-state" id="fan-empty-state" style="display:none"><strong class="empty-state-title">No fan sensors</strong><span class="empty-state-copy">No fan sensors were reported. CPU, memory, and network monitoring remain available; temperature appears only when this system exposes a supported sensor.</span></div>
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
<button type="button" class="core-details-head" id="core-details-head" aria-expanded="false" aria-controls="core-details-list" onclick="toggleCoreDetails()">Core details</button><div class="core-details-list" id="core-details-list"></div>
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
<div class="bar"><div class="bar-fill" id="temp-bar"></div></div><div id="temp-list"></div><div class="metric-empty" id="temp-empty" style="display:none">CPU temperature sensors are unavailable.</div>
<button type="button" class="all-temp-head" id="all-temp-head" aria-expanded="false" aria-controls="all-temp-list" onclick="toggleRawTemps()">All sensors</button><div class="all-temp-list" id="all-temp-list"></div>
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
<div class="content"><div class="head"><span class="name">Top Processes</span><span class="sort-tabs"><button class="range-tab" id="ps-cpu" aria-pressed="true" onclick="setProcSort('cpu')">CPU</button><button class="range-tab" id="ps-mem" aria-pressed="false" onclick="setProcSort('mem')">MEM</button></span></div>
<div class="proc-tools"><input class="proc-search" id="process-search" type="search" maxlength="64" autocomplete="off" spellcheck="false" placeholder="Search name or PID" aria-label="Search processes" oninput="setProcessFilter(this.value)"></div>
<div class="proc-status" id="process-action-status" role="status" aria-live="polite"></div>
<div id="procs-list"></div><div class="metric-empty" id="procs-empty" style="display:none">No matching processes.</div></div></div>

<div class="foot compact-extra" data-compact-extra="quit"><button class="quit" onclick="window.ipc.postMessage('quit')">Quit PeterFan</button></div>
</main>
<aside class="action-rail" aria-label="Quick actions">
<button class="rail-btn active" id="railDetail" data-rail-action="detail" aria-label="Status" aria-pressed="true" onclick="runRailAction('detail',this)" title="Status"><svg viewBox="0 0 24 24"><rect x="4" y="5" width="16" height="14" rx="2"/><path d="M8 10h8M8 14h5"/></svg><span>Status</span></button>
<button class="rail-btn" id="railFan" data-rail-action="fan" aria-label="Fans" aria-pressed="false" onclick="runRailAction('fan',this)" title="Fans"><svg viewBox="0 0 24 24"><circle cx="12" cy="12" r="2.2"/><path d="M12 4c3 0 4.5 2 3 4.5L12 12M20 12c0 3-2 4.5-4.5 3L12 12M12 20c-3 0-4.5-2-3-4.5L12 12M4 12c0-3 2-4.5 4.5-3L12 12"/></svg><span>Fans</span></button>
<button class="rail-btn" id="railSettings" data-rail-action="settings" aria-label="Settings" aria-pressed="false" onclick="runRailAction('settings',this)" title="Settings"><svg viewBox="0 0 24 24"><path d="M12 15.5a3.5 3.5 0 1 0 0-7 3.5 3.5 0 0 0 0 7z"/><path d="M19.4 15a1.7 1.7 0 0 0 .3 1.9l.1.1-2 3.4-.2-.1a1.7 1.7 0 0 0-1.9-.1 8 8 0 0 1-1.4.8 1.7 1.7 0 0 0-1.1 1.5V23h-4v-.5A1.7 1.7 0 0 0 8.1 21a8 8 0 0 1-1.4-.8 1.7 1.7 0 0 0-1.9.1l-.2.1-2-3.4.1-.1A1.7 1.7 0 0 0 3 15a8.6 8.6 0 0 1 0-1.7 1.7 1.7 0 0 0-.3-1.9l-.1-.1 2-3.4.2.1a1.7 1.7 0 0 0 1.9.1A8 8 0 0 1 8.1 7a1.7 1.7 0 0 0 1.1-1.5V5h4v.5A1.7 1.7 0 0 0 14.3 7a8 8 0 0 1 1.4.8 1.7 1.7 0 0 0 1.9-.1l.2-.1 2 3.4-.1.1a1.7 1.7 0 0 0-.3 1.9 8.6 8.6 0 0 1 0 2z"/></svg><span>Settings</span></button>
<button class="rail-btn" id="railSystem" data-rail-action="system" aria-label="System" aria-pressed="false" onclick="runRailAction('system',this)" title="System"><svg viewBox="0 0 24 24"><ellipse cx="12" cy="5" rx="7" ry="2.5"/><path d="M5 5v7c0 1.4 3.1 2.5 7 2.5s7-1.1 7-2.5V5M5 12v7c0 1.4 3.1 2.5 7 2.5s7-1.1 7-2.5v-7"/></svg><span>System</span></button>
</aside></div></div>
<div id="runner-character-menu" class="character-menu" role="listbox" aria-label="Runner character" hidden>__RUNNER_OPTIONS__</div>
<div class="chart-tip" id="chart-tip"></div>
<script>
var LANG='__LANG__';
var SHOW_CURVE_EDITOR='__SHOWCURVE__';
var CORE_DETAILS_OPEN=false;
var PROCESS_FILTER_TIMER=0;
var PROCESS_CONFIRM={pid:null,startTime:null,until:0};
var PROCESS_ACTION_REVISION=0;
// Keep WebView failures visible to the native diagnostic log. A blank pane is
// otherwise indistinguishable from a slow sensor read.
window.onerror=function(message,source,line,column,error){
  var detail=String(message||'unknown');
  var stack=error&&error.stack?String(error.stack):'';
  var location=source?(' @ '+String(source)+':'+String(line||0)+':'+String(column||0)):'';
  if(stack)detail+=' | '+stack;
  if(window.ipc)window.ipc.postMessage('js-error:'+(detail+location).slice(0,900));
};
window.onunhandledrejection=function(event){
  var reason=event&&event.reason;
  var detail=reason&&reason.stack?String(reason.stack):String(reason||'unhandled rejection');
  if(window.ipc)window.ipc.postMessage('js-error:'+detail.slice(0,900));
};
var FAN_CONTROL_FIX_PENDING=false;
var SETUP_PROGRESS={phase:'idle',value:0,target:0,frame:null,started:null,from:0};
var FAN_CONTROL_FIX_REVISION=0;
var FAN_DIAGNOSTIC_PENDING=false;
var FAN_DIAGNOSTIC_STARTED_AT=0;
var LOGIN_ITEM_TOGGLE_PENDING=false;
var APP_UPDATE_CHECK_PENDING=false;
var APP_UPDATE_STATUS=null;
var APP_UPDATE_STATUS_STORAGE_KEY='pf.update.status.v1';
var APP_PERSISTED_UPDATE_KEY='';
var FAN_CONTROL_PENDING=null;
var FAN_CONTROL_RESULT=null;
var DATA_LOADING_TIMER=setTimeout(function(){
  if(document.body.classList.contains('data-ready'))return;
  var text=document.getElementById('data-loading-text');
  var retry=document.getElementById('data-loading-retry');
  if(text)text.textContent=LANG==='ko'?'센서 조회가 지연되고 있습니다':'Sensor reading is taking longer than expected';
  if(retry){retry.style.display='';retry.disabled=false;}
},4000);
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
function loadUpdateStatusSnapshot(){
  var raw=storageGet(APP_UPDATE_STATUS_STORAGE_KEY);
  if(!raw)return null;
  try{
    var saved=JSON.parse(raw),savedAt=Number(saved&&saved.saved_at||0);
    if(!savedAt||Date.now()-savedAt>15*60*1000)return null;
    delete saved.saved_at;
    saved.cached=true;
    return saved;
  }catch(e){return null;}
}
function rememberUpdateStatus(status){
  if(!status)return;
  var phase=String(status.phase||'');
  if(phase==='checking'||phase==='downloading'||phase==='queued')return;
  if(!status.latest&&phase!=='failed'&&phase!=='current')return;
  var snapshot={};
  Object.keys(status).forEach(function(key){snapshot[key]=status[key];});
  snapshot.saved_at=Date.now();
  storageSet(APP_UPDATE_STATUS_STORAGE_KEY,JSON.stringify(snapshot));
}
APP_UPDATE_STATUS=loadUpdateStatusSnapshot();
var RAIL_VIEW=storageGet('pf.rail.view')||'overview';
if(RAIL_VIEW==='update'||RAIL_VIEW==='more')RAIL_VIEW='system';
if(!/^(overview|fan|settings|system)$/.test(RAIL_VIEW))RAIL_VIEW='overview';
function railView(){
  return RAIL_VIEW||'overview';
}
function setRailView(view){
  if(view==='update'||view==='more')view='system';
  if(!/^(overview|fan|settings|system)$/.test(view))view='overview';
  resetProcessConfirmation();
  RAIL_VIEW=view;
  refreshSetupProgressMotion();
  storageSet('pf.rail.view',view);
  document.body.setAttribute('data-rail-view',view);
  applyRailView(true);
  if(window.ipc)window.ipc.postMessage('view:'+view);
  requestAnimationFrame(function(){
    if(window.__pf&&window.__pf.update&&window.__pf_pending)window.__pf.update(window.__pf_pending);
  });
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
  var all=['range-tabs','health-verdict','summary-strip','setup-row','fan-control-section','curve-editor-section','sec-cpu','sec-mem','sec-storage','sec-temp','sec-batt','sec-network','sec-procs','foot','rail-update-panel','rail-settings-panel','rail-more-panel'];
  all.forEach(function(id){setVisible(id,false);});
  setVisible('range-tabs',true);
  if(view==='fan'){
    setVisible('setup-row',true);
    setVisible('fan-control-section',true);
    if(SHOW_CURVE_EDITOR==='1')setVisible('curve-editor-section',true);
  } else if(view==='settings'){
    setVisible('setup-row',true);
    ['rail-settings-panel','rail-update-panel'].forEach(function(id){setVisible(id,true);});
  } else if(view==='system'){
    ['rail-more-panel','sec-storage','sec-batt','sec-network','sec-procs','foot'].forEach(function(id){setVisible(id,true);});
  } else {
    ['health-verdict','summary-strip','sec-cpu','sec-mem','sec-temp'].forEach(function(id){setVisible(id,true);});
  }
  ['Detail','Fan','Settings','System'].forEach(function(name){
    var key=name.toLowerCase();
    if(key==='detail')key='overview';
    setRailButtonActive('rail'+name,view===key);
  });
  var title=document.querySelector('.view-title');
  if(title)title.textContent=view==='fan'
    ?(LANG==='ko'?'팬 제어':'Fans')
    :(view==='settings'
      ?(LANG==='ko'?'설정':'Settings')
      :(view==='system'?(LANG==='ko'?'시스템':'System'):'PeterFan'));
  if(resetScroll)resetRailPaneScroll();
  setRunnerPreviewVisibility(view==='settings');
}
document.body.classList.add('compact');
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
function retryDashboard(){
  var retry=document.getElementById('data-loading-retry');
  var text=document.getElementById('data-loading-text');
  if(retry)retry.disabled=true;
  if(text)text.textContent=LANG==='ko'?'다시 확인하는 중…':'Retrying sensor read…';
  if(window.ipc)window.ipc.postMessage('refresh');
  setTimeout(function(){if(retry&&!document.body.classList.contains('data-ready'))retry.disabled=false;},1500);
}
function setText(id,text){
  var el=document.getElementById(id);
  var value=String(text||'—');
  if(el&&el.textContent!==value)el.textContent=value;
}
function compareVersions(a,b){
  function numeric(a,b){return a.length===b.length?(a===b?0:(a<b?-1:1)):(a.length<b.length?-1:1);}
  function parse(value){
    var match=String(value||'').match(/^v?(0|[1-9][0-9]*)(?:\.(0|[1-9][0-9]*))?(?:\.(0|[1-9][0-9]*))?(?:-([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?(?:\+([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?$/);
    if(!match)return null;
    var core=[match[1],match[2]||'0',match[3]||'0'];
    if(core.some(function(part){return numeric(part,'18446744073709551615')>0;}))return null;
    var pre=match[4]?match[4].split('.'):[];
    if(pre.some(function(part){return /^[0-9]+$/.test(part)&&part.length>1&&part[0]==='0';}))return null;
    return {core:core,pre:pre};
  }
  var aa=parse(a),bb=parse(b);
  if(!aa||!bb)return null;
  for(var i=0;i<3;i++){var order=numeric(aa.core[i],bb.core[i]);if(order)return order;}
  if(!aa.pre.length||!bb.pre.length)return aa.pre.length===bb.pre.length?0:(aa.pre.length?-1:1);
  for(var j=0;j<Math.max(aa.pre.length,bb.pre.length);j++){
    if(j>=aa.pre.length)return -1;if(j>=bb.pre.length)return 1;
    var x=aa.pre[j],y=bb.pre[j];if(x===y)continue;
    var xn=/^[0-9]+$/.test(x),yn=/^[0-9]+$/.test(y);
    return xn&&yn?numeric(x,y):(xn!==yn?(xn?-1:1):(x<y?-1:1));
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
function updateCheckTime(value,empty){
  var unix=Number(value);
  if(!Number.isFinite(unix)||unix<=0)return empty;
  return new Date(unix*1000).toLocaleString(LANG==='ko'?'ko-KR':'en-US',{month:'short',day:'numeric',hour:'numeric',minute:'2-digit'});
}
function updateCheckMetadata(status,native){
  ['channel','last_attempt_unix','last_success_unix','next_check_unix','check_error','auto_check_enabled'].forEach(function(key){status[key]=native[key];});
  return status;
}
function reconcileUpdateReceipt(receipt,current,now){
  if(!receipt)return null;
  var timestamp=Number(receipt.updated_at_unix),age=now-timestamp;
  if(!Number.isFinite(timestamp)||timestamp<=0||age<0||age>24*60*60)return null;
  var comparison=compareVersions(current,receipt.version);
  if(comparison===null)return null;
  if(receipt.status==='installed')return comparison===0?receipt:null;
  // A detached installer can still be working, but a stale receipt must not
  // imply permanent progress after a crash or an interrupted installation.
  if(receipt.status==='pending'){
    if(comparison===0)return null;
    return age<=5*60&&comparison<0?receipt:null;
  }
  return (receipt.status==='failed'||receipt.status==='rolled_back')&&comparison<0?receipt:null;
}
function renderUpdateStatus(status){
  if(status)APP_UPDATE_STATUS=status;
  var s=APP_UPDATE_STATUS||{};
  var phase=s.phase||'';
  var never=LANG==='ko'?'아직 없음':'Not yet';
  setText('update-last-attempt',updateCheckTime(s.last_attempt_unix,never));
  setText('update-last-success',updateCheckTime(s.last_success_unix,never));
  setText('update-next-check',s.auto_check_enabled===false
    ?(LANG==='ko'?'모의 실행에서 꺼짐':'Disabled in mock mode')
    :updateCheckTime(s.next_check_unix,phase==='checking'?(LANG==='ko'?'확인 중':'Checking'):'—'));
  var current=s.current||(window.__pf_pending&&window.__pf_pending.app_version)||'';
  setText('update-current-version',current?('v'+String(current).replace(/^v/,'')):'—');
  var validLatest=s.latest&&compareVersions(s.latest,s.latest)!==null;
  setText('update-latest-version',validLatest?('v'+String(s.latest).replace(/^v/,'')):(s.latest?(LANG==='ko'?'확인 불가':'Unverified'):'—'));
  var result=document.getElementById('update-check-result');
  var link=document.getElementById('update-release-link');
  var install=document.getElementById('rail-update-install');
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
  if(s.checking||phase==='checking'){
    msg=s.message||(LANG==='ko'?'GitHub 최신 릴리즈를 확인 중입니다.':'Checking the latest GitHub release.');
    pillText=LANG==='ko'?'확인 중':'Checking';
    pillTone='info';
    setText('update-check-result',LANG==='ko'?'확인 중…':'checking…');
  } else if(phase==='downloading'){
    msg=s.message||(LANG==='ko'?'새 버전을 내려받아 서명과 공증을 확인하고 있습니다.':'Downloading the new version and verifying its signature and notarization.');
    pillText=LANG==='ko'?'설치 중':'Installing';
    pillTone='info';
    setText('update-check-result',LANG==='ko'?'검증 및 설치 중':'verifying and installing');
  } else if(phase==='queued'){
    msg=s.message||(LANG==='ko'?'업데이트가 준비되었습니다. PeterFan이 자동으로 다시 열립니다.':'The update is ready. PeterFan will reopen automatically.');
    pillText=LANG==='ko'?'재실행 중':'Relaunching';
    pillTone='ok';
    setText('update-check-result',LANG==='ko'?'업데이트 준비 완료':'update ready');
  } else if(phase==='disabled'||s.auto_check_enabled===false){
    msg=LANG==='ko'?'모의 실행에서는 업데이트 서버에 연결하지 않습니다.':'Update services are disabled in mock mode.';
    pillText=LANG==='ko'?'꺼짐':'Disabled';
    setText('update-check-result',LANG==='ko'?'모의 실행':'mock mode');
  } else if(phase==='channel_failed'){
    msg=s.message||(LANG==='ko'?'업데이트 채널을 저장하지 못했습니다.':'The update channel could not be saved.');
    pillText=LANG==='ko'?'저장 실패':'Save failed';
    pillTone='warn';
    setText('update-check-result',LANG==='ko'?'채널 저장 실패':'channel save failed');
  } else if(s.check_error){
    msg=(LANG==='ko'?'업데이트 확인 실패. 자동으로 다시 시도합니다. ':'Update check failed. Another check is scheduled. ')+s.check_error;
    pillText=LANG==='ko'?'재시도 예정':'Retry scheduled';
    pillTone='warn';
    setText('update-check-result',LANG==='ko'?'확인 실패 · 재시도 예정':'check failed · retry scheduled');
  } else if(phase==='failed'){
    msg=s.message||(LANG==='ko'?'업데이트를 완료하지 못했습니다. 설치 상태를 확인하고 다시 시도해 주세요.':'The update could not be completed. Check the installation and try again.');
    pillText=LANG==='ko'?'실패':'Failed';
    pillTone='warn';
    setText('update-check-result',LANG==='ko'?'업데이트 실패':'update failed');
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
    msg=(LANG==='ko'?'업데이트를 완료하지 못했습니다. 설치 상태를 확인하고 다시 시도해 주세요.':(s.install_message||'The update could not be completed.'));
    pillText=LANG==='ko'?'실패':'Failed';
    pillTone='warn';
    setText('update-check-result',LANG==='ko'?'설치 실패':'installation failed');
  } else if(s.latest){
    var comparison=compareVersions(current,s.latest);
    var newer=comparison<0,ahead=comparison>0;
    msg=comparison===null
      ?(LANG==='ko'?'릴리스 버전을 확인할 수 없습니다. 다시 확인해 주세요.':'Release version could not be verified. Please check again.')
      :ahead
      ?(LANG==='ko'
        ?'현재 버전이 공개 서명 릴리스보다 최신입니다. 공식 릴리스가 공개되면 업데이트할 수 있습니다.'
        :'The current version is newer than the public signed release. An update will appear when the official release is published.')
      :newer&&!s.install_ready
        ?(LANG==='ko'?'새 버전을 확인했습니다. 검증된 설치 파일이 준비될 때까지 자동으로 다시 확인합니다.':(s.message||'The new release is still preparing verified files. Another check is scheduled.'))
      :((LANG==='ko'?'':s.message)||(newer
        ?(LANG==='ko'?'새 서명 릴리스를 사용할 수 있습니다.':'A newer signed PeterFan release is available.')
        :(LANG==='ko'?'최신 서명 릴리스를 사용 중입니다.':'You are running the latest signed release.')));
    pillText=comparison===null?(LANG==='ko'?'확인 필요':'Needs verification'):newer
      ?(LANG==='ko'?'업데이트 있음':'Update')
      :(ahead?(LANG==='ko'?'공식 릴리스 대기':'Awaiting Signed Release'):(LANG==='ko'?'최신':'Current'));
    pillTone=comparison===null?'warn':(newer?'warn':(ahead?'info':'ok'));
    setText('update-check-result',comparison===null?(LANG==='ko'?'버전 정보 확인 필요':'invalid version metadata'):newer
      ?(LANG==='ko'?'업데이트 가능':'update available')
      :(ahead?(LANG==='ko'?'서명 릴리스보다 앞섬':'ahead of signed release'):(LANG==='ko'?'최신 상태':'up to date')));
  } else {
    msg=s.message||(LANG==='ko'?'6시간마다 자동으로 확인합니다. 실패하면 자동으로 재시도합니다.':'Updates are checked every 6 hours, with automatic retries after failures.');
    setText('update-check-result',LANG==='ko'?'자동 확인 대기 중':'automatic check scheduled');
  }
  if(s.cached)msg=(LANG==='ko'?'이전 세션의 확인 결과입니다. ':'Result from a previous session. ')+msg;
  if(copy)copy.textContent=msg;
  setPanelPill('rail-update-pill',pillText,pillTone);
  if(link){
    link.style.display=s.url?'':'none';
    link.dataset.url=s.url||'';
    link.textContent=LANG==='ko'?'릴리즈 보기':'View Release';
  }
  if(install){
    var comparison=s.latest?compareVersions(current,s.latest):0;
    var updateKnown=!!s.latest&&comparison<0;
    var checking=!!s.checking||phase==='checking';
    var installing=phase==='downloading'||phase==='queued';
    var canInstall=s.auto_check_enabled!==false&&updateKnown&&s.install_ready===true&&!s.checking&&phase!=='checking'&&!installing;
    install.style.display='';
    install.disabled=!canInstall;
    install.textContent=installing
      ?(LANG==='ko'?'설치 중…':'Installing…')
      :(checking
        ?(LANG==='ko'?'확인 중…':'Checking…')
        :(s.latest&&comparison===null
          ?(LANG==='ko'?'확인 필요':'Needs verification')
          :(updateKnown
          ?(s.install_ready===true
            ?(LANG==='ko'?'지금 업데이트':'Install Update')
            :(LANG==='ko'?'설치 준비 중':'Preparing Update'))
          :(s.latest&&comparison===0
            ?(LANG==='ko'?'최신 버전':'Up to Date')
            :(s.latest&&comparison>0
              ?(LANG==='ko'?'공식 릴리스 대기':'Awaiting Signed Release')
              :(LANG==='ko'?'확인 전':'Not checked'))))));
  }
  rememberUpdateStatus(APP_UPDATE_STATUS);
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
    case 'settings':setRailView('settings');break;
    case 'system':case 'more':setRailView('system');break;
    case 'update':setRailView('settings');break;
  }
}
function coreGroupName(kind){
  if(kind==='performance')return LANG==='ko'?'성능 코어':'Performance cores';
  if(kind==='efficiency')return LANG==='ko'?'효율 코어':'Efficiency cores';
  return LANG==='ko'?'논리 코어':'Logical cores';
}
function coreMetadata(groups){
  var metadata={};
  (groups||[]).forEach(function(group){
    (group.cores||[]).forEach(function(core){metadata[Number(core.index)]=core;});
  });
  return metadata;
}
function renderCoreDetails(d){
  var head=document.getElementById('core-details-head');
  var list=document.getElementById('core-details-list');
  if(!head||!list)return;
  var groups=d.core_groups||[];
  var total=(d.cores||[]).length;
  head.textContent=(CORE_DETAILS_OPEN?'▾ ':'▸ ')+(LANG==='ko'?'코어 상세':'Core details')+(total?' · '+total:'');
  head.setAttribute('aria-expanded',CORE_DETAILS_OPEN?'true':'false');
  list.classList.toggle('open',CORE_DETAILS_OPEN);
  if(!CORE_DETAILS_OPEN)return;
  list.innerHTML='';
  groups.forEach(function(group){
    var section=document.createElement('div');section.className='core-group';
    var heading=document.createElement('div');heading.className='core-group-head';
    var name=document.createElement('span');name.className='core-group-name';name.textContent=coreGroupName(group.kind);
    var stats=document.createElement('span');stats.className='core-group-stats';
    stats.textContent=(LANG==='ko'?'평균 ':'avg ')+Number(group.average||0).toFixed(0)+'% · '+(LANG==='ko'?'최고 ':'peak ')+Number(group.peak||0).toFixed(0)+'%';
    heading.appendChild(name);heading.appendChild(stats);section.appendChild(heading);
    var grid=document.createElement('div');grid.className='core-detail-grid';
    (group.cores||[]).forEach(function(core){
      var usage=Math.max(0,Math.min(100,Number(core.usage)||0));
      var cell=document.createElement('div');cell.className='core-detail';cell.title=String(core.label||'Core')+': '+usage.toFixed(1)+'%';
      var label=document.createElement('span');label.className='core-detail-label';label.textContent=core.label||'—';
      var value=document.createElement('span');value.className='core-detail-value';value.textContent=usage.toFixed(0)+'%';
      var meter=document.createElement('span');meter.className='core-detail-meter';
      var fill=document.createElement('span');fill.className=usage<50?'':(usage<80?'y':'r');fill.style.width=usage+'%';
      meter.appendChild(fill);cell.appendChild(label);cell.appendChild(value);cell.appendChild(meter);grid.appendChild(cell);
    });
    section.appendChild(grid);list.appendChild(section);
  });
}
function toggleCoreDetails(){
  CORE_DETAILS_OPEN=!CORE_DETAILS_OPEN;
  renderCoreDetails(window.__pf_pending||{});
}
var RAW_TEMP_OPEN=false;
function toggleRawTemps(){
  RAW_TEMP_OPEN=!RAW_TEMP_OPEN;
  if(window.ipc)window.ipc.postMessage('rawtemps:'+(RAW_TEMP_OPEN?'1':'0'));
  renderRawTempList(window.__pf_pending||{});
}
function renderRawTempList(d){
  var ah=document.getElementById('all-temp-head'),al=document.getElementById('all-temp-list'),all=d.all_temps||[];
  if(ah){
    ah.textContent=(RAW_TEMP_OPEN?'▾ ':'▸ ')+(LANG==='ko'?'전체 센서':'All sensors')+(all.length?' · '+all.length:'');
    ah.setAttribute('aria-expanded',RAW_TEMP_OPEN?'true':'false');
    ah.style.display='';
  }
  if(al){
    al.style.display=RAW_TEMP_OPEN?'':'none';
    al.innerHTML='';
    if(RAW_TEMP_OPEN){
      if(!all.length){
        al.textContent=LANG==='ko'?'센서 읽는 중…':'Reading sensors…';
        al.className='all-temp-list sensor-loading';
        return;
      }
      al.className='all-temp-list';
      var groups=[];
      all.forEach(function(t){var name=t.group||'Other',g=groups.find(function(x){return x.name===name;});if(!g){g={name:name,items:[]};groups.push(g);}g.items.push(t);});
      groups.forEach(function(g){
        var h=document.createElement('div');h.className='sensor-group-head';h.textContent=g.name;al.appendChild(h);
        g.items.forEach(function(t){var r=document.createElement('div');r.className='trow'+(t.stale?' stale':'');r.innerHTML='<span class="l"></span><span class="src"></span><span class="v"></span>';r.children[0].textContent=t.l;r.children[0].title=t.l;r.children[1].textContent=(t.source||'')+(t.stale?' · '+(LANG==='ko'?'오래됨 ':'stale ')+Number(t.age_secs||0)+'s':'');r.children[2].textContent=t.c;r.children[2].className='v '+t.cls;al.appendChild(r);});
      });
    }
  }
}
function updateHealthVerdict(d){
  var root=document.getElementById('health-verdict');
  var title=document.getElementById('health-verdict-title');
  var detail=document.getElementById('health-verdict-detail');
  if(!root||!title||!detail)return;
  var temp=Number(d.temp_pct),cpu=Number(d.cpu_pct||0),fans=Number(d.fan_avg_rpm||0);
  var hasTemp=!!d.temp_present&&!d.temp_stale&&isFinite(temp)&&temp>0;
  var health=d.control_health||{};
  var failsafe=!!health.failsafe_active;
  var tone='ok',heading=LANG==='ko'?'정상':'Normal';
  if(!hasTemp){
    tone='info';
    heading=LANG==='ko'?'모니터링 중':'Monitoring';
  } else if(failsafe||temp>=95){
    tone='hot';
    heading=LANG==='ko'?'확인 필요':'Needs attention';
  } else if(temp>=88){
    tone='hot';
    heading=LANG==='ko'?'뜨거움':'Hot';
  } else if(temp>=78){
    tone='warm';
    heading=LANG==='ko'?'따뜻함':'Warm';
  } else if(cpu>=85){
    tone='info';
    heading=LANG==='ko'?'작업 중':'Busy';
  }
  var parts=[];
  if(hasTemp)parts.push((LANG==='ko'?'CPU 평균 ':'CPU avg ')+Math.round(temp)+'°C');
  else parts.push(d.temp_stale?(LANG==='ko'?'온도 값 오래됨':'temperature stale'):(LANG==='ko'?'온도 센서 없음':'temperature unavailable'));
  parts.push('CPU '+Math.round(cpu)+'%');
  if(fans>0)parts.push((LANG==='ko'?'팬 ':'fans ')+Math.round(fans)+' RPM');
  if(failsafe)parts.push(LANG==='ko'?'macOS 자동 복귀':'macOS safety fallback');
  root.className='health-verdict '+tone;
  title.textContent=heading;
  detail.textContent=parts.join(' · ');
}
function summaryFanRpm(d){
  var value=Number(d.fan_avg_rpm);
  return (d.fans||[]).length&&Number.isFinite(value)&&value>=0?Math.round(value).toLocaleString('en-US'):'—';
}
function cssColor(token,fallback){
  var value=getComputedStyle(document.documentElement).getPropertyValue(token);
  return String(value||'').trim()||fallback;
}
window.__pf={
 update:function(d){
 function cls(p){return p<50?'g':p<80?'y':'r';}
 function bar(id,p,c){var b=document.getElementById(id);if(b){b.style.width=Math.max(0,Math.min(100,p))+'%';b.className='bar-fill '+(c||cls(p));}}
 function set(id,t){var e=document.getElementById(id);if(e)e.textContent=t;}
 function show(id,on){var e=document.getElementById(id);if(e)e.style.display=on?'':'none';}
 window.__pf_pending=d;
 if(d.fan_control_installing){
   if(!FAN_CONTROL_FIX_PENDING)FAN_CONTROL_FIX_REVISION=Number(d.fan_control_install_revision||0);
   FAN_CONTROL_FIX_PENDING=true;
 } else if(FAN_CONTROL_FIX_PENDING&&Number(d.fan_control_install_revision||0)>FAN_CONTROL_FIX_REVISION){
   FAN_CONTROL_FIX_PENDING=false;
 }
 document.body.classList.add('data-ready');
 clearTimeout(DATA_LOADING_TIMER);
 var loading=document.getElementById('data-loading');
 if(loading)loading.setAttribute('aria-hidden','true');
 var loadingRetry=document.getElementById('data-loading-retry');
 if(loadingRetry){loadingRetry.style.display='none';loadingRetry.disabled=false;}
 var view=railView();
 CHART_RANGE_LABEL=d.chart_range;
 updateRail(d);
 updateNotificationSettings(d);
 updateAutomationRules(d);
 updateConfigTransfer(d);
 updateCoreGraphSetting(d);
 if(view==='overview'){
   updateHealthVerdict(d);
   set('summary-cpu',d.cpu_text||'—');
   set('summary-mem',d.mem_text||'—');
   set('summary-temp',d.temp_present?(d.temp_stale?'--°C':(d.temp_text||'—')):'—');
   set('summary-fan',summaryFanRpm(d));
   var fanSummary=document.getElementById('summary-fan');
   if(fanSummary){fanSummary.title=d.fan_avg_rpm_text||'—';fanSummary.setAttribute('aria-label',(LANG==='ko'?'팬 평균 속도 ':'Average fan speed ')+(d.fan_avg_rpm_text||'—'));}
   bar('summary-cpu-bar',d.cpu_pct||0);
   bar('summary-mem-bar',d.mem_pct||0,'info');
   bar('summary-temp-bar',d.temp_stale?0:(d.temp_pct||0),d.temp_stale?'info':(d.temp_cls||''));
   var fanPcts=(d.fans||[]).map(function(f){return Number(f.pct)||0;});
   var fanPct=fanPcts.length?fanPcts.reduce(function(sum,p){return sum+p;},0)/fanPcts.length:0;
   bar('summary-fan-bar',fanPct,'info');
   var summaryCpu=document.getElementById('summary-cpu');
   if(summaryCpu)summaryCpu.className='summary-value '+cls(d.cpu_pct||0);
   var summaryMem=document.getElementById('summary-mem');
   if(summaryMem)summaryMem.className='summary-value '+cls(d.mem_pct||0);
   var summaryTemp=document.getElementById('summary-temp');
   if(summaryTemp)summaryTemp.className='summary-value '+(d.temp_stale?'info':(d.temp_cls||''));
   var summaryFan=document.getElementById('summary-fan');
   if(summaryFan)summaryFan.className='summary-value '+(d.fan_avg_rpm>0?'info':'');
   renderCoreGraph(d);
   set('cpu-val',d.cpu_text);set('cpu-sub',d.cpu_sub);bar('cpu-bar',d.cpu_pct);
   var cc=document.getElementById('cores');if(cc){var coreMeta=coreMetadata(d.core_groups);cc.innerHTML='';(d.cores||[]).forEach(function(p,i){var s=document.createElement('span'),meta=coreMeta[i];s.className='core '+cls(p);s.style.height=Math.max(8,Math.min(100,p))+'%';s.title=(meta&&meta.label?meta.label:('Core '+(i+1)))+': '+p.toFixed(1)+'%';cc.appendChild(s);});}
   renderCoreDetails(d);
   set('mem-val',d.mem_text);set('mem-sub',d.mem_sub);bar('mem-bar',d.mem_pct);
   show('sec-temp',true);if(d.temp_present){show('temp-empty',false);set('temp-name',(LANG==='ko'?'온도':'Temperature')+(d.temp_source?' · '+d.temp_source:'')+(d.temp_stale?' · '+(LANG==='ko'?'오래됨 ':'stale ')+Number(d.temp_age_secs||0)+'s':''));set('temp-val',d.temp_text);var tv=document.getElementById('temp-val');if(tv)tv.classList.toggle('stale',!!d.temp_stale);bar('temp-bar',d.temp_stale?0:d.temp_pct,d.temp_cls);
     var tl=document.getElementById('temp-list');if(tl){tl.innerHTML='';(d.temps||[]).forEach(function(t){var r=document.createElement('div');r.className='trow'+(t.stale?' stale':'');r.innerHTML='<span class="l"></span><span class="v"></span>';r.children[0].textContent=t.l;r.children[1].textContent=t.c+(t.stale?' · '+Number(t.age_secs||0)+'s':'');r.children[1].className='v '+t.cls;tl.appendChild(r);});}
     renderRawTempList(d);} else {
     show('temp-empty',true);set('temp-name',LANG==='ko'?'온도':'Temperature');set('temp-val','—');bar('temp-bar',0,'info');
     var tlEmpty=document.getElementById('temp-list');if(tlEmpty)tlEmpty.innerHTML='';var tvEmpty=document.getElementById('temp-val');if(tvEmpty)tvEmpty.classList.remove('stale');clearChart('temp-chart');set('temp-chart-stats','');renderRawTempList(d);
   }
   drawChart('cpu-chart', d.cpu_hist, cssColor('--accent','#6ea8ff'), 100, function(v){return v.toFixed(1)+'%';});
   drawChart('mem-chart', d.mem_hist, cssColor('--accent','#6ea8ff'), 100, function(v){return v.toFixed(1)+'%';});
   if(d.temp_present)drawChart('temp-chart', d.temp_stale?[]:d.temp_hist, cssColor('--y','#f4c95d'), null, function(v){return v.toFixed(0)+'°C';});
   document.querySelectorAll('.range-tabs .range-tab').forEach(function(b){var active=b.dataset.range===d.chart_range;b.classList.toggle('active',active);b.setAttribute('aria-pressed',active?'true':'false');});
 } else if(view==='settings'||view==='system'){
   if(view==='settings'){updateSetup(d);updateMenubarDisplay(d);renderMenuItems(d);renderCpuWidget(d);}
   else {
     updateHardwareAvailability(d);
     set('system-load',d.load_avg_text||'—');
     set('system-power',d.power_text||'—');
     set('system-network-rate',d.network_rate_text||'—');
     set('system-uptime',d.uptime_text||'—');
   }
   var systemLoading=document.getElementById('system-loading');
   if(systemLoading)systemLoading.style.display=view==='system'&&!d.slow_data_ready?'':'none';
   set('disk-val',d.disk_text);set('disk-sub',d.disk_sub);bar('disk-bar',d.disk_pct);
   show('disk-io-sub',d.disk_io_present);if(d.disk_io_present)set('disk-io-sub',d.disk_io_sub);
   show('disk-io-chart',d.disk_io_present);show('disk-io-chart-stats',d.disk_io_present);
   if(d.disk_io_present)drawChart('disk-io-chart', d.disk_io_hist, cssColor('--y','#f4c95d'), null, fmtBytesPerSec);
   show('sec-batt',d.batt_present);if(d.batt_present){set('batt-val',d.batt_text);set('batt-sub',d.batt_sub);bar('batt-bar',d.batt_pct,d.batt_pct>50?'g':d.batt_pct>20?'y':'r');}
   var battSec=document.getElementById('sec-batt');if(battSec)battSec.dataset.present=d.batt_present?'1':'0';
   set('net-sub',d.net_sub);show('net-ip',!!d.net_ip);if(d.net_ip)set('net-ip',d.net_ip);
   var psCpu=document.getElementById('ps-cpu'),psMem=document.getElementById('ps-mem');
   if(psCpu){var cpuSort=d.proc_sort!=='mem';psCpu.classList.toggle('active',cpuSort);psCpu.setAttribute('aria-pressed',cpuSort?'true':'false');}
   if(psMem){var memSort=d.proc_sort==='mem';psMem.classList.toggle('active',memSort);psMem.setAttribute('aria-pressed',memSort?'true':'false');}
   if(psMem)psMem.classList.toggle('active',d.proc_sort==='mem');
   renderProcesses(d);
   drawChart('net-chart', d.net_hist, cssColor('--g','#5dd879'), null, fmtBytesPerSec);
 } else if(view==='fan'){
   updateSetup(d);
   var note=document.getElementById('ctl-note');
   var fanStateReady=d.fan_control_state_ready!==false;
   var guide=document.getElementById('profile-guide');if(guide)guide.style.display=fanStateReady?'':'none';
   if(!fanStateReady){
     set('ctl-status',LANG==='ko'?'상태 확인 중…':'Checking fan state…');
     updateProfileStrip(Object.assign({},d,{can_control:false}));
     if(note)note.style.display='none';
     var pendingCards=document.getElementById('fan-cards');if(pendingCards)pendingCards.innerHTML='';
     updateFanEmptyState(d);
   } else if(d.fan_control_supported){
     var fanCount=Number(d.fan_count||0),controllableCount=Number(d.controllable_fan_count||0);
     set('ctl-status',controllableCount>0
       ?(d.ctl_status||'')
       :(fanCount>0?(LANG==='ko'?'RPM 모니터링만 가능':'RPM monitoring only'):(LANG==='ko'?'팬 센서 없음':'No fan sensors')));
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
     renderFanCards(d.fans,d.can_control);
     updateFanEmptyState(d);
   } else {
     set('ctl-status',LANG==='ko'?'사용 불가':'unavailable');
     updateProfileStrip(d);
     updateFanApplyStatus(d);
     if(note){note.style.display='';note.textContent=LANG==='ko'?'이 시스템에서는 팬 제어를 사용할 수 없습니다. 감지 가능한 RPM만 표시합니다.':'Fan control is unavailable on this system; only detected RPM is shown.';}
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
var WEBVIEW_READY_SENT=false;
function sendWebviewReady(){
  if(!WEBVIEW_READY_SENT&&window.ipc){
    WEBVIEW_READY_SENT=true;
    window.ipc.postMessage('ready');
  }
}
sendWebviewReady();
// A prewarmed macOS WebView can expose its IPC bridge a frame after the page
// script runs. One retry makes the ready handshake deterministic without
// touching the sensor path or delaying the first render.
setTimeout(sendWebviewReady,250);
// One card per controllable fan — independent Auto/Manual toggle + a slider
// bounded to that fan's own min/max RPM (not a 0-100% abstraction), so you
// can pin e.g. just the left fan while the right one keeps following the
// curve. Built once per fan id and updated in place on every tick, so an
// in-progress slider drag never gets clobbered by the next poll.
function renderFanCards(fans,enabled){
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
    card.dataset.controlEnabled=enabled?'1':'0';
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
    var fanName=f.l||(LANG==='ko'?'팬 '+f.id:'Fan '+f.id);
    card.querySelector('.fn').textContent=fanName;
    var unit=useRpm?'RPM':'%';
    card.querySelector('input[type=range]').setAttribute('aria-label',fanName+' '+(LANG==='ko'?'목표 속도 조절':'target speed slider')+' ('+unit+')');
    card.querySelector('.fa-num').setAttribute('aria-label',fanName+' '+(LANG==='ko'?'목표 속도 입력':'target speed input')+' ('+unit+')');
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
    card.querySelector('.fa-auto').disabled=!enabled||!!pendingMode;
    card.querySelector('.fa-manual').disabled=!enabled||!!pendingMode;
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
    slider.disabled=!enabled||!!pendingMode;
    numInput.disabled=!enabled||!!pendingMode;
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
  if(card.dataset.controlEnabled!=='1')return false;
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
function cancelSetupProgressFrame(){
  if(SETUP_PROGRESS.frame!==null)cancelAnimationFrame(SETUP_PROGRESS.frame);
  SETUP_PROGRESS.frame=null;
  SETUP_PROGRESS.started=null;
}
function setupProgressCanAnimate(){
  return !document.hidden&&(railView()==='fan'||railView()==='settings')
    &&!(window.matchMedia&&window.matchMedia('(prefers-reduced-motion: reduce)').matches);
}
function paintSetupProgress(){
  var state=SETUP_PROGRESS,percent=Math.min(state.target,Math.round(state.value));
  setText('setup-progress-percent',percent+'%');
  var track=document.getElementById('setup-progress-track');
  var fill=document.getElementById('setup-progress-fill');
  if(fill)fill.style.width=state.value+'%';
  if(track){
    track.setAttribute('aria-valuenow',String(percent));
    track.setAttribute('aria-label',LANG==='ko'?'설치 단계':'Installation stages');
    track.setAttribute('aria-valuetext',(LANG==='ko'?'설치 단계 ':'Installation stages ')+percent+'%');
  }
  var labels=LANG==='ko'
    ?{preflight:'설치 준비',installing:'승인 및 설치',verifying:'데몬 응답 확인',complete:'설치 완료',failed:'설치 미완료'}
    :{preflight:'Preparing',installing:'Approval and installation',verifying:'Verifying daemon',complete:'Installation complete',failed:'Installation incomplete'};
  // Announce stage changes only; per-frame percentage belongs to the progressbar.
  setText('setup-progress-label',(LANG==='ko'?'설치 단계 · ':'Installation stages · ')+(labels[state.phase]||''));
}
function tickSetupProgress(timestamp){
  var state=SETUP_PROGRESS;
  state.frame=null;
  if(!setupProgressCanAnimate()){
    state.value=state.target;state.started=null;paintSetupProgress();return;
  }
  if(state.started===null){state.started=timestamp;state.from=state.value;}
  var fraction=Math.min(1,Math.max(0,(timestamp-state.started)/450));
  state.value=Math.min(state.target,state.from+(state.target-state.from)*(1-Math.pow(1-fraction,3)));
  paintSetupProgress();
  if(fraction<1&&state.value<state.target)state.frame=requestAnimationFrame(tickSetupProgress);
  else{state.value=state.target;state.started=null;}
}
function refreshSetupProgressMotion(){
  var state=SETUP_PROGRESS;
  cancelSetupProgressFrame();
  if(!setupProgressCanAnimate()||state.phase==='failed'||state.phase==='idle'){
    state.value=state.target;paintSetupProgress();return;
  }
  if(state.value<state.target)state.frame=requestAnimationFrame(tickSetupProgress);
}
function resetSetupProgress(){
  cancelSetupProgressFrame();
  SETUP_PROGRESS.phase='idle';SETUP_PROGRESS.value=0;SETUP_PROGRESS.target=0;
  paintSetupProgress();
}
function renderSetupProgress(progress){
  var state=SETUP_PROGRESS,phase=progress.phase||'idle',target=Number(progress.percent);
  if(!Number.isFinite(target)||!['preflight','installing','verifying','complete','failed'].includes(phase)){
    resetSetupProgress();phase='idle';target=0;
  }
  target=Math.max(0,Math.min(phase==='complete'?100:99,target));
  if(target<state.target)resetSetupProgress();
  state.phase=phase;
  if(target!==state.target){cancelSetupProgressFrame();state.target=target;}
  var box=document.getElementById('setup-progress');
  if(box){box.style.display=phase==='idle'?'none':'';box.classList.toggle('failed',phase==='failed');}
  if(phase==='failed'||phase==='idle'||!setupProgressCanAnimate()){
    cancelSetupProgressFrame();state.value=target;paintSetupProgress();
  } else if(state.frame===null&&state.value<target){
    state.frame=requestAnimationFrame(tickSetupProgress);
  } else paintSetupProgress();
}
function setupProgressVisibilityChanged(){refreshSetupProgressMotion();}
document.addEventListener('visibilitychange',setupProgressVisibilityChanged);
window.addEventListener('pagehide',cancelSetupProgressFrame);
if(window.matchMedia){
  var setupProgressMotionQuery=window.matchMedia('(prefers-reduced-motion: reduce)');
  if(setupProgressMotionQuery.addEventListener)setupProgressMotionQuery.addEventListener('change',setupProgressVisibilityChanged);
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
function handleFanSetupAction(btn){
  if(railView()!=='settings'){
    closeSetupMenu();
    setRailView('settings');
    updateSetup(window.__pf_pending||{});
    requestAnimationFrame(function(){
      var action=document.getElementById('setup-fan');
      if(action)action.focus();
    });
    return;
  }
  startFanControlSetup(btn);
}
function startFanControlSetup(btn){
  if(FAN_CONTROL_FIX_PENDING)return;
  FAN_CONTROL_FIX_REVISION=Number((window.__pf_pending||{}).fan_control_install_revision||0);
  FAN_CONTROL_FIX_PENDING=true;
  resetSetupProgress();
  renderSetupProgress({phase:'preflight',percent:10});
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
  // Native setup publishes a completion revision and immediately refreshes
  // this payload. Keep a watchdog only for a crashed native worker; never
  // unlock while the installer still reports itself in flight.
  setTimeout(function(){
    var current=window.__pf_pending||{};
    if(!current.fan_control_installing)FAN_CONTROL_FIX_PENDING=false;
  },60000);
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
var SUPPORT_REPORT_COPY_PENDING=false;
var SUPPORT_REPORT_COPY_REVISION=0;
var SUPPORT_REPORT_COPY_RESULT_UNTIL=0;
function copySupportReport(btn){
  if(SUPPORT_REPORT_COPY_PENDING||!window.__pf_pending)return;
  SUPPORT_REPORT_COPY_PENDING=true;
  SUPPORT_REPORT_COPY_REVISION=Number(window.__pf_pending.support_report_copy_revision||0);
  if(btn){btn.disabled=true;btn.textContent=LANG==='ko'?'복사 중…':'Copying…';}
  window.ipc.postMessage('copydiagnostics');
  setTimeout(function(){
    if(!SUPPORT_REPORT_COPY_PENDING)return;
    SUPPORT_REPORT_COPY_PENDING=false;
    SUPPORT_REPORT_COPY_RESULT_UNTIL=Date.now()+2500;
    var current=document.getElementById('support-report-button');
    if(current){current.disabled=false;current.textContent=LANG==='ko'?'시간 초과':'Timed out';}
  },8000);
}
function renderSupportReportButton(d){
  var button=document.getElementById('support-report-button');
  if(!button)return;
  var revision=Number(d.support_report_copy_revision||0);
  if(SUPPORT_REPORT_COPY_PENDING&&revision>SUPPORT_REPORT_COPY_REVISION){
    SUPPORT_REPORT_COPY_PENDING=false;
    SUPPORT_REPORT_COPY_RESULT_UNTIL=Date.now()+2500;
  }
  var showingResult=!SUPPORT_REPORT_COPY_PENDING&&Date.now()<SUPPORT_REPORT_COPY_RESULT_UNTIL;
  button.disabled=SUPPORT_REPORT_COPY_PENDING;
  button.textContent=SUPPORT_REPORT_COPY_PENDING
    ?(LANG==='ko'?'복사 중…':'Copying…')
    :(showingResult
      ?(d.support_report_copy_ok?(LANG==='ko'?'복사됨':'Copied'):(LANG==='ko'?'복사 실패':'Copy failed'))
      :(LANG==='ko'?'리포트 복사':'Copy Report'));
}
function renderFanActionLog(d){
  var list=document.getElementById('fan-action-log');
  var button=document.getElementById('fan-diagnostic-button');
  var entries=Array.isArray(d.fan_action_log)?d.fan_action_log:[];
  renderSupportReportButton(d);
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
function startAppUpdate(mode,btn){
  // The compact setup menu is available from the fan view, but update
  // progress and the final result live in Settings. Navigate first so a
  // successful click always has visible feedback instead of looking inert.
  if(RAIL_VIEW!=='settings')setRailView('settings');
  if(APP_UPDATE_CHECK_PENDING)return;
  APP_UPDATE_CHECK_PENDING=true;
  if(btn){
    if(!btn.dataset.defaultLabel){
      var current=btn.querySelector('span');
      btn.dataset.defaultLabel=current?current.textContent:btn.textContent;
    }
    btn.disabled=true;
    setButtonLabel(btn,mode==='install'
      ?(LANG==='ko'?'설치 준비 중…':'Preparing…')
      :(LANG==='ko'?'확인 중…':'Checking…'));
  }
  var previous=APP_UPDATE_STATUS||{};
  renderUpdateStatus(updateCheckMetadata({
    current:(window.__pf_pending&&window.__pf_pending.app_version)||'',
    phase:'checking',
    message:mode==='install'
      ?(LANG==='ko'?'설치할 서명 릴리스를 확인 중입니다.':'Checking the signed release before installation.')
      :''
  },previous));
  updateUpdateChannel(window.__pf_pending||{});
  window.ipc.postMessage(mode==='install'?'installupdate':'checkupdates');
  setTimeout(function(){
    if(APP_UPDATE_CHECK_PENDING){
      APP_UPDATE_CHECK_PENDING=false;
      if(btn){
        btn.disabled=false;
        setButtonLabel(btn,btn.dataset.defaultLabel||(mode==='install'
          ?(LANG==='ko'?'지금 업데이트':'Install Update')
          :(LANG==='ko'?'업데이트 확인':'Check for Updates')));
      }
    }
  },120000);
}
function checkAppUpdates(btn){startAppUpdate('check',btn);}
function installAppUpdate(btn){startAppUpdate('install',btn);}
function changeUpdateChannel(value){
  if(value!=='stable'&&value!=='pre-release')return;
  var select=document.getElementById('update-channel');
  if(select)select.disabled=true;
  window.ipc.postMessage('updatechannel:'+value);
}
function updateUpdateChannel(d){
  var select=document.getElementById('update-channel');
  if(!select)return;
  var channel=(d.app_update_status&&d.app_update_status.channel)||d.update_channel;
  select.value=channel==='pre-release'?'pre-release':'stable';
  select.disabled=APP_UPDATE_CHECK_PENDING;
  var options=select.options;
  if(options.length>=2){
    options[0].textContent=LANG==='ko'?'안정 릴리스':'Stable releases';
    options[1].textContent=LANG==='ko'?'프리릴리스 빌드':'Pre-release builds';
  }
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
function notificationThreshold(){
  var input=document.getElementById('notification-temp-threshold');
  var value=input?Math.round(Number(input.value||85)):85;
  return Math.max(50,Math.min(110,isFinite(value)?value:85));
}
function toggleTemperatureNotification(enabled){
  var input=document.getElementById('notification-temp-threshold');
  if(input)input.disabled=!enabled;
  window.ipc.postMessage('notifications:temperature:'+(enabled?notificationThreshold():'off'));
}
function changeTemperatureNotification(){
  var toggle=document.getElementById('notification-temp-toggle');
  var input=document.getElementById('notification-temp-threshold');
  var value=notificationThreshold();
  if(input)input.value=String(value);
  if(toggle&&toggle.checked)window.ipc.postMessage('notifications:temperature:'+value);
}
function setNotificationBoolean(kind,enabled){
  window.ipc.postMessage('notifications:'+kind+':'+(enabled?'1':'0'));
}
function updateNotificationSettings(d){
  var settings=d.notifications||{};
  var threshold=Number(settings.temperature_c);
  var temperatureEnabled=isFinite(threshold)&&threshold>=50&&threshold<=110;
  var temperatureToggle=document.getElementById('notification-temp-toggle');
  var temperatureInput=document.getElementById('notification-temp-threshold');
  if(temperatureToggle)temperatureToggle.checked=temperatureEnabled;
  if(temperatureInput){
    temperatureInput.disabled=!temperatureEnabled;
    if(temperatureEnabled&&document.activeElement!==temperatureInput)temperatureInput.value=String(Math.round(threshold));
  }
  var fanToggle=document.getElementById('notification-fan-toggle');
  var updateToggle=document.getElementById('notification-update-toggle');
  if(fanToggle)fanToggle.checked=settings.fan_failures!==false;
  if(updateToggle)updateToggle.checked=settings.updates!==false;
  var enabled=(temperatureEnabled?1:0)+(settings.fan_failures!==false?1:0)+(settings.updates!==false?1:0);
  setPanelPill('notification-pill',LANG==='ko'?(enabled+'개 켬'):(enabled+' on'),enabled?'info':'');
}
var AUTOMATION_RULES=null;
var AUTOMATION_RULES_RENDERED='';
var AUTOMATION_RULES_DIRTY=false;
var AUTOMATION_RULES_SAVE_PENDING=null;
var AUTOMATION_RULES_EDIT_REVISION=0;
var SETTINGS_REQUEST_SESSION='';
var SETTINGS_REQUEST_COUNTER=0;
function settingsRequest(operation,payload,revision){
  if(!SETTINGS_REQUEST_SESSION){
    SETTINGS_REQUEST_SESSION=window.crypto&&typeof window.crypto.randomUUID==='function'?window.crypto.randomUUID():Date.now().toString(36)+'-'+Math.random().toString(36).slice(2);
  }
  return {id:SETTINGS_REQUEST_SESSION+'-'+(++SETTINGS_REQUEST_COUNTER),operation:operation,payload:payload,revision:revision};
}
function settingsResult(d,request){
  if(!request||!Array.isArray(d.settings_results))return null;
  return d.settings_results.find(function(result){return result.id===request.id&&result.operation===request.operation;})||null;
}
function settingsSavedText(result,imported,newerEdits){
  var text=LANG==='ko'?(imported?'설정을 가져왔습니다':'규칙을 저장했습니다'):(imported?'Config imported':'Rules saved');
  if(result.daemon_reload==='reloaded')text+=LANG==='ko'?'. 데몬에 다시 적용했습니다':'. Daemon reloaded';
  else if(result.daemon_reload==='deferred')text+=LANG==='ko'?'. 데몬 다시 적용 대기 중':'. Daemon reload pending';
  else if(result.daemon_reload==='failed')text+=LANG==='ko'?'. 데몬 다시 적용 실패':'. Daemon reload failed';
  if(newerEdits)text+=LANG==='ko'?'. 이후 변경사항은 저장되지 않았습니다':'. Newer changes are unsaved';
  return text;
}
function settingsSavedTone(result,newerEdits){
  return newerEdits||result.daemon_reload==='deferred'||result.daemon_reload==='failed'?'warn':'ok';
}
function automationRuleFromData(rule){
  var when=String((rule&&rule.when)||'on_battery').toLowerCase();
  var profile=String((rule&&rule.profile)||'balanced').toLowerCase();
  if(when==='on_battery')return {kind:'on_battery',profile:profile};
  if(when==='on_ac')return {kind:'on_ac',profile:profile};
  if(when.indexOf('cpu_above:')===0){
    var temp=Number(when.slice(10));
    return {kind:'cpu_above',temp:isFinite(temp)?Math.max(0,Math.min(120,Math.round(temp))):85,profile:profile};
  }
  if(when.indexOf('time:')===0){
    var pieces=when.slice(5).split('-');
    var start=Number(pieces[0]),end=Number(pieces[1]);
    return {kind:'time',start:isFinite(start)?Math.max(0,Math.min(23,Math.round(start))):22,end:isFinite(end)?Math.max(0,Math.min(23,Math.round(end))):7,profile:profile};
  }
  return {kind:'on_battery',profile:profile};
}
function automationRuleWhen(rule){
  if(rule.kind==='on_ac'||rule.kind==='on_battery')return rule.kind;
  if(rule.kind==='cpu_above')return 'cpu_above:'+Math.max(0,Math.min(120,Math.round(Number(rule.temp)||85)));
  return 'time:'+Math.max(0,Math.min(23,Math.round(Number(rule.start)||0)))+'-'+Math.max(0,Math.min(23,Math.round(Number(rule.end)||0)));
}
function automationRulesPayload(){
  return (AUTOMATION_RULES||[]).map(function(rule){return {when:automationRuleWhen(rule),profile:rule.profile||'balanced'};});
}
function automationRulesSignature(rules){return JSON.stringify(rules||[]);}
function automationRuleLabel(kind){
  if(LANG==='ko')return {on_battery:'배터리 사용 중',on_ac:'전원 연결 중',time:'시간 범위',cpu_above:'CPU 온도 초과'}[kind]||kind;
  return {on_battery:'On battery',on_ac:'On AC power',time:'Time window',cpu_above:'CPU temperature above'}[kind]||kind;
}
function automationProfileLabel(profile){
  if(LANG==='ko')return {silent:'무음',balanced:'균형',gaming:'게이밍',performance:'고성능',maximum:'최대',custom:'사용자 곡선'}[profile]||profile;
  return {silent:'Silent',balanced:'Balanced',gaming:'Gaming',performance:'Performance',maximum:'Maximum',custom:'Custom'}[profile]||profile;
}
function automationSetStatus(text,tone){
  var status=document.getElementById('automation-status');
  if(status){status.textContent=text||'';status.className='automation-status '+(tone||'');}
}
function automationMarkDirty(){
  AUTOMATION_RULES_DIRTY=true;
  AUTOMATION_RULES_EDIT_REVISION++;
  automationSetStatus(AUTOMATION_RULES_SAVE_PENDING?(LANG==='ko'?'저장 중입니다. 이후 변경사항은 아직 저장되지 않았습니다':'Saving earlier changes. Newer changes are unsaved'):(LANG==='ko'?'변경사항이 저장되지 않았습니다':'Unsaved changes'),'warn');
}
function renderAutomationRules(d){
  var incoming=(d.rules||[]).map(automationRuleFromData);
  var incomingSignature=automationRulesSignature(incoming);
  var result=settingsResult(d,AUTOMATION_RULES_SAVE_PENDING);
  if(result){
    var newerEdits=AUTOMATION_RULES_EDIT_REVISION!==AUTOMATION_RULES_SAVE_PENDING.revision;
    AUTOMATION_RULES_SAVE_PENDING=null;
    AUTOMATION_RULES_DIRTY=!result.ok||newerEdits;
    automationSetStatus(result.ok?settingsSavedText(result,false,newerEdits):String(result.message||'Rules save failed'),result.ok?settingsSavedTone(result,newerEdits):'warn');
  }
  if(AUTOMATION_RULES===null||(!AUTOMATION_RULES_DIRTY&&!AUTOMATION_RULES_SAVE_PENDING)){
    if(AUTOMATION_RULES===null||(!AUTOMATION_RULES_DIRTY&&incomingSignature!==automationRulesSignature(AUTOMATION_RULES))){
      AUTOMATION_RULES=incoming;
      AUTOMATION_RULES_RENDERED='';
    }
  }
  var list=document.getElementById('automation-rules');
  if(!list)return;
  var signature=automationRulesSignature(AUTOMATION_RULES);
  setPanelPill('automation-pill',LANG==='ko'?((AUTOMATION_RULES||[]).length+'개 규칙'):((AUTOMATION_RULES||[]).length+' rules'),(AUTOMATION_RULES||[]).length?'info':'');
  var add=document.getElementById('automation-add'),save=document.getElementById('automation-save');
  if(add)add.disabled=AUTOMATION_RULES_SAVE_PENDING||(AUTOMATION_RULES||[]).length>=32;
  if(save)save.disabled=!!AUTOMATION_RULES_SAVE_PENDING;
  if(signature===AUTOMATION_RULES_RENDERED)return;
  AUTOMATION_RULES_RENDERED=signature;
  list.innerHTML='';
  if(!AUTOMATION_RULES.length){
    var empty=document.createElement('div');empty.className='automation-copy';empty.textContent=LANG==='ko'?'아직 규칙이 없습니다.':'No rules yet.';list.appendChild(empty);return;
  }
  AUTOMATION_RULES.forEach(function(rule,index){
    var row=document.createElement('div');row.className='automation-rule';
    var head=document.createElement('div');head.className='automation-rule-head';
    var number=document.createElement('span');number.className='automation-rule-number';number.textContent=(LANG==='ko'?'규칙 ':'Rule ')+(index+1);head.appendChild(number);
    var remove=document.createElement('button');remove.className='automation-rule-remove';remove.type='button';remove.textContent='×';remove.title=LANG==='ko'?'규칙 삭제':'Remove rule';remove.onclick=function(){AUTOMATION_RULES.splice(index,1);automationMarkDirty();AUTOMATION_RULES_RENDERED='';renderAutomationRules(d);};head.appendChild(remove);row.appendChild(head);
    var grid=document.createElement('div');grid.className='automation-rule-grid';
    var conditionField=document.createElement('label');conditionField.className='automation-rule-field';var conditionLabel=document.createElement('span');conditionLabel.textContent=LANG==='ko'?'조건':'Condition';conditionField.appendChild(conditionLabel);
    var condition=document.createElement('select');['on_battery','on_ac','time','cpu_above'].forEach(function(kind){var option=document.createElement('option');option.value=kind;option.textContent=automationRuleLabel(kind);option.selected=rule.kind===kind;condition.appendChild(option);});
    condition.onchange=function(){rule.kind=condition.value; if(rule.kind==='time'){rule.start=rule.start==null?22:rule.start;rule.end=rule.end==null?7:rule.end;} if(rule.kind==='cpu_above'&&rule.temp==null)rule.temp=85;automationMarkDirty();AUTOMATION_RULES_RENDERED='';renderAutomationRules(d);};conditionField.appendChild(condition);grid.appendChild(conditionField);
    var profileField=document.createElement('label');profileField.className='automation-rule-field';var profileLabel=document.createElement('span');profileLabel.textContent=LANG==='ko'?'프로파일':'Profile';profileField.appendChild(profileLabel);
    var profile=document.createElement('select');['silent','balanced','gaming','performance','maximum','custom'].forEach(function(name){var option=document.createElement('option');option.value=name;option.textContent=automationProfileLabel(name);option.selected=rule.profile===name;profile.appendChild(option);});profile.onchange=function(){rule.profile=profile.value;automationMarkDirty();};profileField.appendChild(profile);grid.appendChild(profileField);row.appendChild(grid);
    if(rule.kind==='time'||rule.kind==='cpu_above'){
      var extra=document.createElement('div');extra.className='automation-rule-extra';
      if(rule.kind==='time'){
        extra.appendChild(document.createTextNode(LANG==='ko'?'시작':'From'));
        var start=document.createElement('input');start.type='number';start.min='0';start.max='23';start.value=String(rule.start==null?22:rule.start);start.setAttribute('aria-label',LANG==='ko'?'시작 시간':'Start hour');start.onchange=function(){rule.start=Math.max(0,Math.min(23,Number(start.value)||0));automationMarkDirty();};extra.appendChild(start);
        extra.appendChild(document.createTextNode(LANG==='ko'?'시':'h  '));extra.appendChild(document.createTextNode(LANG==='ko'?'종료':'to'));
        var end=document.createElement('input');end.type='number';end.min='0';end.max='23';end.value=String(rule.end==null?7:rule.end);end.setAttribute('aria-label',LANG==='ko'?'종료 시간':'End hour');end.onchange=function(){rule.end=Math.max(0,Math.min(23,Number(end.value)||0));automationMarkDirty();};extra.appendChild(end);extra.appendChild(document.createTextNode(LANG==='ko'?'시':'h'));
      } else {
        extra.appendChild(document.createTextNode(LANG==='ko'?'기준 온도':'Threshold'));
        var temp=document.createElement('input');temp.type='number';temp.min='0';temp.max='120';temp.step='1';temp.value=String(rule.temp==null?85:rule.temp);temp.setAttribute('aria-label',LANG==='ko'?'CPU 기준 온도':'CPU temperature threshold');temp.onchange=function(){rule.temp=Math.max(0,Math.min(120,Number(temp.value)||85));automationMarkDirty();};extra.appendChild(temp);extra.appendChild(document.createTextNode('°C'));
      }
      row.appendChild(extra);
    }
    list.appendChild(row);
  });
}
function updateAutomationRules(d){renderAutomationRules(d);}
function addAutomationRule(){
  if(!AUTOMATION_RULES)AUTOMATION_RULES=[];
  if(AUTOMATION_RULES.length>=32)return;
  AUTOMATION_RULES.push({kind:'on_battery',profile:'silent'});
  automationMarkDirty();AUTOMATION_RULES_RENDERED='';renderAutomationRules({rules:AUTOMATION_RULES});
}
function applyQuietAcPreset(){
  if(AUTOMATION_RULES_SAVE_PENDING)return;
  AUTOMATION_RULES=[{kind:'on_battery',profile:'silent'},{kind:'on_ac',profile:'performance'}];
  automationMarkDirty();
  AUTOMATION_RULES_RENDERED='';
  renderAutomationRules({rules:AUTOMATION_RULES});
  automationSetStatus(LANG==='ko'?'배터리 저소음 / AC 고성능 프리셋을 준비했습니다. 저장을 눌러 적용하세요':'Quiet on battery / performance on AC preset ready. Save to apply','warn');
}
function saveAutomationRules(){
  if(AUTOMATION_RULES_SAVE_PENDING)return;
  var request=settingsRequest('rules',JSON.stringify(automationRulesPayload()),AUTOMATION_RULES_EDIT_REVISION);
  AUTOMATION_RULES_SAVE_PENDING=request;
  AUTOMATION_RULES_DIRTY=true;
  automationSetStatus(LANG==='ko'?'규칙 저장을 확인하는 중…':'Confirming rules save…','');
  var save=document.getElementById('automation-save');if(save)save.disabled=true;
  window.ipc.postMessage('settingsrequest:'+JSON.stringify(request));
  setTimeout(function(){
    if(AUTOMATION_RULES_SAVE_PENDING!==request)return;
    AUTOMATION_RULES_SAVE_PENDING=null;AUTOMATION_RULES_DIRTY=true;
    automationSetStatus(LANG==='ko'?'저장 확인 응답이 없습니다. 변경사항은 유지했습니다. 저장된 설정을 확인한 뒤 다시 시도하세요':'Save confirmation not received. Your edits are kept. Check the saved settings before retrying','warn');
    renderAutomationRules({rules:[]});
  },15000);
}
var CONFIG_IMPORT_DIRTY=false;
var CONFIG_IMPORT_PENDING=null;
var CONFIG_IMPORT_EDIT_REVISION=0;
var CONFIG_COPY_REVISION_SEEN=0;
function configTransferSetStatus(text,tone){
  var status=document.getElementById('config-transfer-status');
  if(status){status.textContent=text||'';status.className='config-transfer-status '+(tone||'');}
}
function markConfigImportDirty(){
  CONFIG_IMPORT_DIRTY=true;
  CONFIG_IMPORT_EDIT_REVISION++;
  configTransferSetStatus(CONFIG_IMPORT_PENDING?(LANG==='ko'?'가져오는 중입니다. 이후 변경사항은 아직 저장되지 않았습니다':'Importing earlier text. Newer changes are unsaved'):(LANG==='ko'?'가져올 설정이 변경되었습니다':'Config text changed'),'warn');
}
function updateConfigTransfer(d){
  var textarea=document.getElementById('config-transfer-text');
  var copy=document.getElementById('config-copy');
  var importer=document.getElementById('config-import');
  var result=settingsResult(d,CONFIG_IMPORT_PENDING);
  if(result){
    var newerEdits=CONFIG_IMPORT_EDIT_REVISION!==CONFIG_IMPORT_PENDING.revision;
    CONFIG_IMPORT_PENDING=null;
    CONFIG_IMPORT_DIRTY=!result.ok||newerEdits;
    configTransferSetStatus(result.ok?settingsSavedText(result,true,newerEdits):String(result.message||'Config import failed'),result.ok?settingsSavedTone(result,newerEdits):'warn');
  }
  if(textarea&&!CONFIG_IMPORT_DIRTY&&!CONFIG_IMPORT_PENDING&&typeof d.config_toml==='string'&&document.activeElement!==textarea&&textarea.value!==d.config_toml)textarea.value=d.config_toml;
  var revision=Number(d.config_copy_revision||0);
  if(revision>CONFIG_COPY_REVISION_SEEN){
    CONFIG_COPY_REVISION_SEEN=revision;
    if(!CONFIG_IMPORT_PENDING&&!result)configTransferSetStatus(d.config_copy_ok?(LANG==='ko'?'현재 설정을 클립보드에 복사했습니다':'Current config copied to clipboard'):(LANG==='ko'?'설정 복사 실패':'Config copy failed'),d.config_copy_ok?'ok':'warn');
  }
  setPanelPill('config-transfer-pill',CONFIG_IMPORT_PENDING?(LANG==='ko'?'적용 중':'Applying'):(CONFIG_IMPORT_DIRTY?(LANG==='ko'?'변경됨':'Edited'):(LANG==='ko'?'준비됨':'Ready')),CONFIG_IMPORT_PENDING?'warn':(CONFIG_IMPORT_DIRTY?'info':''));
  if(copy)copy.disabled=!!CONFIG_IMPORT_PENDING;
  if(importer)importer.disabled=!!CONFIG_IMPORT_PENDING;
}
function copyConfigExport(){
  if(CONFIG_IMPORT_PENDING)return;
  configTransferSetStatus(LANG==='ko'?'현재 설정을 클립보드에 복사하는 중…':'Copying current config…','');
  window.ipc.postMessage('copyconfig');
}
function importConfig(){
  if(CONFIG_IMPORT_PENDING)return;
  var textarea=document.getElementById('config-transfer-text');
  var value=textarea?textarea.value.trim():'';
  if(!value){configTransferSetStatus(LANG==='ko'?'가져올 TOML을 붙여넣으세요':'Paste a TOML config first','warn');return;}
  var request=settingsRequest('import_config',value,CONFIG_IMPORT_EDIT_REVISION);
  CONFIG_IMPORT_PENDING=request;CONFIG_IMPORT_DIRTY=true;
  configTransferSetStatus(LANG==='ko'?'설정 가져오기를 확인하는 중…':'Confirming config import…','');
  var importer=document.getElementById('config-import');if(importer)importer.disabled=true;
  window.ipc.postMessage('settingsrequest:'+JSON.stringify(request));
  setTimeout(function(){
    if(CONFIG_IMPORT_PENDING!==request)return;
    CONFIG_IMPORT_PENDING=null;CONFIG_IMPORT_DIRTY=true;
    configTransferSetStatus(LANG==='ko'?'가져오기 확인 응답이 없습니다. 입력은 유지했습니다. 저장된 설정을 확인한 뒤 다시 시도하세요':'Import confirmation not received. Your text is kept. Check the saved settings before retrying','warn');
    updateConfigTransfer({});
  },15000);
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
  ctx.strokeStyle=cssColor('--accent','#6ea8ff');ctx.lineWidth=1.5;ctx.stroke();
  sorted.forEach(function(p){
    ctx.beginPath();ctx.arc(s.px(p[0]),s.py(p[1]),4,0,Math.PI*2);
    ctx.fillStyle=cssColor('--accent','#6ea8ff');ctx.fill();
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
  document.querySelectorAll('.range-tabs .range-tab').forEach(function(b){var active=b.dataset.range===r;b.classList.toggle('active',active);b.setAttribute('aria-pressed',active?'true':'false');});
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
  var snapshot=window.__pf_pending||{can_control:true,active_control_mode:'',active_profile:'',last_cmd_status:''};
  updateProfileStrip(snapshot);
  updateFanApplyStatus(snapshot);
  return true;
}
function fanControlStatusFailed(status){
  return /error|failed|invalid|cancel|unavailable|not ready/i.test(status||'');
}
function profileDutyAt(points,temp){
  if(!points||!points.length)return 0;
  if(temp<=points[0][0])return points[0][1];
  for(var i=1;i<points.length;i++){
    if(temp<=points[i][0]){
      var left=points[i-1],right=points[i];
      var ratio=(temp-left[0])/(right[0]-left[0]);
      return left[1]+(right[1]-left[1])*ratio;
    }
  }
  return points[points.length-1][1];
}
function renderProfileGuide(mode,profile){
  var root=document.getElementById('profile-guide');
  var title=document.getElementById('profile-guide-title');
  var detail=document.getElementById('profile-guide-detail');
  var bars=document.querySelectorAll('#profile-preview-bars span');
  if(!root||!title||!detail)return;
  var guides={
    silent:{en:['Quiet','Lowest noise; allows warmer temperatures.'],ko:['저소음','소음을 줄이고 더 높은 온도를 허용합니다.'],curve:[[30,0],[50,20],[70,40],[85,70]]},
    balanced:{en:['Balanced','Default balance of noise and cooling.'],ko:['균형','소음과 냉각의 기본 균형입니다.'],curve:[[30,15],[50,25],[70,45],[85,75],[90,100]]},
    gaming:{en:['Gaming','Ramps earlier for sustained workloads.'],ko:['게임','지속 부하에 대비해 더 일찍 회전합니다.'],curve:[[30,30],[50,50],[70,75],[85,100]]},
    performance:{en:['Performance','Aggressive cooling for heavy work.'],ko:['고성능','무거운 작업을 위해 적극적으로 냉각합니다.'],curve:[[30,40],[50,60],[75,90],[85,100]]},
    maximum:{en:['Maximum','Fans stay at 100%; loudest and coolest.'],ko:['최대','팬을 100%로 유지합니다. 가장 크고 시원합니다.'],curve:[[0,100],[100,100]]}
  };
  var isAuto=mode!=='profile'||!guides[profile];
  var guide=isAuto
    ?{en:['macOS Auto','macOS manages fan speed for the current workload.'],ko:['macOS 자동','현재 작업에 맞춰 macOS가 팬 속도를 관리합니다.'],curve:null}
    :guides[profile];
  var copy=LANG==='ko'?guide.ko:guide.en;
  title.textContent=copy[0];
  detail.textContent=copy[1];
  root.dataset.mode=isAuto?'auto':profile;
  var temps=[30,50,70,85,90];
  Array.prototype.slice.call(bars).forEach(function(bar,index){
    var duty=isAuto?[18,30,46,68,88][index]:profileDutyAt(guide.curve,temps[index]);
    bar.style.height=Math.max(2,Math.round(duty*.26))+'px';
  });
}
function updateProfileStrip(d){
  var strip=document.getElementById('profile-strip');
  if(!strip)return;
  // A generic control path is not enough to enable profile actions: the
  // current machine must expose at least one fan that accepts writes.
  var enabled=!!d.can_control&&Number(d.controllable_fan_count||0)>0;
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
  if(!activeMode)activeMode='auto';
  var pending=!!FAN_CONTROL_PENDING;
  strip.classList.toggle('disabled',!enabled);
  strip.classList.toggle('pending',pending);
  strip.setAttribute('aria-busy',pending?'true':'false');
  Array.prototype.slice.call(strip.querySelectorAll('button')).forEach(function(b){
    b.disabled=!enabled||pending;
    var isAuto=b.dataset.mode==='auto'&&activeMode==='auto';
    var isProfile=b.dataset.mode==='profile'&&activeMode==='profile'&&b.dataset.profile===activeProfile;
    var selected=isAuto||isProfile;
    b.classList.toggle('active',selected);
    b.setAttribute('aria-pressed',selected?'true':'false');
  });
  renderProfileGuide(activeMode,activeProfile);
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
  var recentResult=FAN_CONTROL_RESULT&&Date.now()-FAN_CONTROL_RESULT.at<12000;
  var strip=document.getElementById('profile-strip');
  if(strip){
    strip.classList.toggle('confirmed',!!recentResult&&!!FAN_CONTROL_RESULT.ok);
    strip.classList.toggle('failed',!!recentResult&&!FAN_CONTROL_RESULT.ok);
  }
  if(FAN_CONTROL_PENDING){
    mode=FAN_CONTROL_PENDING.mode;profile=FAN_CONTROL_PENDING.profile;tone='pending';
  } else if(recentResult){
    mode=FAN_CONTROL_RESULT.mode;profile=FAN_CONTROL_RESULT.profile;tone=FAN_CONTROL_RESULT.ok?'ok':'error';
  }
  el.style.display=FAN_CONTROL_PENDING||recentResult?'':'none';
  var parts=[fanModeLabel(mode,profile)];
  if(FAN_CONTROL_PENDING)parts.push(LANG==='ko'?'적용 확인 중…':'confirming…');
  else if(recentResult){
    parts.push(FAN_CONTROL_RESULT.ok?(LANG==='ko'?'적용 완료':'applied'):(LANG==='ko'?'적용 실패':'failed'));
    if(!FAN_CONTROL_RESULT.ok&&FAN_CONTROL_RESULT.message)parts.push(FAN_CONTROL_RESULT.message);
  }
  if(mode!=='auto'&&typeof d.fan_curve_input_temp_c==='number')parts.push((LANG==='ko'?'입력 ':'input ')+Math.round(d.fan_curve_input_temp_c)+'°C');
  var targets=(d.fans||[]).filter(function(f){return typeof f.target_pct==='number';}).map(function(f){return Number(f.target_pct);}).filter(function(v){return isFinite(v);});
  if(mode!=='auto'&&targets.length)parts.push((LANG==='ko'?'목표 ':'target ')+Math.max.apply(null,targets)+'%');
  el.textContent=parts.filter(Boolean).join(' · ');
  el.className='fan-apply-status '+tone;
}
function processActionText(action){
  if(!action||action.phase==='idle')return '';
  if(action.phase==='pending')return LANG==='ko'?'종료 요청을 확인하는 중…':'Verifying termination request…';
  var name=action.name||('PID '+Number(action.pid||0));
  var messages=LANG==='ko'
    ?{requested:name+' 종료를 요청했습니다.',identity_changed:'프로세스가 바뀌어 요청을 취소했습니다.',not_owned:'현재 사용자의 프로세스만 종료할 수 있습니다.',protected:'PeterFan 또는 시스템 핵심 프로세스는 종료할 수 없습니다.',not_found:'프로세스가 이미 종료되었습니다.',unsupported:'이 시스템에서는 프로세스 종료를 지원하지 않습니다.',signal_failed:'macOS가 종료 요청을 거부했습니다.',invalid_pid:'유효하지 않은 프로세스입니다.'}
    :{requested:'Requested '+name+' to quit.',identity_changed:'The process changed, so the request was cancelled.',not_owned:'Only processes owned by the current user can be quit.',protected:'PeterFan and protected system processes cannot be quit.',not_found:'The process has already exited.',unsupported:'Process termination is unavailable on this system.',signal_failed:'macOS rejected the termination request.',invalid_pid:'The process identity is invalid.'};
  return messages[action.code]||action.message||'';
}
function resetProcessConfirmation(){
  PROCESS_CONFIRM={pid:null,startTime:null,until:0};
}
function renderProcesses(d){
  var input=document.getElementById('process-search');
  if(input&&document.activeElement!==input&&input.value!==(d.process_filter||''))input.value=d.process_filter||'';
  var status=document.getElementById('process-action-status');
  var action=d.process_action||{};
  var revision=Number(action.revision||0);
  if(revision!==PROCESS_ACTION_REVISION){
    PROCESS_ACTION_REVISION=revision;
    resetProcessConfirmation();
  }
  var confirmationActive=PROCESS_CONFIRM.pid!==null&&Date.now()<PROCESS_CONFIRM.until;
  if(status){
    status.textContent=confirmationActive
      ?(LANG==='ko'?'같은 행의 확인 버튼을 한 번 더 누르세요.':'Click Confirm on the same row to request termination.')
      :processActionText(action);
    status.className='proc-status '+(confirmationActive||action.phase==='pending'?'pending':(action.code==='requested'?'ok':(action.phase==='complete'?'error':'')));
  }
  var rows=d.procs||[],pl=document.getElementById('procs-list');
  setVisible('procs-empty',rows.length===0&&d.slow_data_ready);
  if(!pl)return;
  pl.innerHTML='';
  rows.forEach(function(p){
    var r=document.createElement('div');r.className='prow';
    r.innerHTML='<span class="n"></span><span class="pid"></span><span class="c"></span><span class="m"></span><button type="button" class="pkill"></button>';
    r.children[0].textContent=p.name;r.children[0].title=p.name+' · PID '+p.pid;
    r.children[1].textContent='PID '+p.pid;r.children[2].textContent=p.cpu;r.children[3].textContent=p.mem;
    var b=r.children[4],pending=action.phase==='pending'&&action.pid===p.pid;
    var confirming=PROCESS_CONFIRM.pid===p.pid&&PROCESS_CONFIRM.startTime===p.start_time&&Date.now()<PROCESS_CONFIRM.until;
    b.style.visibility=p.can_quit?'visible':'hidden';
    b.disabled=!p.can_quit||!p.start_time||pending;
    b.textContent=pending?'…':(confirming?(LANG==='ko'?'확인':'Confirm'):(LANG==='ko'?'종료':'Quit'));
    b.classList.toggle('confirm',confirming);
    b.title=!p.can_quit?(LANG==='ko'?'현재 사용자가 안전하게 종료할 수 없는 프로세스입니다':'This process is not eligible for safe termination'):(confirming?(LANG==='ko'?'한 번 더 눌러 종료 요청':'Click again to request termination'):(LANG==='ko'?'프로세스 종료 요청':'Request process termination'));
    b.setAttribute('aria-label',b.title+' · '+p.name);
    b.onclick=function(){requestProcessQuit(p);};
    pl.appendChild(r);
  });
}
function setProcessFilter(value){
  clearTimeout(PROCESS_FILTER_TIMER);
  resetProcessConfirmation();
  PROCESS_FILTER_TIMER=setTimeout(function(){if(window.ipc)window.ipc.postMessage('processfilter:'+String(value||'').slice(0,64));},180);
}
function requestProcessQuit(p){
  if(!p||!p.can_quit||!p.start_time)return;
  var same=PROCESS_CONFIRM.pid===p.pid&&PROCESS_CONFIRM.startTime===p.start_time&&Date.now()<PROCESS_CONFIRM.until;
  if(same){
    resetProcessConfirmation();
    if(window.ipc)window.ipc.postMessage('quitprocess:'+p.pid+':'+p.start_time);
    renderProcesses(window.__pf_pending||{});
    return;
  }
  PROCESS_CONFIRM={pid:p.pid,startTime:p.start_time,until:Date.now()+5000};
  renderProcesses(window.__pf_pending||{});
  setTimeout(function(){
    if(PROCESS_CONFIRM.pid===p.pid&&PROCESS_CONFIRM.startTime===p.start_time&&Date.now()>=PROCESS_CONFIRM.until){
      resetProcessConfirmation();
      renderProcesses(window.__pf_pending||{});
    }
  },5050);
}
function setProcSort(s){
  var cpu=document.getElementById('ps-cpu'),mem=document.getElementById('ps-mem');
  if(cpu){cpu.classList.toggle('active',s==='cpu');cpu.setAttribute('aria-pressed',s==='cpu'?'true':'false');}
  if(mem){mem.classList.toggle('active',s==='mem');mem.setAttribute('aria-pressed',s==='mem'?'true':'false');}
  window.ipc.postMessage('procsort:'+s);
}
__CPU_WIDGET_SCRIPT__
__MENU_ITEMS_SCRIPT__
function setMenubarDisplay(style){
  if(!/^(number|cat|both)$/.test(style))return;
  var data=window.__pf_pending||{};
  data.menubar_display=style;
  updateMenubarDisplay(data);
  if(window.ipc)window.ipc.postMessage('display:'+style);
}
function toggleCpuCoreGraph(btn){
  var enabled=btn.getAttribute('aria-pressed')!=='true';
  var data=window.__pf_pending||{};
  data.show_cpu_core_graph=enabled;
  updateCoreGraphSetting(data);
  if(window.ipc)window.ipc.postMessage('cpu-core-graph:'+(enabled?'1':'0'));
}
function updateCoreGraphSetting(d){
  var btn=document.getElementById('cpu-core-graph-toggle');
  if(!btn)return;
  var enabled=d.show_cpu_core_graph!==false;
  btn.setAttribute('aria-pressed',enabled?'true':'false');
  btn.classList.toggle('active',enabled);
  btn.textContent=enabled?(LANG==='ko'?'켜짐':'On'):(LANG==='ko'?'꺼짐':'Off');
}
var RUNNER_CHARACTERS=__RUNNER_CHARACTERS__;
var RUNNER_PREVIEW_ALPHA=__RUNNER_PREVIEWS__;
var RUNNER_PREVIEW_FRAMES=__RUNNER_PREVIEW_FRAMES__;
var RUNNER_SELECTED_PREVIEW=__RUNNER_SELECTED_PREVIEW__;
var RUNNER_SELECTED_PREVIEW_CHARACTER='__RUNNER_SELECTED_PREVIEW_CHARACTER__';
var RUNNER_PREVIEW_STATE={canvas:null,character:'',cpu:0,motionCpu:0,frame:0,band:0,phaseMs:0,lastTick:0,timer:null,visible:true,hasRealCpuSample:false};
var RUNNER_PREVIEW_IMAGES=new Map();
function runnerPreviewBand(cpu){
  var load=Math.max(0,Math.min(100,Number(cpu)||0));
  return load<20?0:(load<55?1:(load<80?2:3));
}
function runnerPreviewBandWithHysteresis(cpu,current){
  var load=Math.max(0,Math.min(100,Number(cpu)||0));
  if(current===0&&load>=24)return 1;
  if(current===0)return 0;
  if(current===1&&load<16)return 0;
  if(current===1&&load>=59)return 2;
  if(current===1)return 1;
  if(current===2&&load<51)return 1;
  if(current===2&&load>=84)return 3;
  if(current===2)return 2;
  if(current===3&&load<76)return 2;
  if(current===3)return 3;
  return load<20?0:(load<55?1:(load<80?2:3));
}
function runnerSelectedPreviewMatchesCharacter(payload,character){
  return Array.isArray(payload.runner_selected_preview)
    &&payload.runner_selected_preview.length===32
    &&payload.runner_selected_preview_character===character;
}
function runnerSelectedPreviewMatchesCurrent(payload,character){
  return runnerSelectedPreviewMatchesCharacter(payload,character)
    &&RUNNER_SELECTED_PREVIEW_CHARACTER===character
    &&payload.runner_selected_preview.every(function(frame,index){
      return frame===RUNNER_SELECTED_PREVIEW[index];
    });
}
function paintRunnerPreview(canvas,character,frame,band){
  if(!canvas)return;
  var selected=canvas.id==='runner-character-preview';
  var canonical=selected&&RUNNER_SELECTED_PREVIEW_CHARACTER===character;
  var frames=canonical?RUNNER_SELECTED_PREVIEW:RUNNER_PREVIEW_FRAMES[character];
  var pose=(Number(frame)||0)%8;
  var index=canonical?(Math.max(0,Math.min(3,Number(band)||0))*8+pose):pose;
  var alpha=frames&&frames.length?frames[index]:RUNNER_PREVIEW_ALPHA[character];
  if(!alpha)return;
  var ctx=canvas.getContext('2d');
  if(!ctx)return;
  var image=RUNNER_PREVIEW_IMAGES.get(alpha);
  if(!image){
    image=ctx.createImageData(canvas.width,canvas.height);
    for(var i=0;i<alpha.length;i+=2){
      var offset=i*2,a=parseInt(alpha.slice(i,i+2),16);
      image.data[offset]=255;image.data[offset+1]=255;image.data[offset+2]=255;image.data[offset+3]=a;
    }
    if(RUNNER_PREVIEW_IMAGES.size>=128)RUNNER_PREVIEW_IMAGES.clear();
    RUNNER_PREVIEW_IMAGES.set(alpha,image);
  }
  ctx.clearRect(0,0,canvas.width,canvas.height);
  ctx.putImageData(image,0,0);
}
function runnerPreviewInterval(cpu,frame,character){
  var sample=Number(cpu);
  var load=Number.isFinite(sample)?Math.max(0,Math.min(100,sample))/100:0;
  return Math.round(20+160*Math.pow(1-load,1.6));
}
function runnerPreviewCanAnimate(){
  var state=RUNNER_PREVIEW_STATE;
  return state&&state.canvas&&state.visible&&!document.hidden
    &&railView()==='settings'
    &&!(window.matchMedia&&window.matchMedia('(prefers-reduced-motion: reduce)').matches);
}
function scheduleRunnerPreview(){
  var state=RUNNER_PREVIEW_STATE;
  if(!runnerPreviewCanAnimate()||state.timer)return;
  var changing=Math.abs(state.cpu-state.motionCpu)>=0.1;
  var remaining=runnerPreviewInterval(state.motionCpu,state.frame,state.character)-state.phaseMs;
  var delay=Math.max(1,Math.min(changing?30:Infinity,remaining));
  state.timer=setTimeout(function(){
    state.timer=null;
    if(!runnerPreviewCanAnimate())return;
    var now=performance.now(),elapsed=Math.max(0,now-state.lastTick);
    state.lastTick=now;
    if(elapsed>1000){elapsed=0;state.phaseMs=0;}
    var delta=state.cpu-state.motionCpu;
    state.motionCpu=Math.abs(delta)<0.1?state.cpu:
      state.motionCpu+delta*(1-Math.exp(-elapsed/(delta>0?75:300)));
    state.phaseMs+=elapsed;
    while(state.phaseMs>=runnerPreviewInterval(state.motionCpu,state.frame,state.character)){
      state.phaseMs-=runnerPreviewInterval(state.motionCpu,state.frame,state.character);
      state.frame=(state.frame+1)%8;
    }
    state.band=runnerPreviewBandWithHysteresis(state.cpu,state.band);
    paintRunnerPreview(state.canvas,state.character,state.frame,state.band);
    scheduleRunnerPreview();
  },delay);
}
function startRunnerPreview(canvas,character,cpu){
  if(RUNNER_PREVIEW_STATE.timer){
    clearTimeout(RUNNER_PREVIEW_STATE.timer);
    RUNNER_PREVIEW_STATE.timer=null;
  }
  RUNNER_PREVIEW_STATE.canvas=canvas;
  RUNNER_PREVIEW_STATE.character=character;
  RUNNER_PREVIEW_STATE.cpu=Math.max(0,Math.min(100,Number(cpu)||0));
  RUNNER_PREVIEW_STATE.frame=0;
  RUNNER_PREVIEW_STATE.motionCpu=RUNNER_PREVIEW_STATE.cpu;
  RUNNER_PREVIEW_STATE.phaseMs=0;
  RUNNER_PREVIEW_STATE.lastTick=performance.now();
  RUNNER_PREVIEW_STATE.band=runnerPreviewBand(RUNNER_PREVIEW_STATE.cpu);
  paintRunnerPreview(canvas,character,0,RUNNER_PREVIEW_STATE.band);
  var reduced=window.matchMedia&&window.matchMedia('(prefers-reduced-motion: reduce)').matches;
  if(reduced)return;
  scheduleRunnerPreview();
}
function setRunnerPreviewVisibility(visible){
  var state=RUNNER_PREVIEW_STATE;
  if(!state)return;
  state.visible=!!visible;
  if(!visible){
    if(state.timer){clearTimeout(state.timer);state.timer=null;}
    return;
  }
  if(!state.timer){state.lastTick=performance.now();scheduleRunnerPreview();}
}
function seedRunnerPreview(cpu){
  var state=RUNNER_PREVIEW_STATE;
  state.cpu=Math.max(0,Math.min(100,Number(cpu)||0));
  state.motionCpu=state.cpu;
  state.band=runnerPreviewBand(state.cpu);
  paintRunnerPreview(state.canvas,state.character,state.frame,state.band);
}
function setRunnerPreviewCpu(cpu){
  var state=RUNNER_PREVIEW_STATE;
  var previous=state.cpu;
  state.cpu=Math.max(0,Math.min(100,Number(cpu)||0));
  if(!state.hasRealCpuSample){
    state.hasRealCpuSample=true;
    state.band=runnerPreviewBand(state.cpu);
    paintRunnerPreview(state.canvas,state.character,state.frame,state.band);
  }
  if(previous!==state.cpu&&state.timer){clearTimeout(state.timer);state.timer=null;}
  scheduleRunnerPreview();
}
function refreshRunnerPreviewMotionPreference(){
  var state=RUNNER_PREVIEW_STATE;
  if(state.timer){clearTimeout(state.timer);state.timer=null;}
  state.lastTick=performance.now();
  scheduleRunnerPreview();
}
document.addEventListener('visibilitychange',refreshRunnerPreviewMotionPreference);
if(window.matchMedia){
  var runnerMotionQuery=window.matchMedia('(prefers-reduced-motion: reduce)');
  if(runnerMotionQuery.addEventListener)runnerMotionQuery.addEventListener('change',refreshRunnerPreviewMotionPreference);
}
function paintRunnerPreviews(root){
  (root||document).querySelectorAll('canvas[data-runner-preview]').forEach(function(canvas){
    paintRunnerPreview(canvas,canvas.dataset.runnerPreview,1,0);
  });
  var selected=document.getElementById('runner-character-preview');
  if(selected)startRunnerPreview(selected,selected.dataset.runnerPreview,0);
}
function positionRunnerCharacterMenu(){
  var trigger=document.getElementById('runner-character-select');
  var menu=document.getElementById('runner-character-menu');
  if(!trigger||!menu||menu.hidden)return;
  var rect=trigger.getBoundingClientRect();
  var width=Math.min(224,window.innerWidth-16);
  menu.style.width=width+'px';
  var left=Math.max(8,Math.min(window.innerWidth-width-8,rect.right-width));
  var menuHeight=Math.min(260,menu.scrollHeight);
  var below=window.innerHeight-rect.bottom-8;
  var top=below>=Math.min(180,menuHeight)?rect.bottom+5:Math.max(8,rect.top-menuHeight-5);
  menu.style.left=Math.round(left)+'px';
  menu.style.top=Math.round(top)+'px';
}
function closeRunnerCharacterMenu(returnFocus){
  var trigger=document.getElementById('runner-character-select');
  var menu=document.getElementById('runner-character-menu');
  if(!trigger||!menu)return;
  menu.classList.remove('show');
  menu.hidden=true;
  trigger.setAttribute('aria-expanded','false');
  if(returnFocus)trigger.focus();
}
function openRunnerCharacterMenu(focusSelected){
  var trigger=document.getElementById('runner-character-select');
  var menu=document.getElementById('runner-character-menu');
  if(!trigger||!menu)return;
  menu.hidden=false;
  menu.classList.add('show');
  trigger.setAttribute('aria-expanded','true');
  positionRunnerCharacterMenu();
  if(focusSelected){
    var selected=menu.querySelector('[aria-selected="true"]')||menu.querySelector('.character-option');
    if(selected)selected.focus();
  }
}
function toggleRunnerCharacterMenu(event){
  if(event)event.stopPropagation();
  var menu=document.getElementById('runner-character-menu');
  if(menu&&!menu.hidden)closeRunnerCharacterMenu(false);else openRunnerCharacterMenu(false);
}
function handleRunnerTriggerKey(event){
  if(event.key==='ArrowDown'||event.key==='ArrowUp'||event.key==='Enter'||event.key===' '){
    event.preventDefault();openRunnerCharacterMenu(true);
  } else if(event.key==='Escape'){
    closeRunnerCharacterMenu(false);
  }
}
function handleRunnerOptionKey(event){
  var options=Array.prototype.slice.call(document.querySelectorAll('.character-option'));
  var index=options.indexOf(event.currentTarget);
  if(event.key==='ArrowDown'||event.key==='ArrowUp'){
    event.preventDefault();
    var next=event.key==='ArrowDown'?Math.min(options.length-1,index+1):Math.max(0,index-1);
    options[next].focus();
  } else if(event.key==='Home'||event.key==='End'){
    event.preventDefault();options[event.key==='Home'?0:options.length-1].focus();
  } else if(event.key==='Escape'){
    event.preventDefault();closeRunnerCharacterMenu(true);
  } else if(event.key==='Enter'||event.key===' '){
    event.preventDefault();chooseRunnerCharacter(event.currentTarget.dataset.character);
  }
}
function chooseRunnerCharacter(character){
  setRunnerCharacter(character);
  closeRunnerCharacterMenu(true);
}
function setRunnerCharacter(character){
  if(character!=='random'&&RUNNER_CHARACTERS.indexOf(character)<0)return;
  var data=window.__pf_pending||{};
  data.runner_random=character==='random';
  if(character!=='random')data.runner_character=character;
  updateMenubarDisplay(data);
  if(window.ipc)window.ipc.postMessage('character:'+character);
}
function updateMenubarDisplay(d){
  var legacy=document.getElementById("menubar-display-setting");if(legacy)legacy.hidden=d.menu_items_supported===true;
  var style=/^(number|cat|both)$/.test(d.menubar_display)?d.menubar_display:'both';
  document.querySelectorAll('.display-segment button').forEach(function(button){
    var active=button.dataset.display===style;
    button.classList.toggle('active',active);
    button.setAttribute('aria-pressed',active?'true':'false');
  });
  var character=RUNNER_CHARACTERS.indexOf(d.runner_character)>=0?d.runner_character:'cat';
  var cpu=Math.max(0,Math.min(100,Number(d.runner_cpu_pct)||0));
  var canonicalPackChanged=runnerSelectedPreviewMatchesCharacter(d,character)
    &&!runnerSelectedPreviewMatchesCurrent(d,character);
  if(canonicalPackChanged){
    RUNNER_SELECTED_PREVIEW=d.runner_selected_preview;
    RUNNER_SELECTED_PREVIEW_CHARACTER=character;
  }
  var characterSelect=document.getElementById('runner-character-select');
  var selection=d.runner_random?'random':character;
  var selectedOption=document.querySelector('.character-option[data-character="'+selection+'"]');
  document.querySelectorAll('.character-option').forEach(function(option){
    option.setAttribute('aria-selected',option.dataset.character===selection?'true':'false');
  });
  if(characterSelect)characterSelect.dataset.character=selection;
  var characterLabel=document.getElementById('runner-character-label');
  if(characterLabel&&selectedOption)characterLabel.textContent=selectedOption.querySelector('.character-option-label').textContent;
  var characterPreview=document.getElementById('runner-character-preview');
  if(characterPreview&&characterPreview.dataset.runnerPreview!==character){
    characterPreview.dataset.runnerPreview=character;
    startRunnerPreview(characterPreview,character,cpu);
  }
  if(canonicalPackChanged||!RUNNER_PREVIEW_STATE.hasRealCpuSample){
    RUNNER_PREVIEW_STATE.hasRealCpuSample=true;
    seedRunnerPreview(cpu);
  } else {
    setRunnerPreviewCpu(cpu);
  }
  var pace=document.getElementById('runner-pace');
  if(!pace)return;
  if(d.runner_reduce_motion){
    pace.textContent=LANG==='ko'?'동작 줄이기 · 정지 화면':'Reduce Motion · still frame';
    pace.title=LANG==='ko'
      ?'macOS 손쉬운 사용 설정에 따라 러너 애니메이션을 멈췄습니다'
      :'The runner animation is paused by the macOS accessibility setting';
    return;
  }
  var label=cpu<20
    ?(LANG==='ko'?'천천히':'Strolling')
    :(cpu<55
      ?(LANG==='ko'?'가볍게':'Jogging')
      :(cpu<80?(LANG==='ko'?'빠르게':'Running'):(LANG==='ko'?'전력 질주':'Sprinting')));
  pace.textContent='CPU '+Math.round(cpu)+'% · '+label;
  pace.title=(LANG==='ko'?'CPU 사용률에 따라 러너 속도가 바뀝니다':'Runner speed follows CPU usage')
    +' · '+Math.round(Number(d.runner_interval_ms)||0)+' ms';
}
paintRunnerPreviews(document);
document.addEventListener('click',function(event){
  var menu=document.getElementById('runner-character-menu');
  var trigger=document.getElementById('runner-character-select');
  if(menu&&!menu.hidden&&!menu.contains(event.target)&&trigger&&!trigger.contains(event.target)){
    closeRunnerCharacterMenu(false);
  }
});
document.addEventListener('keydown',function(event){
  if(event.key==='Escape'&&PROCESS_CONFIRM.pid!==null){
    resetProcessConfirmation();
    renderProcesses(window.__pf_pending||{});
  }
});
window.addEventListener('resize',function(){closeRunnerCharacterMenu(false);});
var runnerScrollPane=document.querySelector('.main-pane');
if(runnerScrollPane)runnerScrollPane.addEventListener('scroll',function(){closeRunnerCharacterMenu(false);},{passive:true});
function updateSetup(d){
  renderSetupProgress(d.fan_control_install_progress||{phase:'idle',percent:0});
  var setup=document.getElementById('setup-row');
  var fanView=railView()==='fan';
  var stateReady=d.fan_control_state_ready!==false;
  var supported=!!d.fan_control_supported;
  var fanCount=Number(d.fan_count||0);
  var controllableCount=Number(d.controllable_fan_count||0);
  var noFans=stateReady&&fanCount===0;
  var readOnly=stateReady&&fanCount>0&&controllableCount===0;
  var statusTitle=noFans
    ?(LANG==='ko'?'팬 감지 안 됨':'No fans detected')
    :(readOnly
      ?(LANG==='ko'?'읽기 전용 팬':'Read-only fans')
      :(LANG==='ko'?'팬 모니터링':'Fan monitoring'));
  var statusDetail=noFans
    ?(LANG==='ko'?'이 시스템에서 팬 센서를 찾지 못했습니다':'No fan sensors were reported by this system')
    :(readOnly
      ?(LANG==='ko'?'RPM 모니터링만 가능':'RPM monitoring only')
      :(LANG==='ko'?'이 시스템에서는 RPM 읽기만 지원':'This system exposes RPM monitoring only'));
  if(setup)setup.style.display=fanView||railView()==='settings'?'flex':'none';
  var title=document.getElementById('setup-title');
  if(title){
    title.textContent=!stateReady
      ?(LANG==='ko'?'팬 제어 확인 중…':'Checking fan control…')
      :(!supported||noFans||readOnly
        ?statusTitle
        :(d.setup_title||'Ready'));
  }
  var detail=document.getElementById('setup-detail');
  if(detail){
    detail.textContent=!stateReady
      ?(LANG==='ko'?'데몬과 팬 상태를 확인하는 중':'Reading daemon and fan state')
      :(!supported||noFans||readOnly
        ?statusDetail
        :(d.setup_detail||('v'+(d.app_version||''))));
  }
  var dot=document.getElementById('setup-dot');
  if(dot)dot.className='setup-dot '+(!stateReady?'info':(noFans||readOnly||!supported?'warn':(d.setup_tone||'info')));
  var fan=document.getElementById('setup-fan');
  if(fan){
    var showSetup=stateReady&&supported&&controllableCount>0&&(d.fan_setup_needed||d.daemon_update_needed);
    fan.style.display=showSetup?'':'none';
    fan.disabled=FAN_CONTROL_FIX_PENDING||!showSetup;
    fan.title=fanView
      ?(LANG==='ko'?'팬 제어 설정 열기':'Open fan-control settings')
      :(d.daemon_update_needed
        ?(LANG==='ko'?'팬 제어 재설치':'Reinstall fan control')
        :(LANG==='ko'?'팬 제어 활성화':'Enable fan control'));
    fan.textContent=FAN_CONTROL_FIX_PENDING
      ?(LANG==='ko'?'설치 중…':'Installing…')
      :(fanView?(LANG==='ko'?'설정하기':'Set Up')
        :(d.daemon_update_needed?(LANG==='ko'?'팬 제어 재설치':'Reinstall Fan Control'):(LANG==='ko'?'팬 제어 활성화':'Enable Fan Control')));
  }
  var more=document.getElementById('setup-more');
  if(more){
    more.disabled=!stateReady||APP_UPDATE_CHECK_PENDING;
    more.title=LANG==='ko'?'설정 작업':'Setup actions';
    more.setAttribute('aria-label',more.title);
  }
  var update=document.getElementById('setup-update');
  if(update){
    update.disabled=APP_UPDATE_CHECK_PENDING;
    update.textContent=APP_UPDATE_CHECK_PENDING?(LANG==='ko'?'업데이트 중…':'Updating…'):(LANG==='ko'?'지금 업데이트':'Update Now');
    update.title=LANG==='ko'?'최신 버전을 확인하고 바로 설치':'Check for and immediately install an app update';
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
    var settings=document.getElementById('railSettings');
    if(settings){setButtonLabel(settings,LANG==='ko'?'설정':'Settings');settings.title=LANG==='ko'?'설정 열기':'Open settings';}
    var system=document.getElementById('railSystem');
    if(system){setButtonLabel(system,LANG==='ko'?'시스템':'System');system.title=LANG==='ko'?'시스템 지표 열기':'Open system metrics';}
    RAIL_NAV_READY=true;
  }
  var view=railView();
  var nativeUpdate=d.app_update_status||{};
  var nativePhase=nativeUpdate.phase||'idle';
  var nativePending=nativePhase==='checking'||nativePhase==='downloading'||nativePhase==='queued';
  APP_UPDATE_CHECK_PENDING=nativePending;
  updateUpdateChannel(d);
  if(view==='settings'){
    var persisted=reconcileUpdateReceipt(d.update_install_result,d.app_version||'',Date.now()/1000);
    if(nativePhase!=='idle'){
      APP_UPDATE_STATUS={
        current:d.app_version||'',
        latest:nativeUpdate.latest||'',
        url:nativeUpdate.url||'',
        notes:formatReleaseNotes(nativeUpdate.notes||''),
        message:nativeUpdate.message||'',
        install_ready:!!nativeUpdate.install_ready,
        phase:nativePhase,
        persisted:false
      };
    } else if(persisted){
      var persistedKey=[persisted.status||'',persisted.version||'',persisted.updated_at_unix||''].join(':');
      if(persistedKey!==APP_PERSISTED_UPDATE_KEY||!APP_UPDATE_STATUS||!APP_UPDATE_STATUS.persisted){
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
    if(nativePhase==='idle'&&(!persisted||!APP_UPDATE_STATUS||!APP_UPDATE_STATUS.persisted)){
      APP_UPDATE_STATUS={current:d.app_version||'',phase:'idle',message:nativeUpdate.message||''};
    }
    if(APP_UPDATE_STATUS)updateCheckMetadata(APP_UPDATE_STATUS,nativeUpdate);
    if(APP_UPDATE_STATUS){APP_UPDATE_STATUS.current=d.app_version||APP_UPDATE_STATUS.current;renderUpdateStatus();}
    else renderUpdateStatus({current:d.app_version||''});
    var updCheck=document.getElementById('rail-update-check');
    if(updCheck){
      updCheck.disabled=nativePending||nativeUpdate.auto_check_enabled===false;
      updCheck.textContent=nativePending
        ?(LANG==='ko'?'확인 중…':'Checking…')
        :(LANG==='ko'?'업데이트 확인':'Check for Updates');
    }
    var updInstall=document.getElementById('rail-update-install');
    if(updInstall&&nativePending)updInstall.disabled=true;
    updateHealthPanel(d);
  } else if(view==='system'){
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
  var tone=failsafe?'warn':(d.daemon_update_needed?'warn':(d.daemon_running?'ok':(d.fan_setup_needed?'warn':'info')));
  var pill=failsafe
    ?(LANG==='ko'?'OS 자동 복귀':'OS fallback')
    :(d.daemon_update_needed
    ?(LANG==='ko'?'재설치 필요':'Reinstall')
    :(d.daemon_running?(LANG==='ko'?'정상':'OK'):(d.fan_setup_needed?(LANG==='ko'?'설정 필요':'Setup'):(LANG==='ko'?'읽기 전용':'Read-only'))));
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
    :(d.can_control?(LANG==='ko'?'앱 직접 제어':'app direct'):(d.fan_setup_needed?(LANG==='ko'?'1회 설정 필요':'setup required'):(LANG==='ko'?'읽기 전용':'read-only')));
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
    :(d.daemon_update_needed?(LANG==='ko'?'재설치 때 1회':'one prompt to reinstall'):(d.fan_setup_needed?(LANG==='ko'?'최초 설정 때 1회':'one prompt for setup'):(LANG==='ko'?'필요 없음':'not needed')));
  setHealthValue('health-approval',approval,d.daemon_running&&!d.daemon_update_needed?'ok':(d.fan_setup_needed?'warn':'info'));
  setHealthValue('health-app','v'+(d.app_version||''),'info');
  renderFanActionLog(d);
}
function updateHardwareAvailability(d){
  var fanCount=d.fan_count||0, controllable=d.controllable_fan_count||0;
  var battery=!!d.batt_present;
  var networkCount=d.network_count||0, networkActive=!!d.network_active;
  var card=document.getElementById('hardware-availability-card');
  if(card)card.style.display='';
  var live=(fanCount>0)||battery||networkCount>0;
  setPanelPill('hardware-pill',live?(LANG==='ko'?'실시간':'Live'):(LANG==='ko'?'요약':'Summary'),live?'ok':'info');
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
  if(d.fan_control_state_ready===false){
    empty.style.display='';
    var loadingTitle=empty.querySelector('.empty-state-title');
    var loadingCopy=empty.querySelector('.empty-state-copy');
    if(loadingTitle)loadingTitle.textContent=LANG==='ko'?'팬 상태 확인 중…':'Checking fan state…';
    if(loadingCopy)loadingCopy.textContent=LANG==='ko'?'팬 센서와 제어 가능 여부를 확인하고 있습니다.':'Reading fan sensors and checking available controls.';
    empty.setAttribute('aria-busy','true');
    return;
  }
  empty.setAttribute('aria-busy','false');
  var fans=d.fans||[];
  var controllable=fans.filter(function(f){return !!f.controllable;}).length;
  var show=controllable===0;
  empty.style.display=show?'':'none';
  if(show){
    var title=empty.querySelector('.empty-state-title');
    var copy=empty.querySelector('.empty-state-copy');
    var noFans=fans.length===0;
    if(title)title.textContent=noFans
      ?(LANG==='ko'?'팬 센서가 없습니다':'No fan sensors')
      :(LANG==='ko'?'읽기 전용 팬':'Read-only fans');
    if(copy)copy.textContent=noFans
      ?(LANG==='ko'?'팬 센서를 찾지 못했습니다. CPU, 메모리와 네트워크는 계속 표시되며, 온도는 지원 센서가 있을 때만 표시됩니다.':'No fan sensors were reported. CPU, memory, and network monitoring remain available; temperature appears only when a supported sensor exists.')
      :(LANG==='ko'?'팬은 감지됐지만 앱에서 직접 제어할 수 없습니다. 실시간 RPM은 계속 표시됩니다.':'Fans are detected, but this system does not expose controllable writes. Live RPM continues to be monitored.');
  }
}
function clearChart(id){
  var cv=document.getElementById(id);
  if(!cv)return;
  var ctx=cv.getContext('2d');
  if(ctx)ctx.clearRect(0,0,cv.width||0,cv.height||0);
  cv._data=[];
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
function renderCoreGraph(d){
  var cv=document.getElementById('summary-cpu-core-chart');
  if(!cv)return;
  var enabled=d.show_cpu_core_graph!==false;
  var cell=cv.closest('.summary-cell.cpu');
  if(cell)cell.classList.toggle('core-graph-enabled',enabled);
  cv.hidden=!enabled;
  if(!enabled)return;
  var series=d.core_hist||[];
  var width=Math.max(1,cv.clientWidth||80),height=Math.max(1,cv.clientHeight||24),ratio=window.devicePixelRatio||1;
  var pixelWidth=Math.max(1,Math.round(width*ratio)),pixelHeight=Math.max(1,Math.round(height*ratio));
  if(cv.width!==pixelWidth||cv.height!==pixelHeight){cv.width=pixelWidth;cv.height=pixelHeight;}
  var ctx=cv.getContext('2d');
  if(!ctx)return;
  ctx.setTransform(ratio,0,0,ratio,0,0);
  ctx.clearRect(0,0,width,height);
  ctx.strokeStyle=cssColor('--track','rgba(255,255,255,.09)');
  ctx.lineWidth=1;
  [0.25,0.5,0.75].forEach(function(level){
    var y=Math.round(height*(1-level))+.5;
    ctx.beginPath();ctx.moveTo(0,y);ctx.lineTo(width,y);ctx.stroke();
  });
  series.forEach(function(values,index){
    if(!values||values.length<2)return;
    var n=values.length;
    ctx.beginPath();
    for(var i=0;i<n;i++){
      var value=Math.max(0,Math.min(100,Number(values[i])||0));
      var x=n>1?(i/(n-1))*width:width;
      var y=height-(value/100)*height;
      if(i===0)ctx.moveTo(x,y);else ctx.lineTo(x,y);
    }
    ctx.strokeStyle=cssColor('--accent','#6ea8ff');
    ctx.globalAlpha=Math.min(.78,.28+index*.035);
    ctx.lineWidth=1.05;
    ctx.stroke();
  });
  ctx.globalAlpha=1;
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

    const RUNNER_QA_SCALE: u32 = 4;
    const RUNNER_QA_PADDING: u32 = 4;

    /// Regression test for the v1.9.3 bug: tray-icon shows the attached menu
    /// on left-click by default, silently pre-empting our own click handler
    /// and making the popover permanently unreachable. No OS/window-server
    /// interaction needed — `TrayIconAttributes` is plain data.
    #[test]
    fn cpu_widget_anchor_tracks_clicked_item_and_rejects_invalid_native_geometry() {
        let runner = tray_rect(100., 0., 52, 30);
        let cpu = tray_rect(500., 0., 98, 30);
        let event = tray_rect(501., 0., 98, 30);
        assert_eq!(
            clicked_tray_anchor(
                "cpu",
                Some(("runner", Some(runner))),
                Some(("cpu", Some(cpu))),
                event
            ),
            Some(cpu)
        );
        assert_eq!(
            clicked_tray_anchor(
                "runner",
                Some(("runner", Some(runner))),
                Some(("cpu", Some(cpu))),
                event
            ),
            Some(runner)
        );
        assert_eq!(
            clicked_tray_anchor(
                "unknown",
                Some(("runner", Some(runner))),
                Some(("cpu", Some(cpu))),
                event
            ),
            None
        );
        assert_eq!(
            clicked_tray_anchor(
                "cpu",
                Some(("runner", Some(runner))),
                Some(("cpu", Some(tray_rect(0., 0., 98, 0)))),
                event
            ),
            Some(event)
        );
        assert_eq!(
            clicked_tray_anchor("cpu", None, Some(("cpu", None)), Rect::default()),
            None
        );
        assert_eq!(
            clicked_tray_anchor(
                "cpu",
                None,
                Some(("cpu", Some(tray_rect(f64::NAN, 0., 98, 30)))),
                event
            ),
            Some(event)
        );
    }

    #[test]
    fn native_preference_bridges_accept_cpu_graph_and_widget_requests() {
        assert!(is_native_preference_command("cpu-core-graph:1"));
        assert!(is_native_preference_command("cpu-core-graph:0"));
        assert!(is_native_preference_command("settingsrequest:{}"));
        assert!(is_native_preference_command("display:both"));
        assert!(!is_native_preference_command("cpu-core-graph:invalid"));
        assert!(!is_native_preference_command("cmd:hold:1"));
        let bridge_call = ["|| is_native_preference", "_command(body)"].concat();
        assert_eq!(include_str!("main.rs").matches(&bridge_call).count(), 2);
    }

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

    #[cfg(target_os = "macos")]
    #[test]
    fn native_status_item_width_is_stable_for_each_display_style() {
        assert_eq!(native_status_item_width(MenubarDisplay::Graph), 36.0);
        assert_eq!(native_status_item_width(MenubarDisplay::Number), 50.0);
        assert_eq!(native_status_item_width(MenubarDisplay::Both), 84.0);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn number_only_transition_cannot_alias_the_hidden_icon_cache() {
        let mut last_runner_icon = Some(7);
        invalidate_runner_icon(&mut last_runner_icon);

        assert_eq!(last_runner_icon, Some(usize::MAX));
        assert_ne!(last_runner_icon, None);
        assert_eq!(
            native_status_item_image_position(MenubarDisplay::Number),
            NSCellImagePosition::NoImage
        );
        assert_eq!(
            native_status_item_image_position(MenubarDisplay::Graph),
            NSCellImagePosition::ImageOnly
        );
        assert_eq!(
            native_status_item_image_position(MenubarDisplay::Both),
            NSCellImagePosition::ImageLeft
        );
    }

    #[test]
    fn tray_focus_dismissal_closes_once_without_swallowing_a_later_open() {
        let now = Instant::now();
        let mut dismissed = Some(now);
        assert!(consume_tray_focus_dismissal(
            &mut dismissed,
            now + Duration::from_millis(25)
        ));
        assert!(dismissed.is_none());
        assert!(!consume_tray_focus_dismissal(&mut dismissed, now));
        dismissed = Some(now);
        assert!(!consume_tray_focus_dismissal(
            &mut dismissed,
            now + Duration::from_secs(1)
        ));
        assert!(dismissed.is_none());
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
    fn cpu_core_groups_split_valid_performance_and_efficiency_layouts() {
        let usages = (0..14).map(|index| index as f32).collect::<Vec<_>>();
        let groups = cpu_core_groups_for_layout(&usages, Some((10, 4)));

        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].kind, "efficiency");
        assert_eq!(groups[0].start_index, 0);
        assert_eq!(groups[0].usages, usages[..4]);
        assert_eq!(groups[1].kind, "performance");
        assert_eq!(groups[1].start_index, 4);
        assert_eq!(groups[1].usages, usages[4..]);
    }

    #[test]
    fn cpu_core_groups_fall_back_when_platform_counts_do_not_match() {
        let usages = vec![12.0, 34.0, 56.0, 78.0];
        let groups = cpu_core_groups_for_layout(&usages, Some((8, 2)));

        assert_eq!(
            groups,
            vec![CpuCoreGroup {
                kind: "logical",
                start_index: 0,
                usages,
            }]
        );
    }

    #[test]
    fn pending_commands_coalesce_ready_and_fan_targets_without_crossing_fans() {
        let mut queue = Vec::new();
        queue_pending_command(&mut queue, "ready:popover".into());
        queue_pending_command(&mut queue, "ready:popover".into());
        queue_pending_command(&mut queue, "profile:balanced".into());
        queue_pending_command(&mut queue, "profile:performance".into());
        queue_pending_command(&mut queue, "fanhold:fan.left:40".into());
        queue_pending_command(&mut queue, "fanhold:fan.left:65".into());
        queue_pending_command(&mut queue, "fanhold:fan.right:55".into());
        queue_pending_command(&mut queue, "display:number".into());
        queue_pending_command(&mut queue, "display:both".into());

        assert_eq!(
            queue,
            vec![
                "ready:popover",
                "profile:performance",
                "fanhold:fan.left:65",
                "fanhold:fan.right:55",
                "display:both",
            ]
        );
    }

    #[test]
    fn process_filter_commands_coalesce_to_the_latest_query() {
        let mut queue = Vec::new();
        queue_pending_command(&mut queue, "processfilter:saf".into());
        queue_pending_command(&mut queue, "processfilter:safari".into());

        assert_eq!(queue, vec!["processfilter:safari"]);
    }

    #[test]
    fn guarded_process_request_requires_pid_and_start_token() {
        assert_eq!(
            parse_process_quit_command("quitprocess:42:1700000000"),
            Some(peterfan_platform::process_control::ProcessQuitRequest {
                pid: 42,
                expected_start_time_unix_secs: 1_700_000_000,
            })
        );
        assert_eq!(parse_process_quit_command("quitprocess:42"), None);
        assert_eq!(parse_process_quit_command("quitprocess:0:1"), None);
    }

    #[test]
    fn native_process_command_surface_is_narrow_and_explicit() {
        assert!(is_native_process_command("processfilter:safari"));
        assert!(is_native_process_command("quitprocess:42:1700000000"));
        assert!(!is_native_process_command("killproc:42"));
        assert!(!is_native_process_command("quitprocess:42"));
        assert!(!is_native_process_command("process:kill:42"));
    }

    #[test]
    fn old_direct_process_kill_path_is_absent() {
        let source = include_str!("main.rs");
        let direct_prefix = ["strip_prefix(\"", "killproc:", "\")"].concat();
        let direct_function = ["fn kill_", "process("].concat();
        assert!(!source.contains(&direct_prefix));
        assert!(!source.contains(&direct_function));
    }

    #[test]
    fn dashboard_process_row_carries_identity_and_native_quit_eligibility() {
        let process = peterfan_core::metrics::ProcessInfo {
            pid: 4242,
            name: "Safari".into(),
            cpu_percent: 12.5,
            memory: 512_000_000,
            start_time_unix_secs: Some(1_700_000_000),
            owned_by_current_user: Some(true),
        };

        let row = dashboard_process_row(&process);

        assert_eq!(row["pid"], 4242);
        assert_eq!(row["start_time"], 1_700_000_000_u64);
        assert_eq!(row["can_quit"], cfg!(target_os = "macos"));
    }

    #[test]
    fn dashboard_process_row_fails_closed_without_owner_or_start_time() {
        let process = peterfan_core::metrics::ProcessInfo {
            pid: 4242,
            name: "Safari".into(),
            cpu_percent: 12.5,
            memory: 512_000_000,
            start_time_unix_secs: None,
            owned_by_current_user: None,
        };

        assert_eq!(dashboard_process_row(&process)["can_quit"], false);
    }

    #[test]
    fn process_action_errors_map_to_stable_ui_codes() {
        use peterfan_platform::process_control::ProcessQuitError;

        assert_eq!(
            process_action_error_code(&ProcessQuitError::IdentityChanged),
            "identity_changed"
        );
        assert_eq!(
            process_action_error_code(&ProcessQuitError::NotOwned),
            "not_owned"
        );
        assert_eq!(
            process_action_error_code(&ProcessQuitError::SignalFailed(1)),
            "signal_failed"
        );
    }

    #[test]
    fn completed_process_action_preserves_pid_native_name_and_stable_code() {
        use peterfan_platform::process_control::{ProcessQuitError, ProcessQuitRequest};

        let request = ProcessQuitRequest {
            pid: 4242,
            expected_start_time_unix_secs: 1_700_000_000,
        };
        let success = completed_process_action(7, request, Ok("Safari".into()));
        assert_eq!(success.revision, 7);
        assert_eq!(success.phase, "complete");
        assert_eq!(success.pid, Some(4242));
        assert_eq!(success.name.as_deref(), Some("Safari"));
        assert_eq!(success.code, Some("requested"));

        let rejected = completed_process_action(8, request, Err(ProcessQuitError::IdentityChanged));
        assert_eq!(rejected.code, Some("identity_changed"));
        assert_eq!(rejected.name, None);
    }

    #[test]
    fn process_action_payload_exposes_revision_and_user_safe_result() {
        let state = ProcessActionState {
            revision: 12,
            phase: "complete",
            pid: Some(4242),
            name: Some("Safari".into()),
            code: Some("requested"),
            message: Some("Termination requested".into()),
        };

        assert_eq!(
            process_action_json(&state),
            serde_json::json!({
                "revision": 12,
                "phase": "complete",
                "pid": 4242,
                "name": "Safari",
                "code": "requested",
                "message": "Termination requested",
            })
        );
    }

    #[test]
    fn filtered_process_results_expand_only_to_the_bounded_limit() {
        assert_eq!(dashboard_process_limit(""), 5);
        assert_eq!(dashboard_process_limit("safari"), 20);
    }

    #[test]
    fn dashboard_process_filter_cache_starts_empty() {
        assert_eq!(DashboardSlowCache::default().proc_filter, "");
    }

    #[test]
    fn process_control_ui_searches_and_requires_inline_identity_confirmation() {
        let html = dashboard_html(ResolvedLanguage::En, false);

        assert!(html.contains(r#"id="process-search""#));
        assert!(html.contains(r#"id="process-action-status" role="status" aria-live="polite""#));
        assert!(html.contains("processfilter:"));
        assert!(html.contains("quitprocess:'+p.pid+':'+p.start_time"));
        assert!(html.contains("PROCESS_CONFIRM.pid===p.pid"));
        assert!(html.contains("PROCESS_CONFIRM.startTime===p.start_time"));
        assert!(html.contains(r#"<span class="pid"></span>"#));
        assert!(html.contains("PROCESS_ACTION_REVISION"));
        assert!(html.contains("event.key==='Escape'"));
        assert!(html.contains("resetProcessConfirmation()"));
        assert!(!html.contains("confirm(msg)"));
        assert!(!html.contains("killproc:"));
    }

    #[test]
    fn process_action_log_uses_verified_native_name_without_command_line_data() {
        let state = ProcessActionState {
            revision: 1,
            phase: "complete",
            pid: Some(4242),
            name: Some("Safari Networking".into()),
            code: Some("requested"),
            message: None,
        };

        assert_eq!(
            process_action_log_entry(&state),
            "process termination completed pid=4242 name=Safari Networking code=requested"
        );
    }

    #[test]
    fn notification_temperature_rule_uses_hysteresis_and_does_not_repeat() {
        let settings = NotificationConfig {
            temperature_c: Some(80.0),
            ..NotificationConfig::default()
        };
        let mut runtime = NotificationRuntime::default();
        let health = serde_json::json!({});

        let first = evaluate_notification_rules(&settings, &mut runtime, Some(82.0), &health);
        assert_eq!(first.len(), 1);
        assert!(first[0].body.contains("82°C"));
        assert!(
            evaluate_notification_rules(&settings, &mut runtime, Some(84.0), &health).is_empty()
        );
        assert!(
            evaluate_notification_rules(&settings, &mut runtime, Some(78.0), &health).is_empty()
        );
        assert!(
            evaluate_notification_rules(&settings, &mut runtime, Some(76.0), &health).is_empty()
        );

        let retriggered = evaluate_notification_rules(&settings, &mut runtime, Some(81.0), &health);
        assert_eq!(retriggered.len(), 1);
    }

    #[test]
    fn notification_fan_rule_baselines_then_reports_new_failures() {
        let settings = NotificationConfig::default();
        let mut runtime = NotificationRuntime::default();
        assert!(
            evaluate_notification_rules(&settings, &mut runtime, None, &serde_json::json!({}))
                .is_empty()
        );
        assert_eq!(runtime.fan_failure_baseline, None);

        let initial = serde_json::json!({
            "fan_write_failure_count": 2,
            "fan_readback_failure_count": 1,
        });
        assert!(evaluate_notification_rules(&settings, &mut runtime, None, &initial).is_empty());

        let failed = serde_json::json!({
            "fan_write_failure_count": 3,
            "fan_readback_failure_count": 1,
            "last_error": "RPM verification timed out",
        });
        let notices = evaluate_notification_rules(&settings, &mut runtime, None, &failed);
        assert_eq!(notices.len(), 1);
        assert!(notices[0].body.contains("RPM verification timed out"));
        assert!(evaluate_notification_rules(&settings, &mut runtime, None, &failed).is_empty());
    }

    #[test]
    fn preference_failed_write_keeps_live_notification_settings_and_allows_retry() {
        let mut live = NotificationConfig::default();
        let original = live.clone();
        let mut candidate = live.clone();
        apply_notification_command(&mut candidate, "notifications:updates:0").unwrap();
        let result = persist_preference(&mut live, candidate.clone(), |_| Err("disk full"));
        assert_eq!(result, Err("disk full"));
        assert_eq!(live.updates, original.updates);
        assert_eq!(live.temperature_c, original.temperature_c);
        persist_preference(&mut live, candidate, |_| Ok::<_, &str>(())).unwrap();
        assert!(!live.updates);
    }

    #[test]
    fn runner_preference_build_failure_never_persists_selection() {
        let persisted = std::cell::Cell::new(false);
        let result = prepare_runner_preference::<()>(
            RunnerCharacter::Cat,
            RunnerCharacter::Fox,
            |_| Err("invalid artwork".into()),
            |_| {
                persisted.set(true);
                Ok(())
            },
        );
        assert_eq!(result, Err("invalid artwork".to_string()));
        assert!(!persisted.get());
    }

    #[test]
    fn runner_preference_write_failure_returns_no_assets_to_commit() {
        let built = std::cell::Cell::new(false);
        let result = prepare_runner_preference(
            RunnerCharacter::Cat,
            RunnerCharacter::Fox,
            |_| {
                built.set(true);
                Ok("prepared fox")
            },
            |_| Err("read-only filesystem".into()),
        );
        assert_eq!(result, Err("read-only filesystem".to_string()));
        assert!(built.get());
        let saved = prepare_runner_preference(
            RunnerCharacter::Cat,
            RunnerCharacter::Fox,
            |_| Ok("prepared fox"),
            |_| Ok(()),
        )
        .unwrap();
        assert_eq!(saved, Some("prepared fox"));
    }

    #[test]
    fn runner_mode_change_persists_without_rebuilding_current_motion() {
        let persisted = std::cell::Cell::new(false);
        let result = prepare_runner_preference::<()>(
            RunnerCharacter::Cat,
            RunnerCharacter::Cat,
            |_| panic!("same-character mode changes must preserve assets"),
            |character| {
                assert_eq!(character, RunnerCharacter::Cat);
                persisted.set(true);
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(result, None);
        assert!(persisted.get());
    }

    #[test]
    fn notification_fan_rule_reports_once_per_failure_incident() {
        let settings = NotificationConfig::default();
        let mut runtime = NotificationRuntime::default();
        let healthy = serde_json::json!({
            "fan_write_failure_count": 0,
            "fan_readback_failure_count": 0,
            "failsafe_active": false,
            "consecutive_fan_write_failures": 0,
            "consecutive_fan_readback_failures": 0,
            "stale_fan_ids": [],
        });
        assert!(evaluate_notification_rules(&settings, &mut runtime, None, &healthy).is_empty());

        let first_failure = serde_json::json!({
            "fan_write_failure_count": 1,
            "fan_readback_failure_count": 0,
            "failsafe_active": true,
            "consecutive_fan_write_failures": 1,
            "consecutive_fan_readback_failures": 0,
            "stale_fan_ids": [],
            "last_error": "write failed",
        });
        assert_eq!(
            evaluate_notification_rules(&settings, &mut runtime, None, &first_failure).len(),
            1
        );

        let same_incident_retry = serde_json::json!({
            "fan_write_failure_count": 2,
            "fan_readback_failure_count": 0,
            "failsafe_active": true,
            "consecutive_fan_write_failures": 2,
            "consecutive_fan_readback_failures": 0,
            "stale_fan_ids": [],
            "last_error": "write failed again",
        });
        assert!(
            evaluate_notification_rules(&settings, &mut runtime, None, &same_incident_retry)
                .is_empty()
        );

        let recovered = serde_json::json!({
            "fan_write_failure_count": 2,
            "fan_readback_failure_count": 0,
            "failsafe_active": false,
            "consecutive_fan_write_failures": 0,
            "consecutive_fan_readback_failures": 0,
            "stale_fan_ids": [],
        });
        assert!(evaluate_notification_rules(&settings, &mut runtime, None, &recovered).is_empty());

        let new_incident = serde_json::json!({
            "fan_write_failure_count": 2,
            "fan_readback_failure_count": 1,
            "failsafe_active": true,
            "consecutive_fan_write_failures": 0,
            "consecutive_fan_readback_failures": 1,
            "stale_fan_ids": ["fan.left"],
            "last_error": "RPM verification timed out",
        });
        assert_eq!(
            evaluate_notification_rules(&settings, &mut runtime, None, &new_incident).len(),
            1
        );
    }

    #[test]
    fn notification_fan_rule_does_not_repeat_an_incident_seen_at_startup() {
        let settings = NotificationConfig::default();
        let mut runtime = NotificationRuntime::default();
        let already_failed = serde_json::json!({
            "fan_write_failure_count": 7,
            "fan_readback_failure_count": 2,
            "failsafe_active": true,
            "consecutive_fan_write_failures": 3,
            "consecutive_fan_readback_failures": 1,
            "stale_fan_ids": ["fan.left"],
        });
        assert!(
            evaluate_notification_rules(&settings, &mut runtime, None, &already_failed).is_empty()
        );

        let retry = serde_json::json!({
            "fan_write_failure_count": 8,
            "fan_readback_failure_count": 2,
            "failsafe_active": true,
            "consecutive_fan_write_failures": 4,
            "consecutive_fan_readback_failures": 1,
            "stale_fan_ids": ["fan.left"],
        });
        assert!(evaluate_notification_rules(&settings, &mut runtime, None, &retry).is_empty());
    }

    #[test]
    fn notification_commands_validate_and_update_preferences() {
        let mut settings = NotificationConfig::default();
        apply_notification_command(&mut settings, "notifications:temperature:82").unwrap();
        apply_notification_command(&mut settings, "notifications:fan-failures:0").unwrap();
        apply_notification_command(&mut settings, "notifications:updates:0").unwrap();
        assert_eq!(settings.temperature_c, Some(82.0));
        assert!(!settings.fan_failures);
        assert!(!settings.updates);

        apply_notification_command(&mut settings, "notifications:temperature:off").unwrap();
        assert_eq!(settings.temperature_c, None);
        assert!(apply_notification_command(&mut settings, "notifications:temperature:49").is_err());
        assert!(apply_notification_command(&mut settings, "notifications:updates:maybe").is_err());
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

    #[derive(Debug)]
    struct DisplayRegressionScenario {
        name: &'static str,
        tray: Rect,
        displays: [DisplayBounds; 3],
        popover_width: f64,
        expected_display: usize,
    }

    fn display_regression_scenarios() -> [DisplayRegressionScenario; 3] {
        [
            DisplayRegressionScenario {
                name: "left external display",
                tray: tray_rect(-40.0, 0.0, 24, 24),
                displays: [
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
                    DisplayBounds {
                        x: 0.0,
                        y: -900.0,
                        width: 1440.0,
                        height: 900.0,
                    },
                ],
                popover_width: 440.0,
                expected_display: 0,
            },
            DisplayRegressionScenario {
                name: "right mixed-DPI display",
                tray: tray_rect(4260.0, 0.0, 24, 24),
                displays: [
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
                    DisplayBounds {
                        x: 0.0,
                        y: -900.0,
                        width: 1440.0,
                        height: 900.0,
                    },
                ],
                popover_width: POPOVER_W,
                expected_display: 1,
            },
            DisplayRegressionScenario {
                name: "upper display with negative global Y",
                tray: tray_rect(700.0, -40.0, 24, 24),
                displays: [
                    DisplayBounds {
                        x: 0.0,
                        y: 0.0,
                        width: 1728.0,
                        height: 1117.0,
                    },
                    DisplayBounds {
                        x: -1920.0,
                        y: 0.0,
                        width: 1920.0,
                        height: 1080.0,
                    },
                    DisplayBounds {
                        x: 0.0,
                        y: -900.0,
                        width: 1440.0,
                        height: 900.0,
                    },
                ],
                popover_width: 440.0,
                expected_display: 2,
            },
        ]
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
    fn multi_display_regression_matrix_keeps_popover_on_clicked_display() {
        for scenario in display_regression_scenarios() {
            let pos = popover_position_for_rect(
                scenario.tray,
                scenario.popover_width,
                &scenario.displays,
            );
            let display = scenario.displays[scenario.expected_display];

            assert!(
                pos.x >= display.x + 8.0,
                "{}: popover escaped the clicked display on the left",
                scenario.name
            );
            assert!(
                pos.x + scenario.popover_width <= display.right() - 8.0,
                "{}: popover escaped the clicked display on the right",
                scenario.name
            );
            assert_eq!(
                pos.y,
                scenario.tray.position.y + scenario.tray.size.height as f64,
                "{}: popover should remain flush below the menu bar",
                scenario.name
            );
        }
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
    fn appkit_top_left_conversion_preserves_external_display_coordinates() {
        assert_eq!(
            appkit_window_top_left(LogicalPosition::new(3800.0, 24.0), 1117.0),
            (3800.0, 1093.0)
        );
        assert_eq!(
            appkit_window_top_left(LogicalPosition::new(-1500.0, -1056.0), 1117.0),
            (-1500.0, 2173.0)
        );
    }

    #[test]
    fn fan_controls_require_a_usable_daemon_or_direct_write_access() {
        assert!(!fan_control_access(true, false, false, false));
        assert!(fan_control_access(true, true, false, false));
        assert!(fan_control_access(true, false, true, false));
        assert!(fan_control_access(true, false, false, true));
        assert!(fan_control_access(false, true, false, false));
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
        assert!(en.contains(r#"<html lang="en">"#));
        assert!(en.contains(">General<"));
        assert!(en.contains(">Hardware<"));
        assert!(en.contains("var LANG='en';"));
        assert!(!en.contains("__LANG__"));
        assert!(!en.contains("__SHOWCURVE__"));

        let ko = dashboard_html(ResolvedLanguage::Ko, false);
        assert!(ko.contains(">팬 제어<"));
        assert!(ko.contains(">PeterFan 종료<"));
        assert!(ko.contains(r#"<html lang="ko">"#));
        assert!(ko.contains(">일반<"));
        assert!(ko.contains(">하드웨어<"));
        assert!(ko.contains(">시스템<"));
        assert!(ko.contains(">상세 창 열기<"));
        assert!(ko.contains(">코어 상세<"));
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
            assert!(html.contains("requestProcessQuit"));
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
            assert!(html.contains("installupdate"));
            assert!(html.contains("checkAppUpdates"));
            assert!(html.contains("installAppUpdate"));
            assert!(html.contains("updateSetup"));
            assert!(html.contains("daemon_update_needed"));
            assert!(html.contains("Reinstall fan control"));
            assert!(html.contains("Reinstall Fan Control"));
            assert!(html.contains("Check for and immediately install an app update"));
            assert!(html.contains("cmd:fanhold:"));
            assert!(html.contains("cmd:fanauto:"));
            assert!(html.contains("savecurve:"));
        }
    }

    #[test]
    fn dashboard_html_has_simple_three_action_rail() {
        let en = dashboard_html(ResolvedLanguage::En, false);
        let ko = dashboard_html(ResolvedLanguage::Ko, false);

        for html in [&en, &ko] {
            assert!(html.contains(r#"class="dashboard-shell""#));
            assert!(html.contains(r#"class="action-rail""#));
            assert!(html.contains("railDetail"));
            assert!(html.contains("railFan"));
            assert!(html.contains("railSettings"));
            assert!(html.contains("railSystem"));
            assert!(!html.contains(r#"id="railUpdate""#));
            assert!(!html.contains(r#"id="railMore""#));
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
        assert!(en.contains(">Fans<"));
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
        }
    }

    #[test]
    fn menu_bar_uses_cpu_driven_runner_icon() {
        let _idle = menubar_runner_icon(8.0, 0);
        let _busy = make_runner_icon(RunnerCharacter::Cat, 92.0, 3);
        assert_ne!(
            make_runner_rgba(RunnerCharacter::Cat, 8.0, 0),
            make_runner_rgba(RunnerCharacter::Cat, 92.0, 0)
        );
        assert_ne!(
            make_runner_rgba(RunnerCharacter::Cat, 50.0, 0),
            make_runner_rgba(RunnerCharacter::Cat, 50.0, 1)
        );
    }

    #[test]
    fn runner_animation_respects_reduce_motion() {
        assert!(runner_should_animate(MenubarDisplay::Graph, false));
        assert!(runner_should_animate(MenubarDisplay::Both, false));
        assert!(!runner_should_animate(MenubarDisplay::Number, false));
        assert!(!runner_should_animate(MenubarDisplay::Graph, true));
        assert!(!runner_should_animate(MenubarDisplay::Both, true));
    }

    #[test]
    fn menu_bar_runner_is_cat_like_and_animated() {
        let idle = make_runner_rgba(RunnerCharacter::Cat, 12.0, 0);
        let active = make_runner_rgba(RunnerCharacter::Cat, 72.0, 1);

        assert!(idle.as_chunks::<4>().0.iter().any(|px| px[3] > 0));
        assert!(active.as_chunks::<4>().0.iter().any(|px| px[3] > 0));
        assert_ne!(idle, active);
    }

    #[test]
    fn runner_characters_have_distinct_animated_silhouettes() {
        let frames = RunnerCharacter::ALL.map(|character| make_runner_rgba(character, 50.0, 2));
        let unique = frames.iter().collect::<std::collections::HashSet<_>>();
        assert_eq!(unique.len(), RunnerCharacter::ALL.len());
        assert!(frames
            .iter()
            .all(|rgba| rgba.as_chunks::<4>().0.iter().any(|pixel| pixel[3] > 0)));
    }

    #[test]
    fn runner_gait_has_distinct_contact_and_flight_poses() {
        for character in RunnerCharacter::ALL {
            let frames = (0..RUNNER_FRAME_COUNT)
                .map(|frame| make_runner_rgba(character, 50.0, frame))
                .collect::<Vec<_>>();
            let unique = frames.iter().collect::<std::collections::HashSet<_>>();
            assert_eq!(
                unique.len(),
                usize::from(RUNNER_FRAME_COUNT),
                "{character:?} has duplicate poses"
            );
        }

        let frames = (0..RUNNER_FRAME_COUNT)
            .map(|frame| make_runner_rgba(RunnerCharacter::Cat, 50.0, frame))
            .collect::<Vec<_>>();
        let ground_pixels = frames
            .iter()
            .map(|rgba| {
                rgba.as_chunks::<4>()
                    .0
                    .iter()
                    .enumerate()
                    .filter(|(index, pixel)| {
                        index / RUNNER_PIXEL_WIDTH as usize >= (RUNNER_PIXEL_HEIGHT - 7) as usize
                            && pixel[3] > 96
                    })
                    .count()
            })
            .collect::<Vec<_>>();
        let least_contact = ground_pixels.iter().min().copied().unwrap_or_default();
        let most_contact = ground_pixels.iter().max().copied().unwrap_or_default();
        assert!(most_contact >= least_contact + 6);
    }

    #[test]
    #[ignore = "writes deterministic runner QA artifacts for visual inspection"]
    fn render_runner_sprite_sheet_for_visual_qa() {
        let artifacts = write_runner_quality_artifacts().expect("runner QA artifacts");
        assert!(artifacts.sheet_path.is_file());
        assert!(artifacts.report_path.is_file());
        assert!(
            std::fs::metadata(&artifacts.sheet_path)
                .expect("sheet metadata")
                .len()
                > 0
        );
        assert!(
            std::fs::metadata(&artifacts.report_path)
                .expect("report metadata")
                .len()
                > 0
        );

        let (width, height, pixels) = decode_png_rgba(&artifacts.sheet_path);
        assert_eq!((width, height), expected_runner_qa_sheet_dimensions());
        assert!(pixels.as_chunks::<4>().0.iter().any(|pixel| pixel[3] > 0));

        let report: serde_json::Value = serde_json::from_slice(
            &std::fs::read(&artifacts.report_path).expect("read quality report"),
        )
        .expect("parse quality report JSON");
        let characters = report["characters"].as_array().expect("character array");
        assert_eq!(characters.len(), RunnerCharacter::FEATURED.len());
        assert!(characters
            .iter()
            .zip(RunnerCharacter::FEATURED)
            .all(|(entry, character)| {
                entry["name"].as_str() == Some(character.as_str())
                    && entry["bands"]
                        .as_array()
                        .is_some_and(|bands| bands.len() == RUNNER_LOAD_BANDS)
            }));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn native_runner_image_rejects_malformed_buffers_before_allocating_pixels() {
        let expected = (RUNNER_PIXEL_WIDTH * RUNNER_PIXEL_HEIGHT * 4) as usize;
        for len in [0, expected - 1, expected + 1] {
            let result =
                make_runner_native_image_from_rgba(RunnerCharacter::Ghost, 7, &vec![255; len]);
            let message = result
                .expect_err("invalid RGBA length must fail")
                .to_string();
            assert!(message.contains("native cache frame 7"));
            assert!(message.contains(&format!("expected {expected} rgba bytes, got {len}")));
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn native_runner_image_preserves_eager_rgba_backing_and_template_geometry() {
        use objc2_app_kit::{NSBitmapFormat, NSBitmapImageRep};

        let mut rgba = vec![0; (RUNNER_PIXEL_WIDTH * RUNNER_PIXEL_HEIGHT * 4) as usize];
        rgba[..12].copy_from_slice(&[241, 37, 19, 127, 17, 229, 61, 255, 53, 73, 233, 0]);
        let image = make_runner_native_image_from_rgba(RunnerCharacter::Cat, 0, &rgba)
            .expect("native RGBA image");
        assert_eq!(image.size(), NSSize::new(27.0, 18.0));
        assert!(image.isTemplate());
        let representations = image.representations();
        assert_eq!(representations.len(), 1);
        let bitmap = representations
            .objectAtIndex(0)
            .downcast::<NSBitmapImageRep>()
            .expect("eager bitmap representation");
        assert_eq!(bitmap.pixelsWide(), RUNNER_PIXEL_WIDTH as isize);
        assert_eq!(bitmap.pixelsHigh(), RUNNER_PIXEL_HEIGHT as isize);
        assert_eq!(bitmap.size(), image.size());
        assert_eq!(bitmap.bitsPerSample(), 8);
        assert_eq!(bitmap.samplesPerPixel(), 4);
        assert_eq!(bitmap.bitsPerPixel(), 32);
        assert!(!bitmap.isPlanar());
        assert_eq!(bitmap.bitmapFormat(), NSBitmapFormat::AlphaNonpremultiplied);
        assert_eq!(bitmap.bytesPerRow(), (RUNNER_PIXEL_WIDTH * 4) as isize);
        let data = bitmap.bitmapData();
        assert!(!data.is_null());
        // The representation owns this contiguous RGBA allocation for its
        // lifetime, and the validated dimensions above bound the read.
        let actual = unsafe { std::slice::from_raw_parts(data, rgba.len()) };
        assert_eq!(actual, rgba);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn native_runner_timer_closure_only_indexes_prebuilt_native_images() {
        let body = extract_block_body(
            include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/main.rs")),
            "let block = RcBlock::new(move |timer: std::ptr::NonNull<NSTimer>| {",
        );

        for expected in [
            "ease_runner_motion_cpu(",
            "advance_runner_phase(",
            "runner_load_band_with_hysteresis(",
            "let index =",
            "images.get(index)",
            "painter.borrow().paint(image);",
            "runner_native_next_delay(",
        ] {
            assert!(
                body.contains(expected),
                "missing `{expected}` in timer body"
            );
        }

        for forbidden in [
            "RunnerPack::build",
            "render_raw_frame",
            "normalize_cycle",
            "png::Decoder",
            "png::Encoder",
            "make_runner_native_image_from_rgba",
            "runner_icon_from_rgba",
            "Vec::with_capacity",
            "vec![",
            ".to_vec()",
            ".collect::<Vec<_>>()",
        ] {
            assert!(
                !body.contains(forbidden),
                "timer body must stay lightweight: found `{forbidden}`"
            );
        }

        let sync_timer_fn = extract_block_body(
            include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/main.rs")),
            "fn sync_native_runner_timer(app: &mut App) {",
        );
        assert!(sync_timer_fn.contains("let images = app.runner_assets.native_images.clone();"));
        assert!(!sync_timer_fn.contains("render_raw_frame("));
        assert!(!sync_timer_fn.contains("RunnerPack::build("));
    }

    fn expected_runner_qa_sheet_dimensions() -> (u32, u32) {
        let cell_width = RUNNER_PIXEL_WIDTH * (RUNNER_QA_SCALE + 1) + RUNNER_QA_PADDING * 3;
        let cell_height = RUNNER_PIXEL_HEIGHT * RUNNER_QA_SCALE + RUNNER_QA_PADDING * 2;
        (
            cell_width * u32::from(RUNNER_FRAME_COUNT),
            cell_height * RunnerCharacter::FEATURED.len() as u32 * RUNNER_LOAD_BANDS as u32,
        )
    }

    fn decode_png_rgba(path: &std::path::Path) -> (u32, u32, Vec<u8>) {
        let decoder = png::Decoder::new(std::io::BufReader::new(
            std::fs::File::open(path).expect("open PNG for QA validation"),
        ));
        let mut reader = decoder.read_info().expect("read PNG info");
        let mut pixels = vec![0; reader.output_buffer_size().expect("PNG output buffer size")];
        let info = reader.next_frame(&mut pixels).expect("decode QA PNG");
        (
            info.width,
            info.height,
            pixels[..info.buffer_size()].to_vec(),
        )
    }

    #[cfg(target_os = "macos")]
    fn extract_block_body(source: &str, marker: &str) -> String {
        let start = source.find(marker).expect("marker must exist");
        let body_start = start + marker.len();
        let mut depth = 1usize;
        let mut body_end = body_start;
        for (offset, ch) in source[body_start..].char_indices() {
            match ch {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        body_end = body_start + offset;
                        break;
                    }
                }
                _ => {}
            }
        }
        source[body_start..body_end].to_string()
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
        assert!((167..=169).contains(&idle.as_millis()));
        assert!((80..=82).contains(&normal.as_millis()));
        assert!((131..=133).contains(&runner_frame_interval(20.0).as_millis()));
        assert!((20..=22).contains(&busy.as_millis()));
        assert!(idle.as_millis() >= busy.as_millis() * 5);

        assert!(!runner_enabled(MenubarDisplay::Number));
        assert!(runner_enabled(MenubarDisplay::Graph));
        assert!(runner_enabled(MenubarDisplay::Both));
        assert_eq!(
            make_runner_icons(RunnerCharacter::Cat).len(),
            usize::from(RUNNER_FRAME_COUNT) * 4
        );
        assert_eq!(runner_load_band(10.0), 0);
        assert_eq!(runner_load_band(90.0), 3);
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn native_runner_tick_never_outpaces_the_fastest_visible_frame() {
        assert_eq!(RUNNER_NATIVE_TICK, RUNNER_MIN_INTERVAL);
        assert!(RUNNER_NATIVE_TICK <= runner_frame_interval(100.0));
        assert!(RUNNER_NATIVE_TOLERANCE < RUNNER_NATIVE_TICK);
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn native_runner_sleeps_between_idle_frames_but_wakes_for_speed_changes() {
        let settled_idle = runner_native_next_delay(RunnerCharacter::Cat, 0, 5.0, 5.0, 0);
        let settled_busy = runner_native_next_delay(RunnerCharacter::Cat, 0, 100.0, 100.0, 0);
        let accelerating = runner_native_next_delay(RunnerCharacter::Cat, 0, 5.0, 90.0, 0);

        assert!(settled_idle > RUNNER_NATIVE_TICK * 5);
        assert_eq!(settled_busy, RUNNER_NATIVE_TICK);
        assert_eq!(accelerating, RUNNER_NATIVE_TICK);

        let interval_us =
            runner_frame_interval_for_pose(RunnerCharacter::Cat, 5.0, 0).as_micros() as u64;
        assert_eq!(
            runner_native_next_delay(RunnerCharacter::Cat, interval_us - 1, 5.0, 5.0, 0),
            Duration::from_millis(1)
        );
    }

    #[test]
    fn runner_load_bands_do_not_flicker_near_boundaries() {
        assert_eq!(runner_load_band_with_hysteresis(21.0, 0), 0);
        assert_eq!(runner_load_band_with_hysteresis(24.0, 0), 1);
        assert_eq!(runner_load_band_with_hysteresis(19.0, 1), 1);
        assert_eq!(runner_load_band_with_hysteresis(15.0, 1), 0);
        assert_eq!(runner_load_band_with_hysteresis(56.0, 1), 1);
        assert_eq!(runner_load_band_with_hysteresis(59.0, 1), 2);
        assert_eq!(runner_load_band_with_hysteresis(79.0, 3), 3);
        assert_eq!(runner_load_band_with_hysteresis(75.0, 3), 2);

        // Large jumps pass through adjacent gait bands instead of replacing
        // the silhouette with a distant pose in a single frame.
        assert_eq!(runner_load_band_with_hysteresis(95.0, 0), 1);
        assert_eq!(runner_load_band_with_hysteresis(95.0, 1), 2);
        assert_eq!(runner_load_band_with_hysteresis(95.0, 2), 3);
    }

    #[test]
    fn browser_preview_band_lifecycle_matches_native_seed_and_tick_sequence() {
        let mut band = runner_load_band(65.0);
        assert_eq!(
            band, 2,
            "a canonical pack starts from direct load_band thresholds"
        );

        for (cpu, expected) in [
            (60.0, 2), // ordinary update retains band 2 instead of reseeding to 1
            (50.0, 1), // timer tick crosses 2 -> 1 hysteresis threshold
            (52.0, 1), // timer tick remains in band 1 inside hysteresis
            (59.0, 2), // timer tick crosses 1 -> 2 hysteresis threshold
            (83.0, 2), // timer tick remains in band 2 inside hysteresis
            (84.0, 3), // timer tick crosses 2 -> 3 hysteresis threshold
            (76.0, 3), // timer tick remains in band 3 at its inclusive lower edge
            (75.0, 2), // timer tick crosses 3 -> 2 hysteresis threshold
        ] {
            band = runner_load_band_with_hysteresis(cpu, band);
            assert_eq!(band, expected, "cpu {cpu}");
        }

        let html = dashboard_html(ResolvedLanguage::En, false);
        assert!(
            html.contains("RUNNER_PREVIEW_STATE.band=runnerPreviewBand(RUNNER_PREVIEW_STATE.cpu);")
        );
        assert!(html.contains("state.band=runnerPreviewBandWithHysteresis(state.cpu,state.band);"));
        assert!(!html.contains("RUNNER_PREVIEW_STATE.band=0;"));
    }

    #[test]
    fn browser_preview_first_real_payload_seeds_current_cpu_without_resetting_later_updates() {
        let first_real_band = runner_load_band(90.0);
        assert_eq!(first_real_band, 3, "the first real 90% sample seeds band 3");
        let ordinary_update_band = first_real_band;
        assert_eq!(
            ordinary_update_band, 3,
            "a later 60% payload updates the target only; the timer owns hysteresis"
        );

        let html = dashboard_html(ResolvedLanguage::En, false);
        assert!(html.contains("hasRealCpuSample:false"));
        assert!(html.contains(
            "if(!state.hasRealCpuSample){\n    state.hasRealCpuSample=true;\n    state.band=runnerPreviewBand(state.cpu);\n    paintRunnerPreview(state.canvas,state.character,state.frame,state.band);\n  }"
        ));
        assert!(html.contains("function runnerSelectedPreviewMatchesCurrent(payload,character)"));
        assert!(html.contains(
            "var cpu=Math.max(0,Math.min(100,Number(d.runner_cpu_pct)||0));\n  var canonicalPackChanged=runnerSelectedPreviewMatchesCharacter(d,character)"
        ));
        assert!(html.contains(
            "if(canonicalPackChanged||!RUNNER_PREVIEW_STATE.hasRealCpuSample){\n    RUNNER_PREVIEW_STATE.hasRealCpuSample=true;\n    seedRunnerPreview(cpu);\n  } else {\n    setRunnerPreviewCpu(cpu);\n  }"
        ));
        assert!(html.contains("startRunnerPreview(characterPreview,character,cpu);"));
        assert!(!html.contains("startRunnerPreview(characterPreview,character,0);"));
    }

    #[test]
    fn non_macos_runner_selection_keeps_a_hysteretic_band_state() {
        let mut band = 0;
        assert_eq!(runner_icon_index_with_hysteresis(23.0, 4, &mut band), 4);
        assert_eq!(band, 0);
        assert_eq!(runner_icon_index_with_hysteresis(24.0, 4, &mut band), 12);
        assert_eq!(band, 1);
        assert_eq!(runner_icon_index_with_hysteresis(19.0, 4, &mut band), 12);
        assert_eq!(band, 1);
        assert_eq!(runner_icon_index_with_hysteresis(15.0, 4, &mut band), 4);
        assert_eq!(band, 0);
    }

    #[test]
    fn runner_phase_keeps_remainder_when_speed_changes() {
        let (phase, advanced) = runner_phase_step(10_000, 15_000, 100.0, 0);
        assert!(advanced);
        assert_eq!(phase, 5_000);
        let (phase, advanced) = runner_phase_step(phase, 10_000, 100.0, 1);
        assert!(!advanced);
        let (phase, advanced) = runner_phase_step(phase, 10_000, 100.0, 1);
        assert!(advanced);
        assert_eq!(phase, 5_000);
    }

    #[test]
    fn every_runner_lap_uses_one_common_cpu_clock() {
        for cpu in [5.0, 45.0, 95.0] {
            for character in RunnerCharacter::FEATURED {
                let lap: Duration = (0..RUNNER_FRAME_COUNT)
                    .map(|pose| runner_frame_interval_for_pose(character, cpu, pose))
                    .sum();
                assert_eq!(
                    lap,
                    runner_frame_interval(cpu) * u32::from(RUNNER_FRAME_COUNT),
                    "{character:?} at {cpu}% must not have a separate lap speed"
                );
            }
        }
    }

    #[test]
    fn runner_motion_speed_eases_without_overshooting() {
        let tick = Duration::from_millis(30);
        let accelerated = ease_runner_motion_cpu(10.0, 90.0, tick);
        let decelerated = ease_runner_motion_cpu(90.0, 10.0, tick);

        assert!(accelerated > 10.0 && accelerated < 90.0);
        assert!(decelerated > 10.0 && decelerated < 90.0);
        assert!(accelerated - 10.0 > 90.0 - decelerated);
        assert_eq!(ease_runner_motion_cpu(42.04, 42.0, tick), 42.0);
        assert!((0.0..=100.0).contains(&ease_runner_motion_cpu(-20.0, 140.0, tick)));

        let mut value = 0.0;
        for _ in 0..20 {
            let next = ease_runner_motion_cpu(value, 80.0, tick);
            assert!(next >= value);
            value = next;
        }
        assert!(value > 79.0 && value <= 80.0);
    }

    #[test]
    fn runner_reacts_quickly_to_spikes_and_decays_smoothly() {
        assert_eq!(smooth_runner_cpu(0.0, 67.0, false), 67.0);
        let spike = smooth_runner_cpu(20.0, 90.0, true);
        let decay = smooth_runner_cpu(90.0, 20.0, true);
        assert!(spike > 75.0);
        assert!(decay > 70.0);
        assert!(smooth_runner_cpu(40.0, 140.0, true) <= 100.0);
    }

    #[test]
    fn runner_silhouettes_are_wide_and_fill_the_menu_bar_slot() {
        for character in RunnerCharacter::ALL {
            let rgba = make_runner_rgba(character, 55.0, 0);
            let mut min_x = RUNNER_PIXEL_WIDTH;
            let mut max_x = 0;
            let mut min_y = RUNNER_PIXEL_HEIGHT;
            let mut max_y = 0;

            for (index, pixel) in rgba.as_chunks::<4>().0.iter().enumerate() {
                if pixel[3] <= 96 {
                    continue;
                }
                let x = index as u32 % RUNNER_PIXEL_WIDTH;
                let y = index as u32 / RUNNER_PIXEL_WIDTH;
                min_x = min_x.min(x);
                max_x = max_x.max(x);
                min_y = min_y.min(y);
                max_y = max_y.max(y);
            }

            let width = max_x.saturating_sub(min_x) + 1;
            let height = max_y.saturating_sub(min_y) + 1;
            if character == RunnerCharacter::Human {
                // The CC0 source is a deliberately slim human silhouette;
                // preserve its anatomy instead of widening it into a blob.
                assert!(width >= 12, "{character:?} is too narrow: {width}px");
                assert!(height >= 24, "{character:?} is too short: {height}px");
            } else if character == RunnerCharacter::Pushup {
                // The top pose is a shallow plank, not a steep incline. Its
                // contacts, rigid spine, fixed arm lengths and chest stroke
                // are independently checked by the procedural pose tests.
                assert!(width >= 30, "push-up is too narrow: {width}px");
                assert!(
                    (16..=20).contains(&height),
                    "push-up top pose lost its shallow proportions: {height}px"
                );
            } else if matches!(character, RunnerCharacter::Penguin | RunnerCharacter::Robot) {
                // Upright anatomy stays legible without decorative horizontal
                // speed lines that previously inflated this width check.
                assert!(width >= 14, "{character:?} is too narrow: {width}px");
                assert!(height >= 24, "{character:?} is too short: {height}px");
            } else if matches!(
                character,
                RunnerCharacter::Cat
                    | RunnerCharacter::Dog
                    | RunnerCharacter::Rabbit
                    | RunnerCharacter::Fox
            ) {
                // Tucked legs shorten an individual pose; the cycle's full
                // silhouette is independently checked against its manifest.
                assert!(width >= 30, "{character:?} is too narrow: {width}px");
                assert!(height >= 16, "{character:?} is too short: {height}px");
            } else if matches!(
                character,
                RunnerCharacter::Dinosaur | RunnerCharacter::Ghost
            ) {
                // The asset is a deliberately tall side-view T-rex: its
                // head and short arms make a 1.40 horizontal ratio an
                // incorrect quality gate even though the running direction
                // is unambiguous. Ghosts also retain a narrow floating cloak
                // silhouette so their black/white contour does not get
                // artificially stretched just to satisfy this test.
                assert!(width >= 30, "{character:?} is too narrow: {width}px");
                assert!(width as f32 / height as f32 >= 1.0);
            } else {
                // A dancer is upright, the skyline is height-limited, and a
                // slime alternates a narrow tall dome with a broad compression.
                // Their entire cycles still obey the unchanged manifest gates.
                let (min_width, min_height) = match character {
                    RunnerCharacter::Slime => (18, 10),
                    RunnerCharacter::Party => (18, 24),
                    RunnerCharacter::City => (30, 20),
                    RunnerCharacter::Bird | RunnerCharacter::Fish | RunnerCharacter::Orbit => {
                        (34, 14)
                    }
                    RunnerCharacter::Sausage => (36, 16),
                    _ => (30, 16),
                };
                assert!(
                    width >= min_width && height >= min_height,
                    "{character:?} does not fill its optical slot: {width}x{height}"
                );
            }
        }
    }

    #[test]
    fn human_runner_has_eight_distinct_run_frames() {
        let frames: Vec<Vec<u8>> = (0..RUNNER_FRAME_COUNT)
            .map(|frame| make_runner_rgba(RunnerCharacter::Human, 50.0, frame))
            .collect();
        let unique: std::collections::HashSet<Vec<u8>> = frames.into_iter().collect();
        assert_eq!(unique.len(), usize::from(RUNNER_FRAME_COUNT));
    }

    #[test]
    fn all_runners_use_a_white_glyph_and_compact_picker_preview() {
        for character in RunnerCharacter::ALL {
            for frame in [0, 3, 7] {
                let rgba = make_runner_rgba(character, 55.0, frame);
                for pixel in rgba.as_chunks::<4>().0.iter().filter(|pixel| pixel[3] != 0) {
                    assert_eq!(&pixel[..3], &[255, 255, 255], "{character:?} is not white");
                }
            }
        }

        let html = dashboard_html(ResolvedLanguage::En, false);
        assert!(html.contains(".runner-preview{display:block;width:27px;height:18px"));
        assert!(html.contains(".character-select .runner-preview{width:27px;height:18px"));
        assert!(html.contains(
            ".character-menu{display:none;position:fixed;z-index:1000;width:224px;max-height:260px"
        ));
        assert!(html.contains(".character-option{display:grid;grid-template-columns:38px minmax(0,1fr) 14px;align-items:center;gap:7px;width:100%;height:34px"));
        assert!(html.contains("image-rendering:auto"));
        assert!(html
            .contains("image.data[offset]=255;image.data[offset+1]=255;image.data[offset+2]=255;"));
        assert!(!html.contains("__RUNNER_COLORS__"));
    }

    #[test]
    fn runner_frames_keep_a_stable_horizontal_center() {
        for character in RunnerCharacter::ALL {
            let centers: Vec<f32> = (0..RUNNER_FRAME_COUNT)
                .map(|frame| {
                    let rgba = make_runner_rgba(character, 55.0, frame);
                    let (weighted_x, alpha_sum) = rgba.as_chunks::<4>().0.iter().enumerate().fold(
                        (0.0_f32, 0.0_f32),
                        |(weighted_x, alpha_sum), (index, pixel)| {
                            let alpha = f32::from(pixel[3]);
                            (
                                weighted_x + (index as u32 % RUNNER_PIXEL_WIDTH) as f32 * alpha,
                                alpha_sum + alpha,
                            )
                        },
                    );
                    weighted_x / alpha_sum.max(1.0)
                })
                .collect();
            let min = centers.iter().copied().fold(f32::INFINITY, f32::min);
            let max = centers.iter().copied().fold(f32::NEG_INFINITY, f32::max);
            assert!(
                max - min <= 4.5,
                "{character:?} shifts sideways by {:.2}px across its gait",
                max - min
            );
        }
    }

    #[test]
    fn high_cpu_runner_uses_a_larger_motion_envelope() {
        let changed_pixels = |cpu_pct: f32| {
            let extended = make_runner_rgba(RunnerCharacter::Fox, cpu_pct, 0);
            let airborne = make_runner_rgba(RunnerCharacter::Fox, cpu_pct, 3);
            extended
                .as_chunks::<4>()
                .0
                .iter()
                .zip(airborne.as_chunks::<4>().0.iter())
                .filter(|(a, b)| a[3].abs_diff(b[3]) > 48)
                .count()
        };

        let idle_motion = changed_pixels(5.0);
        let sprint_motion = changed_pixels(95.0);
        assert!(
            sprint_motion > idle_motion,
            "high load should enlarge the gait: idle={idle_motion}, sprint={sprint_motion}"
        );
        assert!(
            // Stable head/ribcage volume must not needlessly morph merely to
            // raise the whole-frame pixel difference. Limbs supply the motion.
            sprint_motion >= 120,
            "sprint key poses are too similar: {sprint_motion} changed pixels"
        );
    }

    #[test]
    fn settings_expose_cpu_runner_style_and_live_pace() {
        let en = dashboard_html(ResolvedLanguage::En, false);
        let ko = dashboard_html(ResolvedLanguage::Ko, false);

        for html in [&en, &ko] {
            assert!(html.contains(r#"id="menubar-display-setting""#));
            assert!(html.contains(r#"id="display-number""#));
            assert!(html.contains(r#"id="display-runner""#));
            assert!(html.contains(r#"id="display-both""#));
            assert!(html.contains(r#"id="runner-character-setting""#));
            assert!(html.contains(r#"id="runner-character-select""#));
            assert!(html.contains(r#"id="runner-character-menu""#));
            assert!(html.contains(r#"role="listbox""#));
            assert!(html.contains("function handleRunnerTriggerKey(event)"));
            assert!(html.contains("function handleRunnerOptionKey(event)"));
            for character in RunnerCharacter::FEATURED {
                assert!(
                    html.contains(&format!(r#"data-character="{}""#, character.as_str())),
                    "settings are missing {character:?}"
                );
                assert!(
                    html.contains(&format!(r#"data-runner-preview="{}""#, character.as_str())),
                    "settings preview is missing {character:?}"
                );
            }
            assert!(html.contains("RUNNER_CHARACTERS.indexOf(character)<0"));
            assert!(html.contains("function setRunnerCharacter(character)"));
            assert!(html.contains("window.ipc.postMessage('character:'+character)"));
            assert!(html.contains("function setMenubarDisplay(style)"));
            assert!(html.contains("window.ipc.postMessage('display:'+style)"));
            assert!(html.contains("function updateMenubarDisplay(d)"));
            assert!(!html.contains("__RUNNER_OPTIONS__"));
            assert!(!html.contains("__RUNNER_CHARACTERS__"));
            assert!(!html.contains("__RUNNER_PREVIEWS__"));
        }
        for label in [
            "고양이",
            "강아지",
            "토끼",
            "여우",
            "펭귄",
            "공룡",
            "로봇",
            "유령",
            "달리는 사람",
            "푸시업",
        ] {
            assert!(ko.contains(label), "Korean settings are missing {label}");
        }
        for character in RunnerCharacter::FEATURED {
            assert!(
                ko.contains(&format!(r#"data-character="{}""#, character.as_str())),
                "authored runner missing from picker: {}",
                character.as_str()
            );
            assert!(ko.contains(runner_character_label(ResolvedLanguage::Ko, character)));
        }
    }

    #[test]
    fn cpu_widget_settings_are_present_and_resolved_in_both_localizations() {
        for language in [ResolvedLanguage::En, ResolvedLanguage::Ko] {
            let html = dashboard_html(language, false);
            assert!(html.contains("id=\"cpu-widget-setting\""));
            assert!(html.contains("function renderCpuWidget(d)"));
            assert!(html.contains("function setCpuWidget(field,value)"));
            assert!(!html.contains("__CPU_WIDGET_"));
            assert!(html.contains("renderCpuWidget(d);"));
        }
    }

    #[test]
    #[ignore = "writes isolated CPU-widget settings previews; no installed app operations"]
    fn render_cpu_widget_settings_for_visual_qa() {
        let directory = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join("target/cpu-per-core-qa/exported");
        std::fs::create_dir_all(&directory).unwrap();
        for language in [ResolvedLanguage::En, ResolvedLanguage::Ko] {
            let lang = if language == ResolvedLanguage::Ko {
                "ko"
            } else {
                "en"
            };
            let html = dashboard_html(language, false);
            let bootstrap = r#"<script>
window.__pf_pending={menubar_display:'both',runner_character:'cat',runner_random:false,runner_cpu_pct:45,runner_interval_ms:81,runner_reduce_motion:true,cpu_pct:45,menu_items_supported:true,menu_items:{cpu:true,runner:true,metric:true},cpu_widget_supported:true,cpu_widget:{enabled:true,style:'cores',show_percentage:false},cpu_widget_cores:[0,15,30,50,75,100,25,65,40,85],cpu_widget_history:Array.from({length:60},function(_,i){return Math.max(0,Math.min(100,45+35*Math.sin(i*.3)));}),settings_results:[]};
window.ipc={postMessage:function(message){document.body.dataset.lastCommand=message;if(message.indexOf('settingsrequest:')===0){var r=JSON.parse(message.slice(16));setTimeout(function(){var d=window.__pf_pending;if(window.__pf_fail_next){window.__pf_fail_next=false;d.settings_results=[{id:r.id,operation:r.operation,ok:false,message:'Fixture save failed. Please retry.'}];}else{var value=JSON.parse(r.payload);if(r.operation==='menu_items'){d.menu_items=value;d.cpu_widget.enabled=value.cpu;}else if(r.operation==='cpu_widget'){value.enabled=d.cpu_widget.enabled;d.cpu_widget=value;}d.settings_results=[{id:r.id,operation:r.operation,ok:true}];}renderMenuItems(d);renderCpuWidget(d);},80);}}};
document.body.classList.add('data-ready');var loading=document.getElementById('data-loading');if(loading)loading.hidden=true;
setRailView('settings');updateMenubarDisplay(window.__pf_pending);renderMenuItems(window.__pf_pending);renderCpuWidget(window.__pf_pending);
</script>"#;
            std::fs::write(
                directory.join(format!("settings-{lang}.html")),
                html.replace("</body>", &format!("{bootstrap}</body>")),
            )
            .unwrap();
        }
    }

    #[test]
    fn hidden_primary_disables_runner_animation_in_all_display_modes() {
        for display in [
            MenubarDisplay::Graph,
            MenubarDisplay::Both,
            MenubarDisplay::Number,
        ] {
            assert!(!runner_visibility_should_animate(false, display, false));
            assert!(!runner_visibility_should_animate(false, display, true));
        }
        assert!(runner_visibility_should_animate(
            true,
            MenubarDisplay::Graph,
            false
        ));
        assert!(!runner_visibility_should_animate(
            true,
            MenubarDisplay::Both,
            true
        ));
    }

    #[test]
    fn cpu_widget_cross_item_click_reanchors_and_same_item_toggles_closed() {
        assert!(!tray_press_closes_popover(
            Some("runner"),
            "cpu",
            true,
            false
        ));
        assert!(!tray_press_closes_popover(
            Some("cpu"),
            "runner",
            true,
            false
        ));
        assert!(tray_press_closes_popover(Some("cpu"), "cpu", true, false));
        assert!(tray_press_closes_popover(
            Some("runner"),
            "runner",
            false,
            true
        ));
        assert!(!tray_press_closes_popover(
            Some("runner"),
            "cpu",
            false,
            true
        ));
        assert!(!tray_press_closes_popover(None, "cpu", false, false));
    }

    #[test]
    #[ignore = "writes the real settings picker for local browser verification"]
    fn render_random_runner_settings_for_visual_qa() {
        let html = dashboard_html(ResolvedLanguage::Ko, false);
        let bootstrap = r#"<script>
window.ipc={postMessage:function(message){document.body.dataset.lastCommand=message;}};
window.__pf_pending={menubar_display:'both',runner_character:'pushup',runner_random:false,runner_cpu_pct:45,runner_interval_ms:81};
setRailView('settings');updateMenubarDisplay(window.__pf_pending);
</script>"#;
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join("target");
        std::fs::write(
            root.join("random-runner-settings-qa.html"),
            html.replace("</body>", &format!("{bootstrap}</body>")),
        )
        .unwrap();
    }

    #[test]
    fn runner_picker_previews_every_character_with_real_alpha_data() {
        let previews: serde_json::Value =
            serde_json::from_str(runner_preview_alpha_json()).expect("valid preview JSON");
        let previews = previews.as_object().expect("preview object");
        assert_eq!(previews.len(), RunnerCharacter::FEATURED.len());

        let mut unique = std::collections::HashSet::new();
        for character in RunnerCharacter::FEATURED {
            let alpha = previews
                .get(character.as_str())
                .and_then(serde_json::Value::as_str)
                .expect("character preview");
            assert_eq!(
                alpha.len(),
                (RUNNER_PIXEL_WIDTH * RUNNER_PIXEL_HEIGHT * 2) as usize
            );
            assert!(alpha.bytes().any(|byte| byte != b'0'));
            unique.insert(alpha);
        }
        assert_eq!(unique.len(), RunnerCharacter::FEATURED.len());

        let frames: serde_json::Value =
            serde_json::from_str(runner_preview_frames_json()).expect("valid preview frames JSON");
        let frames = frames.as_object().expect("preview frames object");
        assert_eq!(frames.len(), RunnerCharacter::FEATURED.len());
        for character in RunnerCharacter::FEATURED {
            let character_frames = frames
                .get(character.as_str())
                .and_then(serde_json::Value::as_array)
                .expect("character preview frames");
            assert_eq!(character_frames.len(), usize::from(RUNNER_FRAME_COUNT));
            assert!(character_frames.iter().all(|frame| {
                frame.as_str().is_some_and(|alpha| {
                    alpha.len() == (RUNNER_PIXEL_WIDTH * RUNNER_PIXEL_HEIGHT * 2) as usize
                })
            }));
        }

        let html = dashboard_html(ResolvedLanguage::En, false);
        assert!(html.contains(&format!(
            "var RUNNER_CHARACTERS={};",
            runner_character_values_json()
        )));
        assert!(html.contains("var RUNNER_PREVIEW_ALPHA={"));
        assert!(html.contains("var RUNNER_PREVIEW_FRAMES={"));
        assert!(html.contains("paintRunnerPreviews(document);"));
        assert!(html.contains("paintRunnerPreview(canvas,canvas.dataset.runnerPreview,1,0);"));
        assert!(html.contains("startRunnerPreview(characterPreview,character,cpu);"));
        assert!(!html.contains("startRunnerPreview(characterPreview,character,0);"));
        assert!(html.contains("function runnerPreviewInterval(cpu,frame,character)"));
        assert!(html.contains("function setRunnerPreviewCpu(cpu)"));
        assert!(html.contains("function setRunnerPreviewVisibility(visible)"));
        assert!(html.contains("document.addEventListener('visibilitychange'"));
        assert!(html.contains("prefers-reduced-motion: reduce"));
        assert!(html.contains(r#".character-menu{display:none;position:fixed;"#));
    }

    #[test]
    fn runner_preview_json_matches_shared_runner_pack_alpha() {
        let packs = RunnerCharacter::FEATURED
            .into_iter()
            .map(|character| {
                (
                    character,
                    runner::RunnerPack::build(character).expect("runner pack"),
                )
            })
            .collect::<Vec<_>>();

        let previews: serde_json::Value =
            serde_json::from_str(&runner_preview_alpha_json_for(&packs))
                .expect("valid preview JSON");
        let previews = previews.as_object().expect("preview object");
        let frames: serde_json::Value =
            serde_json::from_str(&runner_preview_frames_json_for(&packs))
                .expect("valid preview frames JSON");
        let frames = frames.as_object().expect("preview frames object");

        for (character, pack) in &packs {
            assert_eq!(
                previews
                    .get(character.as_str())
                    .and_then(serde_json::Value::as_str),
                pack.preview_alpha_hex(1, 1).as_deref()
            );

            let expected = (0..RUNNER_FRAME_COUNT)
                .map(|frame| pack.preview_alpha_hex(1, frame).expect("preview alpha"))
                .collect::<Vec<_>>();
            let actual = frames
                .get(character.as_str())
                .and_then(serde_json::Value::as_array)
                .expect("character preview frames")
                .iter()
                .map(|frame| frame.as_str().expect("frame alpha").to_string())
                .collect::<Vec<_>>();
            assert_eq!(actual, expected);
        }
    }

    #[test]
    fn selected_preview_serializes_the_complete_pack_and_uses_native_hysteresis() {
        let pack = RunnerPack::build(RunnerCharacter::Fox).expect("runner pack");
        let frames: serde_json::Value =
            serde_json::from_str(&runner_selected_preview_frames_json(&pack))
                .expect("selected preview JSON");
        let frames = frames.as_array().expect("selected preview frame array");
        assert_eq!(
            frames.len(),
            usize::from(RUNNER_FRAME_COUNT) * RUNNER_LOAD_BANDS
        );

        for band in 0..RUNNER_LOAD_BANDS {
            for pose in 0..RUNNER_FRAME_COUNT {
                let index = band * usize::from(RUNNER_FRAME_COUNT) + usize::from(pose);
                assert_eq!(
                    frames[index].as_str(),
                    pack.preview_alpha_hex(band, pose).as_deref(),
                    "band {band}, pose {pose} must use the selected canonical pack"
                );
            }
        }

        let html = dashboard_html_with_runner_pack(ResolvedLanguage::En, false, &pack);
        assert!(html.contains("var RUNNER_SELECTED_PREVIEW=["));
        assert!(html.contains("function runnerPreviewBand(cpu)"));
        assert!(html.contains("function runnerSelectedPreviewMatchesCharacter(payload,character)"));
        assert!(html.contains("payload.runner_selected_preview_character===character"));
        assert!(html.contains("RUNNER_SELECTED_PREVIEW_CHARACTER=character"));
        assert!(
            html.contains("RUNNER_PREVIEW_STATE.band=runnerPreviewBand(RUNNER_PREVIEW_STATE.cpu);")
        );
        assert!(html.contains(
            "state.band=runnerPreviewBand(state.cpu);\n  paintRunnerPreview(state.canvas,state.character,state.frame,state.band);"
        ));
        assert!(
            html.contains("var canonical=selected&&RUNNER_SELECTED_PREVIEW_CHARACTER===character;")
        );
        assert!(html.contains(
            "var frames=canonical?RUNNER_SELECTED_PREVIEW:RUNNER_PREVIEW_FRAMES[character];"
        ));
        assert!(html.contains(
            "var index=canonical?(Math.max(0,Math.min(3,Number(band)||0))*8+pose):pose;"
        ));
        assert!(html.contains("function runnerPreviewBandWithHysteresis(cpu,current)"));
        for threshold in [
            "current===0&&load>=24",
            "current===1&&load<16",
            "current===1&&load>=59",
            "current===2&&load<51",
            "current===2&&load>=84",
            "current===3&&load<76",
        ] {
            assert!(
                html.contains(threshold),
                "missing hysteresis threshold {threshold}"
            );
        }
    }

    #[test]
    fn selected_preview_payload_has_matching_character_provenance() {
        let pack = RunnerPack::build(RunnerCharacter::Fox).expect("runner pack");
        let payload = serde_json::json!({
            "runner_character": RunnerCharacter::Fox.as_str(),
            "runner_selected_preview": serde_json::from_str::<serde_json::Value>(
                &runner_selected_preview_frames_json(&pack)
            ).expect("selected preview JSON"),
            "runner_selected_preview_character": RunnerCharacter::Fox.as_str(),
        });

        assert_eq!(
            payload["runner_selected_preview_character"].as_str(),
            payload["runner_character"].as_str(),
            "the canonical payload must identify the character it renders"
        );
    }

    #[test]
    fn preview_omits_failed_characters_after_one_failed_cat_fallback_attempt() {
        let attempts = std::cell::RefCell::new(Vec::new());
        let packs = runner_preview_packs_with(|character| {
            attempts.borrow_mut().push(character);
            if matches!(character, RunnerCharacter::Fox | RunnerCharacter::Cat) {
                Err(RunnerBuildError::InvalidFrame {
                    character,
                    detail: "forced preview failure".to_string(),
                })
            } else {
                RunnerPack::build(character)
            }
        });

        assert_eq!(packs.len(), RunnerCharacter::FEATURED.len() - 2);
        assert!(packs.iter().all(|(character, _)| {
            !matches!(character, RunnerCharacter::Fox | RunnerCharacter::Cat)
        }));
        assert_eq!(
            attempts
                .borrow()
                .iter()
                .filter(|&&character| character == RunnerCharacter::Fox)
                .count(),
            1
        );
        assert_eq!(
            attempts
                .borrow()
                .iter()
                .filter(|&&character| character == RunnerCharacter::Cat)
                .count(),
            1,
            "one prewarm must memoize a failed Cat fallback instead of retrying it"
        );
    }

    #[test]
    fn preview_reuses_one_successful_cat_fallback_with_requested_character_keys() {
        let attempts = std::cell::RefCell::new(Vec::new());
        let packs = runner_preview_packs_with(|character| {
            attempts.borrow_mut().push(character);
            if character == RunnerCharacter::Cat {
                RunnerPack::build(character)
            } else {
                Err(RunnerBuildError::InvalidFrame {
                    character,
                    detail: "forced preview failure".to_string(),
                })
            }
        });

        assert_eq!(packs.len(), RunnerCharacter::FEATURED.len());
        assert!(packs
            .iter()
            .all(|(_, pack)| pack.character() == RunnerCharacter::Cat));
        assert_eq!(
            packs
                .iter()
                .map(|(character, _)| *character)
                .collect::<Vec<_>>(),
            RunnerCharacter::FEATURED.to_vec(),
            "fallback previews retain their requested payload keys"
        );
        assert_eq!(
            attempts
                .borrow()
                .iter()
                .filter(|&&character| character == RunnerCharacter::Cat)
                .count(),
            1,
            "one prewarm must build Cat once and reuse it for every failed character"
        );
    }

    #[test]
    fn runner_assets_keep_the_selected_pack_for_preview_payloads() {
        let assets = build_runner_assets(RunnerCharacter::Dog).expect("dog assets");
        let frames: serde_json::Value =
            serde_json::from_str(&assets.selected_preview_frames_json()).expect("preview JSON");
        let frames = frames.as_array().expect("selected frame array");
        assert_eq!(
            frames.len(),
            usize::from(RUNNER_FRAME_COUNT) * RUNNER_LOAD_BANDS
        );
        assert_eq!(
            frames[23].as_str(),
            assets.pack.preview_alpha_hex(2, 7).as_deref(),
            "the selected Settings payload must come from the retained pack"
        );
        assert_eq!(
            assets.icons.len(),
            assets.pack.frames().len(),
            "native icon cache must be built from each retained pack frame"
        );
        assert_eq!(assets.native_images.len(), assets.pack.frames().len());
    }

    #[test]
    fn runner_character_switch_keeps_previous_assets_on_build_failure() {
        let mut character = RunnerCharacter::Cat;
        let mut assets = build_runner_assets(RunnerCharacter::Cat).expect("cat assets");
        let original_preview = assets
            .pack
            .preview_alpha_hex(1, 1)
            .expect("original preview alpha");

        let error = replace_runner_assets_with(
            &mut character,
            &mut assets,
            RunnerCharacter::Fox,
            |requested| {
                if requested == RunnerCharacter::Fox {
                    Err(runner::RunnerBuildError::InvalidFrame {
                        character: requested,
                        detail: "forced failure".to_string(),
                    })
                } else {
                    build_runner_assets(requested)
                }
            },
        )
        .expect_err("switch should fail");

        assert_eq!(character, RunnerCharacter::Cat);
        assert_eq!(assets.pack.character(), RunnerCharacter::Cat);
        assert_eq!(
            assets.pack.preview_alpha_hex(1, 1).as_deref(),
            Some(original_preview.as_str())
        );
        assert_eq!(
            assets.icons.len(),
            usize::from(RUNNER_FRAME_COUNT) * RUNNER_LOAD_BANDS
        );
        assert!(error.to_string().contains("forced failure"));
    }

    #[test]
    fn runner_character_switch_keeps_previous_assets_on_icon_cache_failure() {
        let mut character = RunnerCharacter::Cat;
        let mut assets = build_runner_assets(RunnerCharacter::Cat).expect("cat assets");
        let original_preview = assets
            .pack
            .preview_alpha_hex(1, 1)
            .expect("original preview alpha");
        let original_icon_count = assets.icons.len();
        let original_native_count = assets.native_images.len();

        let error = replace_runner_assets_with(
            &mut character,
            &mut assets,
            RunnerCharacter::Fox,
            |requested| {
                build_runner_assets_with(
                    requested,
                    RunnerPack::build,
                    |candidate, index, rgba| {
                        if candidate == RunnerCharacter::Fox && index == 3 {
                            Err(RunnerBuildError::Cache {
                                character: candidate,
                                stage: format!("icon cache frame {index}"),
                                detail: "forced icon cache failure".to_string(),
                            })
                        } else {
                            runner_icon_from_rgba(candidate, index, rgba)
                        }
                    },
                    make_runner_native_image_from_rgba,
                )
            },
        )
        .expect_err("switch should fail");

        assert_eq!(character, RunnerCharacter::Cat);
        assert_eq!(assets.pack.character(), RunnerCharacter::Cat);
        assert_eq!(assets.icons.len(), original_icon_count);
        assert_eq!(assets.native_images.len(), original_native_count);
        assert_eq!(
            assets.pack.preview_alpha_hex(1, 1).as_deref(),
            Some(original_preview.as_str())
        );
        assert!(error.to_string().contains("icon cache frame 3"));
    }

    #[test]
    fn runner_character_switch_keeps_previous_assets_on_native_cache_failure() {
        let mut character = RunnerCharacter::Cat;
        let mut assets = build_runner_assets(RunnerCharacter::Cat).expect("cat assets");
        let original_preview = assets
            .pack
            .preview_alpha_hex(1, 1)
            .expect("original preview alpha");
        let original_icon_count = assets.icons.len();
        let original_native_count = assets.native_images.len();

        let error = replace_runner_assets_with(
            &mut character,
            &mut assets,
            RunnerCharacter::Fox,
            |requested| {
                build_runner_assets_with(
                    requested,
                    RunnerPack::build,
                    runner_icon_from_rgba,
                    |candidate, index, rgba| {
                        if candidate == RunnerCharacter::Fox && index == 5 {
                            Err(RunnerBuildError::Cache {
                                character: candidate,
                                stage: format!("native cache frame {index}"),
                                detail: "forced native cache failure".to_string(),
                            })
                        } else {
                            make_runner_native_image_from_rgba(candidate, index, rgba)
                        }
                    },
                )
            },
        )
        .expect_err("switch should fail");

        assert_eq!(character, RunnerCharacter::Cat);
        assert_eq!(assets.pack.character(), RunnerCharacter::Cat);
        assert_eq!(assets.icons.len(), original_icon_count);
        assert_eq!(assets.native_images.len(), original_native_count);
        assert_eq!(
            assets.pack.preview_alpha_hex(1, 1).as_deref(),
            Some(original_preview.as_str())
        );
        assert!(error.to_string().contains("native cache frame 5"));
    }

    #[test]
    fn startup_runner_initialization_returns_controlled_error_when_cat_cache_conversion_fails() {
        let error = initialize_runner_assets_with(RunnerCharacter::Fox, |requested| {
            if requested == RunnerCharacter::Fox {
                Err(RunnerBuildError::InvalidFrame {
                    character: requested,
                    detail: "forced pack failure".to_string(),
                })
            } else {
                build_runner_assets_with(
                    requested,
                    RunnerPack::build,
                    runner_icon_from_rgba,
                    |candidate, index, rgba| {
                        if candidate == RunnerCharacter::Cat && index == 0 {
                            Err(RunnerBuildError::Cache {
                                character: candidate,
                                stage: format!("native cache frame {index}"),
                                detail: "forced cat cache failure".to_string(),
                            })
                        } else {
                            make_runner_native_image_from_rgba(candidate, index, rgba)
                        }
                    },
                )
            }
        })
        .expect_err("startup should return a controlled error");

        assert!(matches!(
            error.original_error,
            Some(RunnerBuildError::InvalidFrame { .. })
        ));
        assert!(matches!(error.fatal_error, RunnerBuildError::Cache { .. }));
        assert!(runner_startup_fatal_message(&error).chars().count() <= 180);
        assert!(error.to_string().contains("forced cat cache failure"));
    }

    #[test]
    fn system_view_has_high_value_quick_facts() {
        let en = dashboard_html(ResolvedLanguage::En, false);

        for id in [
            "system-load",
            "system-power",
            "system-network-rate",
            "system-uptime",
        ] {
            assert!(en.contains(&format!(r#"id="{id}""#)));
        }
        assert_eq!(format_uptime(90), "1m");
        assert_eq!(format_uptime(7_320), "2h 2m");
        assert_eq!(format_uptime(183_600), "2d 3h");
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
    fn dashboard_overview_has_product_summary_metrics() {
        let en = dashboard_html(ResolvedLanguage::En, false);
        let source = include_str!("main.rs");

        for id in [
            "summary-strip",
            "summary-cpu",
            "summary-temp",
            "summary-fan",
        ] {
            assert!(en.contains(&format!(r##"id="{id}""##)));
        }
        assert!(en.contains(r#"id="summary-temp-label">CPU temp<"#));
        assert!(en.contains(r#"id="summary-fan-label">Fan RPM<"#));
        assert!(en.contains(r#"id="summary-cpu-core-chart"#));
        assert!(en.contains(r#"id="show-core-graph-setting"#));
        assert!(en.contains(r#"id="cpu-core-graph-toggle"#));
        assert!(en.contains("function renderCoreGraph(d)"));
        assert!(en.contains("d.core_hist"));
        assert!(en.contains("d.fan_avg_rpm_text"));
        assert!(en.contains("fan_avg_rpm"));
        assert!(source.contains("\"fan_avg_rpm_text\": fan_avg_rpm_text"));
        assert!(en.contains("setVisible('range-tabs',true);"));
        assert!(en.contains("['health-verdict','summary-strip','sec-cpu','sec-mem','sec-temp']"));
        assert!(en.contains(r#"id="core-details-head""#));
        assert!(en.contains(r#"id="core-details-list""#));
        assert!(en.contains("function renderCoreDetails(d)"));
        assert!(en.contains("function coreGroupName(kind)"));
        assert!(en.contains("d.core_groups"));
        assert!(source.contains("\"core_groups\": cpu_core_groups"));
        assert!(source.contains("\"core_hist\": core_hist"));
    }

    #[test]
    fn dashboard_html_uses_one_fixed_compact_popover_mode() {
        let en = dashboard_html(ResolvedLanguage::En, false);

        assert!(en.contains("compact-extra"));
        assert!(en.contains(r#"data-compact-extra="storage""#));
        assert!(en.contains(r#"data-compact-extra="battery""#));
        assert!(en.contains(r#"data-compact-extra="network""#));
        assert!(en.contains(r#"data-compact-extra="processes""#));
        assert!(en.contains("document.body.classList.add('compact')"));
        assert!(!en.contains("setPopoverExpanded"));
        assert!(!en.contains("pf.compact"));
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
            ("railSettings", "settings"),
            ("railSystem", "system"),
        ] {
            assert!(en.contains(&format!(r#"id="{id}""#)));
            assert!(en.contains(&format!(r#"data-rail-action="{action}""#)));
        }

        assert!(en.contains("function runRailAction(action,btn)"));
        assert!(!en.contains("flashRailButton(btn)"));
        assert!(en.contains("case 'detail':setRailView('overview');break;"));
        assert!(en.contains("case 'fan':setRailView('fan');break;"));
        assert!(en.contains("case 'settings':setRailView('settings');break;"));
        assert!(!en.contains(
            "case 'login':setRailView('login');window.ipc.postMessage('togglelogin');break;"
        ));
        assert!(!en.contains("case 'license':setRailView('license');break;"));
        assert!(en.contains("case 'system':case 'more':setRailView('system');break;"));
        assert!(en.contains("case 'update':setRailView('settings');break;"));
        assert!(en.contains("function setButtonLabel(btn,label)"));
        assert!(en.contains("btn.querySelector('span')"));
        assert!(en.contains("btn.dataset.defaultLabel"));
        assert!(en.contains("el.classList.add('focus-pulse')"));
    }

    #[test]
    fn update_rail_keeps_status_button_as_navigation_not_detail_command() {
        let en = dashboard_html(ResolvedLanguage::En, false);
        let ko = dashboard_html(ResolvedLanguage::Ko, false);

        assert!(en.contains(r#"title="Status""#));
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
    fn settings_panel_exposes_persistent_native_notification_rules() {
        let en = dashboard_html(ResolvedLanguage::En, false);
        let ko = dashboard_html(ResolvedLanguage::Ko, false);

        for id in [
            "notification-settings",
            "notification-temp-toggle",
            "notification-temp-threshold",
            "notification-fan-toggle",
            "notification-update-toggle",
        ] {
            assert!(en.contains(&format!(r#"id="{id}""#)));
        }
        assert!(en.contains("function updateNotificationSettings(d)"));
        assert!(en.contains("updateNotificationSettings(d);"));
        assert!(en.contains("notifications:temperature:"));
        assert!(en.contains("notifications:'+kind+':"));
        assert!(ko.contains(">알림<"));
        assert!(ko.contains(">CPU 온도 경고<"));
        assert!(ko.contains(">팬 제어 실패<"));
        assert!(ko.contains(">앱 업데이트<"));
    }

    #[test]
    fn settings_panel_exposes_automation_rule_editor() {
        let en = dashboard_html(ResolvedLanguage::En, false);
        let ko = dashboard_html(ResolvedLanguage::Ko, false);
        for html in [&en, &ko] {
            assert!(html.contains(r#"id="automation-settings""#));
            assert!(html.contains(r#"id="automation-rules""#));
            assert!(html.contains(r#"id="automation-add""#));
            assert!(html.contains(r#"id="automation-preset""#));
            assert!(html.contains(r#"id="automation-save""#));
            assert!(html.contains(r#"id="config-transfer-settings""#));
            assert!(html.contains(r#"id="config-transfer-text""#));
            assert!(html.contains(r#"id="config-copy""#));
            assert!(html.contains(r#"id="config-import""#));
            assert!(html.contains("function updateAutomationRules(d)"));
            assert!(html.contains("function applyQuietAcPreset()"));
            assert!(html.contains("function updateConfigTransfer(d)"));
            assert!(html.contains("window.ipc.postMessage('copyconfig')"));
            assert!(html.contains("settingsRequest('import_config',value"));
            assert!(
                html.contains("settingsRequest('rules',JSON.stringify(automationRulesPayload())")
            );
            assert!(
                html.contains("window.ipc.postMessage('settingsrequest:'+JSON.stringify(request))")
            );
            assert!(html.contains("d.rules"));
            assert!(html.contains("d.config_toml"));
        }
        assert!(en.contains(">Automation Rules<"));
        assert!(ko.contains(">자동화 규칙<"));
        assert!(ko.contains(">규칙 추가<"));
        assert!(ko.contains(">배터리 저소음 / AC 고성능<"));
        assert!(ko.contains(">규칙 저장<"));
        assert!(!ko.contains(">Automation Rules<"));
        assert!(en.contains(">Configuration Transfer<"));
        assert!(ko.contains(">설정 가져오기 및 내보내기<"));
        assert!(en.contains("CPU temperature above"));
        assert!(ko.contains("CPU 온도 초과"));
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
        assert!(en.contains(r#"id="fan-diagnostic-button" disabled"#));
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
        let source = include_str!("main.rs");

        assert!(en.contains(r#"id="hardware-availability-card""#));
        assert!(en.contains("var card=document.getElementById('hardware-availability-card');"));
        assert!(en.contains("if(card)card.style.display='';"));
        assert!(en.contains(">Hardware Availability<"));
        assert!(en.contains(r#"id="hardware-fans""#));
        assert!(en.contains(r#"id="hardware-battery""#));
        assert!(en.contains(r#"id="hardware-network""#));
        assert!(en.contains(r#"id="fan-empty-state""#));
        assert!(en.contains(r#"id="data-loading""#));
        assert!(en.contains("function retryDashboard()"));
        assert!(en.contains("window.ipc.postMessage('refresh')"));
        assert!(en.contains("document.body.classList.add('data-ready');"));
        assert!(en.contains(r#"id="temp-empty""#));
        assert!(en.contains("show('sec-temp',true);"));
        assert!(en.contains("clearChart('temp-chart');"));
        assert!(en.contains("function updateHardwareAvailability(d)"));
        assert!(en.contains("updateHardwareAvailability(d);"));
        assert!(en.contains("d.network_count"));
        assert!(en.contains("d.network_active"));
        assert!(en.contains("Sensor reading is taking longer than expected"));
        assert!(source.contains(r#"body == "refresh""#));
        assert!(source.contains("CONTROL_REFRESH_REQUESTED.store(true, Ordering::Release);"));
        assert!(en.contains(">No fan sensors<"));
        assert!(en.contains("No fan sensors were reported"));
        assert!(en.contains("Read-only fans"));
        assert!(en.contains("controllableCount>0"));
        assert!(en.contains(r#"id="profile-strip""#));
        assert!(en.contains(
            r#"disabled data-mode="auto" aria-pressed="true" title="Auto" onclick="setAuto()">"#
        ));

        assert!(ko.contains(">하드웨어 감지 상태<"));
        assert!(ko.contains(">배터리<"));
        assert!(ko.contains(">네트워크<"));
        assert!(ko.contains(">팬 센서 없음<"));
        assert!(ko.contains("팬 센서를 찾지 못했습니다"));
        assert!(ko.contains("읽기 전용 팬"));
    }

    #[test]
    fn hardware_prefetch_and_loading_overlays_do_not_shift_the_dashboard() {
        let en = dashboard_html(ResolvedLanguage::En, false);
        let source = include_str!("main.rs");

        assert!(source.contains("refresh_fan_cache(app, now);"));
        assert!(source.contains("refresh_daemon_cache(app, now);"));
        assert!(source.contains("daemon_read: Arc<BackgroundRead<Option<serde_json::Value>>>"));
        assert!(en.contains(".data-loading{position:absolute;"));
        assert!(en.contains(".view-loading{position:absolute;"));
        assert!(en.contains(
            "body.data-ready .data-loading{opacity:0;visibility:hidden;pointer-events:none;}"
        ));
        assert!(!en.contains("body.data-ready .data-loading{display:none;}"));
    }

    #[test]
    fn settings_panel_contains_startup_toggle() {
        let en = dashboard_html(ResolvedLanguage::En, false);
        let ko = dashboard_html(ResolvedLanguage::Ko, false);

        for html in [en, ko] {
            assert!(html.contains(r#"id="startup-setting"#));
            assert!(html.contains(">Run on startup<") || html.contains("부팅 시 자동 실행"));
            assert!(html.contains(r#"id="startup-toggle"#));
            assert!(html.contains(r#"id="startup-toggle" class="panel-action secondary" disabled"#));
            assert!(html.contains("toggleStartupItem(this)"));
        }
    }

    #[test]
    fn automatic_update_notification_is_deduplicated_per_channel_and_version() {
        let version = "notification-regression-test";
        assert!(claim_update_notification(UpdateChannel::Stable, version));
        assert!(!claim_update_notification(UpdateChannel::Stable, version));
        assert!(claim_update_notification(
            UpdateChannel::PreRelease,
            version
        ));
        assert!(!claim_update_notification(
            UpdateChannel::PreRelease,
            version
        ));
        assert!(claim_update_notification(
            UpdateChannel::Stable,
            "notification-regression-test-next"
        ));
    }

    #[test]
    fn restored_update_history_keeps_fresh_startup_and_no_release_claims() {
        let mut state = APP_UPDATE_STATE.lock().unwrap().clone();
        let history = update_history::UpdateHistory::capture(
            UpdateChannel::Stable,
            Some(90),
            Some(80),
            Some("offline".into()),
        );
        initialize_update_discovery(&mut state, UpdateChannel::Stable, 100, Some(history));
        assert_eq!(state.schedule.last_attempt_unix, Some(90));
        assert_eq!(state.schedule.last_success_unix, Some(80));
        assert_eq!(state.schedule.next_check_unix, Some(120));
        assert!(!state.schedule.due(119));
        assert!(state.schedule.due(120));
        assert_eq!(state.phase, "idle");
        assert!(state.latest.is_none() && state.release_url.is_none() && state.notes.is_none());
        assert!(!state.install_ready);
        assert_eq!(state.check_error.as_deref(), Some("offline"));
        state.schedule.begin(120);
        state.schedule.finish(121, false);
        assert_eq!(state.schedule.next_check_unix, Some(421));
    }

    #[test]
    fn stale_release_response_clears_old_channel_metadata_and_schedules_new_channel() {
        let mut state = AppUpdateState {
            phase: "available",
            latest: Some("9.0.0".into()),
            release_url: Some("https://example.invalid/release".into()),
            notes: Some("Old channel".into()),
            message: None,
            install_ready: true,
            channel: UpdateChannel::Stable,
            schedule: UpdateSchedule::new(),
            check_error: Some("old error".into()),
        };
        assert!(!discard_stale_update_channel(
            &mut state,
            UpdateChannel::Stable,
            UpdateChannel::Stable,
            10
        ));
        assert!(state.install_ready);
        assert!(discard_stale_update_channel(
            &mut state,
            UpdateChannel::Stable,
            UpdateChannel::PreRelease,
            20
        ));
        assert_eq!(state.channel, UpdateChannel::PreRelease);
        assert_eq!(state.phase, "idle");
        assert!(state.latest.is_none() && state.release_url.is_none() && state.notes.is_none());
        assert!(!state.install_ready);
        assert!(state.check_error.is_none());
        assert!(state.schedule.due(20));
        assert!(state.schedule.last_success_unix.is_none());
        state.schedule.begin(20);
        state.schedule.finish(21, true);
        assert!(!reconcile_update_channel(
            &mut state,
            UpdateChannel::PreRelease,
            22,
            true
        ));
        assert_eq!(
            state.schedule.next_check_unix,
            Some(21 + update_schedule::CHECK_INTERVAL)
        );
        assert!(reconcile_update_channel(
            &mut state,
            UpdateChannel::Stable,
            23,
            true
        ));
        assert!(state.schedule.due(23));
        assert!(state.schedule.last_success_unix.is_none());
        assert!(reconcile_update_channel(
            &mut state,
            UpdateChannel::PreRelease,
            24,
            false
        ));
        assert!(
            !state.schedule.due(24),
            "mock imports must not schedule network work"
        );
    }

    #[test]
    fn update_panel_shows_current_latest_and_check_result() {
        let en = dashboard_html(ResolvedLanguage::En, false);

        assert!(en.contains(r#"id="update-current-version""#));
        assert!(en.contains(r#"id="update-latest-version""#));
        assert!(en.contains(r#"id="update-check-result""#));
        assert!(en.contains(r#"id="update-channel""#));
        assert!(en.contains("changeUpdateChannel(this.value)"));
        assert!(include_str!("main.rs").contains("fetch_release_for_channel"));
        assert!(en.contains(r#"id="rail-update-check""#));
        assert!(en.contains(r#"id="rail-update-install""#));
        assert!(en.contains(r#"id="update-release-link""#));
        assert!(en.contains(r#"id="update-release-notes-card""#));
        assert!(en.contains(r#"id="update-release-notes""#));
        assert!(en.contains("function compareVersions(a,b)"));
        assert!(en.contains("function formatReleaseNotes(body)"));
        assert!(en.contains("function renderUpdateStatus(status)"));
        assert!(en.contains("function loadUpdateStatusSnapshot()"));
        assert!(en.contains("function rememberUpdateStatus(status)"));
        assert!(en.contains("pf.update.status.v1"));
        assert!(en.contains("15*60*1000"));
        assert!(en.contains("APP_UPDATE_STATUS=loadUpdateStatusSnapshot();"));
        assert!(en.contains("if(RAIL_VIEW!=='settings')setRailView('settings');"));
        assert!(!en.contains("function fetchLatestReleaseStatus()"));
        assert!(!en.contains("api.github.com/repos/uulab-official/peterfan/releases/latest"));
        assert!(en.contains("d.app_update_status||{}"));
        assert!(en.contains("phase:'checking'"));
        assert!(en.contains("Check for Updates"));
        assert!(en.contains("Install Update"));
        assert!(en.contains("function installAppUpdate(btn)"));
        assert!(en.contains("mode==='install'?'installupdate':'checkupdates'"));
        assert!(en.contains("s.install_ready===true"));
        assert!(en.contains("install_ready:!!nativeUpdate.install_ready"));
        assert!(en.contains("Preparing Update"));
        assert!(en.contains("var checking=!!s.checking||phase==='checking';"));
        assert!(en.contains("Awaiting Signed Release"));
        assert!(en.contains("Up to Date"));
        assert!(!en.contains(
            r#"id="rail-update-install" disabled onclick="installAppUpdate(this)" style="display:none""#
        ));
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
        assert!(en.contains("window.ipc.postMessage('rawtemps:'+(RAW_TEMP_OPEN?'1':'0'))"));
        assert!(include_str!("main.rs").contains(
            "let raw_temps_visible = overview_visible && RAW_TEMPS_OPEN.load(Ordering::Relaxed);"
        ));
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
    fn sensor_group_labels_distinguish_cpu_and_gpu_die_roles() {
        let cpu_average = temp("cpu.die", SensorKind::Cpu, 52.0);
        let cpu_hot = temp("cpu.die.hot", SensorKind::Cpu, 67.0);
        let gpu_die = temp("gpu.die", SensorKind::Gpu, 58.0);

        assert_eq!(
            sensor_group_label_for_sensor(ResolvedLanguage::En, &cpu_average),
            "CPU average"
        );
        assert_eq!(
            sensor_group_label_for_sensor(ResolvedLanguage::En, &cpu_hot),
            "CPU hottest"
        );
        assert_eq!(
            sensor_group_label_for_sensor(ResolvedLanguage::Ko, &gpu_die),
            "GPU 다이"
        );
    }

    #[test]
    fn rail_view_switch_resets_scroll_and_marks_pressed_state() {
        let en = dashboard_html(ResolvedLanguage::En, false);

        assert!(en.contains(
            r#"id="railDetail" data-rail-action="detail" aria-label="Status" aria-pressed="true""#
        ));
        assert!(en.contains(
            r#"id="railFan" data-rail-action="fan" aria-label="Fans" aria-pressed="false""#
        ));
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
        assert!(en.contains("case 'settings':setRailView('settings');break;"));
        assert!(!en.contains(
            "case 'login':setRailView('login');window.ipc.postMessage('togglelogin');break;"
        ));
        assert!(!en.contains("case 'license':setRailView('license');break;"));
        assert!(en.contains("case 'system':case 'more':setRailView('system');break;"));
        assert!(en.contains("case 'update':setRailView('settings');break;"));
        assert!(!en.contains("case 'license':toggleLicForm();break;"));
    }

    #[test]
    fn fan_setup_prompt_is_scoped_to_fan_and_settings_views() {
        let en = dashboard_html(ResolvedLanguage::En, false);
        let source = include_str!("main.rs");

        assert!(en.contains(
            "if(view==='fan'){\n    setVisible('setup-row',true);\n    setVisible('fan-control-section',true);"
        ));
        assert!(en.contains(
            "if(setup)setup.style.display=fanView||railView()==='settings'?'flex':'none';"
        ));
        assert!(en.contains("var stateReady=d.fan_control_state_ready!==false;"));
        assert!(en.contains("var fanCount=Number(d.fan_count||0);"));
        assert!(en.contains("var controllableCount=Number(d.controllable_fan_count||0);"));
        assert!(en.contains("var noFans=stateReady&&fanCount===0;"));
        assert!(en.contains("var readOnly=stateReady&&fanCount>0&&controllableCount===0;"));
        assert!(en.contains(
            "var showSetup=stateReady&&supported&&controllableCount>0&&(d.fan_setup_needed||d.daemon_update_needed);"
        ));
        assert!(en.contains("setVisible('setup-row',true);"));
        assert!(
            en.contains(
                "} else {\n    ['health-verdict','summary-strip','sec-cpu','sec-mem','sec-temp'].forEach"
            )
        );
        assert!(!en.contains(
            "['range-tabs','setup-row','summary-strip','sec-cpu','sec-mem','sec-temp'].forEach"
        ));
        assert!(en.contains("setButtonLabel(fan,LANG==='ko'?'팬 제어':'Fans');"));
        assert!(!en.contains("setButtonLabel(fan,d.fan_setup_needed"));
        assert!(!en.contains("fan.classList.toggle('active',!!d.can_control);"));
        assert!(source.contains("const DAEMON_STALE_AFTER: Duration = Duration::from_secs(8);"));
        assert!(source.contains("app.daemon_json_sampled_at = Some(now);"));
        assert!(source.contains("fan_control_state_ready"));
        assert!(source.contains("now.duration_since(sampled_at) > DAEMON_STALE_AFTER"));
    }

    #[test]
    fn fan_setup_completion_immediately_refreshes_native_and_webview_state() {
        let en = dashboard_html(ResolvedLanguage::En, false);
        let source = include_str!("main.rs");

        assert!(source.contains("static INSTALL_FAN_CONTROL_REVISION: AtomicU64"));
        assert!(source.contains("INSTALL_FAN_CONTROL_REVISION.fetch_add(1, Ordering::AcqRel);"));
        assert!(source.contains("CONTROL_REFRESH_REQUESTED.store(true, Ordering::Release);"));
        assert!(source.contains("let _ = proxy.send_event(());"));
        assert!(en.contains("var FAN_CONTROL_FIX_REVISION=0;"));
        assert!(en.contains("Number(d.fan_control_install_revision||0)>FAN_CONTROL_FIX_REVISION"));
        assert!(en.contains("if(!current.fan_control_installing)FAN_CONTROL_FIX_PENDING=false;"));
        let setup = en
            .split("function startFanControlSetup(btn){")
            .nth(1)
            .unwrap()
            .split("function runFanDiagnostics(btn){")
            .next()
            .unwrap();
        assert!(!setup.contains("},15000);"));
        assert!(setup.contains("},60000);"));
    }

    #[test]
    fn fan_view_keeps_a_stable_header_and_control_summary() {
        let en = dashboard_html(ResolvedLanguage::En, false);

        assert!(en.contains(r#"id="setup-row"#));
        assert!(en.contains(r#"id="setup-title"#));
        assert!(en.contains(r#"id="fan-inputs"#));
        assert!(en.contains(r#"id="fan-curve-input"#));
        assert!(en.contains(r#"id="fan-safety-hottest"#));
        assert!(en.contains(r#"id="fan-critical-limit"#));
        assert!(en.contains("grid-template-columns:repeat(3,minmax(0,1fr))"));
        assert!(en.contains("fan_control_state_ready"));
    }

    #[test]
    fn settings_and_system_views_have_distinct_responsibilities() {
        let en = dashboard_html(ResolvedLanguage::En, false);

        assert!(en.contains(r#"id="rail-more-panel""#));
        assert!(en.contains(r#"id="system-loading""#));
        assert!(en.contains("Reading system metrics…"));
        assert!(en.contains("d.slow_data_ready"));
        assert!(en.contains(r#"id="railSystem""#));
        assert!(en.contains("case 'detail':setRailView('overview');break;"));
        assert!(en.contains("case 'system':case 'more':setRailView('system');break;"));
        assert!(en.contains("case 'update':setRailView('settings');break;"));
        assert!(en.contains(r#"onclick="window.ipc.postMessage('open_detail')""#));
        assert!(en.contains(r#"onclick="window.ipc.postMessage('quit')""#));
        assert_eq!(en.matches(">Open Detail Window<").count(), 1);
        assert!(en.contains("['rail-settings-panel','rail-update-panel'].forEach"));
        assert!(en.contains(
            "['rail-more-panel','sec-storage','sec-batt','sec-network','sec-procs','foot'].forEach"
        ));
        assert!(!en.contains("case 'detail':window.ipc.postMessage('open_detail');break;"));
        assert!(!en.contains("if(view==='more')setPopoverExpanded(true);"));
    }

    #[test]
    fn system_view_reveals_low_priority_metric_sections() {
        let en = dashboard_html(ResolvedLanguage::En, false);

        assert!(en.contains(r#"body.compact[data-rail-view="system"] .compact-extra"#));
        assert!(en.contains(
            "['rail-more-panel','sec-storage','sec-batt','sec-network','sec-procs','foot'].forEach"
        ));
        assert!(en.contains(r#"data-compact-extra="storage""#));
        assert!(en.contains(r#"data-compact-extra="battery""#));
        assert!(en.contains(r#"data-compact-extra="network""#));
        assert!(en.contains(r#"data-compact-extra="processes""#));
        assert!(en.contains(">System<"));
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
        assert!(source.contains("sampled_at: Option<Instant>"));
        assert!(
            source.contains("\"slow_data_ready\": app.dashboard_slow_cache.sampled_at.is_some()")
        );
        assert!(source.contains("fn refresh_dashboard_slow_cache("));
        assert!(source.contains("let refresh_slow_metrics = (settings_visible || system_visible)"));
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

        assert!(en.contains("--content-x:18px;--section-y:16px;--panel-pad:18px;"));
        assert!(en.contains("padding:var(--section-y) var(--content-x)"));
        assert!(en.contains("padding:var(--panel-pad) var(--content-x)"));
        assert!(en.contains(".range-tabs{display:flex;gap:4px;padding:12px var(--content-x) 8px;"));
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
            "else if(view==='settings'||view==='system')",
            "else if(view==='fan')",
        ] {
            assert!(update.contains(guard));
        }
        assert_eq!(
            update
                .matches("else if(view==='settings'||view==='system')")
                .count(),
            1
        );
        assert!(en.contains("updateHealthPanel(d);"));
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
        assert!(en.contains("grid-template-columns:minmax(0,1fr) 50px"));
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
        assert!(en.contains("updateProfileStrip(snapshot);\n  updateFanApplyStatus(snapshot);"));
        assert!(en.contains("revisionBefore:Number(current.applied_control_revision||0)"));
        assert!(en.contains("var revisionApplied=Number(d.applied_control_revision||0)>FAN_CONTROL_PENDING.revisionBefore;"));
        assert!(en.contains("FAN_CONTROL_RESULT={ok:true"));
        assert!(en.contains("hardware confirmation timed out"));
        assert!(en.contains("Date.now()-FAN_CONTROL_PENDING.startedAt>8000"));
        assert!(en.contains("if(d.fan_control_supported)"));
        assert!(en.contains("renderFanCards(d.fans,d.can_control)"));
        assert!(en.contains("card.dataset.controlEnabled=enabled?'1':'0'"));
        assert!(en.contains("slider.disabled=!enabled||!!pendingMode"));
        assert!(en.contains("function updateFanApplyStatus(d)"));
        assert!(en.contains("typeof f.target_pct==='number'"));
        assert!(en.contains("strip.setAttribute('aria-busy',pending?'true':'false');"));
        assert!(en.contains("b.disabled=!enabled||pending;"));
        assert!(en.contains(".profile-strip.pending::after{"));
        assert!(en.contains("strip.classList.toggle('confirmed'"));
        assert!(en.contains("strip.classList.toggle('failed'"));
        assert!(source.contains("static CONTROL_REFRESH_REQUESTED: AtomicBool"));
        assert!(source.contains("CONTROL_REFRESH_REQUESTED.store(true, Ordering::Relaxed);"));
        assert!(source.contains("CONTROL_REFRESH_REQUESTED.swap(false, Ordering::Relaxed)"));
    }

    #[test]
    fn dashboard_charts_follow_the_active_color_theme() {
        let en = dashboard_html(ResolvedLanguage::En, false);

        assert!(en.contains("function cssColor(token,fallback)"));
        assert!(en.contains("cssColor('--accent','#6ea8ff')"));
        assert!(en.contains("cssColor('--y','#f4c95d')"));
        assert!(en.contains("cssColor('--g','#5dd879')"));
        assert!(!en.contains("drawChart('cpu-chart', d.cpu_hist, '#5b9dff'"));
        assert!(!en.contains("ctx.strokeStyle='#5b9dff'"));
    }

    #[test]
    fn dashboard_meets_the_gstack_local_product_gate() {
        let en = dashboard_html(ResolvedLanguage::En, false);
        let ko = dashboard_html(ResolvedLanguage::Ko, false);
        let checklist = include_str!("../../../docs/GSTACK_PRODUCT_CHECKLIST.md");

        assert_eq!(
            checklist
                .lines()
                .filter(|line| line.starts_with("- [x]"))
                .count(),
            10
        );
        for id in [
            "health-verdict",
            "health-verdict-title",
            "health-verdict-detail",
            "profile-guide",
            "profile-guide-title",
            "profile-preview-bars",
        ] {
            assert!(en.contains(&format!(r#"id="{id}""#)), "missing {id}");
        }
        assert!(en.contains("function updateHealthVerdict(d)"));
        assert!(en.contains("function renderProfileGuide(mode,profile)"));
        assert!(en.contains("CPU avg "));
        assert!(en.contains("macOS safety fallback"));
        assert!(en.contains("Latest signed"));
        assert!(en.contains("newer than the public signed release"));
        assert!(ko.contains("최신 서명 릴리스"));
        assert!(ko.contains("Mac 상태 확인 중"));
        assert!(en.contains(
            r#"<button type="button" class="all-temp-head" id="all-temp-head" aria-expanded="false""#
        ));
        assert!(en.contains(r#"id="fan-apply-status" role="status" aria-live="polite""#));
        assert!(en.contains("b.setAttribute('aria-pressed',selected?'true':'false');"));
        assert!(en.contains("b.setAttribute('aria-pressed',active?'true':'false');"));
        assert!(en.contains(".pkill.confirm{background:var(--warn-soft)"));
        assert!(en.contains(".rail-btn{height:44px"));
    }

    #[test]
    fn support_report_is_complete_and_privacy_safe() {
        let temperatures = vec![TempSensor {
            id: "cpu.die".into(),
            label: "CPU Core Average".into(),
            kind: SensorKind::Cpu,
            source: SensorSource::Smc,
            value: Celsius(64.5),
        }];
        let fans = vec![serde_json::json!({
            "id": "fan.0",
            "l": "Left Fan",
            "cur_rpm": 2400,
            "target_pct": 45,
            "readback_status": "settled",
        })];
        let daemon = serde_json::json!({
            "mode": "manual:balanced",
            "backend": "macos",
            "control_revision": 8,
            "applied_control_revision": 8,
            "fan_targets": {"fan.0": 45},
            "fan_readbacks": [{"id": "fan.0", "status": "settled"}],
            "control_health": {"failsafe_active": false},
            "hostname": "must-not-leak",
        });

        let report = support_report_json(SupportReportInput {
            cpu_pct: 37.5,
            logical_cores: 12,
            selected_average_c: Some(64.5),
            core_hottest_c: Some(68.0),
            safety_hottest_c: Some(72.0),
            temperatures: &temperatures,
            temperature_sampled_at_unix_ms: Some(1_234),
            temperature_stale: false,
            fans: &fans,
            daemon_version: Some("1.27.77"),
            daemon: Some(&daemon),
        });

        assert_eq!(report["schema_version"], 1);
        assert_eq!(report["cpu"]["core_average_c"], 64.5);
        assert_eq!(report["temperature_sample"]["sensors"][0]["source"], "smc");
        assert_eq!(report["fans"][0]["readback_status"], "settled");
        assert_eq!(report["daemon"]["mode"], "manual:balanced");
        assert_eq!(report["daemon"]["applied_control_revision"], 8);
        let encoded = serde_json::to_string(&report).unwrap();
        for forbidden_key in [
            "hostname",
            "username",
            "serial_number",
            "ip_address",
            "file_path",
        ] {
            assert!(!encoded.contains(&format!(r#"\"{forbidden_key}\":"#)));
        }
    }

    #[test]
    fn support_report_button_waits_for_native_copy_confirmation() {
        let en = dashboard_html(ResolvedLanguage::En, false);
        let ko = dashboard_html(ResolvedLanguage::Ko, false);
        let source = include_str!("main.rs");

        assert!(en.contains(r#"id="support-report-button""#));
        assert!(en.contains("function copySupportReport(btn)"));
        assert!(en.contains("support_report_copy_revision"));
        assert!(en.contains("d.support_report_copy_ok"));
        assert!(ko.contains(">리포트 복사<"));
        // GitHub's Windows checkout may use CRLF, so do not make this source
        // boundary assertion depend on a particular line-ending style.
        let product_source = source
            .rfind("mod tests {")
            .map(|index| &source[..index])
            .unwrap_or(source);
        assert_eq!(
            product_source
                .matches(r#"body == "copydiagnostics""#)
                .count(),
            2
        );
        assert!(write_support_report_to_clipboard(" ").is_err());
    }

    #[test]
    fn dashboard_requests_refresh_when_webview_becomes_ready() {
        let en = dashboard_html(ResolvedLanguage::En, false);
        let source = include_str!("main.rs");

        assert!(en.contains("window.__pf_pending"));
        assert!(en.contains("function applyPendingUpdate()"));
        assert!(en.contains("function sendWebviewReady()"));
        assert!(en.contains("setTimeout(sendWebviewReady,250)"));
        assert_eq!(en.matches("window.ipc.postMessage('ready')").count(), 1);
        assert!(en.contains("runner_reduce_motion"));
        assert!(en.contains("Reduce Motion · still frame"));
        assert!(en.contains("applyPendingUpdate();"));
        assert!(source.contains("ready:popover"));
        assert!(source.contains("ready:detail"));
        assert!(source.contains("app.dashboard_script.as_deref()"));
        assert!(source.contains("evaluate_dashboard_script(wv, script, \"popover\")"));
        assert!(source.contains("evaluate_dashboard_script(wv, script, \"detail\")"));
        assert!(source.contains("popover view requested view={view}"));
        assert!(source.contains("detail view requested view={view}"));
    }

    #[test]
    fn popover_click_path_prepares_dashboard_before_show() {
        let source = include_str!("main.rs");

        assert!(source.contains("POPOVER_PREWARM_DELAY"));
        assert!(source.contains(".with_focused(false)"));
        assert!(source.contains(".with_accept_first_mouse(true)"));
        assert!(source.contains("defer_dashboard_io_after_open(app);"));
        assert!(source.contains("w.set_focus();"));
        assert!(source.contains("configure_native_popover_window(w, position, POPOVER_W, height)"));
        assert!(source.contains("native_window.setFrameTopLeftPoint"));
        assert!(source.contains("#[cfg(not(target_os = \"macos\"))]"));
        assert!(source.contains("inject a payload"));
        assert!(source.contains("next_metric_at = now;"));
        assert!(source.contains("*control_flow = ControlFlow::WaitUntil(now);"));
        assert!(!source.contains("app.popover_visible = true;\n    update(app);"));
    }

    #[test]
    fn scheduled_popover_is_rendered_while_still_hidden() {
        assert!(!dashboard_render_requested(false, false, false));
        assert!(dashboard_render_requested(true, false, false));
        assert!(dashboard_render_requested(false, true, false));
        assert!(dashboard_render_requested(false, false, true));
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
    fn daemon_config_capability_requires_an_explicit_success_reply() {
        assert!(daemon_supports_user_config(Some(
            "ok logged-in-user (macos)"
        )));
        assert!(!daemon_supports_user_config(Some("error: unknown command")));
        assert!(!daemon_supports_user_config(None));
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
        assert!(peterfan_platform::daemon_update_required("1.27.22"));
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
            "--character".to_string(),
            "fox".to_string(),
            "--update-health-token".to_string(),
            "1234567890abcdef1234567890abcdef".to_string(),
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
    fn menu_bar_display_style_labels_match_selectable_runner() {
        assert_eq!(strings(ResolvedLanguage::En).style_graph, "Runner");
        assert_eq!(strings(ResolvedLanguage::En).style_both, "Number + Runner");
        assert_eq!(strings(ResolvedLanguage::Ko).style_graph, "러너");
        assert_eq!(strings(ResolvedLanguage::Ko).style_both, "숫자 + 러너");

        let help = help_text();
        assert!(help.contains("--display <number|cat|both>"));
        assert!(help.contains("--character <name>"));
        assert!(help.contains("CPU runner character (20 choices)"));
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
            daemon_reinstall_hint(ResolvedLanguage::Ko, Some("1.32.5")),
            "조용히 가능"
        );
        assert_eq!(
            daemon_reinstall_hint(ResolvedLanguage::En, Some("1.32.5")),
            "no prompt"
        );
    }

    #[test]
    fn stale_daemon_is_not_usable_for_cached_state() {
        assert!(!daemon_control_usable(Some("1.26.24"), Some("1.26.24")));
        assert!(daemon_control_usable(
            Some(peterfan_platform::MIN_REQUIRED_DAEMON_VERSION),
            Some(peterfan_platform::MIN_REQUIRED_DAEMON_VERSION)
        ));
        for resident in [None, Some("1.32.4"), Some("unknown")] {
            assert!(!daemon_control_usable(Some("1.32.5"), resident));
        }
        assert!(!daemon_control_usable(None, Some("1.32.5")));
    }

    #[test]
    fn insecure_or_unknown_daemon_never_self_updates_silently() {
        for disk in ["1.26.24", "1.27.22", "1.32.4"] {
            for resident in [None, Some("1.32.4"), Some("1.32.5")] {
                assert!(!daemon_can_self_update_silently(disk, resident));
            }
        }
    }

    #[test]
    fn secure_previous_helpers_upgrade_to_long_run_safety_without_reapproval() {
        for version in ["1.32.5", "1.32.6"] {
            assert!(!daemon_control_usable(Some(version), Some(version)));
            assert!(daemon_can_self_update_silently(version, Some(version)));
            assert_eq!(
                fan_control_install_plan(Some(version), Some(version), true),
                FanControlInstallPlan::SelfReinstall
            );
        }
    }

    #[test]
    fn secure_self_reinstall_requires_both_disk_and_resident_capability() {
        for (disk, resident) in [
            (None, Some("1.32.5")),
            (Some("1.32.5"), None),
            (Some("1.32.4"), Some("1.32.5")),
            (Some("1.32.5"), Some("1.32.4")),
        ] {
            assert!(!daemon_self_reinstall_is_secure(disk, resident));
        }
        assert!(daemon_self_reinstall_is_secure(
            Some("1.32.5"),
            Some("1.32.5")
        ));
    }

    #[test]
    fn setup_detail_shows_daemon_version_when_ready() {
        let en = setup_detail(ResolvedLanguage::En, true, false, Some("1.32.5"));
        assert_eq!(en, "daemon v1.32.5 · no additional approval");
        assert!(!en.contains("login"));

        let ko = setup_detail(ResolvedLanguage::Ko, true, false, Some("1.32.5"));
        assert_eq!(ko, "데몬 v1.32.5 · 추가 승인 없음");
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
    fn fan_control_install_avoids_reapproval_for_existing_daemons() {
        assert_eq!(
            fan_control_install_plan(
                Some(peterfan_platform::MIN_REQUIRED_DAEMON_VERSION),
                Some(peterfan_platform::MIN_REQUIRED_DAEMON_VERSION),
                true
            ),
            FanControlInstallPlan::AlreadyReady
        );
        assert_eq!(
            fan_control_install_plan(Some("1.32.5"), Some("1.32.6"), true),
            FanControlInstallPlan::SelfReinstall
        );
        assert_eq!(
            fan_control_install_plan(None, None, false),
            FanControlInstallPlan::PrivilegedInstall
        );
    }

    #[test]
    fn explicit_activation_repairs_an_installed_but_unreachable_daemon() {
        for version in ["1.27.29", "1.31.30", env!("CARGO_PKG_VERSION")] {
            assert_eq!(
                fan_control_install_plan(Some(version), None, false),
                FanControlInstallPlan::PrivilegedInstall,
                "explicit activation must offer approved repair for unreachable v{version}"
            );
        }
    }

    #[test]
    fn explicit_activation_repairs_unknown_or_mismatched_resident_versions() {
        for (disk, resident) in [
            ("1.32.2", None),
            ("1.32.4", Some("1.31.30")),
            ("1.32.5", None),
            ("1.32.5", Some("1.32.4")),
        ] {
            assert_eq!(
                fan_control_install_plan(Some(disk), resident, true),
                FanControlInstallPlan::PrivilegedInstall,
                "a reachable insecure or unknown resident requires fresh approved repair"
            );
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn manual_daemon_reinstall_falls_back_to_one_privileged_install() {
        use peterfan_platform::daemon_install::InstallOutcome;

        let mut privileged_called = false;
        let result = reinstall_or_privileged_install(
            || Err("reinstall source is not a trusted PeterFan release".to_string()),
            || {
                privileged_called = true;
                Ok(InstallOutcome::Installed)
            },
        );

        assert!(matches!(result, Ok(InstallOutcome::Installed)));
        assert!(privileged_called);
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
    fn fan_cache_ignores_transient_empty_samples() {
        let fan = Fan {
            id: "fan.test".into(),
            label: "Test Fan".into(),
            rpm: 3_200,
            min_rpm: Some(2_000),
            max_rpm: Some(6_000),
            duty_percent: None,
            controllable: true,
        };
        let mut cache = vec![fan];
        let mut empty_samples = 0;

        merge_fan_sample(&mut cache, &mut empty_samples, Vec::new());
        merge_fan_sample(&mut cache, &mut empty_samples, Vec::new());
        assert_eq!(cache.len(), 1);
        assert_eq!(empty_samples, 2);

        merge_fan_sample(&mut cache, &mut empty_samples, Vec::new());
        assert!(cache.is_empty());
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
    fn parse_automation_rules_accepts_supported_conditions_in_order() {
        let rules = parse_automation_rules(
            r#"[
                {"when":"on_battery","profile":"silent"},
                {"when":"on_ac","profile":"balanced"},
                {"when":"time:22-7","profile":"gaming"},
                {"when":"cpu_above:85","profile":"maximum"}
            ]"#,
        )
        .unwrap();
        assert_eq!(rules.len(), 4);
        assert_eq!(rules[2].when, "time:22-7");
        assert_eq!(rules[3].profile, Profile::Maximum);
    }

    #[test]
    fn parse_automation_rules_rejects_invalid_or_oversized_rules() {
        assert!(parse_automation_rules(r#"[{"when":"time:24-7","profile":"silent"}]"#).is_err());
        assert!(
            parse_automation_rules(r#"[{"when":"cpu_above:NaN","profile":"silent"}]"#).is_err()
        );
        let oversized = (0..33)
            .map(|_| serde_json::json!({"when":"on_ac","profile":"balanced"}))
            .collect::<Vec<_>>();
        assert!(parse_automation_rules(&serde_json::to_string(&oversized).unwrap()).is_err());
    }

    #[test]
    fn parse_imported_config_accepts_defaults_and_rules() {
        let config = parse_imported_config(
            r#"
profile = "performance"
rules = [{ when = "on_ac", profile = "maximum" }]
"#,
        )
        .unwrap();
        assert_eq!(config.profile, Profile::Performance);
        assert_eq!(config.rules.len(), 1);
        assert_eq!(config.interval_secs, 2);
    }

    #[test]
    fn parse_imported_config_rejects_invalid_control_settings() {
        assert!(parse_imported_config("interval_secs = 0").is_err());
        assert!(parse_imported_config(
            "rules = [{ when = \"cpu_above:NaN\", profile = \"silent\" }]"
        )
        .is_err());
    }

    #[test]
    fn settings_requests_keep_the_window_request_id_and_operation() {
        let request = parse_settings_request(
            r#"{"id":"window-1_request-2","operation":"rules","payload":"[]"}"#,
        )
        .unwrap();
        assert_eq!(request.id, "window-1_request-2");
        assert_eq!(request.operation, SettingsOperation::Rules);
        assert_eq!(request.payload, "[]");
        let imported = parse_settings_request(
            r#"{"id":"window-2_request-1","operation":"import_config","payload":"profile = 'silent'"}"#,
        )
        .unwrap();
        assert_eq!(imported.operation, SettingsOperation::ImportConfig);
    }

    #[test]
    fn settings_requests_reject_invalid_ids_and_unknown_operations() {
        for id in ["".to_string(), "wrong id".to_string(), "x".repeat(129)] {
            assert!(parse_settings_request(
                &serde_json::json!({"id": id, "operation": "rules", "payload": "[]"}).to_string()
            )
            .is_none());
        }
        assert!(parse_settings_request(
            r#"{"id":"window-1","operation":"run_shell","payload":"anything"}"#
        )
        .is_none());
        assert!(parse_settings_request("not JSON").is_none());
    }

    #[test]
    fn settings_reload_results_require_explicit_daemon_confirmation() {
        let confirmed = settings_saved_result(
            "rules saved (1)".into(),
            Some("ok reloaded (1 rules) (smc)"),
            true,
        );
        assert!(confirmed.ok);
        assert_eq!(confirmed.daemon_reload, "reloaded");
        for reply in ["error: permission denied", "ok legacy daemon", ""] {
            let failed = settings_saved_result("rules saved (1)".into(), Some(reply), true);
            assert!(
                failed.ok,
                "The disk save succeeded even when daemon reload failed"
            );
            assert_eq!(failed.daemon_reload, "failed");
            assert!(failed.message.contains("daemon reload failed"));
        }
        let deferred = settings_saved_result("rules saved (1)".into(), None, true);
        assert!(deferred.ok);
        assert_eq!(deferred.daemon_reload, "deferred");
        assert!(deferred.message.contains("daemon reload pending"));
        let windows = settings_saved_result("rules saved (1)".into(), None, false);
        assert_eq!(windows.daemon_reload, "not_applicable");
        assert_eq!(windows.message, "rules saved (1)");
    }

    #[test]
    fn invalid_settings_payload_returns_a_correlated_failure_without_saving() {
        let id = "unit-test-invalid-rules";
        let status = run_settings_request(
            &serde_json::json!({"id": id, "operation": "rules", "payload": "not JSON"}).to_string(),
        )
        .unwrap();
        assert!(status.starts_with("rules save failed:"));
        let results = SETTINGS_RESULTS.lock().unwrap();
        let result = results.iter().find(|result| result["id"] == id).unwrap();
        assert_eq!(result["operation"], "rules");
        assert_eq!(result["ok"], false);
        assert_eq!(result["daemon_reload"], "not_attempted");
        assert!(result.get("payload").is_none());
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
    fn ranged_history_clear_removes_pre_sleep_samples() {
        let mut h = RangedHistory::new();
        for value in 0..120 {
            h.push(value as f32);
        }
        assert!(!h.minute.is_empty());
        h.clear();
        assert!(h.minute.is_empty());
        assert!(h.hour.is_empty());
        assert!(h.day.is_empty());
        assert!(h.minute_acc.is_empty());
        assert!(h.hour_acc.is_empty());
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
        let sampled = now - TEMPERATURE_STALE_AFTER;
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
    fn temperature_refresh_slows_only_while_dashboards_are_closed() {
        assert_eq!(
            temperature_refresh_interval(true),
            TEMPERATURE_REFRESH_VISIBLE
        );
        assert_eq!(
            temperature_refresh_interval(false),
            TEMPERATURE_REFRESH_BACKGROUND
        );
        assert!(TEMPERATURE_REFRESH_BACKGROUND > TEMPERATURE_REFRESH_VISIBLE);
        assert!(TEMPERATURE_STALE_AFTER > TEMPERATURE_REFRESH_BACKGROUND * 2);
    }

    #[test]
    fn fan_refresh_is_fast_when_visible_and_stable_in_background() {
        assert_eq!(fan_refresh_interval(true), FAN_REFRESH_VISIBLE);
        assert_eq!(fan_refresh_interval(false), FAN_REFRESH_BACKGROUND);
        assert!(FAN_REFRESH_BACKGROUND > FAN_REFRESH_VISIBLE);
        assert!(FAN_STALE_AFTER > FAN_REFRESH_BACKGROUND * 2);
    }

    #[test]
    fn pause_recovery_requires_a_real_event_loop_gap() {
        let now = Instant::now();
        let mut clock = pause_clock::PauseClock::new(RESUME_RECOVERY_GAP);
        assert!(!clock.observe(now, None));
        assert!(!clock.observe(now + RESUME_RECOVERY_GAP - Duration::from_millis(1), None));
        assert!(clock.observe(
            now + RESUME_RECOVERY_GAP * 2 - Duration::from_millis(1),
            None
        ));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn native_suspend_clock_returns_monotonic_nanosecond_readings() {
        let before = suspend_inclusive_time().expect("supported Darwin continuous clock");
        std::thread::sleep(Duration::from_millis(2));
        let after = suspend_inclusive_time().expect("second continuous reading");
        let elapsed = after
            .checked_sub(before)
            .expect("clock must not go backwards");
        assert!(elapsed >= Duration::from_millis(1));
        assert!(elapsed < Duration::from_secs(30));
    }

    #[test]
    fn resume_recovery_plan_invalidates_cached_state_before_first_post_wake_sample() {
        assert_eq!(
            resume_recovery_plan(),
            ResumeRecoveryPlan {
                hide_popover: true,
                invalidate_hardware_caches: true,
                clear_dashboard_histories: true,
                discard_completed_reads: true,
                schedule_refreshes_immediately: true,
                clear_control_confirmation: true,
            }
        );
    }

    #[test]
    fn temperature_ui_marks_cached_readings_as_stale() {
        let html = dashboard_html(ResolvedLanguage::En, false);
        assert!(html.contains("d.temp_stale?' · '"));
        assert!(html.contains("t.stale?' stale':'"));
        assert!(html.contains("d.temp_stale?[]:d.temp_hist"));
        assert!(html.contains(".trow.stale"));
    }

    #[test]
    fn storage_mount_matching_prefers_the_disk_containing_the_user() {
        assert!(mount_match_score("/", "/Users/bonjin") > 0);
        assert!(mount_match_score("/Volumes/Data", "/Volumes/Data/work") > 1);
        assert_eq!(mount_match_score("/Volumes/Other", "/Users/bonjin"), 0);
    }

    #[test]
    fn windows_acpi_temperature_is_labeled_as_system_not_cpu() {
        let selected = SelectedTemperature {
            id: "system.acpi.thermal_zone.0".into(),
            value: 47.0,
            label_hint: Some("system"),
        };
        assert_eq!(
            display_temperature_source(ResolvedLanguage::En, Some(&selected)),
            "System thermal zone"
        );
        assert_eq!(
            display_temperature_source(ResolvedLanguage::Ko, Some(&selected)),
            "시스템 열 영역"
        );
    }
}
