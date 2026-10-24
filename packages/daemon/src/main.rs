//! `peterfand` — the PeterFan fan-control daemon.
//!
//! Applies a fan curve continuously: every interval it reads the representative
//! CPU temperature, evaluates the chosen profile's curve, and drives the fans
//! to the resulting duty. Two safety behaviors are built in:
//!
//! - **Critical-temperature override** — above `--critical` °C the fans are
//!   forced to 100%, regardless of the curve.
//! - **Restore on exit** — on `Ctrl-C`/`SIGTERM` (or a panic) the daemon hands
//!   the fans back to automatic (OS-managed) control before quitting, so it
//!   never leaves them stuck at a forced speed.
//!
//! Fan writes are privileged: run with `sudo peterfand`, or install it as a
//! LaunchDaemon (runs as root) — see `scripts/install-daemon-macos.sh`.
//! `peterfand --mock` exercises the whole loop against the simulated machine
//! without root.

use std::panic::AssertUnwindSafe;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use anyhow::{bail, Result};
use clap::Parser;
use serde::{Deserialize, Serialize};

use peterfan_core::config::{Config, RuleContext};
use peterfan_core::profile::Profile;
use peterfan_core::thermals::{
    representative_temperature_c, safety_temperature_c, valid_control_temperature_c,
};
use peterfan_core::{HardwareProvider, SystemMonitor};

/// Set by the signal handler; the control loop checks it and exits cleanly.
static STOP: AtomicBool = AtomicBool::new(false);
/// Set by the IPC handler whenever a command changes the fan-control mode
/// (auto/rules/profile/hold). The control loop's sleep checks this every
/// 200ms and wakes early so a "Max" click (say) is applied within a couple
/// hundred ms instead of waiting out the rest of the multi-second tick
/// interval — the interval is for periodic temperature re-evaluation, not
/// for how long a user-issued command should take to land.
static APPLY_NOW: AtomicBool = AtomicBool::new(false);
static CONTROL_THREAD: std::sync::LazyLock<Mutex<Option<thread::Thread>>> =
    std::sync::LazyLock::new(|| Mutex::new(None));
static FAN_WRITE_LOCK: std::sync::LazyLock<Mutex<()>> = std::sync::LazyLock::new(|| Mutex::new(()));
const FAN_WRITE_RETRY_BASE_SECS: u64 = 5;
const FAN_WRITE_RETRY_MAX_SECS: u64 = 60;
const FAN_DUTY_RAMP_UP_PER_SEC: u64 = 4;
const FAN_DUTY_RAMP_DOWN_PER_SEC: u64 = 3;
const FAN_READBACK_SETTLE_GRACE_MS: u64 = 10_000;
const FAN_READBACK_PROGRESS_GRACE_MS: u64 = 6_000;
const FAN_READBACK_MIN_PROGRESS_RPM: u32 = 50;
const FAN_READBACK_STALE_CONFIRMATIONS: u32 = 3;
const FAN_READBACK_RETRY_SECS: u64 = 15;
const SENSOR_STALE_MIN_MS: u64 = 6_000;

/// Move a requested fan duty toward its target at a bounded rate. The control
/// loop can observe a temperature spike in one sample, but the hardware does
/// not need to jump there in one write. Critical temperatures bypass the ramp
/// so the safety override remains immediate.
fn slew_fan_duty(current: u8, target: u8, elapsed_ms: u64, immediate: bool) -> u8 {
    if immediate || current == target {
        return target;
    }

    let rate_per_sec = if target > current {
        FAN_DUTY_RAMP_UP_PER_SEC
    } else {
        FAN_DUTY_RAMP_DOWN_PER_SEC
    };
    let max_delta = rate_per_sec
        .saturating_mul(elapsed_ms)
        .saturating_add(999)
        .checked_div(1_000)
        .unwrap_or(u64::MAX)
        .clamp(1, 100) as u8;

    if target > current {
        current.saturating_add(max_delta).min(target)
    } else {
        current.saturating_sub(max_delta).max(target)
    }
}

fn previous_fan_duty(state: &State, fan_id: &str, fallback: u8) -> u8 {
    state
        .fan_targets
        .get(fan_id)
        .copied()
        .or_else(|| {
            state
                .last_fans
                .iter()
                .find(|fan| fan.id == fan_id)
                .and_then(|fan| fan.duty_percent)
        })
        .unwrap_or(fallback)
}

// ── State persistence ────────────────────────────────────────────────────────

/// Serialized daemon state written to disk on every IPC change and read on
/// startup, so the user's last fan setting survives a reboot.
#[derive(Serialize, Deserialize, Default)]
struct SavedState {
    /// "auto" | "hold" | "profile" | "rules"
    mode: String,
    /// Set when mode = "hold".
    #[serde(skip_serializing_if = "Option::is_none")]
    hold_pct: Option<u8>,
    /// Last active profile name (remembered across all modes for "rules" resume).
    #[serde(skip_serializing_if = "Option::is_none")]
    profile: Option<String>,
    /// Per-fan manual overrides, restored on top of `mode` after a reboot.
    #[serde(default, skip_serializing_if = "std::collections::HashMap::is_empty")]
    fan_overrides: std::collections::HashMap<String, u8>,
}

fn state_file_path() -> PathBuf {
    // macOS LaunchDaemon convention; falls back to /tmp for other platforms.
    #[cfg(target_os = "macos")]
    {
        PathBuf::from("/Library/Application Support/peterfand/state.toml")
    }
    #[cfg(not(target_os = "macos"))]
    {
        PathBuf::from("/var/lib/peterfand/state.toml")
    }
}

#[cfg(unix)]
fn save_state(state: &State) {
    let saved = SavedState {
        mode: if state.auto {
            "auto".into()
        } else if state.held_duty.is_some() {
            "hold".into()
        } else if state.manual {
            "profile".into()
        } else {
            "rules".into()
        },
        hold_pct: state.held_duty,
        profile: Some(state.profile.as_str().to_string()),
        fan_overrides: state.fan_overrides.clone(),
    };
    if let Ok(s) = toml::to_string(&saved) {
        let path = state_file_path();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(&path, s);
    }
}

fn load_saved_state() -> Option<SavedState> {
    let bytes = std::fs::read_to_string(state_file_path()).ok()?;
    toml::from_str(&bytes).ok()
}

fn fresh_daemon_starts_in_auto(profile_was_explicit: bool) -> bool {
    !profile_was_explicit
}

#[cfg(target_os = "macos")]
fn macos_user_config_path(home: &std::path::Path) -> PathBuf {
    home.join("Library/Application Support/peterfan/config.toml")
}

#[cfg(target_os = "macos")]
fn logged_in_user_config_path() -> Option<PathBuf> {
    use std::ffi::CStr;
    use std::os::unix::fs::MetadataExt;

    let uid = std::fs::metadata("/dev/console").ok()?.uid();
    if uid == 0 {
        return None;
    }
    // SAFETY: getpwuid returns a process-owned passwd record. We copy pw_dir
    // into an owned PathBuf before leaving this block and check every pointer.
    let home = unsafe {
        let passwd = libc::getpwuid(uid);
        if passwd.is_null() || (*passwd).pw_dir.is_null() {
            return None;
        }
        CStr::from_ptr((*passwd).pw_dir)
            .to_string_lossy()
            .into_owned()
    };
    let home = PathBuf::from(home);
    home.is_absolute().then(|| macos_user_config_path(&home))
}

#[cfg(any(target_os = "macos", test))]
fn load_valid_config_file(path: &std::path::Path) -> Result<Config, String> {
    const MAX_CONFIG_BYTES: u64 = 256 * 1024;
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|error| format!("could not read config metadata: {error}"))?;
    if !metadata.file_type().is_file() {
        return Err("config must be a regular file".to_string());
    }
    if metadata.len() > MAX_CONFIG_BYTES {
        return Err("config is larger than 256 KiB".to_string());
    }
    let text =
        std::fs::read_to_string(path).map_err(|error| format!("could not read config: {error}"))?;
    let config = Config::from_toml(&text).map_err(|error| format!("invalid config: {error}"))?;
    config.validate_for_control()?;
    Ok(config)
}

/// A system LaunchDaemon has HOME=/var/root. On macOS, fan preferences belong
/// to the user at the console, so explicitly read that user's config. At the
/// login window (or on other platforms), retain the normal platform fallback.
fn load_daemon_config() -> Config {
    #[cfg(target_os = "macos")]
    if let Some(path) = logged_in_user_config_path() {
        match load_valid_config_file(&path) {
            Ok(config) => return config,
            Err(error) if path.exists() => {
                eprintln!("peterfand: user config rejected: {error}; using safe defaults");
                return Config::default();
            }
            Err(_) => {}
        }
    }

    let config = peterfan_platform::config::load();
    if let Err(error) = config.validate_for_control() {
        eprintln!("peterfand: daemon config rejected: {error}; using safe defaults");
        Config::default()
    } else {
        config
    }
}

// ────────────────────────────────────────────────────────────────────────────

/// Live control state, shared between the control loop and the IPC server.
#[derive(Clone)]
struct State {
    /// Active profile whose curve is applied (effective; reflects rules too).
    profile: Profile,
    /// When set, fans are held at this fixed duty % (overrides the curve).
    /// Cleared by `auto` / `rules` / `profile` commands.
    held_duty: Option<u8>,
    /// Per-fan manual overrides (fan id -> duty%), independent of the global
    /// mode above — e.g. pin just the left fan while the right one keeps
    /// following the curve. Cleared per-fan by `fanauto <id>`, or all at once
    /// by any of the global `auto`/`rules`/`profile` commands.
    fan_overrides: std::collections::HashMap<String, u8>,
    /// Stable allowlist discovered from the hardware at startup.
    known_fan_ids: Vec<String>,
    /// When true, fans are handed back to the OS (no curve applied).
    auto: bool,
    /// When true, the profile was set manually (IPC) and overrides automation
    /// rules until `rules`/`auto` is requested.
    manual: bool,
    /// Backend name (e.g. "macos", "mock") — surfaced in IPC replies so the UI
    /// can tell real control from a simulated daemon.
    #[cfg(unix)]
    backend: String,
    /// Live copy of the config, refreshed by `reload`. The base profile and
    /// automation rules are read from here each control-loop tick.
    config: peterfan_core::config::Config,
    /// Most recent temperature readings from the last control-loop tick.
    /// Used by the `temps` IPC command so the CLI can skip SMC init.
    last_temps: Vec<peterfan_core::types::TempSensor>,
    /// Wall-clock time of the last successful, trustworthy temperature sample.
    last_temps_sampled_at_unix_ms: Option<u64>,
    /// Most recent fan states from the last control-loop tick.
    last_fans: Vec<peterfan_core::types::Fan>,
    /// Most recent power draw in watts.
    last_power_w: Option<f32>,
    /// Runtime safety telemetry. This is intentionally not persisted: every
    /// daemon launch starts by proving that sensors and writes work again.
    control_health: ControlHealth,
    /// Monotonic request/apply revisions let clients distinguish an accepted
    /// mode change from one that has actually reached the hardware loop.
    control_revision: u64,
    applied_control_revision: u64,
    /// Fan-specific duty targets from the most recent successful hardware
    /// write. OS-managed fans are omitted because their target belongs to macOS.
    fan_targets: std::collections::HashMap<String, u8>,
    last_control_apply_unix_ms: Option<u64>,
    fan_readbacks: Vec<FanReadback>,
    fan_readback_trackers: std::collections::HashMap<String, FanReadbackTracker>,
}

#[derive(Clone, Debug)]
struct FanReadbackTracker {
    target_rpm: u32,
    tolerance_rpm: u32,
    started_ms: u64,
    last_progress_ms: u64,
    last_distance_rpm: u32,
    stale: bool,
}

#[derive(Clone, Debug, Serialize)]
struct FanReadback {
    id: String,
    label: String,
    current_rpm: Option<u32>,
    target_rpm: u32,
    tolerance_rpm: u32,
    status: String,
}

#[derive(Clone, Debug, Default, Serialize)]
struct ControlHealth {
    failsafe_active: bool,
    sensor_failure_count: u64,
    consecutive_sensor_failures: u32,
    fan_write_failure_count: u64,
    consecutive_fan_write_failures: u32,
    fan_readback_failure_count: u64,
    consecutive_fan_readback_failures: u32,
    last_fan_readback_ok_unix: Option<u64>,
    stale_fan_ids: Vec<String>,
    retry_after_unix: Option<u64>,
    last_sensor_ok_unix: Option<u64>,
    last_error: Option<String>,
}

#[derive(Parser)]
#[command(
    name = "peterfand",
    version,
    about = "PeterFan fan-control daemon — applies a fan curve with safety overrides."
)]
struct Cli {
    /// Use the simulated machine (no root needed; for testing).
    #[arg(long)]
    mock: bool,
    /// Profile whose curve to apply (default: from config, or balanced).
    #[arg(long)]
    profile: Option<String>,
    /// Seconds between curve updates (default: from config, or 2).
    #[arg(long)]
    interval: Option<u64>,
    /// Above this temperature (°C) the fans are forced to 100% (default: from config, or 90).
    #[arg(long)]
    critical: Option<f32>,
    /// Apply the curve once and exit (for testing).
    #[arg(long)]
    once: bool,
    /// Disable IPC for isolated process-lifecycle tests.
    #[arg(long, hide = true)]
    no_ipc: bool,
    /// Internal staged-update check. Never starts the daemon or loads settings.
    #[cfg(target_os = "macos")]
    #[arg(long, hide = true, requires = "reinstall_from_version")]
    validate_self_reinstall_version: Option<PathBuf>,
    #[cfg(target_os = "macos")]
    #[arg(long, hide = true, requires = "validate_self_reinstall_version")]
    reinstall_from_version: Option<String>,
}

fn main() {
    let cli = Cli::parse();
    #[cfg(target_os = "macos")]
    if let Some(candidate) = cli.validate_self_reinstall_version.as_deref() {
        if let Err(error) =
            verify_self_reinstall_version(candidate, cli.reinstall_from_version.as_deref().unwrap())
        {
            eprintln!("error: {error}");
            std::process::exit(1);
        }
        return;
    }
    if let Err(e) = run(cli) {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn run(cli: Cli) -> Result<()> {
    if cli.no_ipc && !cli.mock {
        bail!("--no-ipc is only available with --mock");
    }

    // Resolve settings: explicit flags win, otherwise fall back to the config
    // file, otherwise the built-in defaults.
    let cfg = load_daemon_config();

    let profile = match &cli.profile {
        Some(name) => {
            Profile::parse(name).ok_or_else(|| anyhow::anyhow!("unknown profile '{name}'"))?
        }
        None => cfg.profile,
    };
    let interval = cli.interval.unwrap_or(cfg.interval_secs).max(1);
    let critical = cli.critical.unwrap_or(cfg.critical_temp_c);

    let provider: Arc<dyn HardwareProvider> = if cli.mock {
        peterfan_platform::mock().into()
    } else {
        peterfan_platform::detect().into()
    };
    if !provider.capabilities().control_fans {
        bail!(
            "the '{}' backend cannot control fans on this machine",
            provider.name()
        );
    }

    let fan_ids: Vec<String> = provider
        .fans()?
        .into_iter()
        .filter(|f| f.controllable)
        .map(|f| f.id)
        .collect();
    if fan_ids.is_empty() {
        bail!("no controllable fans found");
    }

    // A SIGKILL cannot run cleanup code. launchd restarts this daemon through
    // KeepAlive, so every new process must first clear any forced SMC mode the
    // previous process may have left behind before restoring the saved policy.
    let startup_restore_errors = restore_fans_to_auto(provider.as_ref(), &fan_ids);
    if !startup_restore_errors.is_empty() {
        bail!(
            "could not establish OS automatic fan control at startup: {}",
            startup_restore_errors.join("; ")
        );
    }
    println!(
        "peterfand: reset {} fan(s) to automatic control at startup",
        fan_ids.len()
    );

    install_signal_handlers();

    // Automation rules only need battery state. Keep the daemon on the quick
    // monitor so its control cadence never scans processes, disks, or network
    // interfaces in the background.
    let mut monitor: Box<dyn SystemMonitor> = if cli.mock {
        peterfan_platform::mock_monitor()
    } else {
        peterfan_platform::quick_monitor()
    };

    let initial_state = {
        // Store CLI-resolved values back so the control loop always reads
        // from state.config (and reload() refreshes them from disk).
        let mut resolved_cfg = cfg.clone();
        resolved_cfg.interval_secs = interval;
        resolved_cfg.critical_temp_c = critical;

        let mut s = State {
            profile,
            held_duty: None,
            fan_overrides: std::collections::HashMap::new(),
            known_fan_ids: fan_ids.clone(),
            // A fresh daemon starts in the safest, least surprising mode:
            // macOS owns the fans. An explicit --profile or a persisted user
            // choice below opts into PeterFan's control loop.
            auto: fresh_daemon_starts_in_auto(cli.profile.is_some()),
            manual: false,
            #[cfg(unix)]
            backend: provider.name().to_string(),
            config: resolved_cfg,
            last_temps: Vec::new(),
            last_temps_sampled_at_unix_ms: None,
            last_fans: Vec::new(),
            last_power_w: None,
            control_health: ControlHealth::default(),
            control_revision: 1,
            applied_control_revision: 0,
            fan_targets: std::collections::HashMap::new(),
            last_control_apply_unix_ms: None,
            fan_readbacks: Vec::new(),
            fan_readback_trackers: std::collections::HashMap::new(),
        };
        // Restore the last user-chosen mode so a reboot doesn't reset fan settings.
        if let Some(saved) = load_saved_state() {
            match saved.mode.as_str() {
                "auto" => {
                    s.auto = true;
                }
                "hold" => {
                    if let Some(pct) = saved.hold_pct {
                        s.auto = false;
                        s.held_duty = Some(pct);
                        s.manual = true;
                    }
                }
                "profile" => {
                    if let Some(name) = &saved.profile {
                        if let Some(p) = Profile::parse(name) {
                            s.auto = false;
                            s.profile = p;
                            s.manual = true;
                        }
                    }
                }
                "rules" => {
                    s.auto = false;
                }
                _ => {} // Unknown state keeps the safe OS-auto default.
            }
            for (id, duty) in saved.fan_overrides {
                let _ = set_fan_override(&mut s, &id, duty);
            }
        }
        s
    };
    let restored_mode = if initial_state.auto {
        "auto".to_string()
    } else if let Some(d) = initial_state.held_duty {
        format!("hold:{d}%")
    } else if initial_state.manual {
        format!("profile:{}", initial_state.profile.as_str())
    } else {
        format!("rules:{}", initial_state.profile.as_str())
    };

    let shared = Arc::new(Mutex::new(initial_state));

    // IPC server (so the menu-bar app can switch profile / go auto without
    // root). Not started for one-shot runs. Unix-only for now (see
    // `spawn_ipc_server`) — on Windows the daemon still runs and applies its
    // curve, it just isn't remotely controllable yet.
    #[cfg(unix)]
    if !cli.once && !cli.no_ipc {
        spawn_ipc_server(
            Arc::clone(&shared),
            Arc::clone(&provider),
            fan_ids.clone(),
            cli.mock,
        );
    }

    println!(
        "peterfand: profile={} interval={interval}s critical={critical:.0}°C rules={} fans={} backend={} restored={}",
        profile.as_str(),
        cfg.rules.len(),
        fan_ids.len(),
        provider.name(),
        restored_mode
    );

    // Run the control loop, then ALWAYS restore automatic control — even on a
    // panic — so we never leave the fans forced.
    let loop_result = run_with_auto_restore(provider.as_ref(), &fan_ids, || {
        control_loop(
            provider.as_ref(),
            monitor.as_mut(),
            &fan_ids,
            cli.once,
            &shared,
        )
    });
    #[cfg(unix)]
    if !cli.no_ipc {
        for p in peterfan_platform::ipc::PATHS {
            let _ = std::fs::remove_file(p);
        }
    }
    loop_result
}

fn run_with_auto_restore<F>(
    provider: &dyn HardwareProvider,
    fan_ids: &[String],
    operation: F,
) -> Result<()>
where
    F: FnOnce() -> Result<()>,
{
    let operation_result = std::panic::catch_unwind(AssertUnwindSafe(operation));
    let restore_errors = restore_fans_to_auto(provider, fan_ids);
    if restore_errors.is_empty() {
        println!(
            "peterfand: restored {} fan(s) to automatic control",
            fan_ids.len()
        );
    } else {
        eprintln!(
            "peterfand: failed to restore automatic fan control: {}",
            restore_errors.join("; ")
        );
    }

    match (operation_result, restore_errors.is_empty()) {
        (Ok(result), true) => result,
        (Ok(Ok(())), false) => bail!(
            "control loop stopped but automatic fan restore failed: {}",
            restore_errors.join("; ")
        ),
        (Ok(Err(error)), false) => bail!(
            "{error}; automatic fan restore also failed: {}",
            restore_errors.join("; ")
        ),
        (Err(_), true) => bail!("control loop panicked (fans restored to auto)"),
        (Err(_), false) => bail!(
            "control loop panicked and automatic fan restore failed: {}",
            restore_errors.join("; ")
        ),
    }
}

fn control_temperatures(
    temps: &[peterfan_core::types::TempSensor],
) -> Result<(f32, f32), &'static str> {
    let safety = safety_temperature_c(temps).ok_or("no trustworthy temperature reading")?;
    let representative = representative_temperature_c(temps)
        .filter(|value| valid_control_temperature_c(*value))
        .unwrap_or(safety);
    Ok((representative, safety))
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn unix_now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u64::MAX as u128) as u64)
        .unwrap_or(0)
}

fn sensor_stale_after_ms(interval_secs: u64) -> u64 {
    interval_secs
        .max(1)
        .saturating_mul(3_000)
        .max(SENSOR_STALE_MIN_MS)
}

fn sensor_sample_freshness(
    sampled_at_unix_ms: Option<u64>,
    now_unix_ms: u64,
    interval_secs: u64,
) -> (Option<u64>, bool) {
    let age_ms = sampled_at_unix_ms.and_then(|sampled| now_unix_ms.checked_sub(sampled));
    let stale = age_ms
        .map(|age| age > sensor_stale_after_ms(interval_secs))
        .unwrap_or(true);
    (age_ms, stale)
}

fn fresh_control_temperatures(state: &State, now_ms: u64) -> Result<(f32, f32), &'static str> {
    if state.control_health.consecutive_sensor_failures != 0
        || sensor_sample_freshness(
            state.last_temps_sampled_at_unix_ms,
            now_ms,
            state.config.interval_secs,
        )
        .1
    {
        return Err("temperature sample is stale or failed");
    }
    control_temperatures(&state.last_temps)
}

fn fan_target_rpm(fan: &peterfan_core::types::Fan, duty_percent: u8) -> Option<(u32, u32)> {
    let (min, max) = (fan.min_rpm?, fan.max_rpm?);
    if max <= min {
        return None;
    }
    let span = max - min;
    let target = (min as f32 + (duty_percent.min(100) as f32 / 100.0) * span as f32).round() as u32;
    let tolerance = ((span as f32 * 0.04).round() as u32).max(150);
    Some((target, tolerance))
}

/// Compare live RPM with the most recently written targets. A fan gets a long
/// acceleration grace period, then must either arrive within tolerance or keep
/// making measurable progress toward the target. Returns true only after the
/// same stale condition has been observed repeatedly.
fn refresh_fan_readbacks(
    state: &mut State,
    fans: &[peterfan_core::types::Fan],
    now_ms: u64,
) -> bool {
    if state.fan_targets.is_empty() {
        state.fan_readbacks.clear();
        state.fan_readback_trackers.clear();
        state.control_health.consecutive_fan_readback_failures = 0;
        state.control_health.stale_fan_ids.clear();
        return false;
    }

    let targets = state.fan_targets.clone();
    state
        .fan_readback_trackers
        .retain(|id, _| targets.contains_key(id));

    let mut readbacks = Vec::new();
    let mut stale_ids = Vec::new();
    let mut newly_stale = 0u64;
    let mut all_settled = true;

    for (id, duty) in targets {
        let fan = fans.iter().find(|fan| fan.id == id);
        let resolved = fan.and_then(|fan| fan_target_rpm(fan, duty)).or_else(|| {
            state
                .fan_readback_trackers
                .get(&id)
                .map(|tracker| (tracker.target_rpm, tracker.tolerance_rpm))
        });
        let Some((target_rpm, tolerance_rpm)) = resolved else {
            all_settled = false;
            continue;
        };

        let current_rpm = fan.map(|fan| fan.rpm);
        let distance = current_rpm
            .map(|rpm| rpm.abs_diff(target_rpm))
            .unwrap_or(u32::MAX);
        let tracker = state
            .fan_readback_trackers
            .entry(id.clone())
            .or_insert_with(|| FanReadbackTracker {
                target_rpm,
                tolerance_rpm,
                started_ms: now_ms,
                last_progress_ms: now_ms,
                last_distance_rpm: distance,
                stale: false,
            });

        if tracker.target_rpm.abs_diff(target_rpm) > tracker.tolerance_rpm.max(tolerance_rpm) {
            tracker.started_ms = now_ms;
            tracker.last_progress_ms = now_ms;
            tracker.last_distance_rpm = distance;
            tracker.stale = false;
        }
        tracker.target_rpm = target_rpm;
        tracker.tolerance_rpm = tolerance_rpm;

        if distance <= tolerance_rpm
            || tracker.last_distance_rpm.saturating_sub(distance) >= FAN_READBACK_MIN_PROGRESS_RPM
        {
            tracker.last_progress_ms = now_ms;
        }

        let status = if distance <= tolerance_rpm {
            "settled"
        } else if now_ms.saturating_sub(tracker.started_ms) < FAN_READBACK_SETTLE_GRACE_MS
            || now_ms.saturating_sub(tracker.last_progress_ms) < FAN_READBACK_PROGRESS_GRACE_MS
        {
            all_settled = false;
            "adjusting"
        } else {
            all_settled = false;
            stale_ids.push(id.clone());
            if !tracker.stale {
                newly_stale = newly_stale.saturating_add(1);
            }
            "stale"
        };

        tracker.last_distance_rpm = distance;
        tracker.stale = status == "stale";
        readbacks.push(FanReadback {
            id: id.clone(),
            label: fan.map(|fan| fan.label.clone()).unwrap_or(id),
            current_rpm,
            target_rpm,
            tolerance_rpm,
            status: status.to_string(),
        });
    }

    readbacks.sort_by(|a, b| a.id.cmp(&b.id));
    stale_ids.sort();
    state.fan_readbacks = readbacks;
    state.control_health.fan_readback_failure_count = state
        .control_health
        .fan_readback_failure_count
        .saturating_add(newly_stale);
    state.control_health.stale_fan_ids = stale_ids;
    if state.control_health.stale_fan_ids.is_empty() {
        state.control_health.consecutive_fan_readback_failures = 0;
        if all_settled && !state.fan_readbacks.is_empty() {
            state.control_health.last_fan_readback_ok_unix = Some(now_ms / 1_000);
        }
    } else {
        state.control_health.consecutive_fan_readback_failures = state
            .control_health
            .consecutive_fan_readback_failures
            .saturating_add(1);
    }

    state.control_health.consecutive_fan_readback_failures >= FAN_READBACK_STALE_CONFIRMATIONS
}

fn fan_targets_materially_changed(
    previous: &std::collections::HashMap<String, u8>,
    next: &std::collections::HashMap<String, u8>,
) -> bool {
    previous.len() != next.len()
        || next.iter().any(|(id, duty)| {
            previous
                .get(id)
                .map(|old| old.abs_diff(*duty) >= 5)
                .unwrap_or(true)
        })
}

fn publish_successful_fan_targets(
    state: &mut State,
    targets: std::collections::HashMap<String, u8>,
) {
    if fan_targets_materially_changed(&state.fan_targets, &targets) {
        state.fan_readbacks.clear();
        state.fan_readback_trackers.clear();
        state.control_health.consecutive_fan_readback_failures = 0;
        state.control_health.stale_fan_ids.clear();
    }
    state.fan_targets = targets;
    state.last_control_apply_unix_ms = Some(unix_now_ms());
}

fn fan_write_retry_delay_secs(consecutive_failures: u32) -> u64 {
    let exponent = consecutive_failures.saturating_sub(1).min(6);
    FAN_WRITE_RETRY_BASE_SECS
        .saturating_mul(1u64 << exponent)
        .min(FAN_WRITE_RETRY_MAX_SECS)
}

fn restore_fans_to_auto(provider: &dyn HardwareProvider, fan_ids: &[String]) -> Vec<String> {
    fan_ids
        .iter()
        .filter_map(|id| {
            provider
                .set_fan_auto(id)
                .err()
                .map(|error| format!("{id}: {error}"))
        })
        .collect()
}

fn sleep_control_interval(interval: u64) {
    let deadline = std::time::Instant::now() + Duration::from_secs(interval);
    while !STOP.load(Ordering::Relaxed) && std::time::Instant::now() < deadline {
        if APPLY_NOW.swap(false, Ordering::Acquire) {
            return;
        }
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        thread::park_timeout(remaining.min(Duration::from_millis(200)));
    }
}

#[cfg(unix)]
fn request_apply_now() {
    APPLY_NOW.store(true, Ordering::Release);
    if let Some(control_thread) = CONTROL_THREAD
        .lock()
        .expect("control thread poisoned")
        .as_ref()
    {
        control_thread.unpark();
    }
}

#[cfg(unix)]
fn mark_control_requested(state: &mut State) {
    state.control_revision = state.control_revision.wrapping_add(1).max(1);
    request_apply_now();
}

fn publish_effective_rule_profile(
    shared: &Arc<Mutex<State>>,
    observed_revision: u64,
    profile: Profile,
) {
    let mut current = shared.lock().expect("state poisoned");
    if !current.manual && current.control_revision == observed_revision {
        current.profile = profile;
    }
}

fn selected_profile(state: &State, context: &RuleContext) -> Profile {
    if state.manual {
        state.profile
    } else {
        state
            .config
            .active_profile(context)
            .unwrap_or(state.config.profile)
    }
}

fn control_loop(
    provider: &dyn HardwareProvider,
    monitor: &mut dyn SystemMonitor,
    fan_ids: &[String],
    once: bool,
    shared: &Arc<Mutex<State>>,
) -> Result<()> {
    control_loop_while(provider, monitor, fan_ids, once, shared, || {
        !STOP.load(Ordering::Relaxed)
    })
}

fn control_loop_while(
    provider: &dyn HardwareProvider,
    monitor: &mut dyn SystemMonitor,
    fan_ids: &[String],
    once: bool,
    shared: &Arc<Mutex<State>>,
    mut keep_running: impl FnMut() -> bool,
) -> Result<()> {
    *CONTROL_THREAD.lock().expect("control thread poisoned") = Some(thread::current());
    let mut auto_applied = false;
    let mut was_critical = false;
    // Track last-logged duty/mode so we only log on changes (keeps the log lean).
    let mut last_duty: Option<u8> = None;
    let mut last_src = String::new();
    while keep_running() {
        monitor.refresh_quick();
        let state = shared.lock().expect("state poisoned").clone();
        // Read interval/critical from live config so `reload` takes effect immediately.
        let interval = state.config.interval_secs.max(1);
        let auto = state.auto;
        let critical = state.config.critical_temp_c;

        let temperature_result = provider.temperatures();
        let (temps, temperature_read_error) = match temperature_result {
            Ok(temps) => (temps, None),
            Err(error) => (Vec::new(), Some(error.to_string())),
        };
        match control_temperatures(&temps) {
            Ok(_) if temperature_read_error.is_none() => {
                let mut s = shared.lock().expect("state poisoned");
                s.last_temps = temps.clone();
                s.last_temps_sampled_at_unix_ms = Some(unix_now_ms());
                s.control_health.consecutive_sensor_failures = 0;
                s.control_health.last_sensor_ok_unix = Some(unix_now());
            }
            _ => {
                let reason = temperature_read_error
                    .unwrap_or_else(|| "no trustworthy temperature reading".to_string());
                // Invalidate immediately, before waiting for slow hardware:
                // a queued worker must not overtake this restore using the old
                // sample. In-flight writes recheck this health before success.
                let first_failure = {
                    let mut s = shared.lock().expect("state poisoned");
                    s.last_temps_sampled_at_unix_ms = None;
                    let first = !s.control_health.failsafe_active;
                    s.control_health.failsafe_active = true;
                    s.control_health.sensor_failure_count =
                        s.control_health.sensor_failure_count.saturating_add(1);
                    s.control_health.consecutive_sensor_failures = s
                        .control_health
                        .consecutive_sensor_failures
                        .saturating_add(1);
                    s.control_health.last_error =
                        Some(format!("sensor fail-safe: {reason}; restoring OS auto"));
                    s.fan_targets.clear();
                    s.fan_readbacks.clear();
                    s.fan_readback_trackers.clear();
                    first
                };
                // State is unlocked before taking the hardware lock. Any
                // in-flight manual write finishes before this final restore.
                let write_guard = FAN_WRITE_LOCK.lock().expect("fan write lock poisoned");
                let restore_errors = restore_fans_to_auto(provider, fan_ids);
                let detail = if restore_errors.is_empty() {
                    format!("sensor fail-safe: {reason}; fans returned to OS auto")
                } else {
                    format!(
                        "sensor fail-safe: {reason}; auto restore errors: {}",
                        restore_errors.join(", ")
                    )
                };
                {
                    let mut s = shared.lock().expect("state poisoned");
                    s.control_health.last_error = Some(detail.clone());
                    if !restore_errors.is_empty() {
                        s.control_health.fan_write_failure_count = s
                            .control_health
                            .fan_write_failure_count
                            .saturating_add(restore_errors.len() as u64);
                    }
                }
                drop(write_guard);
                if first_failure {
                    eprintln!("peterfand: {detail}");
                }
                if once {
                    break;
                }
                sleep_control_interval(interval);
                continue;
            }
        };
        // Do not delay failed-sensor invalidation behind unrelated hardware
        // reads. RPM and power sampling are needed only for a usable scan.
        let (fans_now, fan_read_error) = match provider.fans() {
            Ok(fans) => (fans, None),
            Err(error) => (Vec::new(), Some(error.to_string())),
        };
        let power_now = provider.power_watts();
        {
            let mut s = shared.lock().expect("state poisoned");
            s.last_fans = fans_now.clone();
            s.last_power_w = power_now;
        }

        // Sensor scans can block while the cached worker updates targets or
        // enters a cooldown. Re-read live state only after taking the write
        // lock, without holding State while waiting for that lock.
        let write_guard = FAN_WRITE_LOCK.lock().expect("fan write lock poisoned");
        let current = shared.lock().expect("state poisoned").clone();
        if current.control_revision != state.control_revision {
            continue;
        }
        let state = current;
        let Ok((representative_temp, safety_temp)) =
            fresh_control_temperatures(&state, unix_now_ms())
        else {
            if once {
                break;
            }
            // A long SMC write can age the just-read sample while we wait.
            // Release the guard and scan again before resuming control.
            continue;
        };

        // Keep sampling sensors during cooldown, but do not repeat the prior
        // fail-safe's restore, diagnostics, counters, or retry deadline.
        if state
            .control_health
            .retry_after_unix
            .is_some_and(|retry_after| unix_now() < retry_after)
        {
            drop(write_guard);
            if once {
                break;
            }
            sleep_control_interval(interval);
            continue;
        }

        let readback_failure = {
            let mut current = shared.lock().expect("state poisoned");
            if let Some(error) = fan_read_error.filter(|_| !current.fan_targets.is_empty()) {
                // Missing RPM samples cannot establish a mechanically stalled
                // fan. Stop manual control safely and report the read failure.
                current.control_health.fan_readback_failure_count = current
                    .control_health
                    .fan_readback_failure_count
                    .saturating_add(1);
                current.control_health.consecutive_fan_readback_failures = current
                    .control_health
                    .consecutive_fan_readback_failures
                    .saturating_add(1);
                current.control_health.stale_fan_ids.clear();
                current.fan_readbacks.clear();
                Some(format!("fan RPM readback failed: {error}"))
            } else if refresh_fan_readbacks(&mut current, &fans_now, unix_now_ms()) {
                Some(format!(
                    "fan RPM readback stalled for {}",
                    current.control_health.stale_fan_ids.join(", ")
                ))
            } else {
                None
            }
        };
        if let Some(reason) = readback_failure {
            let restore_errors = restore_fans_to_auto(provider, fan_ids);
            let detail = if restore_errors.is_empty() {
                format!("{reason}; fans returned to OS auto")
            } else {
                format!(
                    "{reason}; auto restore errors: {}",
                    restore_errors.join(", ")
                )
            };
            {
                let mut current = shared.lock().expect("state poisoned");
                current.control_health.failsafe_active = true;
                current.control_health.retry_after_unix =
                    Some(unix_now().saturating_add(FAN_READBACK_RETRY_SECS));
                current.control_health.last_error = Some(detail.clone());
                current.fan_targets.clear();
                current.fan_readback_trackers.clear();
            }
            eprintln!("peterfand: {detail}");
            drop(write_guard);
            if once {
                break;
            }
            sleep_control_interval(interval);
            continue;
        }

        let mut write_errors = Vec::new();
        let mut requested_targets = std::collections::HashMap::new();
        let now_ms = unix_now_ms();
        let elapsed_ms = state
            .last_control_apply_unix_ms
            .map(|last| now_ms.saturating_sub(last))
            .unwrap_or(0);

        if auto {
            // Per-fan overrides still apply on top of the global "auto" mode
            // (e.g. pin one fan manually while the rest follow the OS). An
            // override never survives a critical temperature — `effective_duty`
            // forces the fan to 100% exactly like the non-auto branch below.
            let critical_now = safety_temp >= critical;
            for id in fan_ids {
                let has_override = state.fan_overrides.contains_key(id);
                let result = if has_override {
                    let target = effective_duty(&state.fan_overrides, id, 100, critical_now);
                    let current = previous_fan_duty(&state, id, target);
                    let effective = slew_fan_duty(current, target, elapsed_ms, critical_now);
                    requested_targets.insert(id.clone(), effective);
                    provider.set_fan_duty(id, effective)
                } else {
                    provider.set_fan_auto(id)
                };
                if let Err(e) = result {
                    write_errors.push(format!("{id}: {e}"));
                }
            }
            if !auto_applied {
                println!("peterfand: auto (OS-managed)");
                auto_applied = true;
            }
        } else {
            auto_applied = false;

            // Choose the profile: a manual (IPC) choice wins; otherwise the
            // first matching automation rule; otherwise the base profile.
            let on_ac = match monitor.battery() {
                Some(b) => matches!(b.state.as_str(), "charging" | "full"),
                None => true, // no battery → treat as AC (desktop)
            };
            let ctx = RuleContext {
                on_ac,
                cpu_temp_c: representative_temp,
                hour: local_hour(),
            };
            let profile = selected_profile(&state, &ctx);
            // Reflect a rule-selected profile only if no newer command arrived
            // while the slow sensor scan was in progress. Manual profiles are
            // already stored by the command handler and must never be replaced
            // by an older control-loop snapshot.
            if !state.manual {
                publish_effective_rule_profile(shared, state.control_revision, profile);
            }

            let (duty, why): (u8, String) = if safety_temp >= critical {
                (100, "CRITICAL".into())
            } else if let Some(d) = state.held_duty {
                (d, format!("hold:{d}%"))
            } else {
                // Use config.curve_for() so Profile::Custom resolves to the user-defined curve.
                (
                    state.config.curve_for(profile).duty_at(representative_temp),
                    profile.as_str().into(),
                )
            };
            let critical_now = safety_temp >= critical;
            for id in fan_ids {
                let target = effective_duty(&state.fan_overrides, id, duty, critical_now);
                let current = previous_fan_duty(&state, id, target);
                let effective = slew_fan_duty(current, target, elapsed_ms, critical_now);
                requested_targets.insert(id.clone(), effective);
                if let Err(e) = provider.set_fan_duty(id, effective) {
                    write_errors.push(format!("{id}: {e}"));
                }
            }
            let src = if state.held_duty.is_some() {
                "hold"
            } else if state.manual {
                "manual"
            } else {
                "auto-rule"
            };
            // Only log when duty or mode actually changes (avoids flooding the log).
            if last_duty != Some(duty) || last_src != src {
                println!(
                    "peterfand: avg {representative_temp:.0}°C / safety {safety_temp:.0}°C -> {duty}% ({why}) [{src} ac={on_ac}]"
                );
                last_duty = Some(duty);
                last_src = src.to_string();
            }

            // Edge-triggered critical-temperature alert (with hysteresis).
            if safety_temp >= critical && !was_critical {
                notify(
                    "PeterFan — critical temperature",
                    &format!("{safety_temp:.0}°C ≥ {critical:.0}°C · fans forced to 100%"),
                );
                was_critical = true;
            } else if safety_temp < critical - 5.0 && was_critical {
                notify(
                    "PeterFan",
                    &format!("Temperature back to normal ({safety_temp:.0}°C)"),
                );
                was_critical = false;
            }
        }

        if write_errors.is_empty() {
            let recovered = state.control_health.failsafe_active;
            let mut s = shared.lock().expect("state poisoned");
            s.control_health.failsafe_active = false;
            s.control_health.consecutive_fan_write_failures = 0;
            s.control_health.retry_after_unix = None;
            s.control_health.last_error = None;
            s.applied_control_revision = state.control_revision;
            publish_successful_fan_targets(&mut s, requested_targets);
            drop(s);
            if recovered {
                println!("peterfand: control recovered; resumed requested mode");
            }
        } else {
            let restore_errors = restore_fans_to_auto(provider, fan_ids);
            let detail = if restore_errors.is_empty() {
                format!(
                    "fan write fail-safe: {}; fans returned to OS auto",
                    write_errors.join(", ")
                )
            } else {
                format!(
                    "fan write fail-safe: {}; auto restore errors: {}",
                    write_errors.join(", "),
                    restore_errors.join(", ")
                )
            };
            let first_failure = !state.control_health.failsafe_active;
            let consecutive = state
                .control_health
                .consecutive_fan_write_failures
                .saturating_add(1);
            let retry_after = unix_now().saturating_add(fan_write_retry_delay_secs(consecutive));
            {
                let mut s = shared.lock().expect("state poisoned");
                s.control_health.failsafe_active = true;
                s.control_health.fan_write_failure_count = s
                    .control_health
                    .fan_write_failure_count
                    .saturating_add((write_errors.len() + restore_errors.len()) as u64);
                s.control_health.consecutive_fan_write_failures = consecutive;
                s.control_health.retry_after_unix = Some(retry_after);
                s.control_health.last_error = Some(detail.clone());
                s.fan_targets.clear();
            }
            if first_failure {
                eprintln!("peterfand: {detail}");
            }
        }

        drop(write_guard);
        if once {
            break;
        }
        // Sleep in small slices so a signal stops us promptly, and so a
        // freshly-issued command (APPLY_NOW) wakes us well before the rest
        // of a multi-second interval elapses.
        sleep_control_interval(interval);
    }
    Ok(())
}

#[cfg(unix)]
fn apply_cached_control_request(
    provider: &dyn HardwareProvider,
    fan_ids: &[String],
    shared: &Arc<Mutex<State>>,
) {
    // A queued wakeup represents the latest intent, not a saved State clone.
    // Taking this lock first also observes any intervening fail-safe restore.
    let _write_guard = FAN_WRITE_LOCK.lock().expect("fan write lock poisoned");
    let state = shared.lock().expect("state poisoned").clone();
    if !state.auto && !state.manual {
        return;
    }
    if state
        .control_health
        .retry_after_unix
        .is_some_and(|retry_after| unix_now() < retry_after)
    {
        return;
    }

    let needs_temperature = !state.auto || !state.fan_overrides.is_empty();
    let control_temps = fresh_control_temperatures(&state, unix_now_ms());
    if needs_temperature && control_temps.is_err() {
        return;
    }
    let (representative_temp, safety_temp) = if needs_temperature {
        control_temps.expect("fresh sensor sample checked")
    } else {
        (0.0, 0.0)
    };
    let critical_now = needs_temperature && safety_temp >= state.config.critical_temp_c;
    let duty = state.held_duty.unwrap_or_else(|| {
        state
            .config
            .curve_for(state.profile)
            .duty_at(representative_temp)
    });

    if shared.lock().expect("state poisoned").control_revision != state.control_revision {
        return;
    }

    let mut write_errors = Vec::new();
    let mut requested_targets = std::collections::HashMap::new();
    let now_ms = unix_now_ms();
    let elapsed_ms = state
        .last_control_apply_unix_ms
        .map(|last| now_ms.saturating_sub(last))
        .unwrap_or(0);
    for id in fan_ids {
        let result = if state.auto && !state.fan_overrides.contains_key(id) {
            provider.set_fan_auto(id)
        } else {
            let target = effective_duty(&state.fan_overrides, id, duty, critical_now);
            let current = previous_fan_duty(&state, id, target);
            let effective = slew_fan_duty(current, target, elapsed_ms, critical_now);
            requested_targets.insert(id.clone(), effective);
            provider.set_fan_duty(id, effective)
        };
        if let Err(error) = result {
            write_errors.push(format!("{id}: {error}"));
        }
    }

    let mut current = shared.lock().expect("state poisoned");
    if current.control_revision != state.control_revision {
        return;
    }
    if write_errors.is_empty() {
        let fresh_sensors = fresh_control_temperatures(&current, unix_now_ms()).is_ok();
        if needs_temperature && !fresh_sensors {
            return;
        }
        // Auto remains available without a usable sensor, but writing Auto is
        // not evidence that a sensor failure has recovered.
        if fresh_sensors {
            current.control_health.failsafe_active = false;
            current.control_health.consecutive_fan_write_failures = 0;
            current.control_health.retry_after_unix = None;
            current.control_health.last_error = None;
        }
        current.applied_control_revision = state.control_revision;
        publish_successful_fan_targets(&mut current, requested_targets);
    } else {
        drop(current);
        let restore_errors = restore_fans_to_auto(provider, fan_ids);
        let detail = if restore_errors.is_empty() {
            format!(
                "fan write fail-safe: {}; fans returned to OS auto",
                write_errors.join(", ")
            )
        } else {
            format!(
                "fan write fail-safe: {}; auto restore errors: {}",
                write_errors.join(", "),
                restore_errors.join(", ")
            )
        };
        let mut current = shared.lock().expect("state poisoned");
        current.control_health.failsafe_active = true;
        current.control_health.fan_write_failure_count = current
            .control_health
            .fan_write_failure_count
            .saturating_add(write_errors.len() as u64);
        current.control_health.consecutive_fan_write_failures = current
            .control_health
            .consecutive_fan_write_failures
            .saturating_add(1);
        current.control_health.retry_after_unix = Some(
            unix_now()
                + fan_write_retry_delay_secs(current.control_health.consecutive_fan_write_failures),
        );
        current.control_health.last_error = Some(detail);
        current.fan_targets.clear();
    }
}

const MAX_FAN_ID_BYTES: usize = 128;
const MAX_FAN_OVERRIDES: usize = 32;

fn set_fan_override(state: &mut State, id: &str, percent: u8) -> Result<(), &'static str> {
    if id.is_empty()
        || id.len() > MAX_FAN_ID_BYTES
        || !state.known_fan_ids.iter().any(|known| known == id)
    {
        return Err("error: unknown or invalid fan id");
    }
    if !state.fan_overrides.contains_key(id) && state.fan_overrides.len() >= MAX_FAN_OVERRIDES {
        return Err("error: too many fan overrides");
    }
    state.fan_overrides.insert(id.to_string(), percent.min(100));
    Ok(())
}

#[cfg(any(unix, test))]
fn ipc_peer_allowed(peer: u32, daemon: u32, console: Option<u32>, mock: bool) -> bool {
    peer == 0
        || console.is_some_and(|uid| uid != 0 && uid == peer)
        || (mock && daemon != 0 && peer == daemon)
}

#[cfg(target_os = "macos")]
fn ipc_peer_uid(stream: &std::os::unix::net::UnixStream) -> std::io::Result<u32> {
    use std::os::fd::AsRawFd;
    let mut uid = 0;
    let mut gid = 0;
    // SAFETY: a live UnixStream supplies the descriptor; both output pointers are valid.
    if unsafe { libc::getpeereid(stream.as_raw_fd(), &mut uid, &mut gid) } == 0 {
        Ok(uid)
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn ipc_peer_uid(stream: &std::os::unix::net::UnixStream) -> std::io::Result<u32> {
    use std::os::fd::AsRawFd;
    let mut credentials: libc::ucred = unsafe { std::mem::zeroed() };
    let mut length = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    // SAFETY: credentials and length point to correctly sized writable storage.
    let result = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            &mut credentials as *mut _ as *mut libc::c_void,
            &mut length,
        )
    };
    if result == 0 && length as usize == std::mem::size_of::<libc::ucred>() {
        Ok(credentials.uid)
    } else {
        Err(std::io::Error::other("could not authenticate IPC peer"))
    }
}

#[cfg(all(
    unix,
    not(any(target_os = "macos", target_os = "linux", target_os = "android"))
))]
fn ipc_peer_uid(_stream: &std::os::unix::net::UnixStream) -> std::io::Result<u32> {
    Err(std::io::Error::other(
        "IPC peer authentication is unsupported",
    ))
}

#[cfg(unix)]
fn ipc_console_uid() -> Option<u32> {
    #[cfg(target_os = "macos")]
    {
        use std::os::unix::fs::MetadataExt;
        std::fs::metadata("/dev/console")
            .ok()
            .map(|metadata| metadata.uid())
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

#[cfg(unix)]
const MAX_IPC_REQUEST_BYTES: usize = 4096;
#[cfg(unix)]
const IPC_IO_TIMEOUT: Duration = Duration::from_secs(1);

#[cfg(unix)]
fn valid_ipc_request_arity(line: &str) -> bool {
    let mut words = line.split_whitespace();
    let expected = match words.next() {
        Some(
            "ping" | "version" | "auto" | "rules" | "status" | "temps" | "config-source" | "reload"
            | "stop",
        ) => 0,
        Some("profile" | "hold" | "fanauto" | "reinstall-fan-control") => 1,
        Some("fanhold") => 2,
        _ => return false,
    };
    words.count() == expected
}

#[cfg(unix)]
fn read_ipc_request(
    stream: &mut std::os::unix::net::UnixStream,
    timeout: Duration,
) -> std::io::Result<String> {
    use std::io::{Error, ErrorKind, Read};
    let deadline = std::time::Instant::now() + timeout;
    let mut bytes = Vec::new();
    loop {
        let remaining = deadline
            .checked_duration_since(std::time::Instant::now())
            .filter(|duration| !duration.is_zero())
            .ok_or_else(|| Error::new(ErrorKind::TimedOut, "IPC request timed out"))?;
        stream.set_read_timeout(Some(remaining))?;
        let mut chunk = [0; 256];
        let capacity = chunk.len().min(MAX_IPC_REQUEST_BYTES - bytes.len());
        let count = stream.read(&mut chunk[..capacity])?;
        if count == 0 {
            return Err(Error::new(
                ErrorKind::UnexpectedEof,
                "incomplete IPC request",
            ));
        }
        bytes.extend_from_slice(&chunk[..count]);
        if let Some(end) = bytes.iter().position(|byte| *byte == b'\n') {
            if end + 1 != bytes.len() {
                return Err(Error::new(ErrorKind::InvalidData, "multiple IPC requests"));
            }
            bytes.pop();
            if bytes.last() == Some(&b'\r') {
                bytes.pop();
            }
            break;
        }
        if bytes.len() == MAX_IPC_REQUEST_BYTES {
            return Err(Error::new(ErrorKind::InvalidData, "IPC request too long"));
        }
    }
    let line = String::from_utf8(bytes)
        .map_err(|_| Error::new(ErrorKind::InvalidData, "IPC request is not UTF-8"))?;
    if line.chars().any(char::is_control) || !valid_ipc_request_arity(&line) {
        return Err(Error::new(ErrorKind::InvalidData, "invalid IPC request"));
    }
    Ok(line)
}

/// One hardware worker plus one pending wakeup bounds command-burst resources.
/// A full channel already promises another pass over the latest shared State.
#[cfg(unix)]
struct ControlApplyWorker {
    sender: Option<std::sync::mpsc::SyncSender<()>>,
    thread: Option<thread::JoinHandle<()>>,
}

#[cfg(unix)]
impl ControlApplyWorker {
    fn spawn(apply: impl Fn() + Send + 'static) -> std::io::Result<Self> {
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        let thread = thread::Builder::new()
            .name("fan-control-apply".into())
            .spawn(move || {
                while receiver.recv().is_ok() {
                    apply();
                }
            })?;
        Ok(Self {
            sender: Some(sender),
            thread: Some(thread),
        })
    }

    fn request(&self) {
        if let Some(sender) = &self.sender {
            let _ = sender.try_send(());
        }
    }
}

#[cfg(unix)]
impl Drop for ControlApplyWorker {
    fn drop(&mut self) {
        self.sender.take();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[cfg(unix)]
fn spawn_ipc_server(
    shared: Arc<Mutex<State>>,
    provider: Arc<dyn HardwareProvider>,
    fan_ids: Vec<String>,
    mock: bool,
) {
    use std::io::Write;

    let (listener, path) = match peterfan_platform::ipc::bind_listener() {
        Ok(x) => x,
        Err(e) => {
            eprintln!("peterfand: IPC disabled ({e})");
            return;
        }
    };
    println!("peterfand: listening on {}", path.display());

    std::thread::spawn(move || {
        let worker_provider = Arc::clone(&provider);
        let worker_shared = Arc::clone(&shared);
        let worker = match ControlApplyWorker::spawn(move || {
            apply_cached_control_request(worker_provider.as_ref(), &fan_ids, &worker_shared)
        }) {
            Ok(worker) => Some(worker),
            Err(error) => {
                // The periodic loop is still woken by every accepted command.
                eprintln!("peterfand: cached control worker unavailable ({error})");
                None
            }
        };
        for conn in listener.incoming() {
            let Ok(mut stream) = conn else { continue };
            let Ok(peer_uid) = ipc_peer_uid(&stream) else {
                continue;
            };
            // Authenticate before reading even one byte from an untrusted client.
            if !ipc_peer_allowed(
                peer_uid,
                unsafe { libc::geteuid() },
                ipc_console_uid(),
                mock,
            ) {
                continue;
            }
            if stream.set_write_timeout(Some(IPC_IO_TIMEOUT)).is_err() {
                continue;
            }
            let Ok(line) = read_ipc_request(&mut stream, IPC_IO_TIMEOUT) else {
                continue;
            };
            let revision_before = shared.lock().expect("state poisoned").control_revision;
            let reply = handle_command(line.trim(), &shared);
            let changed =
                shared.lock().expect("state poisoned").control_revision != revision_before;
            // A first Apple Silicon manual-mode unlock can take several seconds.
            // Acknowledge the accepted command first, then perform the SMC write
            // in a worker while clients confirm it through the apply revision.
            let _ = writeln!(stream, "{reply}");
            if changed {
                if let Some(worker) = &worker {
                    worker.request();
                }
            }
        }
    });
}

#[cfg(target_os = "macos")]
fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

#[cfg(target_os = "macos")]
fn validate_self_reinstall_source(path: &str) -> Result<PathBuf, String> {
    let canonical = std::fs::canonicalize(path).map_err(|e| e.to_string())?;
    if canonical.as_path()
        != std::path::Path::new(peterfan_platform::daemon_install::APP_BUNDLE_DAEMON_BIN)
    {
        return Err(format!(
            "reinstall source must be {}",
            peterfan_platform::daemon_install::APP_BUNDLE_DAEMON_BIN
        ));
    }
    if !canonical.is_file() {
        return Err("reinstall source is not a file".to_string());
    }
    Ok(canonical)
}

#[cfg(target_os = "macos")]
const DAEMON_CODE_REQUIREMENT: &str = "identifier peterfand and anchor apple generic and certificate 1[field.1.2.840.113635.100.6.2.6] exists and certificate leaf[field.1.2.840.113635.100.6.1.13] exists and certificate leaf[subject.OU] = \"N99FMBQ662\"";

#[cfg(target_os = "macos")]
fn code_requirement_argument(requirement: &str) -> String {
    // codesign interprets unprefixed -R arguments as requirement file paths.
    format!("={requirement}")
}

#[cfg(any(target_os = "macos", test))]
fn self_reinstall_version_allowed(candidate: &str, running: &str) -> bool {
    !peterfan_platform::daemon_update_required(candidate)
        && !peterfan_platform::daemon_update_required(running)
        && !peterfan_platform::updater::is_newer(candidate, running)
}

#[cfg(target_os = "macos")]
fn bounded_command_output(
    command: &mut std::process::Command,
    timeout: Duration,
) -> Result<Vec<u8>, String> {
    use std::io::Read;
    use std::os::fd::AsRawFd;
    use std::process::Stdio;
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| error.to_string())?;
    let mut output = child.stdout.take().unwrap();
    let result = (|| {
        // A nonblocking pipe keeps both output growth and the entire probe bounded,
        // even if a signed candidate hangs or keeps its output pipe open.
        let descriptor = output.as_raw_fd();
        let flags = unsafe { libc::fcntl(descriptor, libc::F_GETFL) };
        if flags < 0
            || unsafe { libc::fcntl(descriptor, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0
        {
            return Err("could not bound helper verification output".into());
        }
        let deadline = std::time::Instant::now() + timeout;
        let mut bytes = Vec::new();
        loop {
            let status = child.try_wait().map_err(|error| error.to_string())?;
            let mut chunk = [0; 512];
            loop {
                match output.read(&mut chunk) {
                    Ok(0) => break,
                    Ok(count) => {
                        if bytes.len() + count > 4096 {
                            return Err("helper verification output exceeded limit".into());
                        }
                        bytes.extend_from_slice(&chunk[..count]);
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                    Err(error) => return Err(error.to_string()),
                }
            }
            if let Some(status) = status {
                if status.success() {
                    return Ok(bytes);
                }
                return Err("helper verification command failed".into());
            }
            if std::time::Instant::now() >= deadline {
                return Err("helper verification timed out".into());
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    })();
    if result.is_err() {
        let _ = child.kill();
    }
    let _ = child.wait();
    result
}

#[cfg(target_os = "macos")]
fn verify_self_reinstall_version(candidate: &std::path::Path, running: &str) -> Result<(), String> {
    // This internal command also verifies trust itself before executing a candidate.
    // Its normal caller supplies an already-verified file in a root-only stage.
    bounded_command_output(
        std::process::Command::new("/usr/bin/codesign")
            .args(["--verify", "--strict", "-R"])
            .arg(code_requirement_argument(DAEMON_CODE_REQUIREMENT))
            .arg(candidate),
        Duration::from_secs(5),
    )?;
    let output = bounded_command_output(
        std::process::Command::new(candidate).arg("--version"),
        Duration::from_secs(2),
    )?;
    let output = std::str::from_utf8(&output).map_err(|_| "invalid helper version output")?;
    let candidate_version = output
        .trim()
        .strip_prefix("peterfand ")
        .ok_or("invalid helper version output")?;
    if self_reinstall_version_allowed(candidate_version, running) {
        Ok(())
    } else {
        Err("helper replacement would downgrade security or the running version".into())
    }
}

#[cfg(target_os = "macos")]
fn self_reinstall_script(
    source: &std::path::Path,
    destination: &std::path::Path,
    stage_template: &std::path::Path,
    version_verifier: &std::path::Path,
    running: &str,
) -> String {
    let source = shell_quote(&source.to_string_lossy());
    let destination = shell_quote(&destination.to_string_lossy());
    let stage_template = shell_quote(&stage_template.to_string_lossy());
    let requirement = shell_quote(&code_requirement_argument(DAEMON_CODE_REQUIREMENT));
    let version_verifier = shell_quote(&version_verifier.to_string_lossy());
    let running = shell_quote(running);
    let label = peterfan_platform::daemon_install::DAEMON_LABEL;
    format!(
        r#"set -e
stage=$(/usr/bin/mktemp -d {stage_template})
trap '/bin/rm -rf "$stage"' EXIT
trap 'exit 1' HUP INT TERM
/bin/chmod 700 "$stage"
/usr/bin/install -m 500 {source} "$stage/peterfand"
/usr/bin/codesign --verify --strict --verbose=2 -R {requirement} "$stage/peterfand"
{version_verifier} --validate-self-reinstall-version "$stage/peterfand" --reinstall-from-version {running}
(
    trap '/bin/rm -rf "$stage"' EXIT
    trap 'exit 1' HUP INT TERM
    /bin/sleep 0.3
    /usr/bin/install -m 755 "$stage/peterfand" {destination}
    /bin/rm -rf "$stage"
    trap - EXIT HUP INT TERM
    /bin/launchctl kickstart -k system/{label}
) </dev/null >/dev/null 2>&1 &
trap - EXIT HUP INT TERM
"#
    )
}

#[cfg(target_os = "macos")]
fn start_self_reinstall(path: &str) -> Result<String, String> {
    if unsafe { libc::geteuid() } != 0 {
        return Err("error: daemon self-reinstall requires a root daemon".into());
    }
    let source = validate_self_reinstall_source(path)?;
    // Neither the directory nor its staged bytes are writable by IPC callers.
    // Copy first, then verify and install those exact bytes rather than reopening
    // the user-writable app bundle after its signature has been checked.
    let script = self_reinstall_script(
        &source,
        std::path::Path::new(peterfan_platform::daemon_install::DAEMON_BIN),
        std::path::Path::new("/private/var/root/peterfan-self-reinstall.XXXXXXXX"),
        std::path::Path::new(peterfan_platform::daemon_install::DAEMON_BIN),
        env!("CARGO_PKG_VERSION"),
    );
    let status = std::process::Command::new("/bin/sh")
        .arg("-c")
        .arg(script)
        .status()
        .map_err(|e| e.to_string())?;
    if status.success() {
        Ok("ok reinstalling fan control".to_string())
    } else {
        Err("error: reinstall source is not a trusted PeterFan release".to_string())
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
fn start_self_reinstall(_path: &str) -> Result<String, String> {
    Err("error: fan-control reinstall is only available on macOS".to_string())
}

#[cfg(unix)]
fn handle_command(line: &str, shared: &Arc<Mutex<State>>) -> String {
    if !valid_ipc_request_arity(line) {
        return "error: invalid command or arguments".into();
    }
    let backend = shared.lock().expect("state poisoned").backend.clone();
    let mut parts = line.split_whitespace();
    match parts.next() {
        Some("ping") => format!("ok peterfand ({backend})"),
        Some("version") => format!("ok peterfand {}", env!("CARGO_PKG_VERSION")),
        Some("auto") => {
            let mut s = shared.lock().expect("state poisoned");
            s.auto = true;
            s.held_duty = None;
            s.fan_overrides.clear();
            save_state(&s);
            mark_control_requested(&mut s);
            format!("ok auto ({backend})")
        }
        // Hand control back to the automation rules (clear manual override).
        Some("rules") => {
            let mut s = shared.lock().expect("state poisoned");
            s.manual = false;
            s.auto = false;
            s.held_duty = None;
            s.fan_overrides.clear();
            save_state(&s);
            mark_control_requested(&mut s);
            format!("ok rules ({backend})")
        }
        Some("profile") => match parts.next().and_then(Profile::parse) {
            Some(p) => {
                let mut s = shared.lock().expect("state poisoned");
                s.profile = p;
                s.auto = false;
                s.held_duty = None;
                s.manual = true;
                s.fan_overrides.clear();
                save_state(&s);
                mark_control_requested(&mut s);
                format!("ok {} ({backend})", p.as_str())
            }
            None => "error: unknown profile".into(),
        },
        // Hold fans at a fixed duty % until `auto`/`rules`/`profile`.
        Some("hold") => match parts.next().and_then(|s| s.parse::<u8>().ok()) {
            Some(pct) => {
                let d = pct.min(100);
                let mut s = shared.lock().expect("state poisoned");
                s.held_duty = Some(d);
                s.auto = false;
                s.manual = true;
                s.fan_overrides.clear();
                save_state(&s);
                mark_control_requested(&mut s);
                format!("ok hold:{d}% ({backend})")
            }
            None => "error: hold requires a percent 0-100".into(),
        },
        // Pin one specific fan to a fixed duty %, independent of the global
        // mode — the per-fan "Manual" toggle + slider in the UI.
        Some("fanhold") => {
            let id = parts.next().map(str::to_string);
            let pct = parts.next().and_then(|s| s.parse::<u8>().ok());
            match (id, pct) {
                (Some(id), Some(pct)) => {
                    let mut s = shared.lock().expect("state poisoned");
                    let d = pct.min(100);
                    if let Err(error) = set_fan_override(&mut s, &id, d) {
                        return error.into();
                    }
                    save_state(&s);
                    mark_control_requested(&mut s);
                    format!("ok fanhold:{id}:{d}% ({backend})")
                }
                _ => "error: fanhold requires <fan_id> <percent 0-100>".into(),
            }
        }
        // Return one fan to whatever the global mode dictates — the per-fan
        // "Auto" toggle.
        Some("fanauto") => match parts.next() {
            Some(id) => {
                let mut s = shared.lock().expect("state poisoned");
                s.fan_overrides.remove(id);
                save_state(&s);
                mark_control_requested(&mut s);
                format!("ok fanauto:{id} ({backend})")
            }
            None => "error: fanauto requires <fan_id>".into(),
        },
        Some("status") => {
            let s = shared.lock().expect("state poisoned");
            let mode = if s.auto {
                "auto".to_string()
            } else if let Some(d) = s.held_duty {
                format!("hold:{d}%")
            } else if s.manual {
                format!("manual:{}", s.profile.as_str())
            } else {
                format!("rules:{}", s.profile.as_str())
            };
            format!("ok {mode} ({backend})")
        }
        // Return the last-cached temps + fans as compact JSON.
        // The CLI uses this to skip SMC init (saves ~350ms per invocation).
        Some("temps") => {
            let s = shared.lock().expect("state poisoned");
            let (temps_age_ms, temps_stale) = sensor_sample_freshness(
                s.last_temps_sampled_at_unix_ms,
                unix_now_ms(),
                s.config.interval_secs,
            );
            let mode = if s.auto {
                "auto".to_string()
            } else if let Some(d) = s.held_duty {
                format!("hold:{d}%")
            } else if s.manual {
                format!("manual:{}", s.profile.as_str())
            } else {
                format!("rules:{}", s.profile.as_str())
            };
            match serde_json::to_string(&serde_json::json!({
                "daemon_version": env!("CARGO_PKG_VERSION"),
                "temps": s.last_temps,
                "temps_sampled_at_unix_ms": s.last_temps_sampled_at_unix_ms,
                "temps_age_ms": temps_age_ms,
                "temps_stale": temps_stale,
                "fans": s.last_fans,
                "power_w": s.last_power_w,
                "mode": mode,
                "backend": s.backend,
                "fan_overrides": s.fan_overrides,
                "control_health": s.control_health,
                "control_revision": s.control_revision,
                "applied_control_revision": s.applied_control_revision,
                "fan_targets": s.fan_targets,
                "last_control_apply_unix_ms": s.last_control_apply_unix_ms,
                "fan_readbacks": s.fan_readbacks,
            })) {
                Ok(json) => format!("ok {json}"),
                Err(_) => "error: serialization failed".into(),
            }
        }
        // Capability probe used by newer menu-bar apps. Older daemons return
        // "unknown command", which lets the signed app replace the helper
        // through its already-approved self-reinstall path without prompting.
        Some("config-source") => format!("ok logged-in-user ({backend})"),
        Some("reload") => {
            let new_cfg = load_daemon_config();
            let rules = new_cfg.rules.len();
            let mut s = shared.lock().expect("state poisoned");
            s.config = new_cfg;
            mark_control_requested(&mut s);
            format!("ok reloaded ({rules} rules) ({backend})")
        }
        Some("stop") => {
            STOP.store(true, Ordering::Relaxed);
            format!("ok stopping ({backend})")
        }
        Some("reinstall-fan-control") => match (parts.next(), parts.next()) {
            (Some(path), None) => match start_self_reinstall(path) {
                Ok(reply) => reply,
                Err(e) => e,
            },
            _ => "error: reinstall-fan-control requires <app-bundled-peterfand-path>".into(),
        },
        _ => "error: unknown command".into(),
    }
}

/// Post a desktop notification (best-effort).
#[cfg(target_os = "macos")]
fn notify(title: &str, message: &str) {
    use std::os::unix::fs::MetadataExt;

    let Ok(console) = std::fs::metadata("/dev/console") else {
        return;
    };
    let Some(mut command) = notification_command_for_uid(console.uid(), title, message) else {
        return;
    };
    if let Ok(output) = command.output() {
        if !output.status.success() {
            let detail = String::from_utf8_lossy(&output.stderr);
            eprintln!("peterfand: notification failed: {}", detail.trim());
        }
    }
}

#[cfg(target_os = "macos")]
fn notification_command_for_uid(
    uid: u32,
    title: &str,
    message: &str,
) -> Option<std::process::Command> {
    // At the login window /dev/console belongs to root and there is no GUI
    // bootstrap namespace to receive notifications.
    if uid == 0 {
        return None;
    }
    let script = format!(
        "display notification {} with title {}",
        applescript_quote(message),
        applescript_quote(title)
    );
    let mut command = std::process::Command::new("/bin/launchctl");
    command
        .arg("asuser")
        .arg(uid.to_string())
        .arg("/usr/bin/osascript")
        .arg("-e")
        .arg(script);
    Some(command)
}

#[cfg(not(target_os = "macos"))]
fn notify(_title: &str, _message: &str) {}

/// Quote a string as an AppleScript string literal.
#[cfg(target_os = "macos")]
fn applescript_quote(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// Local hour (0–23) for time-based automation rules.
#[cfg(unix)]
fn local_hour() -> u8 {
    unsafe {
        let t = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&t, &mut tm);
        tm.tm_hour.clamp(0, 23) as u8
    }
}

#[cfg(not(unix))]
fn local_hour() -> u8 {
    12
}

#[cfg(unix)]
fn install_signal_handlers() {
    extern "C" fn handle(_sig: libc::c_int) {
        STOP.store(true, Ordering::Relaxed);
    }
    let h = handle as *const () as libc::sighandler_t;
    unsafe {
        libc::signal(libc::SIGINT, h);
        libc::signal(libc::SIGTERM, h);
    }
}

#[cfg(not(unix))]
fn install_signal_handlers() {}

/// The duty% actually written to one fan this tick: a per-fan manual pin
/// (`overrides`) wins over the globally-computed `duty` — *except* when the
/// machine is critically hot, where safety always wins regardless of what
/// any fan is pinned to.
fn effective_duty(
    overrides: &std::collections::HashMap<String, u8>,
    fan_id: &str,
    duty: u8,
    critical_now: bool,
) -> u8 {
    if critical_now {
        duty
    } else {
        overrides.get(fan_id).copied().unwrap_or(duty)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ipc_peer_policy_rejects_other_users_and_unknown_console_sessions() {
        assert!(ipc_peer_allowed(0, 0, None, false));
        assert!(ipc_peer_allowed(501, 0, Some(501), false));
        assert!(!ipc_peer_allowed(502, 0, Some(501), false));
        assert!(!ipc_peer_allowed(501, 0, None, false));
        assert!(!ipc_peer_allowed(501, 0, Some(0), false));
        assert!(ipc_peer_allowed(502, 502, None, true));
        assert!(!ipc_peer_allowed(501, 502, None, true));
        assert!(!ipc_peer_allowed(502, 502, None, false));
    }

    #[cfg(unix)]
    #[test]
    fn ipc_request_rejects_partial_overlong_and_invalid_messages() {
        use std::io::Write;
        use std::os::unix::net::UnixStream;
        for bytes in [
            b"ping".to_vec(),
            b"\n".to_vec(),
            b"ping\0\n".to_vec(),
            b"ping extra\n".to_vec(),
            b"ping\nstop\n".to_vec(),
            vec![0xff, b'\n'],
            vec![b'x'; 4097],
            format!("fanhold {} 30\n", "x".repeat(4096)).into_bytes(),
        ] {
            let (mut client, mut server) = UnixStream::pair().unwrap();
            client.write_all(&bytes).unwrap();
            client.shutdown(std::net::Shutdown::Write).unwrap();
            assert!(
                read_ipc_request(&mut server, Duration::from_millis(100)).is_err(),
                "{bytes:?}"
            );
        }
        let (mut client, mut server) = UnixStream::pair().unwrap();
        client.write_all(b"version\n").unwrap();
        assert_eq!(
            read_ipc_request(&mut server, Duration::from_millis(100)).unwrap(),
            "version"
        );
    }

    #[cfg(unix)]
    #[test]
    fn ipc_request_has_an_overall_deadline_even_for_a_slow_sender() {
        use std::io::Write;
        use std::os::unix::net::UnixStream;
        let (mut client, mut server) = UnixStream::pair().unwrap();
        let sender = std::thread::spawn(move || {
            for byte in b"version\n" {
                if client.write_all(&[*byte]).is_err() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        });
        let started = std::time::Instant::now();
        assert!(read_ipc_request(&mut server, Duration::from_millis(60)).is_err());
        assert!(started.elapsed() < Duration::from_secs(1));
        drop(server);
        sender.join().unwrap();
    }

    #[cfg(any(target_os = "macos", target_os = "linux", target_os = "android"))]
    #[test]
    fn ipc_peer_uid_comes_from_the_kernel() {
        let (client, _server) = std::os::unix::net::UnixStream::pair().unwrap();
        assert_eq!(ipc_peer_uid(&client).unwrap(), unsafe { libc::geteuid() });
    }

    #[test]
    fn fan_override_rejects_unknown_ids_and_bounds_persisted_entries() {
        let shared = test_state();
        let mut state = shared.lock().unwrap();
        assert!(set_fan_override(&mut state, "not-a-real-fan", 30).is_err());
        assert!(set_fan_override(&mut state, &"x".repeat(129), 30).is_err());
        for n in 0..33 {
            state.known_fan_ids.push(format!("fan.{n}"));
        }
        for n in 0..32 {
            set_fan_override(&mut state, &format!("fan.{n}"), 30).unwrap();
        }
        assert!(set_fan_override(&mut state, "fan.32", 30).is_err());
        set_fan_override(&mut state, "fan.0", 255).unwrap();
        assert_eq!(state.fan_overrides.len(), 32);
        assert_eq!(state.fan_overrides.get("fan.0"), Some(&100));
    }

    #[test]
    fn effective_duty_prefers_override_when_not_critical() {
        let mut overrides = std::collections::HashMap::new();
        overrides.insert("fan.left".to_string(), 30u8);
        assert_eq!(effective_duty(&overrides, "fan.left", 80, false), 30);
        // No override for this id → falls back to the computed duty.
        assert_eq!(effective_duty(&overrides, "fan.right", 80, false), 80);
    }

    #[test]
    fn effective_duty_ignores_override_when_critical() {
        // A fan pinned low must not stay low while the machine is overheating.
        let mut overrides = std::collections::HashMap::new();
        overrides.insert("fan.left".to_string(), 10u8);
        assert_eq!(effective_duty(&overrides, "fan.left", 100, true), 100);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn notification_targets_the_logged_in_users_bootstrap_session() {
        use std::ffi::OsStr;

        let command = notification_command_for_uid(501, "PeterFan", "Temperature normal")
            .expect("a logged-in user should receive notifications");
        assert_eq!(command.get_program(), OsStr::new("/bin/launchctl"));
        let args = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert_eq!(
            args,
            [
                "asuser",
                "501",
                "/usr/bin/osascript",
                "-e",
                "display notification \"Temperature normal\" with title \"PeterFan\"",
            ]
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn notification_is_skipped_when_no_user_is_logged_in() {
        assert!(notification_command_for_uid(0, "PeterFan", "test").is_none());
    }

    #[test]
    fn saved_state_roundtrips_fan_overrides() {
        let mut fan_overrides = std::collections::HashMap::new();
        fan_overrides.insert("fan.cpu".to_string(), 42u8);
        let saved = SavedState {
            mode: "hold".to_string(),
            hold_pct: Some(50),
            profile: Some("balanced".to_string()),
            fan_overrides,
        };
        let toml_str = toml::to_string(&saved).expect("serializes");
        let back: SavedState = toml::from_str(&toml_str).expect("deserializes");
        assert_eq!(back.fan_overrides.get("fan.cpu"), Some(&42));
    }

    #[test]
    fn saved_state_without_fan_overrides_still_parses() {
        // Old state files (written before this field existed) must still load.
        let toml_str = "mode = \"auto\"\n";
        let back: SavedState = toml::from_str(toml_str).expect("deserializes");
        assert!(back.fan_overrides.is_empty());
    }

    #[test]
    fn fresh_daemon_defaults_to_os_auto_unless_profile_is_explicit() {
        assert!(fresh_daemon_starts_in_auto(false));
        assert!(!fresh_daemon_starts_in_auto(true));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_daemon_uses_the_console_users_application_support_config() {
        assert_eq!(
            macos_user_config_path(std::path::Path::new("/Users/example")),
            std::path::Path::new("/Users/example/Library/Application Support/peterfan/config.toml")
        );
    }

    #[test]
    fn daemon_config_loader_validates_control_settings() {
        let path = std::env::temp_dir().join(format!(
            "peterfan-daemon-config-test-{}-{}.toml",
            std::process::id(),
            unix_now_ms()
        ));
        std::fs::write(&path, "profile = \"gaming\"\ncritical_temp_c = 90\n")
            .expect("write valid config");
        let valid = load_valid_config_file(&path).expect("load valid config");
        assert_eq!(valid.profile, Profile::Gaming);

        std::fs::write(&path, "critical_temp_c = 200\n").expect("write invalid config");
        assert!(load_valid_config_file(&path).is_err());
        let _ = std::fs::remove_file(path);
    }

    fn test_state() -> Arc<Mutex<State>> {
        Arc::new(Mutex::new(State {
            profile: Profile::Balanced,
            held_duty: None,
            fan_overrides: std::collections::HashMap::new(),
            known_fan_ids: vec!["fan.left".into(), "fan.test".into()],
            auto: false,
            manual: false,
            #[cfg(unix)]
            backend: "mock".to_string(),
            config: peterfan_core::config::Config::default(),
            last_temps: Vec::new(),
            last_temps_sampled_at_unix_ms: None,
            last_fans: Vec::new(),
            last_power_w: None,
            control_health: ControlHealth::default(),
            control_revision: 1,
            applied_control_revision: 0,
            fan_targets: std::collections::HashMap::new(),
            last_control_apply_unix_ms: None,
            fan_readbacks: Vec::new(),
            fan_readback_trackers: std::collections::HashMap::new(),
        }))
    }

    #[test]
    fn rules_mode_uses_the_reloaded_base_profile() {
        let shared = test_state();
        let context = RuleContext {
            on_ac: true,
            cpu_temp_c: 50.0,
            hour: 12,
        };
        let mut state = shared.lock().unwrap();
        state.profile = Profile::Balanced;
        state.config.profile = Profile::Gaming;
        assert_eq!(selected_profile(&state, &context), Profile::Gaming);

        state.manual = true;
        state.profile = Profile::Silent;
        assert_eq!(selected_profile(&state, &context), Profile::Silent);
    }

    #[cfg(unix)]
    #[test]
    fn daemon_advertises_logged_in_user_config_support() {
        let reply = handle_command("config-source", &test_state());
        assert_eq!(reply, "ok logged-in-user (mock)");
    }

    #[cfg(unix)]
    #[test]
    fn daemon_reports_its_resident_version_without_changing_legacy_health_replies() {
        let shared = test_state();
        assert_eq!(
            handle_command("version", &shared),
            format!("ok peterfand {}", env!("CARGO_PKG_VERSION"))
        );
        assert_eq!(handle_command("ping", &shared), "ok peterfand (mock)");
        assert_eq!(
            handle_command("status", &shared),
            "ok rules:balanced (mock)"
        );
    }

    type WriteGate = (
        std::sync::mpsc::SyncSender<()>,
        Mutex<std::sync::mpsc::Receiver<()>>,
    );

    struct FaultProvider {
        fail_temperatures: bool,
        fail_fan_reads: AtomicBool,
        fail_writes: bool,
        fail_auto: bool,
        fan_read_calls: std::sync::atomic::AtomicUsize,
        duty_calls: std::sync::atomic::AtomicUsize,
        auto_calls: std::sync::atomic::AtomicUsize,
        first_duty_gate: Option<WriteGate>,
        first_auto_gate: Option<WriteGate>,
        failed_sample_notice: Option<std::sync::mpsc::SyncSender<()>>,
        auto_notice: Option<std::sync::mpsc::SyncSender<()>>,
        write_events: Mutex<Vec<String>>,
    }

    impl FaultProvider {
        fn new(fail_temperatures: bool, fail_writes: bool) -> Self {
            Self {
                fail_temperatures,
                fail_fan_reads: AtomicBool::new(false),
                fail_writes,
                fail_auto: false,
                fan_read_calls: std::sync::atomic::AtomicUsize::new(0),
                duty_calls: std::sync::atomic::AtomicUsize::new(0),
                auto_calls: std::sync::atomic::AtomicUsize::new(0),
                first_duty_gate: None,
                first_auto_gate: None,
                failed_sample_notice: None,
                auto_notice: None,
                write_events: Mutex::new(Vec::new()),
            }
        }

        fn with_auto_failure(mut self) -> Self {
            self.fail_auto = true;
            self
        }

        fn with_fan_read_failure(self) -> Self {
            self.fail_fan_reads.store(true, Ordering::Relaxed);
            self
        }
    }

    impl HardwareProvider for FaultProvider {
        fn name(&self) -> &str {
            "fault-test"
        }

        fn capabilities(&self) -> peterfan_core::provider::Capabilities {
            peterfan_core::provider::Capabilities {
                read_temps: true,
                read_fans: true,
                control_fans: true,
            }
        }

        fn hardware_info(
            &self,
        ) -> peterfan_core::error::Result<peterfan_core::types::HardwareInfo> {
            Ok(peterfan_core::types::HardwareInfo {
                cpu: "Fault Test CPU".into(),
                gpu: None,
                motherboard: None,
                memory: None,
                os: "Test".into(),
            })
        }

        fn temperatures(
            &self,
        ) -> peterfan_core::error::Result<Vec<peterfan_core::types::TempSensor>> {
            use peterfan_core::types::{Celsius, SensorKind, SensorSource, TempSensor};

            if self.fail_temperatures {
                if let Some(notice) = &self.failed_sample_notice {
                    let _ = notice.try_send(());
                }
                return Err(peterfan_core::error::CoreError::Hardware(
                    "injected sensor failure".into(),
                ));
            }
            Ok(vec![TempSensor {
                id: "cpu.die.hot".into(),
                label: "CPU Core Hottest".into(),
                kind: SensorKind::Cpu,
                source: SensorSource::Simulated,
                value: Celsius(70.0),
            }])
        }

        fn fans(&self) -> peterfan_core::error::Result<Vec<peterfan_core::types::Fan>> {
            self.fan_read_calls.fetch_add(1, Ordering::Relaxed);
            if self.fail_fan_reads.load(Ordering::Relaxed) {
                return Err(peterfan_core::error::CoreError::Hardware(
                    "injected fan read failure".into(),
                ));
            }
            Ok(vec![peterfan_core::types::Fan {
                id: "fan.test".into(),
                label: "Test Fan".into(),
                rpm: 1_000,
                min_rpm: Some(500),
                max_rpm: Some(2_000),
                duty_percent: Some(25),
                controllable: true,
            }])
        }

        fn set_fan_duty(
            &self,
            _fan_id: &str,
            duty_percent: u8,
        ) -> peterfan_core::error::Result<()> {
            let call = self.duty_calls.fetch_add(1, Ordering::Relaxed);
            if call == 0 {
                if let Some((started, resume)) = &self.first_duty_gate {
                    let _ = started.try_send(());
                    let _ = resume.lock().unwrap().recv_timeout(Duration::from_secs(3));
                }
            }
            self.write_events
                .lock()
                .unwrap()
                .push(format!("duty:{duty_percent}"));
            if self.fail_writes {
                Err(peterfan_core::error::CoreError::Hardware(
                    "injected fan write failure".into(),
                ))
            } else {
                Ok(())
            }
        }

        fn set_fan_auto(&self, _fan_id: &str) -> peterfan_core::error::Result<()> {
            let call = self.auto_calls.fetch_add(1, Ordering::Relaxed);
            if call == 0 {
                if let Some((started, resume)) = &self.first_auto_gate {
                    let _ = started.try_send(());
                    let _ = resume.lock().unwrap().recv_timeout(Duration::from_secs(3));
                }
            }
            self.write_events.lock().unwrap().push("auto".into());
            if let Some(notice) = &self.auto_notice {
                let _ = notice.try_send(());
            }
            if self.fail_auto {
                Err(peterfan_core::error::CoreError::Hardware(
                    "injected automatic restore failure".into(),
                ))
            } else {
                Ok(())
            }
        }
    }

    #[test]
    fn panic_restores_every_fan_to_os_auto() {
        let provider = FaultProvider::new(false, false);
        let fan_ids = vec!["fan.left".to_string(), "fan.right".to_string()];

        let result = run_with_auto_restore(&provider, &fan_ids, || {
            panic!("injected control-loop panic")
        });

        assert!(result
            .unwrap_err()
            .to_string()
            .contains("panicked (fans restored to auto)"));
        assert_eq!(provider.auto_calls.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn automatic_restore_failure_is_never_reported_as_success() {
        let provider = FaultProvider::new(false, false).with_auto_failure();

        let result = run_with_auto_restore(&provider, &["fan.test".into()], || Ok(()));

        let error = result.unwrap_err().to_string();
        assert!(error.contains("automatic fan restore failed"));
        assert!(error.contains("fan.test"));
        assert_eq!(provider.auto_calls.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn sensor_failure_restores_os_auto_without_writing_duty() {
        STOP.store(false, Ordering::Relaxed);
        let provider = FaultProvider::new(true, false);
        let mut monitor = peterfan_platform::mock_monitor();
        let shared = test_state();
        {
            let mut state = shared.lock().unwrap();
            state.last_temps = FaultProvider::new(false, false).temperatures().unwrap();
            state.last_temps_sampled_at_unix_ms = Some(1_234);
        }

        control_loop(
            &provider,
            monitor.as_mut(),
            &["fan.test".into()],
            true,
            &shared,
        )
        .unwrap();

        assert_eq!(provider.duty_calls.load(Ordering::Relaxed), 0);
        assert_eq!(provider.auto_calls.load(Ordering::Relaxed), 1);
        let state = shared.lock().unwrap();
        assert!(state.control_health.failsafe_active);
        assert_eq!(state.control_health.sensor_failure_count, 1);
        assert_eq!(state.control_health.consecutive_sensor_failures, 1);
        assert_eq!(state.last_temps.len(), 1);
        assert_eq!(state.last_temps[0].id, "cpu.die.hot");
        assert_eq!(state.last_temps_sampled_at_unix_ms, None);
        assert!(state
            .control_health
            .last_error
            .as_deref()
            .is_some_and(|error| error.contains("fans returned to OS auto")));
    }

    #[test]
    fn fan_write_failure_restores_os_auto_and_records_health() {
        STOP.store(false, Ordering::Relaxed);
        let provider = FaultProvider::new(false, true);
        let mut monitor = peterfan_platform::mock_monitor();
        let shared = test_state();

        control_loop(
            &provider,
            monitor.as_mut(),
            &["fan.test".into()],
            true,
            &shared,
        )
        .unwrap();

        assert_eq!(provider.duty_calls.load(Ordering::Relaxed), 1);
        assert_eq!(provider.auto_calls.load(Ordering::Relaxed), 1);
        let state = shared.lock().unwrap();
        assert!(state.control_health.failsafe_active);
        assert_eq!(state.control_health.fan_write_failure_count, 1);
        assert_eq!(state.control_health.consecutive_fan_write_failures, 1);
        assert!(state
            .control_health
            .retry_after_unix
            .is_some_and(|retry_after| retry_after >= unix_now() + 4));
        assert!(state
            .control_health
            .last_error
            .as_deref()
            .is_some_and(|error| error.contains("fan write fail-safe")));
    }

    #[test]
    fn fan_read_failure_restores_auto_and_recovers_after_the_existing_cooldown() {
        STOP.store(false, Ordering::Relaxed);
        let provider = FaultProvider::new(false, false).with_fan_read_failure();
        let mut monitor = peterfan_platform::mock_monitor();
        let shared = test_state();
        shared
            .lock()
            .unwrap()
            .fan_targets
            .insert("fan.test".into(), 50);

        control_loop(
            &provider,
            monitor.as_mut(),
            &["fan.test".into()],
            true,
            &shared,
        )
        .unwrap();

        assert_eq!(provider.duty_calls.load(Ordering::Relaxed), 0);
        assert_eq!(provider.auto_calls.load(Ordering::Relaxed), 1);
        {
            let state = shared.lock().unwrap();
            assert!(state.control_health.failsafe_active);
            assert_eq!(state.control_health.fan_readback_failure_count, 1);
            assert_eq!(state.control_health.consecutive_fan_readback_failures, 1);
            assert!(state.control_health.stale_fan_ids.is_empty());
            assert!(state.fan_targets.is_empty());
            assert!(state.fan_readbacks.is_empty());
            assert!(
                state.control_health.retry_after_unix.is_some_and(
                    |retry_after| retry_after >= unix_now() + FAN_READBACK_RETRY_SECS - 1
                )
            );
            let detail = state.control_health.last_error.as_deref().unwrap();
            assert!(detail.contains("fan RPM readback failed"), "{detail}");
            assert!(detail.contains("injected fan read failure"), "{detail}");
            assert!(detail.contains("fans returned to OS auto"), "{detail}");
            assert!(!detail.contains("stalled"), "{detail}");
        }

        provider.fail_fan_reads.store(false, Ordering::Relaxed);
        control_loop(
            &provider,
            monitor.as_mut(),
            &["fan.test".into()],
            true,
            &shared,
        )
        .unwrap();
        assert_eq!(provider.duty_calls.load(Ordering::Relaxed), 0);
        assert!(shared.lock().unwrap().control_health.failsafe_active);

        shared.lock().unwrap().control_health.retry_after_unix = Some(unix_now().saturating_sub(1));
        control_loop(
            &provider,
            monitor.as_mut(),
            &["fan.test".into()],
            true,
            &shared,
        )
        .unwrap();
        assert_eq!(provider.duty_calls.load(Ordering::Relaxed), 1);
        let state = shared.lock().unwrap();
        assert!(!state.control_health.failsafe_active);
        assert!(state.control_health.last_error.is_none());
        assert!(state.control_health.retry_after_unix.is_none());
        assert_eq!(state.control_health.fan_readback_failure_count, 1);
        assert_eq!(state.control_health.consecutive_fan_readback_failures, 0);
    }

    #[test]
    fn fan_read_error_cannot_be_reported_as_a_preexisting_rpm_stall() {
        STOP.store(false, Ordering::Relaxed);
        let provider = FaultProvider::new(false, false).with_fan_read_failure();
        let mut monitor = peterfan_platform::mock_monitor();
        let shared = test_state();
        {
            let mut state = shared.lock().unwrap();
            state.fan_targets.insert("fan.test".into(), 100);
            state.fan_readback_trackers.insert(
                "fan.test".into(),
                FanReadbackTracker {
                    target_rpm: 2_000,
                    tolerance_rpm: 150,
                    started_ms: 0,
                    last_progress_ms: 0,
                    last_distance_rpm: 1_000,
                    stale: true,
                },
            );
            state.control_health.consecutive_fan_readback_failures = 2;
            state.control_health.stale_fan_ids.push("fan.test".into());
        }

        control_loop(
            &provider,
            monitor.as_mut(),
            &["fan.test".into()],
            true,
            &shared,
        )
        .unwrap();

        assert_eq!(provider.duty_calls.load(Ordering::Relaxed), 0);
        assert_eq!(provider.auto_calls.load(Ordering::Relaxed), 1);
        let state = shared.lock().unwrap();
        let detail = state.control_health.last_error.as_deref().unwrap();
        assert!(detail.contains("injected fan read failure"), "{detail}");
        assert!(!detail.contains("stalled"), "{detail}");
        assert!(state.control_health.stale_fan_ids.is_empty());
        assert!(state.fan_readback_trackers.is_empty());
        assert!(state.fan_targets.is_empty());
    }

    fn run_control_samples(
        provider: &dyn HardwareProvider,
        shared: &Arc<Mutex<State>>,
        samples: u8,
    ) {
        let mut monitor = peterfan_platform::mock_monitor();
        let mut remaining = samples;
        control_loop_while(
            provider,
            monitor.as_mut(),
            &["fan.test".into()],
            false,
            shared,
            || {
                if remaining == 0 {
                    return false;
                }
                remaining -= 1;
                true
            },
        )
        .unwrap();
    }

    #[test]
    fn persistent_fan_read_errors_do_not_repeat_the_fail_safe_during_cooldown() {
        STOP.store(false, Ordering::Relaxed);
        let provider = FaultProvider::new(false, false).with_fan_read_failure();
        let shared = test_state();
        shared
            .lock()
            .unwrap()
            .fan_targets
            .insert("fan.test".into(), 50);

        run_control_samples(&provider, &shared, 3);

        assert_eq!(provider.fan_read_calls.load(Ordering::Relaxed), 3);
        assert_eq!(provider.auto_calls.load(Ordering::Relaxed), 1);
        assert_eq!(provider.duty_calls.load(Ordering::Relaxed), 0);
        let state = shared.lock().unwrap();
        assert_eq!(state.control_health.fan_readback_failure_count, 1);
        assert_eq!(state.control_health.consecutive_fan_readback_failures, 1);
        assert!(state.control_health.failsafe_active);
        assert!(state.fan_targets.is_empty());
        assert!(state.control_health.retry_after_unix.is_some());
    }

    #[test]
    fn an_existing_cooldown_survives_repeated_fan_read_errors_without_writes() {
        STOP.store(false, Ordering::Relaxed);
        let provider = FaultProvider::new(false, false).with_fan_read_failure();
        let shared = test_state();
        let retry_after = unix_now() + 30;
        {
            let mut state = shared.lock().unwrap();
            state.fan_targets.insert("fan.test".into(), 50);
            state.control_health.failsafe_active = true;
            state.control_health.retry_after_unix = Some(retry_after);
            state.control_health.last_error = Some("existing failure".into());
            state.control_health.fan_readback_failure_count = 7;
            state.control_health.consecutive_fan_readback_failures = 2;
        }

        run_control_samples(&provider, &shared, 3);

        assert_eq!(provider.fan_read_calls.load(Ordering::Relaxed), 3);
        assert_eq!(provider.auto_calls.load(Ordering::Relaxed), 0);
        assert_eq!(provider.duty_calls.load(Ordering::Relaxed), 0);
        let state = shared.lock().unwrap();
        assert_eq!(state.control_health.retry_after_unix, Some(retry_after));
        assert_eq!(
            state.control_health.last_error.as_deref(),
            Some("existing failure")
        );
        assert_eq!(state.control_health.fan_readback_failure_count, 7);
        assert_eq!(state.control_health.consecutive_fan_readback_failures, 2);
    }

    #[test]
    fn fan_read_errors_without_manual_targets_do_not_add_global_auto_writes_or_failures() {
        STOP.store(false, Ordering::Relaxed);
        let healthy_provider = FaultProvider::new(false, false);
        let failing_provider = FaultProvider::new(false, false).with_fan_read_failure();
        let healthy = test_state();
        let failing = test_state();
        healthy.lock().unwrap().auto = true;
        failing.lock().unwrap().auto = true;

        run_control_samples(&healthy_provider, &healthy, 2);
        run_control_samples(&failing_provider, &failing, 2);

        assert_eq!(healthy_provider.auto_calls.load(Ordering::Relaxed), 2);
        assert_eq!(failing_provider.fan_read_calls.load(Ordering::Relaxed), 2);
        assert_eq!(failing_provider.auto_calls.load(Ordering::Relaxed), 2);
        assert_eq!(failing_provider.duty_calls.load(Ordering::Relaxed), 0);
        let state = failing.lock().unwrap();
        assert!(!state.control_health.failsafe_active);
        assert_eq!(state.control_health.fan_readback_failure_count, 0);
        assert_eq!(state.control_health.consecutive_fan_readback_failures, 0);
        assert!(state.control_health.retry_after_unix.is_none());
        assert!(state.control_health.last_error.is_none());
        assert!(state.fan_targets.is_empty());
    }

    #[test]
    fn fan_write_retry_delay_uses_bounded_exponential_backoff() {
        assert_eq!(fan_write_retry_delay_secs(1), 5);
        assert_eq!(fan_write_retry_delay_secs(2), 10);
        assert_eq!(fan_write_retry_delay_secs(3), 20);
        assert_eq!(fan_write_retry_delay_secs(4), 40);
        assert_eq!(fan_write_retry_delay_secs(5), 60);
        assert_eq!(fan_write_retry_delay_secs(100), 60);
    }

    #[test]
    fn fan_duty_ramp_limits_upward_step_by_elapsed_time() {
        assert_eq!(slew_fan_duty(30, 80, 1_000, false), 34);
        assert_eq!(slew_fan_duty(30, 80, 250, false), 31);
        assert_eq!(slew_fan_duty(30, 80, 20_000, false), 80);
    }

    #[test]
    fn fan_duty_ramp_limits_downward_step_but_critical_is_immediate() {
        assert_eq!(slew_fan_duty(80, 30, 1_000, false), 77);
        assert_eq!(slew_fan_duty(80, 30, 20_000, false), 30);
        assert_eq!(slew_fan_duty(30, 100, 0, true), 100);
    }

    #[test]
    fn active_fan_write_cooldown_skips_hardware_writes() {
        STOP.store(false, Ordering::Relaxed);
        let provider = FaultProvider::new(false, false);
        let mut monitor = peterfan_platform::mock_monitor();
        let shared = test_state();
        {
            let mut state = shared.lock().unwrap();
            state.control_health.failsafe_active = true;
            state.control_health.consecutive_fan_write_failures = 2;
            state.control_health.retry_after_unix = Some(unix_now() + 30);
        }

        control_loop(
            &provider,
            monitor.as_mut(),
            &["fan.test".into()],
            true,
            &shared,
        )
        .unwrap();

        assert_eq!(provider.duty_calls.load(Ordering::Relaxed), 0);
        assert_eq!(provider.auto_calls.load(Ordering::Relaxed), 0);
        assert!(shared.lock().unwrap().control_health.failsafe_active);
    }

    #[test]
    fn control_temperatures_reject_missing_and_non_physical_readings() {
        use peterfan_core::types::{Celsius, SensorKind, SensorSource, TempSensor};

        assert_eq!(
            control_temperatures(&[]),
            Err("no trustworthy temperature reading")
        );
        let invalid = vec![TempSensor {
            id: "cpu.die.hot".into(),
            label: "CPU Core Hottest".into(),
            kind: SensorKind::Cpu,
            source: SensorSource::Smc,
            value: Celsius(255.0),
        }];
        assert_eq!(
            control_temperatures(&invalid),
            Err("no trustworthy temperature reading")
        );
    }

    #[test]
    fn sensor_freshness_uses_three_intervals_with_a_six_second_floor() {
        assert_eq!(sensor_stale_after_ms(1), 6_000);
        assert_eq!(sensor_stale_after_ms(2), 6_000);
        assert_eq!(sensor_stale_after_ms(5), 15_000);
        assert_eq!(
            sensor_sample_freshness(Some(1_000), 7_000, 2),
            (Some(6_000), false)
        );
        assert_eq!(
            sensor_sample_freshness(Some(1_000), 7_001, 2),
            (Some(6_001), true)
        );
        assert_eq!(sensor_sample_freshness(None, 7_001, 2), (None, true));
    }

    #[cfg(unix)]
    #[test]
    fn temps_ipc_exposes_control_health() {
        let shared = test_state();
        {
            let mut state = shared.lock().unwrap();
            state.control_health.failsafe_active = true;
            state.control_health.sensor_failure_count = 2;
            state.control_health.consecutive_sensor_failures = 1;
            state.control_health.last_error = Some("sensor unavailable".into());
        }

        let reply = handle_command("temps", &shared);
        let json: serde_json::Value = serde_json::from_str(
            reply
                .strip_prefix("ok ")
                .expect("temps command should return JSON"),
        )
        .unwrap();
        assert_eq!(json["daemon_version"], env!("CARGO_PKG_VERSION"));
        assert_eq!(json["control_health"]["failsafe_active"], true);
        assert_eq!(json["control_health"]["sensor_failure_count"], 2);
        assert_eq!(json["control_health"]["last_error"], "sensor unavailable");
        assert_eq!(json["temps_stale"], true);
        assert!(json["temps_sampled_at_unix_ms"].is_null());
        assert!(json["temps_age_ms"].is_null());
        assert_eq!(json["control_revision"], 1);
        assert_eq!(json["applied_control_revision"], 0);
        assert_eq!(json["fan_targets"], serde_json::json!({}));
        assert!(json["last_control_apply_unix_ms"].is_null());
        assert_eq!(json["fan_readbacks"], serde_json::json!([]));
    }

    fn readback_fan(rpm: u32) -> peterfan_core::types::Fan {
        peterfan_core::types::Fan {
            id: "fan.test".into(),
            label: "Test Fan".into(),
            rpm,
            min_rpm: Some(500),
            max_rpm: Some(2_000),
            duty_percent: None,
            controllable: true,
        }
    }

    #[test]
    fn fan_readback_allows_ramp_progress_then_confirms_target() {
        let shared = test_state();
        let mut state = shared.lock().unwrap();
        state.fan_targets.insert("fan.test".into(), 100);

        assert!(!refresh_fan_readbacks(
            &mut state,
            &[readback_fan(1_000)],
            0
        ));
        assert_eq!(state.fan_readbacks[0].status, "adjusting");
        assert!(!refresh_fan_readbacks(
            &mut state,
            &[readback_fan(1_500)],
            8_000
        ));
        assert_eq!(state.fan_readbacks[0].status, "adjusting");
        assert!(!refresh_fan_readbacks(
            &mut state,
            &[readback_fan(1_950)],
            12_000
        ));
        assert_eq!(state.fan_readbacks[0].status, "settled");
        assert_eq!(state.control_health.last_fan_readback_ok_unix, Some(12));
    }

    #[test]
    fn fan_readback_requires_repeated_stale_samples_before_failsafe() {
        let shared = test_state();
        let mut state = shared.lock().unwrap();
        state.fan_targets.insert("fan.test".into(), 100);

        assert!(!refresh_fan_readbacks(
            &mut state,
            &[readback_fan(1_000)],
            0
        ));
        assert!(!refresh_fan_readbacks(
            &mut state,
            &[readback_fan(1_000)],
            11_000
        ));
        assert_eq!(state.fan_readbacks[0].status, "stale");
        assert!(!refresh_fan_readbacks(
            &mut state,
            &[readback_fan(1_000)],
            13_000
        ));
        assert!(refresh_fan_readbacks(
            &mut state,
            &[readback_fan(1_000)],
            15_000
        ));
        assert_eq!(state.control_health.fan_readback_failure_count, 1);
        assert_eq!(state.control_health.consecutive_fan_readback_failures, 3);
        assert_eq!(state.control_health.stale_fan_ids, ["fan.test"]);
    }

    #[test]
    fn material_target_change_restarts_fan_readback_grace() {
        let shared = test_state();
        let mut state = shared.lock().unwrap();
        state.fan_targets.insert("fan.test".into(), 100);
        assert!(!refresh_fan_readbacks(
            &mut state,
            &[readback_fan(1_000)],
            0
        ));
        assert!(!refresh_fan_readbacks(
            &mut state,
            &[readback_fan(1_000)],
            11_000
        ));
        assert_eq!(state.fan_readbacks[0].status, "stale");

        state.fan_targets.insert("fan.test".into(), 20);
        assert!(!refresh_fan_readbacks(
            &mut state,
            &[readback_fan(1_000)],
            12_000
        ));
        assert_eq!(state.fan_readbacks[0].status, "adjusting");
        assert_eq!(state.control_health.consecutive_fan_readback_failures, 0);
    }

    #[test]
    fn publishing_new_targets_discards_only_materially_stale_readback() {
        let shared = test_state();
        let mut state = shared.lock().unwrap();
        state.fan_targets.insert("fan.test".into(), 40);
        state.fan_readbacks.push(FanReadback {
            id: "fan.test".into(),
            label: "Test Fan".into(),
            current_rpm: Some(1_100),
            target_rpm: 1_100,
            tolerance_rpm: 150,
            status: "settled".into(),
        });

        publish_successful_fan_targets(
            &mut state,
            std::collections::HashMap::from([("fan.test".into(), 44)]),
        );
        assert_eq!(state.fan_readbacks.len(), 1);

        publish_successful_fan_targets(
            &mut state,
            std::collections::HashMap::from([("fan.test".into(), 50)]),
        );
        assert!(state.fan_readbacks.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn control_revision_is_applied_only_after_hardware_loop_succeeds() {
        STOP.store(false, Ordering::Relaxed);
        let provider = FaultProvider::new(false, false);
        let mut monitor = peterfan_platform::mock_monitor();
        let shared = test_state();

        let reply = handle_command("profile performance", &shared);
        assert!(reply.starts_with("ok performance"));
        {
            let state = shared.lock().unwrap();
            assert_eq!(state.control_revision, 2);
            assert_eq!(state.applied_control_revision, 0);
        }

        control_loop(
            &provider,
            monitor.as_mut(),
            &["fan.test".into()],
            true,
            &shared,
        )
        .unwrap();

        let state = shared.lock().unwrap();
        assert_eq!(state.control_revision, 2);
        assert_eq!(state.applied_control_revision, 2);
    }

    #[cfg(unix)]
    #[test]
    fn cached_apply_burst_is_bounded_and_finishes_at_the_latest_target() {
        use std::sync::atomic::AtomicUsize;
        let (started_tx, started_rx) = std::sync::mpsc::sync_channel(1);
        let (resume_tx, resume_rx) = std::sync::mpsc::sync_channel(1);
        let mut provider = FaultProvider::new(false, false);
        provider.first_duty_gate = Some((started_tx, Mutex::new(resume_rx)));
        let provider = Arc::new(provider);
        let shared = prepared_cached_state();
        let calls = Arc::new(AtomicUsize::new(0));
        let worker_provider = Arc::clone(&provider);
        let worker_state = Arc::clone(&shared);
        let worker_calls = Arc::clone(&calls);
        let worker = ControlApplyWorker::spawn(move || {
            worker_calls.fetch_add(1, Ordering::Relaxed);
            apply_cached_control_request(
                worker_provider.as_ref(),
                &["fan.test".into()],
                &worker_state,
            );
        })
        .unwrap();
        worker.request();
        started_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        for target in 31..=80 {
            let mut state = shared.lock().unwrap();
            state.held_duty = Some(target);
            state.control_revision += 1;
            drop(state);
            worker.request();
        }
        resume_tx.send(()).unwrap();
        drop(worker);
        assert!(
            calls.load(Ordering::Relaxed) <= 2,
            "burst created {} apply jobs",
            calls.load(Ordering::Relaxed)
        );
        assert_eq!(
            provider.write_events.lock().unwrap().last().unwrap(),
            "duty:80"
        );
        let state = shared.lock().unwrap();
        assert_eq!(state.applied_control_revision, state.control_revision);
    }

    #[cfg(unix)]
    fn prepared_cached_state() -> Arc<Mutex<State>> {
        let shared = test_state();
        {
            let mut state = shared.lock().unwrap();
            state.manual = true;
            state.held_duty = Some(30);
            state.last_temps = FaultProvider::new(false, false).temperatures().unwrap();
            state.last_temps_sampled_at_unix_ms = Some(unix_now_ms());
        }
        shared
    }

    #[cfg(unix)]
    #[test]
    fn cached_apply_rejects_failed_missing_or_stale_sensor_cache() {
        for (sampled_at, failed) in [
            (Some(unix_now_ms()), true),
            (None, false),
            (Some(unix_now_ms().saturating_sub(120_000)), false),
            (Some(unix_now_ms().saturating_add(120_000)), false),
        ] {
            let provider = FaultProvider::new(false, false);
            let shared = prepared_cached_state();
            {
                let mut state = shared.lock().unwrap();
                state.last_temps_sampled_at_unix_ms = sampled_at;
                state.control_health.consecutive_sensor_failures = u32::from(failed);
                state.control_health.failsafe_active = true;
                state.control_health.last_error = Some("untrusted sensor cache".into());
            }
            apply_cached_control_request(&provider, &["fan.test".into()], &shared);
            assert_eq!(provider.duty_calls.load(Ordering::Relaxed), 0);
            let state = shared.lock().unwrap();
            assert!(state.control_health.failsafe_active);
            assert_eq!(state.applied_control_revision, 0);
            assert_eq!(
                state.control_health.last_error.as_deref(),
                Some("untrusted sensor cache")
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn cached_auto_does_not_clear_a_sensor_failure() {
        let provider = FaultProvider::new(false, false);
        let shared = prepared_cached_state();
        {
            let mut state = shared.lock().unwrap();
            state.auto = true;
            state.manual = false;
            state.control_health.consecutive_sensor_failures = 1;
            state.control_health.failsafe_active = true;
            state.control_health.last_error = Some("sensor unavailable".into());
        }
        apply_cached_control_request(&provider, &["fan.test".into()], &shared);
        assert_eq!(provider.auto_calls.load(Ordering::Relaxed), 1);
        assert_eq!(provider.duty_calls.load(Ordering::Relaxed), 0);
        let state = shared.lock().unwrap();
        assert!(state.control_health.failsafe_active);
        assert_eq!(
            state.control_health.last_error.as_deref(),
            Some("sensor unavailable")
        );
    }

    #[test]
    fn periodic_write_rechecks_sample_age_after_waiting_for_hardware() {
        let provider = Arc::new(FaultProvider::new(false, false));
        let shared = test_state();
        let write_guard = FAN_WRITE_LOCK.lock().unwrap();
        let loop_provider = Arc::clone(&provider);
        let loop_state = Arc::clone(&shared);
        let control = thread::spawn(move || {
            let mut monitor = peterfan_platform::mock_monitor();
            control_loop_while(
                loop_provider.as_ref(),
                monitor.as_mut(),
                &["fan.test".into()],
                true,
                &loop_state,
                || true,
            )
            .unwrap();
        });
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        loop {
            let mut state = shared.lock().unwrap();
            if state.last_temps_sampled_at_unix_ms.is_some() {
                // Deterministically model the sample aging while the slow SMC
                // writer owns the lock, without a multi-second test sleep.
                state.last_temps_sampled_at_unix_ms = Some(unix_now_ms().saturating_sub(120_000));
                break;
            }
            drop(state);
            assert!(
                std::time::Instant::now() < deadline,
                "sensor scan did not finish"
            );
            thread::sleep(Duration::from_millis(1));
        }
        drop(write_guard);
        control.join().unwrap();
        assert_eq!(provider.duty_calls.load(Ordering::Relaxed), 0);
        assert_eq!(shared.lock().unwrap().applied_control_revision, 0);
        let recovered = FaultProvider::new(false, false);
        let mut monitor = peterfan_platform::mock_monitor();
        control_loop_while(
            &recovered,
            monitor.as_mut(),
            &["fan.test".into()],
            true,
            &shared,
            || true,
        )
        .unwrap();
        assert_eq!(recovered.duty_calls.load(Ordering::Relaxed), 1);
    }

    #[cfg(unix)]
    #[test]
    fn cached_inflight_write_finishes_before_sensor_failure_restores_auto() {
        let (started_tx, started_rx) = std::sync::mpsc::sync_channel(1);
        let (resume_tx, resume_rx) = std::sync::mpsc::sync_channel(1);
        let (failed_tx, failed_rx) = std::sync::mpsc::sync_channel(1);
        let (auto_tx, auto_rx) = std::sync::mpsc::sync_channel(1);
        let (auto_started_tx, auto_started_rx) = std::sync::mpsc::sync_channel(1);
        let (resume_auto_tx, resume_auto_rx) = std::sync::mpsc::sync_channel(1);
        let mut provider = FaultProvider::new(true, false);
        provider.first_duty_gate = Some((started_tx, Mutex::new(resume_rx)));
        provider.first_auto_gate = Some((auto_started_tx, Mutex::new(resume_auto_rx)));
        provider.failed_sample_notice = Some(failed_tx);
        provider.auto_notice = Some(auto_tx);
        let provider = Arc::new(provider);
        let shared = prepared_cached_state();
        let worker_provider = Arc::clone(&provider);
        let worker_state = Arc::clone(&shared);
        let worker = ControlApplyWorker::spawn(move || {
            apply_cached_control_request(
                worker_provider.as_ref(),
                &["fan.test".into()],
                &worker_state,
            )
        })
        .unwrap();
        worker.request();
        started_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        let failure_provider = Arc::clone(&provider);
        let failure_state = Arc::clone(&shared);
        let failure = thread::spawn(move || {
            let mut monitor = peterfan_platform::mock_monitor();
            control_loop_while(
                failure_provider.as_ref(),
                monitor.as_mut(),
                &["fan.test".into()],
                true,
                &failure_state,
                || true,
            )
            .unwrap();
        });
        failed_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_millis(100);
        let invalidated_before_write_finished = loop {
            let state = shared.lock().unwrap();
            if state.control_health.consecutive_sensor_failures > 0
                && state.last_temps_sampled_at_unix_ms.is_none()
            {
                break true;
            }
            drop(state);
            if std::time::Instant::now() >= deadline {
                break false;
            }
            thread::sleep(Duration::from_millis(1));
        };
        // Queue another command before the old write returns: even if the
        // worker wins the mutex again, it must observe the failed sample.
        worker.request();
        let restored_before_write_finished =
            auto_rx.recv_timeout(Duration::from_millis(100)).is_ok();
        resume_tx.send(()).unwrap();
        auto_started_rx
            .recv_timeout(Duration::from_secs(2))
            .unwrap();
        let failure_preserved_during_restore = {
            let state = shared.lock().unwrap();
            state.control_health.failsafe_active
                && state.applied_control_revision == 0
                && state.fan_targets.is_empty()
        };
        resume_auto_tx.send(()).unwrap();
        failure.join().unwrap();
        drop(worker);
        assert!(
            invalidated_before_write_finished,
            "failed sample stayed usable while waiting for hardware"
        );
        assert!(
            failure_preserved_during_restore,
            "old write reported success after the sample failed"
        );
        assert!(
            !restored_before_write_finished,
            "failure restore raced the manual write"
        );
        assert_eq!(*provider.write_events.lock().unwrap(), ["duty:30", "auto"]);
        assert!(shared.lock().unwrap().control_health.failsafe_active);
        // Only a genuinely fresh successful sensor sample may resume the saved intent.
        let recovered = FaultProvider::new(false, false);
        let mut monitor = peterfan_platform::mock_monitor();
        control_loop_while(
            &recovered,
            monitor.as_mut(),
            &["fan.test".into()],
            true,
            &shared,
            || true,
        )
        .unwrap();
        assert_eq!(recovered.duty_calls.load(Ordering::Relaxed), 1);
        assert!(!shared.lock().unwrap().control_health.failsafe_active);
    }

    #[cfg(unix)]
    #[test]
    fn cached_control_request_confirms_hardware_write_immediately() {
        let provider = FaultProvider::new(false, false);
        let shared = test_state();
        {
            let mut state = shared.lock().unwrap();
            state.last_temps = provider.temperatures().unwrap();
            state.last_temps_sampled_at_unix_ms = Some(unix_now_ms());
        }

        let reply = handle_command("profile performance", &shared);
        assert!(reply.starts_with("ok performance"));
        apply_cached_control_request(&provider, &["fan.test".into()], &shared);

        let state = shared.lock().unwrap();
        assert_eq!(state.control_revision, 2);
        assert_eq!(state.applied_control_revision, 2);
        assert_eq!(state.profile, Profile::Performance);
        assert_eq!(provider.duty_calls.load(Ordering::Relaxed), 1);
        assert_eq!(
            state.fan_targets.get("fan.test"),
            Some(&Profile::Performance.default_curve().duty_at(70.0))
        );
        assert!(state.last_control_apply_unix_ms.is_some());
    }

    #[cfg(unix)]
    #[test]
    fn stale_rule_snapshot_cannot_replace_new_manual_profile() {
        let shared = test_state();
        let observed_revision = shared.lock().unwrap().control_revision;

        let reply = handle_command("profile performance", &shared);
        assert!(reply.starts_with("ok performance"));
        publish_effective_rule_profile(&shared, observed_revision, Profile::Silent);

        let state = shared.lock().unwrap();
        assert!(state.manual);
        assert_eq!(state.control_revision, observed_revision + 1);
        assert_eq!(state.profile, Profile::Performance);
    }

    #[cfg(unix)]
    #[test]
    fn fanhold_succeeds_without_entitlement_gate() {
        let shared = test_state();
        let reply = handle_command("fanhold fan.left 30", &shared);
        assert!(reply.starts_with("ok "), "unexpected reply: {reply}");
        assert_eq!(
            shared.lock().unwrap().fan_overrides.get("fan.left"),
            Some(&30)
        );
    }

    #[cfg(unix)]
    #[test]
    fn fanhold_succeeds_normally() {
        let shared = test_state();
        let reply = handle_command("fanhold fan.left 30", &shared);
        assert!(reply.starts_with("ok "), "unexpected reply: {reply}");
        assert_eq!(
            shared.lock().unwrap().fan_overrides.get("fan.left"),
            Some(&30)
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn self_reinstall_rejects_non_app_bundle_sources() {
        let reply = handle_command("reinstall-fan-control /tmp/peterfand", &test_state());
        assert!(
            reply.starts_with("No such file")
                || reply.starts_with("reinstall source")
                || reply.contains("requires a root daemon")
        );
    }

    #[cfg(target_os = "macos")]
    struct ReinstallFixture(PathBuf);

    #[cfg(target_os = "macos")]
    impl ReinstallFixture {
        fn new() -> Self {
            let output = std::process::Command::new("/usr/bin/mktemp")
                .args(["-d", "-t", "peterfan-secure-reinstall-test"])
                .output()
                .unwrap();
            assert!(output.status.success());
            Self(PathBuf::from(
                String::from_utf8(output.stdout).unwrap().trim(),
            ))
        }

        fn run(&self, accept: bool) -> std::process::Output {
            use std::os::unix::fs::PermissionsExt;
            let source = self.0.join("owner's original helper");
            let destination = self.0.join("installed helper");
            std::fs::write(&source, "trusted fixture bytes").unwrap();
            let verifier = self.0.join("verify helper");
            // The external signing authority is replaced; file staging and the
            // generated shell transaction execute normally inside this fixture.
            std::fs::write(
                &verifier,
                r#"#!/bin/sh
set -e
for arg; do staged=$arg; done
[ "$staged" != "$PETERFAN_TEST_SOURCE" ]
[ "$(/bin/cat "$staged")" = 'trusted fixture bytes' ]
[ "$(/usr/bin/stat -f '%Lp' "$(/usr/bin/dirname "$staged")")" = 700 ]
case "$*" in *'certificate leaf[subject.OU] = "N99FMBQ662"'*) ;; *) exit 1;; esac
[ "$PETERFAN_TEST_ACCEPT" = yes ]
/usr/bin/printf '%s' 'changed original bytes' > "$PETERFAN_TEST_SOURCE"
"#,
            )
            .unwrap();
            std::fs::set_permissions(&verifier, std::fs::Permissions::from_mode(0o700)).unwrap();
            let restarter = self.0.join("record restart");
            std::fs::write(
                &restarter,
                "#!/bin/sh\n/usr/bin/printf '%s\\n' \"$@\" > \"$PETERFAN_TEST_RESTART\"\n",
            )
            .unwrap();
            std::fs::set_permissions(&restarter, std::fs::Permissions::from_mode(0o700)).unwrap();
            let checker = self.0.join("check version");
            std::fs::write(&checker, "#!/bin/sh\n[ \"$1\" = --validate-self-reinstall-version ] && [ \"$3\" = --reinstall-from-version ] && [ \"$4\" = 1.32.5 ] && [ \"$(/bin/cat \"$2\")\" = 'trusted fixture bytes' ]\n").unwrap();
            std::fs::set_permissions(&checker, std::fs::Permissions::from_mode(0o700)).unwrap();
            let script = self_reinstall_script(
                &source,
                &destination,
                &self.0.join("owner's stage.XXXXXXXX"),
                &checker,
                "1.32.5",
            )
            .replace(
                "/usr/bin/codesign",
                &shell_quote(&verifier.to_string_lossy()),
            )
            .replace("/bin/launchctl", &shell_quote(&restarter.to_string_lossy()))
            // If the original unsafe redirect returns, catch it in the
            // private fixture without creating anything in shared /tmp.
            .replace(
                "/tmp/peterfan-self-reinstall.log",
                &shell_quote(&self.0.join("unsafe log").to_string_lossy()),
            );
            std::process::Command::new("/bin/sh")
                .args(["-c", &script])
                .env("PETERFAN_TEST_SOURCE", source)
                .env("PETERFAN_TEST_ACCEPT", if accept { "yes" } else { "no" })
                .env("PETERFAN_TEST_RESTART", self.0.join("restart receipt"))
                .output()
                .unwrap()
        }

        fn stages(&self) -> usize {
            std::fs::read_dir(&self.0)
                .unwrap()
                .filter_map(Result::ok)
                .filter(|entry| {
                    entry
                        .file_name()
                        .to_string_lossy()
                        .starts_with("owner's stage.")
                })
                .count()
        }
    }

    #[cfg(target_os = "macos")]
    impl Drop for ReinstallFixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn self_reinstall_installs_only_verified_staged_bytes_and_cleans_before_restart() {
        let fixture = ReinstallFixture::new();
        let output = fixture.run(true);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        let receipt = fixture.0.join("restart receipt");
        let expected = "kickstart\n-k\nsystem/kr.co.uulab.peterfan.daemon\n";
        while std::fs::read_to_string(&receipt).ok().as_deref() != Some(expected)
            && std::time::Instant::now() < deadline
        {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(std::fs::read_to_string(&receipt).unwrap(), expected);
        assert_eq!(
            std::fs::read_to_string(fixture.0.join("installed helper")).unwrap(),
            "trusted fixture bytes"
        );
        assert_eq!(
            std::fs::read_to_string(fixture.0.join("owner's original helper")).unwrap(),
            "changed original bytes"
        );
        assert_eq!(fixture.stages(), 0);
        assert!(!fixture.0.join("unsafe log").exists());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn self_reinstall_signature_rejection_preserves_destination_and_cleans_stage() {
        let fixture = ReinstallFixture::new();
        std::fs::write(
            fixture.0.join("installed helper"),
            "existing installed bytes",
        )
        .unwrap();
        let output = fixture.run(false);
        assert!(!output.status.success());
        assert_eq!(
            std::fs::read_to_string(fixture.0.join("installed helper")).unwrap(),
            "existing installed bytes"
        );
        assert!(!fixture.0.join("restart receipt").exists());
        assert_eq!(fixture.stages(), 0);
    }

    #[test]
    fn self_reinstall_rejects_security_and_runtime_downgrades() {
        for (candidate, running) in [
            ("1.32.6", "1.32.7"),
            ("1.32.7-beta.1", "1.32.7"),
            ("1.32.7", "1.32.8"),
            ("1.32.8-beta.1", "1.32.8"),
            ("unknown", "1.32.7"),
            ("1.32.7", "unknown"),
        ] {
            assert!(
                !self_reinstall_version_allowed(candidate, running),
                "{candidate} from {running}"
            );
        }
        for candidate in ["1.32.7", "1.32.7+build.7", "1.32.8", "1.33.0"] {
            assert!(
                self_reinstall_version_allowed(candidate, "1.32.7"),
                "{candidate}"
            );
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn native_codesign_parses_and_enforces_inline_requirements() {
        let verify = |requirement: &str| {
            std::process::Command::new("/usr/bin/codesign")
                .args(["--verify", "--strict", "-R"])
                .arg(code_requirement_argument(requirement))
                .arg("/usr/bin/true")
                .output()
                .unwrap()
        };
        let accepted = verify("anchor apple");
        assert!(
            accepted.status.success(),
            "{}",
            String::from_utf8_lossy(&accepted.stderr)
        );
        let rejected =
            verify("anchor apple and identifier kr.co.uulab.peterfan.deliberately-wrong");
        assert!(
            !rejected.status.success(),
            "a mismatched designated requirement must reject Apple's signed fixture"
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn helper_probe_caps_time_and_output_without_running_privileged_operations() {
        let started = std::time::Instant::now();
        assert!(bounded_command_output(
            std::process::Command::new("/bin/sleep").arg("2"),
            Duration::from_millis(30)
        )
        .is_err());
        assert!(started.elapsed() < Duration::from_secs(1));
        assert!(bounded_command_output(
            std::process::Command::new("/usr/bin/printf").args(["%05000d", "0"]),
            Duration::from_secs(1)
        )
        .is_err());
        assert_eq!(
            bounded_command_output(
                std::process::Command::new("/usr/bin/printf").arg("peterfand 1.32.5\n"),
                Duration::from_secs(1)
            )
            .unwrap(),
            b"peterfand 1.32.5\n"
        );
    }
}
