//! `peterfand` — the PeterFan fan-control daemon.
//!
//! Applies a fan curve continuously: every interval it reads the hottest
//! temperature, evaluates the chosen profile's curve, and drives the fans to
//! the resulting duty. Two safety behaviors are built in:
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
use peterfan_core::{HardwareProvider, SystemMonitor};

/// Set by the signal handler; the control loop checks it and exits cleanly.
static STOP: AtomicBool = AtomicBool::new(false);

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
        let mut s = State {
            profile,
            held_duty: None,
            auto: false,
            manual: false,
            backend: provider.name().to_string(),
            config: cfg.clone(),
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
    // root). Not started for one-shot runs.
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
            interval,
            critical,
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

#[allow(clippy::too_many_arguments)]
fn control_loop(
    provider: &dyn HardwareProvider,
    monitor: &mut dyn SystemMonitor,
    base: Profile,
    fan_ids: &[String],
    interval: u64,
    critical: f32,
    once: bool,
    shared: &Arc<Mutex<State>>,
) -> Result<()> {
    let mut auto_applied = false;
    let mut was_critical = false;
    while !STOP.load(Ordering::Relaxed) {
        monitor.refresh();
        let state = shared.lock().expect("state poisoned").clone();

        if state.auto {
            if !auto_applied {
                for id in fan_ids {
                    provider.set_fan_auto(id)?;
                }
                println!("peterfand: auto (OS-managed)");
                auto_applied = true;
            }
        } else {
            auto_applied = false;
            let temps = provider.temperatures().unwrap_or_default();
            let hottest = temps.iter().map(|t| t.value.0).fold(0.0_f32, f32::max);

            // Choose the profile: a manual (IPC) choice wins; otherwise the
            // first matching automation rule; otherwise the base profile.
            let on_ac = match monitor.battery() {
                Some(b) => matches!(b.state.as_str(), "charging" | "full"),
                None => true, // no battery → treat as AC (desktop)
            };
            let ctx = RuleContext {
                on_ac,
                cpu_temp_c: hottest,
                hour: local_hour(),
            };
            let profile = if state.manual {
                state.profile
            } else {
                state.config.active_profile(&ctx).unwrap_or(base)
            };
            // Reflect the effective profile so `status` is accurate.
            shared.lock().expect("state poisoned").profile = profile;

            let (duty, why): (u8, String) = if hottest >= critical {
                (100, "CRITICAL".into())
            } else if let Some(d) = state.held_duty {
                (d, format!("hold:{d}%"))
            } else {
                (profile.default_curve().duty_at(hottest), profile.as_str().into())
            };
            for id in fan_ids {
                provider.set_fan_duty(id, duty)?;
            }
            let src = if state.held_duty.is_some() {
                "hold"
            } else if state.manual {
                "manual"
            } else {
                "auto-rule"
            };
            println!("peterfand: {hottest:.0}°C -> {duty}% ({why}) [{src} ac={on_ac}]");

            // Edge-triggered critical-temperature alert (with hysteresis).
            if hottest >= critical && !was_critical {
                notify(
                    "PeterFan — critical temperature",
                    &format!("{hottest:.0}°C ≥ {critical:.0}°C · fans forced to 100%"),
                );
                was_critical = true;
            } else if hottest < critical - 5.0 && was_critical {
                notify(
                    "PeterFan",
                    &format!("Temperature back to normal ({hottest:.0}°C)"),
                );
                was_critical = false;
            }
        }

        if once {
            break;
        }
        // Sleep in small slices so a signal stops us promptly.
        let mut slept = 0u64;
        while slept < interval * 1000 && !STOP.load(Ordering::Relaxed) {
            sleep(Duration::from_millis(200));
            slept += 200;
        }
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
            save_state(&s);
            format!("ok auto ({backend})")
        }
        // Hand control back to the automation rules (clear manual override).
        Some("rules") => {
            let mut s = shared.lock().expect("state poisoned");
            s.manual = false;
            s.auto = false;
            s.held_duty = None;
            save_state(&s);
            format!("ok rules ({backend})")
        }
        Some("profile") => match parts.next().and_then(Profile::parse) {
            Some(p) => {
                let mut s = shared.lock().expect("state poisoned");
                s.profile = p;
                s.auto = false;
                s.held_duty = None;
                s.manual = true;
                save_state(&s);
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
                save_state(&s);
                format!("ok hold:{d}% ({backend})")
            }
            None => "error: hold requires a percent 0-100".into(),
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
        _ => "error: unknown command".into(),
    }
}

/// Post a desktop notification (best-effort).
#[cfg(target_os = "macos")]
fn notify(title: &str, message: &str) {
    let script = format!(
        "display notification {} with title {}",
        applescript_quote(message),
        applescript_quote(title)
    );
    let _ = std::process::Command::new("osascript")
        .arg("-e")
        .arg(script)
        .status();
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
