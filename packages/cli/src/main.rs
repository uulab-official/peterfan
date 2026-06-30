//! `peterfan` — the command-line interface.
//!
//! The CLI is a thin presentation layer over [`peterfan_core`] and two backend
//! seams supplied by [`peterfan_platform`]:
//!
//! - a [`SystemMonitor`] for system metrics (CPU, memory, disk, network,
//!   processes, battery) — real and cross-platform via `sysinfo`;
//! - a [`HardwareProvider`] for thermal hardware (temperatures, fans).
//!
//! It contains no hardware logic of its own.

mod render;

use std::time::Duration;

use anyhow::Result;
use clap::{Parser, Subcommand};
use owo_colors::OwoColorize;

use peterfan_core::metrics::ProcSort;
use peterfan_core::profile::Profile;
use peterfan_core::types::{Fan, TempSensor};
use peterfan_core::{HardwareProvider, SystemMonitor};

/// Delay between the two metric samples; usage % and network rates are deltas.
const SAMPLE_MS: u64 = 300;

#[derive(Parser)]
#[command(
    name = "peterfan",
    version,
    about = "Tiny hardware monitor & fan controller for developers",
    long_about = "PeterFan — cross-platform system monitor and fan controller.\n\
                  Run without a subcommand for a full dashboard."
)]
struct Cli {
    /// Use fully simulated backends instead of real hardware.
    #[arg(long, global = true)]
    mock: bool,

    /// Emit machine-readable JSON instead of formatted text.
    #[arg(long, global = true)]
    json: bool,

    /// Continuously refresh the command until interrupted (Ctrl-C).
    #[arg(long, global = true)]
    watch: bool,

    /// Refresh interval in seconds for --watch (default: from config, or 2).
    #[arg(long, global = true)]
    interval: Option<u64>,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand, Clone)]
enum Command {
    /// Full dashboard: system, CPU, memory, disk, network, battery, thermals (default).
    Status,
    /// CPU usage, per-core load, frequency, and load average.
    Cpu,
    /// Physical and swap memory usage.
    #[command(alias = "mem")]
    Memory,
    /// Mounted disks: capacity and usage.
    #[command(alias = "disks")]
    Disk,
    /// Network interfaces: throughput and totals.
    #[command(alias = "net")]
    Network,
    /// Top processes by CPU (or memory with --mem).
    #[command(alias = "proc")]
    Top {
        /// Rank by memory instead of CPU.
        #[arg(long)]
        mem: bool,
        /// Number of processes to show.
        #[arg(short = 'n', long, default_value_t = 10)]
        count: usize,
    },
    /// Battery charge, health, and time remaining.
    Battery,
    /// Static system information (host, OS, kernel, cores, uptime).
    System,
    /// Temperature sensors.
    Temps,
    /// Fans and their current speeds.
    Fans,
    /// Control fans: `peterfan fan set 60` (forced) or `peterfan fan auto`.
    Fan {
        #[command(subcommand)]
        action: FanAction,
    },
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
    /// Diagnose the active backends, capabilities, and privileges.
    Doctor,
    /// Show the config file path and values (`--init` creates it).
    Config {
        /// Create the config file with defaults if it doesn't exist.
        #[arg(long)]
        init: bool,
    },
}

#[derive(Subcommand, Clone)]
enum FanAction {
    /// Force fan(s) to a duty cycle (0-100%). Persists until `fan auto`.
    Set {
        /// Duty cycle percentage (0-100).
        percent: u8,
        /// Target a single fan by index (default: all controllable fans).
        #[arg(long)]
        fan: Option<usize>,
    },
    /// Restore automatic (OS-managed) fan control.
    Auto {
        /// Target a single fan by index (default: all controllable fans).
        #[arg(long)]
        fan: Option<usize>,
    },
}

impl FanAction {
    fn fan_index(&self) -> Option<usize> {
        match self {
            FanAction::Set { fan, .. } | FanAction::Auto { fan, .. } => *fan,
        }
    }
}

fn main() {
    let cli = Cli::parse();
    if let Err(e) = run(cli) {
        eprintln!("{} {e}", "error:".red().bold());
        std::process::exit(1);
    }
}

fn run(cli: Cli) -> Result<()> {
    let command = cli.command.unwrap_or(Command::Status);
    if cli.watch {
        let interval = cli
            .interval
            .unwrap_or_else(|| peterfan_platform::config::load().interval_secs)
            .max(1);
        watch_loop(command, cli.mock, cli.json, interval)
    } else {
        dispatch(command, cli.mock, cli.json)
    }
}

/// Re-run a command on an interval, clearing the screen each time.
fn watch_loop(command: Command, mock: bool, json: bool, interval: u64) -> Result<()> {
    use std::io::Write;
    loop {
        print!("\x1b[2J\x1b[H"); // clear screen, cursor home
        println!(
            "{}  ·  every {interval}s · Ctrl-C to stop\n",
            "PeterFan watch".bold().cyan()
        );
        dispatch(command.clone(), mock, json)?;
        std::io::stdout().flush().ok();
        std::thread::sleep(Duration::from_secs(interval));
    }
}

fn dispatch(command: Command, mock: bool, json: bool) -> Result<()> {
    match command {
        Command::Status => cmd_status(mock, json),
        Command::Cpu => cmd_cpu(mock, json),
        Command::Memory => cmd_memory(mock, json),
        Command::Disk => cmd_disk(mock, json),
        Command::Network => cmd_network(mock, json),
        Command::Top { mem, count } => cmd_top(mock, json, mem, count),
        Command::Battery => cmd_battery(mock, json),
        Command::System => cmd_system(mock, json),
        Command::Temps => cmd_temps(provider(mock).as_ref(), json),
        Command::Fans => cmd_fans(provider(mock).as_ref(), json),
        Command::Fan { action } => cmd_fan(provider(mock).as_ref(), action, json),
        Command::Profile { name } => cmd_profile(provider(mock).as_ref(), name, json),
        Command::Curve { name } => cmd_curve(name, json),
        Command::Hardware => cmd_hardware(provider(mock).as_ref(), json),
        Command::Doctor => cmd_doctor(mock, json),
        Command::Config { init } => cmd_config(json, init),
    }
}

fn cmd_config(json: bool, init: bool) -> Result<()> {
    if init {
        let p = peterfan_platform::config::init_default()
            .map_err(|e| anyhow::anyhow!("could not write config: {e}"))?;
        if !json {
            println!("config ready at {}", p.display());
        }
    }
    let cfg = peterfan_platform::config::load();
    let path = peterfan_platform::config::path().map(|p| p.display().to_string());
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "path": path,
                "profile": cfg.profile.as_str(),
                "interval_secs": cfg.interval_secs,
                "critical_temp_c": cfg.critical_temp_c,
            }))?
        );
        return Ok(());
    }
    println!("{}", render::heading("Config"));
    print_kv("Path", path.as_deref().unwrap_or("—"));
    print_kv("Profile", cfg.profile.as_str());
    print_kv("Interval", &format!("{}s", cfg.interval_secs));
    print_kv("Critical", &format!("{:.0}°C", cfg.critical_temp_c));
    Ok(())
}

// ---------------------------------------------------------------------------
// Backend acquisition
// ---------------------------------------------------------------------------

fn provider(mock: bool) -> Box<dyn HardwareProvider> {
    if mock {
        peterfan_platform::mock()
    } else {
        peterfan_platform::detect()
    }
}

/// A monitor sampled twice across [`SAMPLE_MS`] so usage % and rates are valid.
fn sampled_monitor(mock: bool) -> Box<dyn SystemMonitor> {
    let mut m = if mock {
        peterfan_platform::mock_monitor()
    } else {
        peterfan_platform::system_monitor()
    };
    m.refresh();
    std::thread::sleep(Duration::from_millis(SAMPLE_MS));
    m.refresh();
    m
}

// ---------------------------------------------------------------------------
// System-metrics commands
// ---------------------------------------------------------------------------

fn cmd_cpu(mock: bool, json: bool) -> Result<()> {
    let m = sampled_monitor(mock);
    let cpu = m.cpu();
    if json {
        println!("{}", serde_json::to_string_pretty(&cpu)?);
        return Ok(());
    }
    println!(
        "{} {}",
        render::heading("CPU"),
        format!("· {}", cpu.brand).dimmed()
    );
    println!(
        "  {:<8} {}  {}",
        "usage",
        render::pct_colored(cpu.usage_percent),
        render::load_bar(cpu.usage_percent)
    );
    println!("  {:<8} {} MHz", "freq", cpu.frequency_mhz);
    if let Some(la) = cpu.load_avg {
        println!(
            "  {:<8} {:.2} {:.2} {:.2}",
            "load", la.one, la.five, la.fifteen
        );
    }
    println!();
    for (i, &c) in cpu.per_core.iter().enumerate() {
        println!(
            "  core {:>2}  {}  {}",
            i,
            render::pct_colored(c),
            render::load_bar(c)
        );
    }
    Ok(())
}

fn cmd_memory(mock: bool, json: bool) -> Result<()> {
    let m = sampled_monitor(mock);
    let mem = m.memory();
    if json {
        println!("{}", serde_json::to_string_pretty(&mem)?);
        return Ok(());
    }
    println!("{}", render::heading("Memory"));
    println!(
        "  {} / {} ({})  {}",
        render::bytes(mem.used),
        render::bytes(mem.total),
        render::pct_colored(mem.used_percent).trim(),
        render::load_bar(mem.used_percent)
    );
    if mem.swap_total > 0 {
        let swap_pct = mem.swap_used as f32 / mem.swap_total as f32 * 100.0;
        println!(
            "  swap {} / {}  {}",
            render::bytes(mem.swap_used),
            render::bytes(mem.swap_total),
            render::load_bar(swap_pct)
        );
    }
    Ok(())
}

fn cmd_disk(mock: bool, json: bool) -> Result<()> {
    let m = sampled_monitor(mock);
    let disks = m.disks();
    if json {
        println!("{}", serde_json::to_string_pretty(&disks)?);
        return Ok(());
    }
    println!("{}", render::heading("Disk"));
    print_disks(&disks);
    Ok(())
}

fn cmd_network(mock: bool, json: bool) -> Result<()> {
    let m = sampled_monitor(mock);
    let mut nets = m.networks();
    if json {
        println!("{}", serde_json::to_string_pretty(&nets)?);
        return Ok(());
    }
    nets.sort_by(|a, b| (b.rx_total + b.tx_total).cmp(&(a.rx_total + a.tx_total)));
    println!("{}", render::heading("Network"));
    print_networks(nets.iter().filter(|n| n.rx_total + n.tx_total > 0));
    Ok(())
}

fn cmd_top(mock: bool, json: bool, mem: bool, count: usize) -> Result<()> {
    let m = sampled_monitor(mock);
    let sort = if mem { ProcSort::Memory } else { ProcSort::Cpu };
    let procs = m.processes(count, sort);
    if json {
        println!("{}", serde_json::to_string_pretty(&procs)?);
        return Ok(());
    }
    println!(
        "{} {}",
        render::heading("Top processes"),
        format!("· by {}", if mem { "memory" } else { "cpu" }).dimmed()
    );
    println!(
        "  {:>7}  {:>6}  {:>10}  {}",
        "PID".dimmed(),
        "CPU%".dimmed(),
        "MEM".dimmed(),
        "NAME".dimmed()
    );
    for p in &procs {
        println!(
            "  {:>7}  {:>6.1}  {:>10}  {}",
            p.pid,
            p.cpu_percent,
            render::bytes(p.memory),
            p.name
        );
    }
    Ok(())
}

fn cmd_battery(mock: bool, json: bool) -> Result<()> {
    let m = sampled_monitor(mock);
    let batt = m.battery();
    if json {
        println!("{}", serde_json::to_string_pretty(&batt)?);
        return Ok(());
    }
    println!("{}", render::heading("Battery"));
    match batt {
        None => println!("  {}", "no battery detected".dimmed()),
        Some(b) => print_battery(&b),
    }
    Ok(())
}

fn cmd_system(mock: bool, json: bool) -> Result<()> {
    let m = sampled_monitor(mock);
    let info = m.system_info();
    if json {
        println!("{}", serde_json::to_string_pretty(&info)?);
        return Ok(());
    }
    println!("{}", render::heading("System"));
    print_system(&info);
    Ok(())
}

// ---------------------------------------------------------------------------
// Status: the full dashboard
// ---------------------------------------------------------------------------

fn cmd_status(mock: bool, json: bool) -> Result<()> {
    let m = sampled_monitor(mock);
    let provider = provider(mock);

    let info = m.system_info();
    let cpu = m.cpu();
    let mem = m.memory();
    let disks = m.disks();
    let mut nets = m.networks();
    nets.sort_by(|a, b| (b.rx_total + b.tx_total).cmp(&(a.rx_total + a.tx_total)));
    let battery = m.battery();
    let sensors = read_sensors(provider.as_ref())?;

    if json {
        let value = serde_json::json!({
            "metrics_backend": m.name(),
            "thermal_backend": provider.name(),
            "simulated_sensors": sensors.simulated,
            "system": info,
            "cpu": cpu,
            "memory": mem,
            "disks": disks,
            "networks": nets,
            "battery": battery,
            "temps": sensors.temps,
            "fans": sensors.fans,
        });
        println!("{}", serde_json::to_string_pretty(&value)?);
        return Ok(());
    }

    println!("{}", render::banner(env!("CARGO_PKG_VERSION")));
    let os = info
        .os_name
        .as_deref()
        .map(|n| match &info.os_version {
            Some(v) => format!("{n} {v}"),
            None => n.to_string(),
        })
        .unwrap_or_else(|| std::env::consts::OS.to_string());
    println!(
        "{} {}  ·  {}  ·  up {}",
        "backend:".dimmed(),
        format!("{} + {}", m.name(), provider.name()).bold(),
        os.dimmed(),
        render::duration(info.uptime_secs).dimmed()
    );
    println!();

    // CPU
    println!(
        "{} {}",
        render::heading("CPU"),
        format!("· {}", cpu.brand).dimmed()
    );
    println!(
        "  {}  {}   cores {}",
        render::pct_colored(cpu.usage_percent),
        render::load_bar(cpu.usage_percent),
        render::core_spark(&cpu.per_core).cyan()
    );
    println!();

    // Memory
    println!("{}", render::heading("Memory"));
    println!(
        "  {} / {} ({})  {}",
        render::bytes(mem.used),
        render::bytes(mem.total),
        render::pct_colored(mem.used_percent).trim(),
        render::load_bar(mem.used_percent)
    );
    println!();

    // Disk
    if !disks.is_empty() {
        println!("{}", render::heading("Disk"));
        print_disks(&disks);
        println!();
    }

    // Network (top interfaces by traffic)
    let active: Vec<_> = nets
        .iter()
        .filter(|n| n.rx_total + n.tx_total > 0)
        .take(3)
        .collect();
    if !active.is_empty() {
        println!("{}", render::heading("Network"));
        print_networks(active.into_iter());
        println!();
    }

    // Battery
    if let Some(b) = &battery {
        println!("{}", render::heading("Battery"));
        print_battery(b);
        println!();
    }

    // Thermals
    println!("{}", render::heading("Temperatures"));
    if sensors.simulated {
        println!("  {}", simulated_note());
    }
    print_temps(&sensors.temps);
    println!();
    println!("{}", render::heading("Fans"));
    print_fans(&sensors.fans);

    if let Some(w) = provider.power_watts() {
        println!();
        println!("{} {}", render::heading("Power"), format!("· {w:.1} W").dimmed());
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Thermal commands (HardwareProvider)
// ---------------------------------------------------------------------------

/// Sensor readings plus whether they came from the simulated backend.
struct Sensors {
    temps: Vec<TempSensor>,
    fans: Vec<Fan>,
    simulated: bool,
}

/// Read temps + fans, transparently falling back to the mock backend (and
/// flagging the data as simulated) when the real backend can't read sensors yet.
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

fn cmd_fan(provider: &dyn HardwareProvider, action: FanAction, json: bool) -> Result<()> {
    if !provider.capabilities().control_fans {
        anyhow::bail!(
            "the '{}' backend can't control fans (no SMC/EC write support here)",
            provider.name()
        );
    }
    let fans = provider.fans().unwrap_or_default();
    let targets: Vec<Fan> = match action.fan_index() {
        Some(idx) => fans
            .into_iter()
            .enumerate()
            .filter(|(i, f)| *i == idx && f.controllable)
            .map(|(_, f)| f)
            .collect(),
        None => fans.into_iter().filter(|f| f.controllable).collect(),
    };
    if targets.is_empty() {
        anyhow::bail!("no matching controllable fan");
    }

    match action {
        FanAction::Set { percent, .. } => {
            let pct = percent.min(100);
            for f in &targets {
                provider.set_fan_duty(&f.id, pct)?;
            }
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "action": "set", "duty_percent": pct,
                        "fans": targets.iter().map(|f| &f.label).collect::<Vec<_>>(),
                    }))?
                );
            } else {
                println!(
                    "Forced {} to {pct}%.",
                    targets.iter().map(|f| f.label.as_str()).collect::<Vec<_>>().join(", ")
                );
                println!(
                    "  {}",
                    "⚠ fans stay forced until you run `peterfan fan auto`".yellow()
                );
            }
        }
        FanAction::Auto { .. } => {
            for f in &targets {
                provider.set_fan_auto(&f.id)?;
            }
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "action": "auto",
                        "fans": targets.iter().map(|f| &f.label).collect::<Vec<_>>(),
                    }))?
                );
            } else {
                println!(
                    "Restored {} to automatic control.",
                    targets.iter().map(|f| f.label.as_str()).collect::<Vec<_>>().join(", ")
                );
            }
        }
    }
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
            println!(
                "  {}",
                "⚠ fans stay forced until you run `peterfan fan auto`".yellow()
            );
        }
    } else if json {
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
    Ok(())
}

fn list_profiles(json: bool) -> Result<()> {
    if json {
        let arr: Vec<_> = Profile::all()
            .iter()
            .map(|p| serde_json::json!({ "name": p.as_str(), "description": p.description() }))
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
    print!("  {}: ", "samples".dimmed());
    for t in [30, 45, 60, 75, 90] {
        print!("{t}°C={}%  ", curve.duty_at(t as f32));
    }
    println!();
    Ok(())
}

fn cmd_doctor(mock: bool, json: bool) -> Result<()> {
    let provider = provider(mock);
    let monitor = sampled_monitor(mock);
    let caps = provider.capabilities();
    let mcaps = monitor.capabilities();
    let elevated = is_elevated();

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "os": std::env::consts::OS,
                "arch": std::env::consts::ARCH,
                "metrics_backend": monitor.name(),
                "thermal_backend": provider.name(),
                "elevated": elevated,
                "metrics": {
                    "cpu": mcaps.cpu, "memory": mcaps.memory, "disks": mcaps.disks,
                    "networks": mcaps.networks, "processes": mcaps.processes, "battery": mcaps.battery,
                },
                "thermal": {
                    "read_temps": caps.read_temps, "read_fans": caps.read_fans,
                    "control_fans": caps.control_fans,
                },
            }))?
        );
        return Ok(());
    }

    println!("{}", render::heading("PeterFan doctor"));
    print_kv(
        "OS / arch",
        &format!("{} / {}", std::env::consts::OS, std::env::consts::ARCH),
    );
    print_kv("Metrics backend", monitor.name());
    print_kv("Thermal backend", provider.name());
    print_kv("Elevated", if elevated { "yes" } else { "no" });
    println!();
    println!("{}", render::heading("System metrics"));
    print_check("cpu", mcaps.cpu);
    print_check("memory", mcaps.memory);
    print_check("disks", mcaps.disks);
    print_check("networks", mcaps.networks);
    print_check("processes", mcaps.processes);
    print_check("battery", mcaps.battery);
    println!();
    println!("{}", render::heading("Thermal hardware"));
    print_check("read temperatures", caps.read_temps);
    print_check("read fans", caps.read_fans);
    print_check("control fans", caps.control_fans);

    if !caps.read_temps {
        println!();
        println!(
            "  {} thermal sensor reading is not implemented for the '{}' backend yet;",
            "note:".yellow().bold(),
            provider.name()
        );
        println!("        the CLI falls back to simulated temps/fans. System metrics are real.");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Shared printers
// ---------------------------------------------------------------------------

fn print_system(info: &peterfan_core::metrics::SystemInfo) {
    print_kv("Host", info.host_name.as_deref().unwrap_or("—"));
    let os = match (&info.os_name, &info.os_version) {
        (Some(n), Some(v)) => format!("{n} {v}"),
        (Some(n), None) => n.clone(),
        _ => std::env::consts::OS.to_string(),
    };
    print_kv("OS", &os);
    print_kv("Kernel", info.kernel_version.as_deref().unwrap_or("—"));
    print_kv("Arch", &info.arch);
    let cores = match info.physical_cores {
        Some(p) => format!("{} logical / {} physical", info.logical_cores, p),
        None => format!("{} logical", info.logical_cores),
    };
    print_kv("Cores", &cores);
    print_kv("Uptime", &render::duration(info.uptime_secs));
}

fn print_disks(disks: &[peterfan_core::metrics::DiskInfo]) {
    for d in disks {
        println!(
            "  {:<14} {} / {} ({})  {}  {}",
            d.mount,
            render::bytes(d.used),
            render::bytes(d.total),
            render::pct_colored(d.used_percent).trim(),
            render::load_bar(d.used_percent),
            d.kind.dimmed(),
        );
    }
}

fn print_networks<'a>(nets: impl Iterator<Item = &'a peterfan_core::metrics::NetInterface>) {
    for n in nets {
        println!(
            "  {:<14} ↓ {:>11}  ↑ {:>11}   {}",
            n.name,
            render::rate(n.rx_rate),
            render::rate(n.tx_rate),
            format!(
                "total ↓{} ↑{}",
                render::bytes(n.rx_total),
                render::bytes(n.tx_total)
            )
            .dimmed(),
        );
    }
}

fn print_battery(b: &peterfan_core::metrics::BatteryInfo) {
    let remaining = match b.state.as_str() {
        "charging" => b
            .time_to_full_secs
            .map(|s| format!("~{} to full", render::duration(s))),
        _ => b
            .time_to_empty_secs
            .map(|s| format!("~{} left", render::duration(s))),
    };
    let mut line = format!(
        "  {}  {}  {}",
        render::pct_colored(b.charge_percent).trim(),
        render::load_bar(b.charge_percent),
        b.state,
    );
    if let Some(r) = remaining {
        line.push_str(&format!("  {}", r.dimmed()));
    }
    println!("{line}");
    let mut details = Vec::new();
    if let Some(h) = b.health_percent {
        details.push(format!("health {h:.0}%"));
    }
    if let Some(c) = b.cycle_count {
        details.push(format!("{c} cycles"));
    }
    if let Some(w) = b.energy_rate_w {
        details.push(format!("{w:.1} W"));
    }
    if !details.is_empty() {
        println!("  {}", details.join("  ·  ").dimmed());
    }
}

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
        println!("  {:<14} {:>5} RPM  {}  {}", f.label, f.rpm, duty, bar);
    }
}

fn print_kv(key: &str, value: &str) {
    println!("  {:<16} {}", format!("{key}:").dimmed(), value);
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
