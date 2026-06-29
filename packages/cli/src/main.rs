//! `peterfan` — the command-line interface.
//!
//! The CLI is a thin presentation layer over [`peterfan_core`] and a
//! [`peterfan_core::HardwareProvider`] supplied by [`peterfan_platform`]. It
//! contains no hardware logic of its own.

mod render;

use anyhow::Result;
use clap::{Parser, Subcommand};
use owo_colors::OwoColorize;

use peterfan_core::profile::Profile;
use peterfan_core::types::{Fan, TempSensor};
use peterfan_core::HardwareProvider;

#[derive(Parser)]
#[command(
    name = "peterfan",
    version,
    about = "Tiny fan control & hardware monitor for developers",
    long_about = "PeterFan — cross-platform fan controller and hardware monitor.\n\
                  Run without a subcommand for a full dashboard."
)]
struct Cli {
    /// Use the fully simulated backend instead of real hardware.
    #[arg(long, global = true)]
    mock: bool,

    /// Emit machine-readable JSON instead of formatted text.
    #[arg(long, global = true)]
    json: bool,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Show a full dashboard: hardware, temperatures, fans, profile (default).
    Status,
    /// Show temperature sensors.
    Temps,
    /// Show fans and their current speeds.
    Fans,
    /// List profiles, or preview/apply one: `peterfan profile gaming`.
    Profile {
        /// Profile name (silent, balanced, gaming, performance, maximum).
        name: Option<String>,
    },
    /// Show a profile's fan curve as a table and ASCII plot.
    Curve {
        /// Profile whose curve to show (default: balanced).
        name: Option<String>,
    },
    /// Show detected hardware (CPU, RAM, OS, …).
    Hardware,
    /// Diagnose the active backend, its capabilities, and privileges.
    Doctor,
}

fn main() {
    let cli = Cli::parse();
    if let Err(e) = run(cli) {
        eprintln!("{} {e}", "error:".red().bold());
        std::process::exit(1);
    }
}

fn run(cli: Cli) -> Result<()> {
    let provider: Box<dyn HardwareProvider> = if cli.mock {
        peterfan_platform::mock()
    } else {
        peterfan_platform::detect()
    };

    match cli.command.unwrap_or(Command::Status) {
        Command::Status => cmd_status(provider.as_ref(), cli.json),
        Command::Temps => cmd_temps(provider.as_ref(), cli.json),
        Command::Fans => cmd_fans(provider.as_ref(), cli.json),
        Command::Profile { name } => cmd_profile(provider.as_ref(), name, cli.json),
        Command::Curve { name } => cmd_curve(name, cli.json),
        Command::Hardware => cmd_hardware(provider.as_ref(), cli.json),
        Command::Doctor => cmd_doctor(provider.as_ref(), cli.json),
    }
}

/// Sensor readings plus whether they came from the simulated backend.
struct Sensors {
    temps: Vec<TempSensor>,
    fans: Vec<Fan>,
    simulated: bool,
}

/// Read temps + fans from `provider`, transparently falling back to the mock
/// backend (and flagging the data as simulated) when the real backend can't
/// read sensors yet. This is what makes `peterfan temps` show *something*
/// useful on macOS today instead of an error.
fn read_sensors(provider: &dyn HardwareProvider) -> Result<Sensors> {
    let caps = provider.capabilities();
    if caps.read_temps && caps.read_fans {
        return Ok(Sensors {
            temps: provider.temperatures()?,
            fans: provider.fans()?,
            simulated: false,
        });
    }
    let mock = peterfan_platform::mock();
    Ok(Sensors {
        temps: mock.temperatures()?,
        fans: mock.fans()?,
        simulated: true,
    })
}

fn simulated_note() -> String {
    format!(
        "{}",
        "(simulated — real sensor reading not implemented on this backend yet)"
            .italic()
            .dimmed()
    )
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

fn cmd_status(provider: &dyn HardwareProvider, json: bool) -> Result<()> {
    let info = provider.hardware_info()?;
    let sensors = read_sensors(provider)?;

    if json {
        let value = serde_json::json!({
            "backend": provider.name(),
            "simulated_sensors": sensors.simulated,
            "hardware": info,
            "temps": sensors.temps,
            "fans": sensors.fans,
        });
        println!("{}", serde_json::to_string_pretty(&value)?);
        return Ok(());
    }

    println!("{}", render::banner(env!("CARGO_PKG_VERSION")));
    println!("{} {}", "backend:".dimmed(), provider.name().bold());
    println!();

    println!("{}", render::heading("Temperatures"));
    if sensors.simulated {
        println!("  {}", simulated_note());
    }
    print_temps(&sensors.temps);
    println!();

    println!("{}", render::heading("Fans"));
    print_fans(&sensors.fans);
    println!();

    println!(
        "{} {}",
        render::heading("Hardware"),
        format!("· {}", info.cpu).dimmed()
    );

    Ok(())
}

fn cmd_temps(provider: &dyn HardwareProvider, json: bool) -> Result<()> {
    let sensors = read_sensors(provider)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&sensors.temps)?);
        return Ok(());
    }
    if sensors.simulated {
        println!("{}", simulated_note());
    }
    print_temps(&sensors.temps);
    Ok(())
}

fn cmd_fans(provider: &dyn HardwareProvider, json: bool) -> Result<()> {
    let sensors = read_sensors(provider)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&sensors.fans)?);
        return Ok(());
    }
    if sensors.simulated {
        println!("{}", simulated_note());
    }
    print_fans(&sensors.fans);
    Ok(())
}

fn cmd_hardware(provider: &dyn HardwareProvider, json: bool) -> Result<()> {
    let info = provider.hardware_info()?;
    if json {
        println!("{}", serde_json::to_string_pretty(&info)?);
        return Ok(());
    }
    println!("{}", render::heading("Hardware"));
    print_kv("CPU", &info.cpu);
    print_kv("GPU", info.gpu.as_deref().unwrap_or("—"));
    print_kv("Motherboard", info.motherboard.as_deref().unwrap_or("—"));
    print_kv("Memory", info.memory.as_deref().unwrap_or("—"));
    print_kv("OS", &info.os);
    Ok(())
}

fn cmd_profile(provider: &dyn HardwareProvider, name: Option<String>, json: bool) -> Result<()> {
    let Some(name) = name else {
        return list_profiles(json);
    };

    let profile = Profile::parse(&name).ok_or_else(|| {
        anyhow::anyhow!(
            "unknown profile '{name}'. Try one of: {}",
            Profile::all()
                .iter()
                .map(|p| p.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )
    })?;

    let curve = profile.default_curve();
    let sensors = read_sensors(provider)?;
    // Use the hottest CPU sensor as the curve input, falling back to the
    // hottest reading overall.
    let temp = hottest_temp(&sensors.temps);
    let duty = curve.duty_at(temp);

    let caps = provider.capabilities();
    if caps.control_fans {
        let mut applied = Vec::new();
        for fan in sensors.fans.iter().filter(|f| f.controllable) {
            provider.set_fan_duty(&fan.id, duty)?;
            applied.push(fan.label.clone());
        }
        if json {
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "profile": profile.as_str(),
                    "input_temp_c": temp,
                    "duty_percent": duty,
                    "applied_to": applied,
                }))?
            );
        } else {
            println!(
                "Applied profile {} → {duty}% (at {:.0}°C) to: {}",
                profile.as_str().bold(),
                temp,
                applied.join(", ")
            );
        }
    } else {
        // Read-only backend: preview only.
        if json {
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "profile": profile.as_str(),
                    "input_temp_c": temp,
                    "duty_percent": duty,
                    "applied": false,
                    "reason": "backend cannot control fans",
                }))?
            );
        } else {
            println!(
                "Profile {} would set fans to {duty}% at {:.0}°C.",
                profile.as_str().bold(),
                temp
            );
            println!(
                "  {}",
                "this backend is read-only — not applied. Use --mock to try it live.".dimmed()
            );
        }
    }
    Ok(())
}

fn list_profiles(json: bool) -> Result<()> {
    if json {
        let arr: Vec<_> = Profile::all()
            .iter()
            .map(|p| {
                serde_json::json!({
                    "name": p.as_str(),
                    "description": p.description(),
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&arr)?);
        return Ok(());
    }
    println!("{}", render::heading("Profiles"));
    for p in Profile::all() {
        println!("  {:<12} {}", p.as_str().bold(), p.description().dimmed());
    }
    println!();
    println!(
        "  {}",
        "Apply with: peterfan profile <name>   ·   inspect: peterfan curve <name>".dimmed()
    );
    Ok(())
}

fn cmd_curve(name: Option<String>, json: bool) -> Result<()> {
    let profile = match name {
        Some(n) => Profile::parse(&n).ok_or_else(|| anyhow::anyhow!("unknown profile '{n}'"))?,
        None => Profile::Balanced,
    };
    let curve = profile.default_curve();

    if json {
        println!("{}", serde_json::to_string_pretty(curve.points())?);
        return Ok(());
    }

    println!(
        "{} {}",
        render::heading("Fan curve"),
        format!("· {}", profile.as_str()).dimmed()
    );
    for p in curve.points() {
        println!(
            "  {:>5.0}°C  →  {:>3}%   {}",
            p.temp_c,
            p.duty_percent,
            render::bar(p.duty_percent as f32, 100.0)
        );
    }
    println!();
    // Show a few interpolated samples so the curve's shape is obvious.
    print!("  {}: ", "samples".dimmed());
    for t in [30, 45, 60, 75, 90] {
        print!("{t}°C={}%  ", curve.duty_at(t as f32));
    }
    println!();
    Ok(())
}

fn cmd_doctor(provider: &dyn HardwareProvider, json: bool) -> Result<()> {
    let caps = provider.capabilities();
    let info = provider.hardware_info().ok();
    let elevated = is_elevated();

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "os": std::env::consts::OS,
                "arch": std::env::consts::ARCH,
                "backend": provider.name(),
                "elevated": elevated,
                "capabilities": {
                    "read_temps": caps.read_temps,
                    "read_fans": caps.read_fans,
                    "control_fans": caps.control_fans,
                },
                "hardware": info,
            }))?
        );
        return Ok(());
    }

    println!("{}", render::heading("PeterFan doctor"));
    print_kv(
        "OS / arch",
        &format!("{} / {}", std::env::consts::OS, std::env::consts::ARCH),
    );
    print_kv("Backend", provider.name());
    print_kv("Elevated", if elevated { "yes" } else { "no" });
    println!();
    println!("{}", render::heading("Capabilities"));
    print_check("read temperatures", caps.read_temps);
    print_check("read fans", caps.read_fans);
    print_check("control fans", caps.control_fans);

    if !caps.read_temps {
        println!();
        println!(
            "  {} sensor reading is not implemented for the '{}' backend yet;",
            "note:".yellow().bold(),
            provider.name()
        );
        println!("        the CLI falls back to simulated data. Try `peterfan --mock status`.");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Shared printers
// ---------------------------------------------------------------------------

fn print_temps(temps: &[TempSensor]) {
    for t in temps {
        println!(
            "  {} {:<14} {:>6}  {}",
            render::kind_label(t.kind),
            t.label,
            render::temp_colored(t.value),
            render::temp_bar_colored(t.value),
        );
    }
}

fn print_fans(fans: &[Fan]) {
    for f in fans {
        let duty = f
            .duty_percent
            .map(|d| format!("{d:>3}%"))
            .unwrap_or_else(|| "  —".to_string());
        let bar = f
            .duty_percent
            .map(|d| render::bar(d as f32, 100.0))
            .unwrap_or_default();
        println!("  {:<14} {:>5} RPM  {}  {}", f.label, f.rpm, duty, bar,);
    }
}

fn print_kv(key: &str, value: &str) {
    println!("  {:<13} {}", format!("{key}:").dimmed(), value);
}

fn print_check(label: &str, ok: bool) {
    let mark = if ok {
        "✓".green().to_string()
    } else {
        "✗".red().to_string()
    };
    println!("  {mark} {label}");
}

fn hottest_temp(temps: &[TempSensor]) -> f32 {
    temps
        .iter()
        .map(|t| t.value.0)
        .fold(f32::MIN, f32::max)
        .max(0.0)
}

/// Whether the process is running with elevated privileges.
#[cfg(unix)]
fn is_elevated() -> bool {
    // SAFETY: geteuid() is always safe to call and has no preconditions.
    unsafe { libc::geteuid() == 0 }
}

#[cfg(not(unix))]
fn is_elevated() -> bool {
    false
}
