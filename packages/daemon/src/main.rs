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
use std::thread::sleep;
use std::time::Duration;

use anyhow::{bail, Result};
use clap::Parser;
use serde::{Deserialize, Serialize};

use peterfan_core::config::RuleContext;
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
const FAN_WRITE_RETRY_BASE_SECS: u64 = 5;
const FAN_WRITE_RETRY_MAX_SECS: u64 = 60;

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
    /// When true, fans are handed back to the OS (no curve applied).
    auto: bool,
    /// When true, the profile was set manually (IPC) and overrides automation
    /// rules until `rules`/`auto` is requested.
    manual: bool,
    /// Backend name (e.g. "macos", "mock") — surfaced in IPC replies so the UI
    /// can tell real control from a simulated daemon.
    backend: String,
    /// Live copy of the config, refreshed by `reload`. The base profile and
    /// automation rules are read from here each control-loop tick.
    config: peterfan_core::config::Config,
    /// Most recent temperature readings from the last control-loop tick.
    /// Used by the `temps` IPC command so the CLI can skip SMC init.
    last_temps: Vec<peterfan_core::types::TempSensor>,
    /// Most recent fan states from the last control-loop tick.
    last_fans: Vec<peterfan_core::types::Fan>,
    /// Most recent power draw in watts.
    last_power_w: Option<f32>,
    /// Runtime safety telemetry. This is intentionally not persisted: every
    /// daemon launch starts by proving that sensors and writes work again.
    control_health: ControlHealth,
}

#[derive(Clone, Debug, Default, Serialize)]
struct ControlHealth {
    failsafe_active: bool,
    sensor_failure_count: u64,
    consecutive_sensor_failures: u32,
    fan_write_failure_count: u64,
    consecutive_fan_write_failures: u32,
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
}

fn main() {
    let cli = Cli::parse();
    if let Err(e) = run(cli) {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn run(cli: Cli) -> Result<()> {
    // Resolve settings: explicit flags win, otherwise fall back to the config
    // file, otherwise the built-in defaults.
    let cfg = peterfan_platform::config::load();

    let profile = match &cli.profile {
        Some(name) => {
            Profile::parse(name).ok_or_else(|| anyhow::anyhow!("unknown profile '{name}'"))?
        }
        None => cfg.profile,
    };
    let interval = cli.interval.unwrap_or(cfg.interval_secs).max(1);
    let critical = cli.critical.unwrap_or(cfg.critical_temp_c);

    let provider: Box<dyn HardwareProvider> = if cli.mock {
        peterfan_platform::mock()
    } else {
        peterfan_platform::detect()
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

    install_signal_handlers();

    // A monitor for battery state (used by automation rules).
    let mut monitor: Box<dyn SystemMonitor> = if cli.mock {
        peterfan_platform::mock_monitor()
    } else {
        peterfan_platform::system_monitor()
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
            auto: false,
            manual: false,
            backend: provider.name().to_string(),
            config: resolved_cfg,
            last_temps: Vec::new(),
            last_fans: Vec::new(),
            last_power_w: None,
            control_health: ControlHealth::default(),
        };
        // Restore the last user-chosen mode so a reboot doesn't reset fan settings.
        if let Some(saved) = load_saved_state() {
            match saved.mode.as_str() {
                "auto" => {
                    s.auto = true;
                }
                "hold" => {
                    if let Some(pct) = saved.hold_pct {
                        s.held_duty = Some(pct);
                        s.manual = true;
                    }
                }
                "profile" => {
                    if let Some(name) = &saved.profile {
                        if let Some(p) = Profile::parse(name) {
                            s.profile = p;
                            s.manual = true;
                        }
                    }
                }
                _ => {} // "rules" or unknown → keep defaults
            }
            s.fan_overrides = saved.fan_overrides;
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
    if !cli.once {
        spawn_ipc_server(Arc::clone(&shared));
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
    let loop_result = std::panic::catch_unwind(AssertUnwindSafe(|| {
        control_loop(
            provider.as_ref(),
            monitor.as_mut(),
            profile,
            &fan_ids,
            cli.once,
            &shared,
        )
    }));

    for id in &fan_ids {
        let _ = provider.set_fan_auto(id);
    }
    #[cfg(unix)]
    for p in peterfan_platform::ipc::PATHS {
        let _ = std::fs::remove_file(p);
    }
    println!(
        "peterfand: restored {} fan(s) to automatic control",
        fan_ids.len()
    );

    match loop_result {
        Ok(r) => r,
        Err(_) => bail!("control loop panicked (fans restored to auto)"),
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
    let mut slept = 0u64;
    while slept < interval * 1000 && !STOP.load(Ordering::Relaxed) {
        sleep(Duration::from_millis(200));
        slept += 200;
        if APPLY_NOW.swap(false, Ordering::Relaxed) {
            break;
        }
    }
}

fn control_loop(
    provider: &dyn HardwareProvider,
    monitor: &mut dyn SystemMonitor,
    base: Profile,
    fan_ids: &[String],
    once: bool,
    shared: &Arc<Mutex<State>>,
) -> Result<()> {
    let mut auto_applied = false;
    let mut was_critical = false;
    // Track last-logged duty/mode so we only log on changes (keeps the log lean).
    let mut last_duty: Option<u8> = None;
    let mut last_src = String::new();
    while !STOP.load(Ordering::Relaxed) {
        monitor.refresh();
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
        let fans_now = provider.fans().unwrap_or_default();
        let power_now = provider.power_watts();
        {
            let mut s = shared.lock().expect("state poisoned");
            s.last_temps = temps.clone();
            s.last_fans = fans_now.clone();
            s.last_power_w = power_now;
        }
        let (representative_temp, safety_temp) = match control_temperatures(&temps) {
            Ok(values) if temperature_read_error.is_none() => {
                let mut s = shared.lock().expect("state poisoned");
                s.control_health.consecutive_sensor_failures = 0;
                s.control_health.last_sensor_ok_unix = Some(unix_now());
                values
            }
            _ => {
                let reason = temperature_read_error
                    .unwrap_or_else(|| "no trustworthy temperature reading".to_string());
                let restore_errors = restore_fans_to_auto(provider, fan_ids);
                let detail = if restore_errors.is_empty() {
                    format!("sensor fail-safe: {reason}; fans returned to OS auto")
                } else {
                    format!(
                        "sensor fail-safe: {reason}; auto restore errors: {}",
                        restore_errors.join(", ")
                    )
                };
                let first_failure = !state.control_health.failsafe_active;
                {
                    let mut s = shared.lock().expect("state poisoned");
                    s.control_health.failsafe_active = true;
                    s.control_health.sensor_failure_count =
                        s.control_health.sensor_failure_count.saturating_add(1);
                    s.control_health.consecutive_sensor_failures = s
                        .control_health
                        .consecutive_sensor_failures
                        .saturating_add(1);
                    s.control_health.last_error = Some(detail.clone());
                    if !restore_errors.is_empty() {
                        s.control_health.fan_write_failure_count = s
                            .control_health
                            .fan_write_failure_count
                            .saturating_add(restore_errors.len() as u64);
                    }
                }
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

        if state
            .control_health
            .retry_after_unix
            .is_some_and(|retry_after| unix_now() < retry_after)
        {
            if once {
                break;
            }
            sleep_control_interval(interval);
            continue;
        }

        let mut write_errors = Vec::new();

        if auto {
            // Per-fan overrides still apply on top of the global "auto" mode
            // (e.g. pin one fan manually while the rest follow the OS). An
            // override never survives a critical temperature — `effective_duty`
            // forces the fan to 100% exactly like the non-auto branch below.
            let critical_now = safety_temp >= critical;
            for id in fan_ids {
                let has_override = state.fan_overrides.contains_key(id);
                let result = if has_override {
                    provider.set_fan_duty(
                        id,
                        effective_duty(&state.fan_overrides, id, 100, critical_now),
                    )
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
            let profile = if state.manual {
                state.profile
            } else {
                state.config.active_profile(&ctx).unwrap_or(base)
            };
            // Reflect the effective profile so `status` is accurate.
            shared.lock().expect("state poisoned").profile = profile;

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
                let effective = effective_duty(&state.fan_overrides, id, duty, critical_now);
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
            }
            if first_failure {
                eprintln!("peterfand: {detail}");
            }
        }

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

/// Accept IPC connections and apply commands to the shared state.
#[cfg(unix)]
fn spawn_ipc_server(shared: Arc<Mutex<State>>) {
    use std::io::{BufRead, BufReader, Write};

    let (listener, path) = match peterfan_platform::ipc::bind_listener() {
        Ok(x) => x,
        Err(e) => {
            eprintln!("peterfand: IPC disabled ({e})");
            return;
        }
    };
    println!("peterfand: listening on {}", path.display());

    std::thread::spawn(move || {
        for conn in listener.incoming() {
            let Ok(mut stream) = conn else { continue };
            let Ok(clone) = stream.try_clone() else {
                continue;
            };
            let mut line = String::new();
            if BufReader::new(clone).read_line(&mut line).is_err() {
                continue;
            }
            let reply = handle_command(line.trim(), &shared);
            let _ = writeln!(stream, "{reply}");
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
fn start_self_reinstall(path: &str) -> Result<String, String> {
    let source = validate_self_reinstall_source(path)?;
    let source = shell_quote(&source.to_string_lossy());
    let daemon_bin = peterfan_platform::daemon_install::DAEMON_BIN;
    let label = peterfan_platform::daemon_install::DAEMON_LABEL;
    let script = format!(
        "set -e\n\
         /usr/bin/codesign --verify --strict --verbose=2 {source}\n\
         /usr/bin/codesign -dv --verbose=4 {source} 2>&1 | /usr/bin/grep -q 'TeamIdentifier=N99FMBQ662'\n\
         (sleep 0.3; /usr/bin/install -m 755 {source} {daemon_bin}; /bin/launchctl kickstart -k system/{label}) >/tmp/peterfan-self-reinstall.log 2>&1 &\n"
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

#[cfg(not(target_os = "macos"))]
fn start_self_reinstall(_path: &str) -> Result<String, String> {
    Err("error: fan-control reinstall is only available on macOS".to_string())
}

#[cfg(unix)]
fn handle_command(line: &str, shared: &Arc<Mutex<State>>) -> String {
    let backend = shared.lock().expect("state poisoned").backend.clone();
    let mut parts = line.split_whitespace();
    match parts.next() {
        Some("ping") => format!("ok peterfand ({backend})"),
        Some("auto") => {
            let mut s = shared.lock().expect("state poisoned");
            s.auto = true;
            s.held_duty = None;
            s.fan_overrides.clear();
            save_state(&s);
            APPLY_NOW.store(true, Ordering::Relaxed);
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
            APPLY_NOW.store(true, Ordering::Relaxed);
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
                APPLY_NOW.store(true, Ordering::Relaxed);
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
                APPLY_NOW.store(true, Ordering::Relaxed);
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
                    s.fan_overrides.insert(id.clone(), d);
                    save_state(&s);
                    APPLY_NOW.store(true, Ordering::Relaxed);
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
                APPLY_NOW.store(true, Ordering::Relaxed);
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
                "temps": s.last_temps,
                "fans": s.last_fans,
                "power_w": s.last_power_w,
                "mode": mode,
                "backend": s.backend,
                "fan_overrides": s.fan_overrides,
                "control_health": s.control_health,
            })) {
                Ok(json) => format!("ok {json}"),
                Err(_) => "error: serialization failed".into(),
            }
        }
        Some("reload") => {
            let new_cfg = peterfan_platform::config::load();
            let rules = new_cfg.rules.len();
            {
                let mut s = shared.lock().expect("state poisoned");
                s.config = new_cfg;
            }
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

    fn test_state() -> Arc<Mutex<State>> {
        Arc::new(Mutex::new(State {
            profile: Profile::Balanced,
            held_duty: None,
            fan_overrides: std::collections::HashMap::new(),
            auto: false,
            manual: false,
            backend: "mock".to_string(),
            config: peterfan_core::config::Config::default(),
            last_temps: Vec::new(),
            last_fans: Vec::new(),
            last_power_w: None,
            control_health: ControlHealth::default(),
        }))
    }

    struct FaultProvider {
        fail_temperatures: bool,
        fail_writes: bool,
        duty_calls: std::sync::atomic::AtomicUsize,
        auto_calls: std::sync::atomic::AtomicUsize,
    }

    impl FaultProvider {
        fn new(fail_temperatures: bool, fail_writes: bool) -> Self {
            Self {
                fail_temperatures,
                fail_writes,
                duty_calls: std::sync::atomic::AtomicUsize::new(0),
                auto_calls: std::sync::atomic::AtomicUsize::new(0),
            }
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
            _duty_percent: u8,
        ) -> peterfan_core::error::Result<()> {
            self.duty_calls.fetch_add(1, Ordering::Relaxed);
            if self.fail_writes {
                Err(peterfan_core::error::CoreError::Hardware(
                    "injected fan write failure".into(),
                ))
            } else {
                Ok(())
            }
        }

        fn set_fan_auto(&self, _fan_id: &str) -> peterfan_core::error::Result<()> {
            self.auto_calls.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }
    }

    #[test]
    fn sensor_failure_restores_os_auto_without_writing_duty() {
        STOP.store(false, Ordering::Relaxed);
        let provider = FaultProvider::new(true, false);
        let mut monitor = peterfan_platform::mock_monitor();
        let shared = test_state();

        control_loop(
            &provider,
            monitor.as_mut(),
            Profile::Balanced,
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
            Profile::Balanced,
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
    fn fan_write_retry_delay_uses_bounded_exponential_backoff() {
        assert_eq!(fan_write_retry_delay_secs(1), 5);
        assert_eq!(fan_write_retry_delay_secs(2), 10);
        assert_eq!(fan_write_retry_delay_secs(3), 20);
        assert_eq!(fan_write_retry_delay_secs(4), 40);
        assert_eq!(fan_write_retry_delay_secs(5), 60);
        assert_eq!(fan_write_retry_delay_secs(100), 60);
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
            Profile::Balanced,
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
        assert_eq!(json["control_health"]["failsafe_active"], true);
        assert_eq!(json["control_health"]["sensor_failure_count"], 2);
        assert_eq!(json["control_health"]["last_error"], "sensor unavailable");
    }

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
        assert!(reply.starts_with("No such file") || reply.starts_with("reinstall source"));
    }
}
