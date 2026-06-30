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
use std::thread::sleep;
use std::time::Duration;

use anyhow::{bail, Result};
use clap::Parser;

use peterfan_core::profile::Profile;
use peterfan_core::HardwareProvider;

/// Set by the signal handler; the control loop checks it and exits cleanly.
static STOP: AtomicBool = AtomicBool::new(false);

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
    /// Profile whose curve to apply (silent, balanced, gaming, performance, maximum).
    #[arg(long, default_value = "balanced")]
    profile: String,
    /// Seconds between curve updates.
    #[arg(long, default_value_t = 2)]
    interval: u64,
    /// Above this temperature (°C) the fans are forced to 100%.
    #[arg(long, default_value_t = 90.0)]
    critical: f32,
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
    let profile = Profile::parse(&cli.profile)
        .ok_or_else(|| anyhow::anyhow!("unknown profile '{}'", cli.profile))?;
    let curve = profile.default_curve();

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

    println!(
        "peterfand: profile={} interval={}s critical={:.0}°C fans={} backend={}",
        profile.as_str(),
        cli.interval,
        cli.critical,
        fan_ids.len(),
        provider.name()
    );

    // Run the control loop, then ALWAYS restore automatic control — even on a
    // panic — so we never leave the fans forced.
    let loop_result =
        std::panic::catch_unwind(AssertUnwindSafe(|| control_loop(provider.as_ref(), &curve, &fan_ids, &cli)));

    for id in &fan_ids {
        let _ = provider.set_fan_auto(id);
    }
    println!("peterfand: restored {} fan(s) to automatic control", fan_ids.len());

    match loop_result {
        Ok(r) => r,
        Err(_) => bail!("control loop panicked (fans restored to auto)"),
    }
}

fn control_loop(
    provider: &dyn HardwareProvider,
    curve: &peterfan_core::curve::FanCurve,
    fan_ids: &[String],
    cli: &Cli,
) -> Result<()> {
    while !STOP.load(Ordering::Relaxed) {
        let temps = provider.temperatures().unwrap_or_default();
        let hottest = temps
            .iter()
            .map(|t| t.value.0)
            .fold(0.0_f32, f32::max);

        let (duty, why) = if hottest >= cli.critical {
            (100u8, "CRITICAL")
        } else {
            (curve.duty_at(hottest), "curve")
        };

        for id in fan_ids {
            provider.set_fan_duty(id, duty)?;
        }
        println!("peterfand: {hottest:.0}°C -> {duty}% ({why})");

        if cli.once {
            break;
        }
        // Sleep in small slices so a signal stops us promptly.
        let mut slept = 0u64;
        while slept < cli.interval * 1000 && !STOP.load(Ordering::Relaxed) {
            sleep(Duration::from_millis(200));
            slept += 200;
        }
    }
    Ok(())
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
