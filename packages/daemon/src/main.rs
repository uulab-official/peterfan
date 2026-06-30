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
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::sleep;
use std::time::Duration;

use anyhow::{bail, Result};
use clap::Parser;

use peterfan_core::config::{Config, RuleContext};
use peterfan_core::profile::Profile;
use peterfan_core::{HardwareProvider, SystemMonitor};

/// Set by the signal handler; the control loop checks it and exits cleanly.
static STOP: AtomicBool = AtomicBool::new(false);

/// Live control state, shared between the control loop and the IPC server.
#[derive(Clone)]
struct State {
    /// Active profile whose curve is applied (effective; reflects rules too).
    profile: Profile,
    /// When true, fans are handed back to the OS (no curve applied).
    auto: bool,
    /// When true, the profile was set manually (IPC) and overrides automation
    /// rules until `rules`/`auto` is requested.
    manual: bool,
    /// Backend name (e.g. "macos", "mock") — surfaced in IPC replies so the UI
    /// can tell real control from a simulated daemon.
    backend: String,
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

    let shared = Arc::new(Mutex::new(State {
        profile,
        auto: false,
        manual: false,
        backend: provider.name().to_string(),
    }));

    // IPC server (so the menu-bar app can switch profile / go auto without
    // root). Not started for one-shot runs.
    if !cli.once {
        spawn_ipc_server(Arc::clone(&shared));
    }

    println!(
        "peterfand: profile={} interval={interval}s critical={critical:.0}°C rules={} fans={} backend={}",
        profile.as_str(),
        cfg.rules.len(),
        fan_ids.len(),
        provider.name()
    );

    // Run the control loop, then ALWAYS restore automatic control — even on a
    // panic — so we never leave the fans forced.
    let loop_result = std::panic::catch_unwind(AssertUnwindSafe(|| {
        control_loop(
            provider.as_ref(),
            monitor.as_mut(),
            &cfg,
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
    cfg: &Config,
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
                cfg.active_profile(&ctx).unwrap_or(base)
            };
            // Reflect the effective profile so `status` is accurate.
            shared.lock().expect("state poisoned").profile = profile;

            let (duty, why) = if hottest >= critical {
                (100u8, "CRITICAL")
            } else {
                (profile.default_curve().duty_at(hottest), profile.as_str())
            };
            for id in fan_ids {
                provider.set_fan_duty(id, duty)?;
            }
            let src = if state.manual { "manual" } else { "auto-rule" };
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
            format!("ok auto ({backend})")
        }
        // Hand control back to the automation rules (clear manual override).
        Some("rules") => {
            let mut s = shared.lock().expect("state poisoned");
            s.manual = false;
            s.auto = false;
            format!("ok rules ({backend})")
        }
        Some("profile") => match parts.next().and_then(Profile::parse) {
            Some(p) => {
                let mut s = shared.lock().expect("state poisoned");
                s.profile = p;
                s.auto = false;
                s.manual = true;
                format!("ok {} ({backend})", p.as_str())
            }
            None => "error: unknown profile".into(),
        },
        Some("status") => {
            let s = shared.lock().expect("state poisoned");
            let mode = if s.auto {
                "auto"
            } else if s.manual {
                "manual"
            } else {
                "rules"
            };
            format!("ok {mode} {} ({backend})", s.profile.as_str())
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
