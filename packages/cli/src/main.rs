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

use std::time::{Duration, Instant};

use anyhow::Result;
use clap::{CommandFactory, Parser, Subcommand};
use owo_colors::OwoColorize;

use peterfan_core::error::CoreError;
use peterfan_core::license::{self, Entitlement, LicenseStatus};
use peterfan_core::metrics::ProcSort;
use peterfan_core::profile::Profile;
use peterfan_core::types::{Fan, TempSensor};
use peterfan_core::{HardwareProvider, SystemMonitor};

/// Delay between the two metric samples; usage % and network rates are deltas.
/// 150 ms is the sweet spot: accurate enough for ≥1% CPU granularity, but
/// fast enough to feel instant. Anything below 100 ms gives noisy readings.
const SAMPLE_MS: u64 = 150;

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
    Status {
        /// One-line summary suitable for shell prompts or status bars.
        #[arg(long)]
        compact: bool,
    },
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
    /// List profiles or apply one. Subcommands: `create`, `delete`.
    /// With no subcommand and no name, lists all profiles.
    #[command(alias = "profiles")]
    Profile {
        /// Profile name (silent, balanced, gaming, performance, maximum, custom).
        name: Option<String>,
        #[command(subcommand)]
        sub: Option<ProfileAction>,
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
    /// Show or edit the config file (`--init` creates it, `--set key value` changes a value).
    Config {
        /// Create the config file with defaults if it doesn't exist.
        #[arg(long)]
        init: bool,
        /// Set a config value: profile, interval, or critical.
        /// Example: --set profile gaming  |  --set interval 3  |  --set critical 95
        #[arg(long, value_names = ["KEY", "VALUE"], num_args = 2)]
        set: Option<Vec<String>>,
        /// Print a single config value.
        /// Example: --get profile
        #[arg(long)]
        get: Option<String>,
    },
    /// Manage fan-control automation rules in the config file.
    /// With no subcommand, lists current rules.
    Rule {
        #[command(subcommand)]
        action: Option<RuleAction>,
    },
    /// Manage the running peterfand daemon (status / reload / stop).
    Daemon {
        #[command(subcommand)]
        action: DaemonAction,
    },
    /// Serve a local JSON HTTP API (`/api/v1/status`, …) for integrations.
    Serve {
        /// Port to listen on (localhost only).
        #[arg(long, default_value_t = 9847)]
        port: u16,
    },
    /// Stress all CPU cores and record temperature / fan / power over time.
    Benchmark {
        /// Duration in seconds.
        #[arg(long, default_value_t = 10)]
        secs: u64,
        /// Apply a fan profile for the duration of the benchmark, then restore.
        /// Example: --profile gaming
        #[arg(long)]
        profile: Option<String>,
    },
    /// Print a shell completion script: `peterfan completions zsh`.
    Completions {
        /// Target shell.
        shell: clap_complete::Shell,
    },
    /// Install the root fan-control daemon (one admin-password prompt — like Macs Fan Control).
    InstallDaemon {
        /// Print what would run instead of asking for the admin password.
        #[arg(long)]
        dry_run: bool,
    },
    /// Remove the root fan-control daemon.
    UninstallDaemon {
        /// Print what would run instead of asking for the admin password.
        #[arg(long)]
        dry_run: bool,
    },
    /// Manage the peterfan-menubar login item (auto-start at login).
    LoginItem {
        #[command(subcommand)]
        action: LoginItemAction,
    },
    /// Continuously emit one metrics row per interval (for logging / piping).
    Log {
        /// Seconds between rows.
        #[arg(long, default_value_t = 2)]
        interval: u64,
        /// Output format.
        #[arg(long, value_enum, default_value_t = LogFormat::Csv)]
        format: LogFormat,
    },
    /// Live single-line display — CPU, memory, temp, fans, daemon mode.
    /// Refreshes in place; ideal for tmux statusbars or quick checks.
    Watch {
        /// Refresh interval in seconds.
        #[arg(long, short, default_value_t = 2)]
        interval: u64,
    },
    /// Check for a newer version on GitHub.
    Update,
    /// Watch metrics and send a desktop notification when a threshold is exceeded.
    /// Run with no args to use saved thresholds from config.
    /// Subcommands: `install` (LaunchAgent), `status`, `remove`.
    Alert {
        /// Alert when CPU usage exceeds this percent (0-100).
        #[arg(long)]
        cpu: Option<f32>,
        /// Alert when memory usage exceeds this percent (0-100).
        #[arg(long, alias = "mem")]
        memory: Option<f32>,
        /// Alert when the hottest sensor exceeds this temperature in °C.
        #[arg(long)]
        temp: Option<f32>,
        /// Check interval in seconds.
        #[arg(long, short)]
        interval: Option<u64>,
        /// Seconds to suppress repeated alerts for the same metric.
        #[arg(long)]
        cooldown: Option<u64>,
        /// Check once and exit — exit code 1 if any threshold exceeded (for cron/scripts).
        #[arg(long)]
        once: bool,
        /// Save these thresholds to config so `peterfan alert` uses them by default.
        #[arg(long)]
        save: bool,
        #[command(subcommand)]
        sub: Option<AlertAction>,
    },
    /// Manage your PeterFan license (the menu-bar app and persistent fan
    /// control need one after the free trial; every other command stays free).
    /// With no subcommand, shows current trial/license status.
    License {
        #[command(subcommand)]
        sub: Option<LicenseAction>,
    },
}

#[derive(Subcommand, Clone)]
enum LicenseAction {
    /// Show trial days remaining or the active license's email/expiry.
    Status,
    /// Save a `PFAN1-...` license key.
    Activate { key: String },
    /// Remove the saved license key (falls back to the trial clock).
    Deactivate,
}

#[derive(Subcommand, Clone)]
enum AlertAction {
    /// Install a user LaunchAgent that runs `peterfan alert` at login.
    Install {
        /// Path to the peterfan binary (default: this binary).
        #[arg(long)]
        binary: Option<String>,
    },
    /// Show whether the alert LaunchAgent is installed.
    Status,
    /// Remove the alert LaunchAgent.
    Remove,
}

#[derive(Subcommand, Clone)]
enum ProfileAction {
    /// Define or update a custom fan curve.
    /// Points are <temp_c>:<duty_pct> pairs, e.g. "30:20,60:50,80:90,90:100".
    Create {
        /// Name for the curve (use "custom" for the default custom slot, or any
        /// identifier for a named curve usable in rules).
        name: String,
        /// Curve definition: comma-separated temp:duty pairs.
        #[arg(long, short)]
        points: String,
    },
    /// Remove a custom curve by name.
    Delete { name: String },
    /// List all custom curves defined in the config.
    List,
}

#[derive(clap::ValueEnum, Clone, Copy)]
enum LogFormat {
    Csv,
    Jsonl,
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
    /// Show current fan control state (daemon mode + live RPM).
    Status,
}

impl FanAction {
    fn fan_index(&self) -> Option<usize> {
        match self {
            FanAction::Set { fan, .. } | FanAction::Auto { fan, .. } => *fan,
            FanAction::Status => None,
        }
    }
}

#[derive(Subcommand, Clone)]
enum RuleAction {
    /// List automation rules (default when no subcommand given).
    List,
    /// Append a new rule: `peterfan rule add on_battery silent`.
    Add {
        /// Condition: on_battery | on_ac | cpu_above:<°C> | time:<start>-<end>
        condition: String,
        /// Profile: silent | balanced | gaming | performance | maximum
        profile: String,
    },
    /// Remove a rule by its 0-based index from `peterfan rule list`.
    Remove {
        /// 0-based rule index.
        index: usize,
    },
    /// Remove all automation rules.
    Clear,
}

#[derive(Subcommand, Clone)]
enum DaemonAction {
    /// Show the running daemon's current fan-control mode.
    Status,
    /// Tell the running daemon to reload its config from disk.
    Reload,
    /// Tell the running daemon to stop (fans restored to automatic).
    Stop,
    /// Print the last N lines of the daemon log (default 40).
    Log {
        /// Number of lines to show.
        #[arg(short = 'n', long, default_value_t = 40)]
        lines: usize,
        /// Follow the log continuously (like tail -f). Press Ctrl-C to stop.
        #[arg(short = 'f', long)]
        follow: bool,
    },
}

#[derive(Subcommand, Clone)]
enum LoginItemAction {
    /// Show whether the login item is installed and which binary it points to.
    Status,
    /// Install the peterfan-menubar login item (auto-starts at login).
    Install {
        /// Path to peterfan-menubar binary (default: sibling of this binary).
        #[arg(long)]
        binary: Option<String>,
        /// What to show in the menu bar: cpu (default), memory, temp, fan, network.
        /// Can also be changed later from the menu-bar item's right-click menu.
        #[arg(long, default_value = "cpu")]
        metric: String,
    },
    /// Remove the peterfan-menubar login item.
    Remove,
}

fn main() {
    let cli = Cli::parse();
    if let Err(e) = run(cli) {
        eprintln!("{} {e}", "error:".red().bold());
        std::process::exit(1);
    }
}

fn run(cli: Cli) -> Result<()> {
    let command = cli.command.unwrap_or(Command::Status { compact: false });
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
        Command::Status { compact } => {
            if compact {
                cmd_status_compact(mock, json)?;
            } else {
                cmd_status(mock, json)?;
            }
            Ok(())
        }
        Command::Cpu => cmd_cpu(mock, json),
        Command::Memory => cmd_memory(mock, json),
        Command::Disk => cmd_disk(mock, json),
        Command::Network => cmd_network(mock, json),
        Command::Top { mem, count } => cmd_top(mock, json, mem, count),
        Command::Battery => cmd_battery(mock, json),
        Command::System => cmd_system(mock, json),
        Command::Temps => cmd_temps(mock, json),
        Command::Fans => cmd_fans(mock, json),
        Command::Fan { action } => cmd_fan(provider(mock).as_ref(), action, json),
        Command::Profile { name, sub } => {
            if let Some(action) = sub {
                cmd_profile_action(json, action)
            } else {
                cmd_profile(provider(mock).as_ref(), name, json)
            }
        }
        Command::Curve { name } => cmd_curve(name, json),
        Command::Hardware => cmd_hardware(provider(mock).as_ref(), json),
        Command::Doctor => cmd_doctor(mock, json),
        Command::Config { init, set, get } => cmd_config(json, init, set, get),
        Command::Rule { action } => cmd_rule(json, action.unwrap_or(RuleAction::List)),
        Command::Daemon { action } => cmd_daemon(json, action),
        Command::Serve { port } => cmd_serve(mock, port),
        Command::Benchmark { secs, profile } => cmd_benchmark(mock, json, secs, profile),
        Command::Completions { shell } => {
            clap_complete::generate(
                shell,
                &mut Cli::command(),
                "peterfan",
                &mut std::io::stdout(),
            );
            Ok(())
        }
        Command::Log { interval, format } => cmd_log(mock, interval, format),
        Command::LoginItem { action } => cmd_login_item(action),
        Command::InstallDaemon { dry_run } => cmd_install_daemon(dry_run),
        Command::UninstallDaemon { dry_run } => cmd_uninstall_daemon(dry_run),
        Command::Watch { interval } => cmd_watch(mock, interval),
        Command::Update => cmd_update(),
        Command::Alert {
            cpu,
            memory,
            temp,
            interval,
            cooldown,
            once,
            save,
            sub,
        } => cmd_alert(mock, cpu, memory, temp, interval, cooldown, once, save, sub),
        Command::License { sub } => cmd_license(json, sub.unwrap_or(LicenseAction::Status)),
    }
}

#[cfg(target_os = "macos")]
fn cmd_login_item(action: LoginItemAction) -> Result<()> {
    use owo_colors::OwoColorize;
    use peterfan_platform::login_item;

    match action {
        LoginItemAction::Status => {
            let Some(path) = login_item::plist_path() else {
                anyhow::bail!("could not determine home directory");
            };
            if path.exists() {
                let content = std::fs::read_to_string(&path).unwrap_or_default();
                let bin_line = content
                    .lines()
                    .skip_while(|l| !l.contains("ProgramArguments"))
                    .nth(2)
                    .map(|l| {
                        l.trim()
                            .trim_start_matches("<string>")
                            .trim_end_matches("</string>")
                    })
                    .unwrap_or("?");
                println!(
                    "  {} login item installed\n  binary: {}\n  plist:  {}",
                    "✓".green(),
                    bin_line.bold(),
                    path.display()
                );
            } else {
                println!(
                    "  {} login item not installed — run `peterfan login-item install`",
                    "✗".yellow()
                );
            }
        }
        LoginItemAction::Install { binary, metric } => {
            let (bin, path) =
                login_item::install(binary.as_deref(), &metric).map_err(|e| anyhow::anyhow!(e))?;
            println!(
                "  {} login item installed\n  binary: {}\n  plist:  {}\n  {} peterfan-menubar will start at login",
                "✓".green(),
                bin.display().bold(),
                path.display(),
                "→".dimmed()
            );
        }
        LoginItemAction::Remove => {
            if !login_item::remove().map_err(|e| anyhow::anyhow!(e))? {
                println!("  {} login item is not installed", "—".dimmed());
                return Ok(());
            }
            println!("  {} login item removed", "✓".green());
        }
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn cmd_login_item(action: LoginItemAction) -> Result<()> {
    let _ = action;
    anyhow::bail!("login-item is only supported on macOS")
}

#[cfg(target_os = "macos")]
fn cmd_install_daemon(dry_run: bool) -> Result<()> {
    use peterfan_platform::daemon_install::InstallOutcome;
    println!(
        "Installing the PeterFan fan-control daemon as root.\n\
         macOS will ask for your password once — after that the menu-bar app and \
         `peterfan fan …` work without sudo (just like Macs Fan Control)."
    );
    match peterfan_platform::daemon_install::install(dry_run) {
        Ok(InstallOutcome::DryRun(script)) => {
            println!("{script}");
            Ok(())
        }
        Ok(InstallOutcome::Installed) => {
            println!("{}", "✓ daemon installed and running (root)".green());
            println!("  logs at /var/log/peterfand.log (rotated at 1 MB, 5 archives)");
            Ok(())
        }
        Ok(InstallOutcome::InstalledButUnreachable) => {
            println!(
                "{}",
                "installed, but the daemon isn't answering yet — check /var/log/peterfand.err"
                    .yellow()
            );
            Ok(())
        }
        Err(e) => anyhow::bail!(e),
    }
}

#[cfg(target_os = "macos")]
fn cmd_uninstall_daemon(dry_run: bool) -> Result<()> {
    use peterfan_platform::daemon_install::InstallOutcome;
    println!("Removing the PeterFan fan-control daemon (one admin prompt)…");
    match peterfan_platform::daemon_install::uninstall(dry_run) {
        Ok(InstallOutcome::DryRun(script)) => {
            println!("{script}");
            Ok(())
        }
        Ok(_) => {
            println!("{}", "✓ daemon removed".green());
            Ok(())
        }
        Err(e) => anyhow::bail!(e),
    }
}

#[cfg(not(target_os = "macos"))]
fn cmd_install_daemon(_dry_run: bool) -> Result<()> {
    anyhow::bail!("the daemon installer is macOS-only for now")
}
#[cfg(not(target_os = "macos"))]
fn cmd_uninstall_daemon(_dry_run: bool) -> Result<()> {
    anyhow::bail!("the daemon installer is macOS-only for now")
}

fn cmd_log(mock: bool, interval: u64, format: LogFormat) -> Result<()> {
    use std::io::Write;
    let interval = interval.max(1);
    let mut monitor = if mock {
        peterfan_platform::mock_monitor()
    } else {
        peterfan_platform::system_monitor()
    };
    let provider = provider(mock);

    if matches!(format, LogFormat::Csv) {
        println!("time,cpu_pct,mem_pct,disk_pct,temp_c,fan_rpm,power_w");
    }
    monitor.refresh();
    loop {
        std::thread::sleep(Duration::from_secs(interval));
        monitor.refresh();
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let cpu = monitor.cpu().usage_percent;
        let mem = monitor.memory().used_percent;
        let disk = monitor
            .disks()
            .first()
            .map(|d| d.used_percent)
            .unwrap_or(0.0);
        let temp = provider
            .temperatures()
            .unwrap_or_default()
            .iter()
            .map(|t| t.value.0)
            .fold(0.0_f32, f32::max);
        let rpm = provider
            .fans()
            .unwrap_or_default()
            .iter()
            .map(|f| f.rpm)
            .max()
            .unwrap_or(0);
        let power = provider.power_watts().unwrap_or(0.0);

        match format {
            LogFormat::Csv => {
                println!("{ts},{cpu:.1},{mem:.1},{disk:.1},{temp:.0},{rpm},{power:.1}")
            }
            LogFormat::Jsonl => println!(
                "{}",
                serde_json::json!({
                    "time": ts, "cpu_pct": cpu, "mem_pct": mem, "disk_pct": disk,
                    "temp_c": temp, "fan_rpm": rpm, "power_w": power,
                })
            ),
        }
        std::io::stdout().flush().ok();
    }
}

// ---------------------------------------------------------------------------
// `benchmark` — CPU stress + thermal/fan/power capture
// ---------------------------------------------------------------------------

fn cmd_benchmark(mock: bool, json: bool, secs: u64, bench_profile: Option<String>) -> Result<()> {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    let secs = secs.max(1);
    let mut monitor = if mock {
        peterfan_platform::mock_monitor()
    } else {
        peterfan_platform::system_monitor()
    };
    let provider = provider(mock);

    // Remember what the daemon is doing now so we can restore it after.
    let pre_mode = ipc_send("status")
        .as_deref()
        .and_then(|r| r.strip_prefix("ok "))
        .and_then(|s| s.split_once(" (").map(|(m, _)| m.to_string()));

    // Apply the requested profile for the duration of the benchmark.
    let applied_profile = if let Some(ref p) = bench_profile {
        let parsed = peterfan_core::profile::Profile::parse(p)
            .ok_or_else(|| anyhow::anyhow!("unknown profile '{p}'"))?;
        let ipc_ok = ipc_send(&format!("profile {}", parsed.as_str())).is_some();
        if !json {
            if ipc_ok {
                println!(
                    "  {} applied profile {} for benchmark duration",
                    "→".cyan(),
                    parsed.as_str().bold()
                );
            } else {
                println!(
                    "  {} daemon not reachable — running without profile override",
                    "!".yellow()
                );
            }
        }
        Some(parsed.as_str().to_string())
    } else {
        None
    };

    let workers = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(8);
    if !json {
        println!(
            "{} {}",
            render::heading("Benchmark"),
            format!("· stressing {workers} threads for {secs}s").dimmed()
        );
    }

    // Spawn CPU-bound workers.
    let stop = Arc::new(AtomicBool::new(false));
    let mut handles = Vec::new();
    for _ in 0..workers {
        let stop = Arc::clone(&stop);
        handles.push(std::thread::spawn(move || {
            let mut x = 1.0f64;
            while !stop.load(Ordering::Relaxed) {
                for _ in 0..500_000 {
                    x = (x * 1.000_000_1 + 0.5).sqrt().sin().abs() + 1.0;
                }
                std::hint::black_box(x);
            }
        }));
    }

    // Sample once a second while the stress runs.
    let mut samples: Vec<(f32, f32, u32, f32)> = Vec::with_capacity(secs as usize);
    monitor.refresh();
    for sec in 1..=secs {
        std::thread::sleep(Duration::from_secs(1));
        monitor.refresh();
        let cpu = monitor.cpu().usage_percent;
        let temp = provider
            .temperatures()
            .unwrap_or_default()
            .iter()
            .map(|t| t.value.0)
            .fold(0.0_f32, f32::max);
        let rpm = provider
            .fans()
            .unwrap_or_default()
            .iter()
            .map(|f| f.rpm)
            .max()
            .unwrap_or(0);
        let power = provider.power_watts().unwrap_or(0.0);
        samples.push((cpu, temp, rpm, power));
        if !json {
            println!(
                "  {sec:>3}s   cpu {:>5.1}%   temp {:>4.0}°C   fan {:>5} RPM   {:>5.1} W",
                cpu, temp, rpm, power
            );
        }
    }

    stop.store(true, Ordering::Relaxed);
    for h in handles {
        let _ = h.join();
    }

    let peak = |f: fn(&(f32, f32, u32, f32)) -> f32| samples.iter().map(f).fold(0.0_f32, f32::max);
    let avg_cpu = samples.iter().map(|s| s.0).sum::<f32>() / samples.len().max(1) as f32;
    let peak_cpu = peak(|s| s.0);
    let peak_temp = peak(|s| s.1);
    let peak_rpm = samples.iter().map(|s| s.2).max().unwrap_or(0);
    let peak_power = peak(|s| s.3);

    // Restore the previous daemon mode after the benchmark.
    if applied_profile.is_some() {
        let restore_cmd = match pre_mode.as_deref() {
            Some(m) if m.starts_with("hold:") => {
                let pct = m.trim_start_matches("hold:").trim_end_matches('%');
                format!("hold {pct}")
            }
            Some(m) if m.starts_with("manual:") || m.starts_with("rules:") => {
                let p = m.split_once(':').map(|(_, p)| p).unwrap_or("balanced");
                format!("profile {p}")
            }
            Some("auto") => "auto".into(),
            _ => "rules".into(),
        };
        let _ = ipc_send(&restore_cmd);
        if !json {
            println!(
                "  {} restored daemon to: {}",
                "↺".cyan(),
                restore_cmd.bold()
            );
        }
    }

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "duration_secs": secs,
                "threads": workers,
                "profile": applied_profile,
                "avg_cpu_percent": avg_cpu,
                "peak_cpu_percent": peak_cpu,
                "peak_temp_c": peak_temp,
                "peak_fan_rpm": peak_rpm,
                "peak_power_w": peak_power,
            }))?
        );
    } else {
        println!();
        println!("{}", render::heading("Result"));
        if let Some(ref p) = applied_profile {
            print_kv("Profile used", p.as_str());
        }
        print_kv("CPU avg / peak", &format!("{avg_cpu:.1}% / {peak_cpu:.1}%"));
        print_kv("Peak temp", &format!("{peak_temp:.0}°C"));
        print_kv("Peak fan", &format!("{peak_rpm} RPM"));
        print_kv("Peak power", &format!("{peak_power:.1} W"));
    }
    Ok(())
}

const API_INDEX_HTML: &str = r#"<!doctype html><meta charset="utf-8"><title>PeterFan API</title>
<style>body{font:14px ui-sans-serif,system-ui,sans-serif;max-width:640px;margin:40px auto;padding:0 20px;color:#111}
h1{font-size:20px}code{background:#f3f4f6;padding:2px 6px;border-radius:5px}a{color:#2563eb}li{margin:5px 0}</style>
<h1>PeterFan — local API</h1>
<p>Live JSON metrics + fan control. <a href="https://github.com/uulab-official/peterfan">GitHub</a></p>
<h3>GET</h3>
<ul>
<li><a href="/api/v1/status">/api/v1/status</a> — full snapshot</li>
<li><a href="/api/v1/cpu">/api/v1/cpu</a> · <a href="/api/v1/memory">/memory</a> · <a href="/api/v1/disks">/disks</a> · <a href="/api/v1/network">/network</a></li>
<li><a href="/api/v1/battery">/api/v1/battery</a> · <a href="/api/v1/temps">/temps</a> · <a href="/api/v1/fans">/fans</a> · <a href="/api/v1/power">/power</a> · <a href="/api/v1/processes">/processes</a> · <a href="/api/v1/system">/system</a></li>
</ul>
<h3>POST</h3>
<ul>
<li><code>POST /api/v1/profile</code> <code>{"name":"gaming"}</code></li>
<li><code>POST /api/v1/fan</code> <code>{"action":"auto"}</code> or <code>{"action":"set","percent":60}</code></li>
</ul>
"#;

// ---------------------------------------------------------------------------
// `serve` — a small local JSON HTTP API for integrations
// ---------------------------------------------------------------------------

fn cmd_serve(mock: bool, port: u16) -> Result<()> {
    use tiny_http::{Header, Response, Server};

    // Single-threaded: the monitor is refreshed about once a second between
    // requests (recv_timeout), so usage %/rates stay valid without needing the
    // monitor to be `Send` for a background thread.
    let mut monitor: Box<dyn SystemMonitor> = if mock {
        peterfan_platform::mock_monitor()
    } else {
        peterfan_platform::system_monitor()
    };
    monitor.refresh();
    let provider = provider(mock);

    let server = Server::http(("127.0.0.1", port))
        .map_err(|e| anyhow::anyhow!("could not bind 127.0.0.1:{port}: {e}"))?;
    println!("PeterFan API on http://127.0.0.1:{port}  —  try /api/v1/status   (Ctrl-C to stop)");

    let mut last = Instant::now();
    loop {
        if last.elapsed() >= Duration::from_secs(1) {
            monitor.refresh();
            last = Instant::now();
        }
        let mut req = match server.recv_timeout(Duration::from_secs(1)) {
            Ok(Some(r)) => r,
            Ok(None) => continue, // timeout — loop back to refresh
            Err(_) => continue,
        };
        let method = req.method().as_str().to_string();
        let path = req.url().split('?').next().unwrap_or("/").to_string();
        let mut body = String::new();
        if method == "POST" {
            let _ = req.as_reader().read_to_string(&mut body);
        }

        // Human-friendly index page.
        if method == "GET" && path == "/" {
            let resp = Response::from_string(API_INDEX_HTML)
                .with_header(
                    Header::from_bytes(&b"Content-Type"[..], &b"text/html; charset=utf-8"[..])
                        .unwrap(),
                )
                .with_header(
                    Header::from_bytes(&b"Access-Control-Allow-Origin"[..], &b"*"[..]).unwrap(),
                );
            let _ = req.respond(resp);
            continue;
        }

        let (code, value) = route(&method, &path, &body, monitor.as_ref(), provider.as_ref());
        let json = serde_json::to_string(&value).unwrap_or_else(|_| "{}".into());
        let resp = Response::from_string(json)
            .with_status_code(code)
            .with_header(
                Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap(),
            )
            .with_header(
                Header::from_bytes(&b"Access-Control-Allow-Origin"[..], &b"*"[..]).unwrap(),
            );
        let _ = req.respond(resp);
    }
}

/// Build a (status, JSON) response for one API request.
fn route(
    method: &str,
    path: &str,
    body: &str,
    m: &dyn SystemMonitor,
    provider: &dyn HardwareProvider,
) -> (u16, serde_json::Value) {
    match (method, path) {
        ("GET", "/api/v1/status") => (
            200,
            serde_json::json!({
                "system": m.system_info(),
                "cpu": m.cpu(),
                "memory": m.memory(),
                "disks": m.disks(),
                "networks": m.networks(),
                "battery": m.battery(),
                "temps": provider.temperatures().unwrap_or_default(),
                "fans": provider.fans().unwrap_or_default(),
                "power_watts": provider.power_watts(),
            }),
        ),
        ("GET", "/api/v1/system") => (200, serde_json::json!(m.system_info())),
        ("GET", "/api/v1/cpu") => (200, serde_json::json!(m.cpu())),
        ("GET", "/api/v1/memory") => (200, serde_json::json!(m.memory())),
        ("GET", "/api/v1/disks") => (200, serde_json::json!(m.disks())),
        ("GET", "/api/v1/network") | ("GET", "/api/v1/networks") => {
            (200, serde_json::json!(m.networks()))
        }
        ("GET", "/api/v1/battery") => (200, serde_json::json!(m.battery())),
        ("GET", "/api/v1/processes") => (200, serde_json::json!(m.processes(20, ProcSort::Cpu))),
        ("GET", "/api/v1/temps") => (
            200,
            serde_json::json!(provider.temperatures().unwrap_or_default()),
        ),
        ("GET", "/api/v1/fans") => (200, serde_json::json!(provider.fans().unwrap_or_default())),
        ("GET", "/api/v1/power") => (200, serde_json::json!({ "watts": provider.power_watts() })),
        ("POST", "/api/v1/profile") => api_apply_profile(body, provider),
        ("POST", "/api/v1/fan") => api_apply_fan(body, provider),
        ("OPTIONS", _) => (204, serde_json::Value::Null),
        _ => (404, serde_json::json!({ "error": "not found" })),
    }
}

fn api_apply_profile(body: &str, provider: &dyn HardwareProvider) -> (u16, serde_json::Value) {
    let name = serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| v.get("name").and_then(|n| n.as_str()).map(str::to_string));
    let Some(name) = name else {
        return (
            400,
            serde_json::json!({ "error": "expected {\"name\": \"...\"}" }),
        );
    };
    let Some(profile) = Profile::parse(&name) else {
        return (
            400,
            serde_json::json!({ "error": format!("unknown profile '{name}'") }),
        );
    };

    // Prefer routing through the daemon (no root needed for the HTTP server process).
    if let Some(reply) = ipc_send(&format!("profile {name}")) {
        let curve = profile.default_curve();
        let temps = provider.temperatures().unwrap_or_default();
        let temp = temps.iter().map(|t| t.value.0).fold(0.0_f32, f32::max);
        let duty = curve.duty_at(temp);
        return (
            200,
            serde_json::json!({
                "applied": true, "via": "daemon", "daemon_reply": reply,
                "profile": profile.as_str(), "duty_percent": duty,
            }),
        );
    }

    if !provider.capabilities().control_fans {
        return (
            200,
            serde_json::json!({ "applied": false, "reason": "no fan control on this backend" }),
        );
    }
    let curve = profile.default_curve();
    let temps = provider.temperatures().unwrap_or_default();
    let temp = temps.iter().map(|t| t.value.0).fold(0.0_f32, f32::max);
    let duty = curve.duty_at(temp);
    for f in provider
        .fans()
        .unwrap_or_default()
        .iter()
        .filter(|f| f.controllable)
    {
        if let Err(e) = provider.set_fan_duty(&f.id, duty) {
            return (
                500,
                serde_json::json!({ "applied": false, "error": e.to_string() }),
            );
        }
    }
    (
        200,
        serde_json::json!({ "applied": true, "profile": profile.as_str(), "duty_percent": duty }),
    )
}

fn api_apply_fan(body: &str, provider: &dyn HardwareProvider) -> (u16, serde_json::Value) {
    let v = match serde_json::from_str::<serde_json::Value>(body) {
        Ok(v) => v,
        Err(_) => return (400, serde_json::json!({ "error": "invalid JSON" })),
    };
    let action = v.get("action").and_then(|a| a.as_str()).unwrap_or("");

    // Prefer routing through the daemon (no root needed for the HTTP server).
    let ipc_reply = match action {
        "auto" => ipc_send("auto"),
        "set" => {
            let pct = v
                .get("percent")
                .and_then(|p| p.as_u64())
                .unwrap_or(50)
                .min(100) as u8;
            ipc_send(&format!("hold {pct}"))
        }
        _ => None,
    };
    if let Some(reply) = ipc_reply {
        return (
            200,
            serde_json::json!({ "applied": true, "via": "daemon", "action": action, "daemon_reply": reply }),
        );
    }

    if !provider.capabilities().control_fans {
        return (
            200,
            serde_json::json!({ "applied": false, "reason": "no fan control on this backend" }),
        );
    }
    let fans: Vec<String> = provider
        .fans()
        .unwrap_or_default()
        .into_iter()
        .filter(|f| f.controllable)
        .map(|f| f.id)
        .collect();
    let result = match action {
        "auto" => fans.iter().try_for_each(|id| provider.set_fan_auto(id)),
        "set" => {
            let pct = v
                .get("percent")
                .and_then(|p| p.as_u64())
                .unwrap_or(50)
                .min(100) as u8;
            fans.iter()
                .try_for_each(|id| provider.set_fan_duty(id, pct))
        }
        _ => {
            return (
                400,
                serde_json::json!({ "error": "action must be 'auto' or 'set'" }),
            )
        }
    };
    match result {
        Ok(()) => (
            200,
            serde_json::json!({ "applied": true, "action": action }),
        ),
        Err(e) => (
            500,
            serde_json::json!({ "applied": false, "error": e.to_string() }),
        ),
    }
}

fn cmd_config(json: bool, init: bool, set: Option<Vec<String>>, get: Option<String>) -> Result<()> {
    if init {
        let p = peterfan_platform::config::init_default()
            .map_err(|e| anyhow::anyhow!("could not write config: {e}"))?;
        if !json {
            println!("config ready at {}", p.display());
        }
    }
    if let Some(key) = get {
        let cfg = peterfan_platform::config::load();
        let value = match key.as_str() {
            "profile" => cfg.profile.as_str().to_string(),
            "interval" | "interval_secs" => cfg.interval_secs.to_string(),
            "critical" | "critical_temp_c" => format!("{:.0}", cfg.critical_temp_c),
            "alert.cpu" | "alert.cpu_pct" => cfg
                .alert
                .cpu_pct
                .map(|v| format!("{v}"))
                .unwrap_or_else(|| "(not set)".into()),
            "alert.memory" | "alert.memory_pct" => cfg
                .alert
                .memory_pct
                .map(|v| format!("{v}"))
                .unwrap_or_else(|| "(not set)".into()),
            "alert.temp" | "alert.temp_c" => cfg
                .alert
                .temp_c
                .map(|v| format!("{v}"))
                .unwrap_or_else(|| "(not set)".into()),
            "alert.cooldown" | "alert.cooldown_secs" => cfg.alert.cooldown_secs.to_string(),
            "alert.interval" | "alert.interval_secs" => cfg.alert.interval_secs.to_string(),
            _ => anyhow::bail!(
                "unknown key '{key}'; valid keys: profile, interval, critical, \
                 alert.cpu, alert.memory, alert.temp, alert.cooldown, alert.interval"
            ),
        };
        if json {
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({key: value}))?
            );
        } else {
            println!("{value}");
        }
        return Ok(());
    }
    if let Some(kv) = set {
        let key = kv[0].as_str();
        let val = kv[1].as_str();
        let mut cfg = peterfan_platform::config::load();
        match key {
            "profile" => {
                cfg.profile = peterfan_core::profile::Profile::parse(val)
                    .ok_or_else(|| anyhow::anyhow!("unknown profile '{val}'"))?;
            }
            "interval" | "interval_secs" => {
                cfg.interval_secs = val
                    .parse::<u64>()
                    .map_err(|_| anyhow::anyhow!("interval must be a number"))?
                    .max(1);
            }
            "critical" | "critical_temp_c" => {
                cfg.critical_temp_c = val
                    .parse::<f32>()
                    .map_err(|_| anyhow::anyhow!("critical must be a number"))?;
            }
            "alert.cpu" | "alert.cpu_pct" => {
                cfg.alert.cpu_pct = Some(
                    val.parse::<f32>()
                        .map_err(|_| anyhow::anyhow!("alert.cpu must be a number"))?,
                );
            }
            "alert.memory" | "alert.memory_pct" => {
                cfg.alert.memory_pct = Some(
                    val.parse::<f32>()
                        .map_err(|_| anyhow::anyhow!("alert.memory must be a number"))?,
                );
            }
            "alert.temp" | "alert.temp_c" => {
                cfg.alert.temp_c = Some(
                    val.parse::<f32>()
                        .map_err(|_| anyhow::anyhow!("alert.temp must be a number"))?,
                );
            }
            "alert.cooldown" | "alert.cooldown_secs" => {
                cfg.alert.cooldown_secs = val
                    .parse::<u64>()
                    .map_err(|_| anyhow::anyhow!("alert.cooldown must be a number"))?;
            }
            "alert.interval" | "alert.interval_secs" => {
                cfg.alert.interval_secs = val
                    .parse::<u64>()
                    .map_err(|_| anyhow::anyhow!("alert.interval must be a number"))?
                    .max(1);
            }
            _ => anyhow::bail!(
                "unknown key '{key}'; valid keys: profile, interval, critical, \
                 alert.cpu, alert.memory, alert.temp, alert.cooldown, alert.interval"
            ),
        }
        let p = peterfan_platform::config::save(&cfg)
            .map_err(|e| anyhow::anyhow!("could not write config: {e}"))?;
        if !json {
            println!(
                "  {} {} = {}  ({})",
                "✓".green(),
                key.bold(),
                val.cyan(),
                p.display()
            );
        }
        notify_daemon_reload(json);
        return Ok(());
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
    if cfg.rules.is_empty() {
        print_kv("Rules", "(none)");
    } else {
        println!("  {}", "Rules:".dimmed());
        for r in &cfg.rules {
            let ok = if r.condition().is_some() {
                ""
            } else {
                "  ⚠ invalid"
            };
            println!("    {:<16} → {}{}", r.when, r.profile.as_str(), ok.yellow());
        }
    }
    if !cfg.alert.is_empty() {
        println!("  {}", "Alert:".dimmed());
        if let Some(c) = cfg.alert.cpu_pct {
            println!("    cpu  > {c}%");
        }
        if let Some(m) = cfg.alert.memory_pct {
            println!("    mem  > {m}%");
        }
        if let Some(t) = cfg.alert.temp_c {
            println!("    temp > {t}°C");
        }
        println!(
            "    interval {}s · cooldown {}s",
            cfg.alert.interval_secs, cfg.alert.cooldown_secs
        );
    }
    Ok(())
}

/// Seconds since the Unix epoch, or 0 if the system clock is before 1970.
fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Format Unix seconds as `YYYY-MM-DD` (UTC), with no date-library dependency —
/// the civil-from-days algorithm (Howard Hinnant, public domain).
fn format_unix_date(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

fn cmd_license(json: bool, action: LicenseAction) -> Result<()> {
    let mut cfg = peterfan_platform::config::load();
    let now = now_unix();

    match action {
        LicenseAction::Status => {
            let entitlement = license::check_entitlement(
                cfg.license.key.as_deref(),
                cfg.license.first_run_unix,
                now,
            );
            if json {
                let val = match &entitlement {
                    Entitlement::Licensed { email } => {
                        serde_json::json!({"status": "licensed", "email": email})
                    }
                    Entitlement::Trial { days_left } => {
                        serde_json::json!({"status": "trial", "days_left": days_left})
                    }
                    Entitlement::TrialExpired => serde_json::json!({"status": "trial_expired"}),
                };
                println!("{}", serde_json::to_string_pretty(&val)?);
                return Ok(());
            }
            println!("{}", render::heading("License"));
            match &entitlement {
                Entitlement::Licensed { email } => {
                    print_kv("Status", &format!("licensed — {email}"));
                }
                Entitlement::Trial { days_left } => {
                    print_kv("Status", &format!("free trial — {days_left} day(s) left"));
                    println!(
                        "  {}",
                        "activate with: peterfan license activate <key>".dimmed()
                    );
                }
                Entitlement::TrialExpired => {
                    print_kv("Status", "trial expired");
                    println!(
                        "  {}",
                        "the menu-bar app and persistent fan control need a license now.".yellow()
                    );
                    println!(
                        "  {}",
                        "every other command (status, temps, fan set, …) stays free.".dimmed()
                    );
                }
            }
        }
        LicenseAction::Activate { key } => match license::verify_key(&key, now) {
            LicenseStatus::Valid { email, expires } => {
                cfg.license.key = Some(key);
                peterfan_platform::config::save(&cfg)
                    .map_err(|e| anyhow::anyhow!("could not write config: {e}"))?;
                if json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&serde_json::json!({
                            "status": "activated", "email": email, "expires": expires,
                        }))?
                    );
                } else {
                    println!("  {} licensed to {}", "✓".green(), email.bold());
                    match expires {
                        Some(exp) => println!("  expires: {}", format_unix_date(exp)),
                        None => println!("  {}", "lifetime license".dimmed()),
                    }
                }
            }
            LicenseStatus::Expired { email, expired_at } => {
                anyhow::bail!(
                    "license for {email} expired on {} — you'll need a new key",
                    format_unix_date(expired_at)
                );
            }
            LicenseStatus::Invalid(reason) => {
                anyhow::bail!("invalid license key: {reason}");
            }
        },
        LicenseAction::Deactivate => {
            cfg.license.key = None;
            peterfan_platform::config::save(&cfg)
                .map_err(|e| anyhow::anyhow!("could not write config: {e}"))?;
            if !json {
                println!("  {} license removed — trial clock resumes", "✓".green());
            }
        }
    }
    Ok(())
}

fn cmd_rule(json: bool, action: RuleAction) -> Result<()> {
    use peterfan_core::config::Rule;

    let mut cfg = peterfan_platform::config::load();

    match action {
        RuleAction::List => {
            if json {
                let rules: Vec<_> = cfg
                    .rules
                    .iter()
                    .enumerate()
                    .map(|(i, r)| {
                        serde_json::json!({"index": i, "when": r.when, "profile": r.profile.as_str()})
                    })
                    .collect();
                println!("{}", serde_json::to_string_pretty(&rules)?);
                return Ok(());
            }
            println!("{}", render::heading("Automation Rules"));
            if cfg.rules.is_empty() {
                println!("  (no rules — daemon follows the default profile)");
            } else {
                for (i, r) in cfg.rules.iter().enumerate() {
                    let valid = if r.condition().is_some() {
                        ""
                    } else {
                        "  ⚠ invalid condition"
                    };
                    println!(
                        "  [{}] {:<20} → {}{}",
                        i,
                        r.when.cyan(),
                        r.profile.as_str().bold(),
                        valid.yellow()
                    );
                }
            }
        }
        RuleAction::Add { condition, profile } => {
            let p = peterfan_core::profile::Profile::parse(&profile)
                .ok_or_else(|| anyhow::anyhow!("unknown profile '{profile}'"))?;
            let rule = Rule {
                when: condition.clone(),
                profile: p,
            };
            if rule.condition().is_none() {
                anyhow::bail!(
                    "invalid condition '{condition}'.\n\
                     Valid forms: on_battery | on_ac | cpu_above:<°C> | time:<start>-<end>\n\
                     Example: cpu_above:85 | time:22-7"
                );
            }
            cfg.rules.push(rule);
            let path = peterfan_platform::config::save(&cfg)
                .map_err(|e| anyhow::anyhow!("could not write config: {e}"))?;
            if !json {
                println!(
                    "  {} added rule: {} → {}  ({})",
                    "✓".green(),
                    condition.cyan(),
                    p.as_str().bold(),
                    path.display()
                );
            }
            notify_daemon_reload(json);
        }
        RuleAction::Remove { index } => {
            if index >= cfg.rules.len() {
                anyhow::bail!(
                    "index {index} out of range (have {} rule(s))",
                    cfg.rules.len()
                );
            }
            let removed = cfg.rules.remove(index);
            let path = peterfan_platform::config::save(&cfg)
                .map_err(|e| anyhow::anyhow!("could not write config: {e}"))?;
            if !json {
                println!(
                    "  {} removed [{}]: {} → {}  ({})",
                    "✓".green(),
                    index,
                    removed.when.cyan(),
                    removed.profile.as_str().bold(),
                    path.display()
                );
            }
            notify_daemon_reload(json);
        }
        RuleAction::Clear => {
            let count = cfg.rules.len();
            cfg.rules.clear();
            let path = peterfan_platform::config::save(&cfg)
                .map_err(|e| anyhow::anyhow!("could not write config: {e}"))?;
            if !json {
                println!(
                    "  {} cleared {count} rule(s)  ({})",
                    "✓".green(),
                    path.display()
                );
            }
            notify_daemon_reload(json);
        }
    }
    Ok(())
}

/// Signal the running daemon to reload its config. Silent if no daemon is up.
fn notify_daemon_reload(json: bool) {
    if let Some(reply) = peterfan_platform::ipc::send_command("reload") {
        if !json {
            if let Some(rest) = reply.strip_prefix("ok ") {
                println!("  {} daemon reloaded: {}", "↺".cyan(), rest);
            }
        }
    }
}

const DAEMON_LOG: &str = "/var/log/peterfand.log";

fn cmd_daemon(json: bool, action: DaemonAction) -> Result<()> {
    if let DaemonAction::Log { lines, follow } = action {
        return cmd_daemon_log(json, lines, follow);
    }
    let (cmd, label) = match &action {
        DaemonAction::Status => ("status", "status"),
        DaemonAction::Reload => ("reload", "reload"),
        DaemonAction::Stop => ("stop", "stop"),
        DaemonAction::Log { .. } => unreachable!(),
    };
    match peterfan_platform::ipc::send_command(cmd) {
        Some(reply) => {
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({"reply": reply}))?
                );
            } else if let Some(rest) = reply.strip_prefix("ok ") {
                println!("  {} {}: {}", "✓".green(), label, rest.bold());
            } else {
                println!("  {} {reply}", "✗".red());
            }
        }
        None => {
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(
                        &serde_json::json!({"error": "daemon not reachable"})
                    )?
                );
            } else {
                println!(
                    "  {} daemon not reachable — run `peterfan install-daemon` first",
                    "✗".red()
                );
            }
        }
    }
    Ok(())
}

fn cmd_daemon_log(json: bool, lines: usize, follow: bool) -> Result<()> {
    use std::io::{BufRead, BufReader, Seek, SeekFrom};

    let path = std::path::Path::new(DAEMON_LOG);
    if !path.exists() {
        if json {
            println!(
                "{}",
                serde_json::to_string_pretty(
                    &serde_json::json!({"error": "log file not found", "path": DAEMON_LOG})
                )?
            );
        } else {
            println!(
                "  {} log not found at {DAEMON_LOG}\n  (daemon may not be installed as a LaunchDaemon)",
                "✗".red()
            );
        }
        return Ok(());
    }

    // Print the last N lines.
    let content = std::fs::read_to_string(path)?;
    let all: Vec<&str> = content.lines().collect();
    let start = all.len().saturating_sub(lines);
    for line in &all[start..] {
        println!("{line}");
    }

    if !follow {
        return Ok(());
    }

    // Follow mode: poll for new content by seeking to the current end.
    let mut file = std::fs::File::open(path)?;
    file.seek(SeekFrom::End(0))?;
    println!("--- following {} (Ctrl-C to stop) ---", DAEMON_LOG);
    loop {
        let mut new_lines = String::new();
        BufReader::new(&file).read_line(&mut new_lines).ok();
        if !new_lines.is_empty() {
            print!("{new_lines}");
        } else {
            std::thread::sleep(std::time::Duration::from_millis(250));
        }
    }
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

/// Single-refresh monitor — no delta sleep. Use for instantaneous values:
/// memory, battery, system info. NOT for CPU%, I/O rates, net rates.
/// Uses the quick backend (skips process/disk/network scan) for ~10x speedup.
fn instant_monitor(mock: bool) -> Box<dyn SystemMonitor> {
    let mut m = if mock {
        peterfan_platform::mock_monitor()
    } else {
        peterfan_platform::quick_monitor()
    };
    m.refresh();
    m
}

/// Double-refresh monitor across [`SAMPLE_MS`] — required for delta metrics:
/// CPU usage %, disk I/O rates, network throughput rates, top process CPU%.
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

/// Returns a double-sampled monitor and hardware provider whose initializations
/// run concurrently — the provider init overlaps the SAMPLE_MS sleep, so the
/// total wall-clock time is max(SAMPLE_MS, provider_init) instead of the sum.
///
/// Safe because `HardwareProvider: Send + Sync`.
fn sampled_monitor_and_provider(mock: bool) -> (Box<dyn SystemMonitor>, Box<dyn HardwareProvider>) {
    let prov_handle = std::thread::spawn(move || provider(mock));
    let mut m = if mock {
        peterfan_platform::mock_monitor()
    } else {
        peterfan_platform::system_monitor()
    };
    m.refresh();
    std::thread::sleep(Duration::from_millis(SAMPLE_MS));
    m.refresh();
    let prov = prov_handle.join().unwrap_or_else(|_| provider(mock));
    (m, prov)
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
    let m = instant_monitor(mock);
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
    if let Some(b) = mem.breakdown {
        println!(
            "  wired {}  ·  active {}  ·  inactive {}  ·  compressed {}",
            render::bytes(b.wired),
            render::bytes(b.active),
            render::bytes(b.inactive),
            render::bytes(b.compressed),
        );
    }
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
    let m = instant_monitor(mock);
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
    let m = instant_monitor(mock);
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
// Status: compact one-liner
// ---------------------------------------------------------------------------

fn cmd_status_compact(mock: bool, json: bool) -> Result<()> {
    // daemon_sensors() gives temps + fans + mode in one IPC call — no second roundtrip.
    let daemon_s = if !mock { daemon_sensors() } else { None };
    let m = if daemon_s.is_some() {
        // Skip provider init and sampling; we have temps from the daemon.
        instant_monitor(mock)
    } else {
        sampled_monitor(mock)
    };
    let cpu = m.cpu();
    let mem = m.memory();
    let (temps, fans, daemon) = if let Some(ds) = daemon_s {
        (ds.temps, ds.fans, ds.daemon_mode)
    } else {
        let prov = provider(mock);
        (
            prov.temperatures().unwrap_or_default(),
            prov.fans().unwrap_or_default(),
            None,
        )
    };
    let hottest = temps.iter().map(|t| t.value.0).fold(0.0_f32, f32::max);
    let fastest = fans.iter().map(|f| f.rpm).fold(0u32, u32::max);

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "cpu_pct": cpu.usage_percent,
                "mem_pct": mem.used_percent,
                "hottest_c": hottest,
                "fastest_rpm": fastest,
                "daemon_mode": daemon,
            }))?
        );
        return Ok(());
    }

    let mut parts = vec![
        format!("CPU {:.0}%", cpu.usage_percent),
        format!("MEM {:.0}%", mem.used_percent),
    ];
    if hottest > 0.0 {
        parts.push(format!("{hottest:.0}°C"));
    }
    if fastest > 0 {
        parts.push(format!("{fastest} RPM"));
    }
    if let Some(mode) = daemon {
        parts.push(mode);
    }
    println!("{}", parts.join(" | "));
    Ok(())
}

// ---------------------------------------------------------------------------
// Status: the full dashboard
// ---------------------------------------------------------------------------

fn cmd_status(mock: bool, json: bool) -> Result<()> {
    // When the daemon has cached thermals, we can skip hardware provider init
    // entirely and run only the system monitor (150 ms sample window).
    // When the daemon is absent, we parallelise provider init with the sample.
    let daemon_s = if !mock { daemon_sensors() } else { None };

    let (m, sensors, thermal_backend, power_w) = if let Some(ds) = daemon_s {
        let m = sampled_monitor(mock);
        let backend = ds.daemon_backend.clone().unwrap_or_else(|| "daemon".into());
        let pw = ds.daemon_power_w;
        (m, ds, backend, pw)
    } else {
        let (m, prov) = sampled_monitor_and_provider(mock);
        let backend = prov.name().to_string();
        let pw = prov.power_watts();
        let s = read_sensors(prov.as_ref())?;
        (m, s, backend, pw)
    };

    let info = m.system_info();
    let cpu = m.cpu();
    let mem = m.memory();
    let disks = m.disks();
    let mut nets = m.networks();
    nets.sort_by(|a, b| (b.rx_total + b.tx_total).cmp(&(a.rx_total + a.tx_total)));
    let battery = m.battery();

    if json {
        let value = serde_json::json!({
            "metrics_backend": m.name(),
            "thermal_backend": thermal_backend,
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
        format!("{} + {}", m.name(), thermal_backend).bold(),
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
    if let Some(b) = mem.breakdown {
        println!(
            "  wired {}  ·  active {}  ·  compressed {}",
            render::bytes(b.wired),
            render::bytes(b.active),
            render::bytes(b.compressed),
        );
    }
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

    // Daemon mode is already in sensor data — no extra IPC round-trip needed.
    if let Some(ref mode) = sensors.daemon_mode {
        println!("  {} {}", "fan daemon:".dimmed(), mode.bold());
    }

    if let Some(w) = power_w {
        println!();
        println!(
            "{} {}",
            render::heading("Power"),
            format!("· {w:.1} W").dimmed()
        );
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Thermal commands (HardwareProvider)
// ---------------------------------------------------------------------------

/// Sensor readings plus context supplied by the daemon cache (if available).
struct Sensors {
    temps: Vec<TempSensor>,
    fans: Vec<Fan>,
    simulated: bool,
    /// Daemon fan-control mode, e.g. "rules:balanced" — set when data came from IPC.
    daemon_mode: Option<String>,
    /// Daemon-reported power draw in watts.
    daemon_power_w: Option<f32>,
    /// Daemon backend name, e.g. "macos".
    daemon_backend: Option<String>,
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
            daemon_mode: None,
            daemon_power_w: None,
            daemon_backend: None,
        });
    }
    let mock = peterfan_platform::mock();
    Ok(Sensors {
        temps: mock.temperatures()?,
        fans: mock.fans()?,
        simulated: true,
        daemon_mode: None,
        daemon_power_w: None,
        daemon_backend: None,
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

fn cmd_temps(mock: bool, json: bool) -> Result<()> {
    let sensors = if !mock {
        if let Some(ds) = daemon_sensors() {
            ds
        } else {
            let prov = provider(mock);
            read_sensors(prov.as_ref())?
        }
    } else {
        let prov = provider(mock);
        read_sensors(prov.as_ref())?
    };
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

fn cmd_fans(mock: bool, json: bool) -> Result<()> {
    let sensors = if !mock {
        if let Some(ds) = daemon_sensors() {
            ds
        } else {
            let prov = provider(mock);
            read_sensors(prov.as_ref())?
        }
    } else {
        let prov = provider(mock);
        read_sensors(prov.as_ref())?
    };
    let daemon_mode = sensors.daemon_mode.clone();
    if json {
        let val = serde_json::json!({
            "fans": sensors.fans,
            "daemon_mode": daemon_mode,
        });
        println!("{}", serde_json::to_string_pretty(&val)?);
        return Ok(());
    }
    if sensors.simulated {
        println!("{}", simulated_note());
    }
    if let Some(ref mode) = daemon_mode {
        println!("  {} daemon: {}", "•".cyan(), mode.bold());
    }
    print_fans(&sensors.fans);
    Ok(())
}

/// True when the process is running as root (fan writes need it).
#[cfg(unix)]
fn is_root() -> bool {
    // SAFETY: geteuid() is always safe and has no preconditions.
    unsafe { libc::geteuid() == 0 }
}
#[cfg(not(unix))]
fn is_root() -> bool {
    true
}

/// Send a command to the running daemon and return the reply (delegates to platform).
#[cfg(unix)]
fn ipc_send(cmd: &str) -> Option<String> {
    peterfan_platform::ipc::send_command(cmd)
}
#[cfg(not(unix))]
fn ipc_send(_cmd: &str) -> Option<String> {
    None
}

fn cmd_fan(provider: &dyn HardwareProvider, action: FanAction, json: bool) -> Result<()> {
    // `fan status` works without control capability — just reads state.
    if matches!(action, FanAction::Status) {
        return cmd_fan_status(provider, json);
    }

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
        FanAction::Status => unreachable!("handled above"),
        FanAction::Set { percent, .. } => {
            let pct = percent.min(100);
            // RPM before the write, so we can verify the fans actually moved.
            let before: Vec<u32> = targets.iter().map(|f| f.rpm).collect();

            // Prefer routing through the daemon: no sudo needed, and the daemon
            // re-asserts the duty every tick so it persists until `fan auto`.
            let via_daemon = if let Some(reply) = ipc_send(&format!("hold {pct}")) {
                if !json {
                    println!(
                        "Routing through daemon: {} {}",
                        reply.green(),
                        "(persists until `peterfan fan auto`)".dimmed()
                    );
                }
                true
            } else {
                // No daemon → direct SMC write (needs root).
                if !json {
                    println!(
                        "Applying… {}",
                        "(unlocking manual control + measuring can take ~10s)".dimmed()
                    );
                }
                for f in &targets {
                    if let Err(e) = provider.set_fan_duty(&f.id, pct) {
                        if matches!(e, CoreError::PermissionDenied(_)) || !is_root() {
                            anyhow::bail!(
                                "fan control needs root — run `sudo peterfan fan set {pct}` \
                                 or install the daemon first: `peterfan install-daemon`"
                            );
                        }
                        return Err(e.into());
                    }
                }
                false
            };

            // Verify: re-read RPM after a few seconds. Whether via daemon or direct
            // write, a non-error does NOT confirm the firmware honored it on Apple
            // Silicon — only an RPM change does.
            let wait = if via_daemon { 3u64 } else { 4 };
            std::thread::sleep(Duration::from_secs(wait));
            let after = provider.fans().unwrap_or_default();
            let mut moved = false;
            let mut rows = Vec::new();
            for (f, &b) in targets.iter().zip(&before) {
                let now = after
                    .iter()
                    .find(|x| x.id == f.id)
                    .map(|x| x.rpm)
                    .unwrap_or(b);
                let delta = now as i64 - b as i64;
                if delta.abs() >= 150 {
                    moved = true;
                }
                rows.push((f.label.clone(), b, now, delta));
            }

            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "action": "set", "duty_percent": pct,
                        "via_daemon": via_daemon, "verified_change": moved,
                        "fans": rows.iter().map(|(l, b, n, d)| serde_json::json!({
                            "label": l, "rpm_before": b, "rpm_after": n, "delta": d,
                        })).collect::<Vec<_>>(),
                    }))?
                );
            } else {
                println!("Sent force-to-{pct}% to {} fan(s):", rows.len());
                for (label, b, n, d) in &rows {
                    println!("  {label:<14} {b} → {n} RPM ({d:+})");
                }
                if moved {
                    println!("  {}", "✓ fans responded to manual control".green());
                    if via_daemon {
                        // Daemon holds the SMC connection open and re-asserts every tick.
                    } else if cfg!(target_arch = "aarch64") {
                        // On Apple Silicon the forced mode reverts when the SMC connection
                        // closes (process exit). Use the daemon for persistent control.
                        println!(
                            "  {}",
                            "⚠ Apple Silicon: forced mode is active while this process runs \
                             but will revert on exit. Use `peterfan install-daemon` for \
                             persistent control."
                                .yellow()
                        );
                    } else {
                        println!(
                            "  {}",
                            "⚠ they stay forced until you run `sudo peterfan fan auto`".yellow()
                        );
                    }
                } else {
                    println!(
                        "  {}",
                        "✗ RPM did not change — the write was accepted but had no effect.".red()
                    );
                    if !via_daemon && !is_root() {
                        println!(
                            "    Retry with sudo: `sudo peterfan fan set {pct}`, \
                             or install the daemon: `peterfan install-daemon`"
                        );
                    } else {
                        println!(
                            "    Apple Silicon firmware may ignore manual fan writes \
                             on some models — try running `peterfan doctor` to check."
                        );
                    }
                }
            }
        }
        FanAction::Auto { .. } => {
            // Prefer daemon IPC: no sudo needed.
            let via_daemon = if let Some(reply) = ipc_send("auto") {
                if !json {
                    println!(
                        "Restored to automatic control via daemon: {}",
                        reply.green()
                    );
                }
                true
            } else {
                for f in &targets {
                    provider.set_fan_auto(&f.id)?;
                }
                false
            };
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "action": "auto",
                        "via_daemon": via_daemon,
                        "fans": targets.iter().map(|f| &f.label).collect::<Vec<_>>(),
                    }))?
                );
            } else if !via_daemon {
                println!(
                    "Restored {} to automatic control.",
                    targets
                        .iter()
                        .map(|f| f.label.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                );
            }
        }
    }
    Ok(())
}

fn cmd_fan_status(provider: &dyn HardwareProvider, json: bool) -> Result<()> {
    let fans = provider.fans().unwrap_or_default();
    let daemon_mode = ipc_send("status")
        .as_deref()
        .and_then(|r| r.strip_prefix("ok "))
        .map(str::to_string);
    let can_control = provider.capabilities().control_fans;

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "can_control": can_control,
                "daemon_mode": daemon_mode,
                "fans": fans.iter().map(|f| serde_json::json!({
                    "id": f.id, "label": f.label, "rpm": f.rpm,
                    "min_rpm": f.min_rpm, "max_rpm": f.max_rpm,
                    "duty_percent": f.duty_percent, "controllable": f.controllable,
                })).collect::<Vec<_>>(),
            }))?
        );
        return Ok(());
    }

    println!("{}", render::heading("Fan control status"));
    print_kv(
        "Control",
        if can_control {
            "available"
        } else {
            "unavailable (read-only backend)"
        },
    );
    match &daemon_mode {
        Some(mode) => print_kv("Daemon mode", mode),
        None => print_kv(
            "Daemon",
            "not running — install with `peterfan install-daemon`",
        ),
    }
    println!();
    println!("{}", render::heading("Fans"));
    print_fans(&fans);
    if daemon_mode.is_none() && can_control {
        println!();
        println!(
            "  {}",
            "tip: `peterfan install-daemon` enables sudo-free fan control".dimmed()
        );
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
    let cfg = peterfan_platform::config::load();
    if json {
        let mut arr: Vec<_> = Profile::all()
            .iter()
            .map(|p| serde_json::json!({ "name": p.as_str(), "description": p.description(), "custom": false }))
            .collect();
        if cfg.custom_curve.is_some() {
            arr.push(serde_json::json!({ "name": "custom", "description": "User-defined curve (config.custom_curve)", "custom": true }));
        }
        for name in cfg.named_curves.keys() {
            arr.push(serde_json::json!({ "name": name, "description": "Named custom curve", "custom": true }));
        }
        println!("{}", serde_json::to_string_pretty(&arr)?);
        return Ok(());
    }
    println!("{}", render::heading("Profiles"));
    for p in Profile::all() {
        println!("  {:<14} {}", p.as_str().bold(), p.description().dimmed());
    }
    if cfg.custom_curve.is_some() {
        println!(
            "  {:<14} {}",
            "custom".bold().cyan(),
            "User-defined curve  →  `peterfan curve custom`".dimmed()
        );
    }
    for name in cfg.named_curves.keys() {
        println!(
            "  {:<14} {}",
            name.bold().cyan(),
            "Named custom curve  →  `peterfan profile list`".dimmed()
        );
    }
    println!();
    println!(
        "  {}",
        "Apply: peterfan profile <name>  ·  Inspect: peterfan curve <name>  ·  Create: peterfan profile create <name> --points 30:20,60:50,...".dimmed()
    );
    Ok(())
}

fn cmd_curve(name: Option<String>, json: bool) -> Result<()> {
    let cfg = peterfan_platform::config::load();
    let (curve, display_name) = match name.as_deref() {
        None => (Profile::Balanced.default_curve(), "balanced".to_string()),
        Some(n) => {
            // Check named custom curves first, then built-in profiles.
            if let Some(c) = cfg.named_curve(n) {
                (c, n.to_string())
            } else if n == "custom" {
                (cfg.curve_for(Profile::Custom), "custom".to_string())
            } else {
                let p = Profile::parse(n)
                    .ok_or_else(|| anyhow::anyhow!("unknown profile or curve '{n}'"))?;
                (cfg.curve_for(p), p.as_str().to_string())
            }
        }
    };
    // shadow for the rest of the function
    let profile = Profile::parse(&display_name).unwrap_or(Profile::Balanced);
    let _ = profile; // used below only for name display

    if json {
        println!("{}", serde_json::to_string_pretty(curve.points())?);
        return Ok(());
    }

    println!(
        "{} {}",
        render::heading("Fan curve"),
        format!("· {display_name}").dimmed()
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
    let monitor = instant_monitor(mock);
    let caps = provider.capabilities();
    let mcaps = monitor.capabilities();
    let elevated = is_elevated();

    if json {
        let mut fan_control = serde_json::json!({
            "elevated": elevated,
            "daemon_reachable": peterfan_platform::daemon_reachable(),
            "control_fans": caps.control_fans,
        });
        #[cfg(target_os = "macos")]
        if let Some(p) = peterfan_platform::fan_control_probe() {
            fan_control["smc_opened"] = serde_json::json!(p.opened);
            fan_control["fan_mode_key"] = serde_json::json!(p.mode_key);
            fan_control["ftst_key"] = serde_json::json!(p.ftst);
            fan_control["fs_key"] = serde_json::json!(p.fs);
        }
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
                "fan_control": fan_control,
            }))?
        );
        return Ok(());
    }

    println!("{}", render::heading("PeterFan doctor"));
    print_kv("Version", env!("CARGO_PKG_VERSION"));
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

    if !mock {
        println!();
        println!("{}", render::heading("Fan control readiness"));
        print_check("running as root", elevated);
        let daemon = peterfan_platform::daemon_reachable();
        print_check("peterfand daemon reachable", daemon);
        #[cfg(target_os = "macos")]
        if let Some(p) = peterfan_platform::fan_control_probe() {
            print_kv("  SMC opened", if p.opened { "yes" } else { "no" });
            print_kv("  fan mode key", p.mode_key.unwrap_or("not found"));
            print_kv(
                "  Ftst unlock key",
                if p.ftst { "present" } else { "absent" },
            );
            print_kv("  FS! force key", if p.fs { "present" } else { "absent" });
        }
        // Verdict.
        let verdict = if !caps.control_fans {
            "this backend can't write fans".yellow().to_string()
        } else if daemon {
            let mode = ipc_send("status")
                .as_deref()
                .and_then(|r| r.strip_prefix("ok "))
                .map(|s| format!(" ({s})"))
                .unwrap_or_default();
            format!(
                "✓ fully ready — daemon is running{mode}; \
                 `peterfan fan set N` and menu-bar buttons work without sudo"
            )
            .green()
            .to_string()
        } else if elevated {
            "ready — try `peterfan fan set 80` (verifies by RPM read-back)"
                .green()
                .to_string()
        } else {
            "not ready — install the daemon once (`peterfan install-daemon`) or \
             run `sudo peterfan fan set 80`"
                .yellow()
                .to_string()
        };
        println!("  → {verdict}");

        // Additional setup checks (macOS only, non-mock).
        #[cfg(target_os = "macos")]
        {
            println!();
            println!("{}", render::heading("Setup"));

            // ── LaunchDaemon ──────────────────────────────────────────────
            let plist_exists =
                std::path::Path::new("/Library/LaunchDaemons/com.uulab.peterfan.daemon.plist")
                    .exists();
            print_check("LaunchDaemon plist installed", plist_exists);
            if plist_exists {
                // `launchctl list <label>` only sees jobs in the *caller's*
                // launchd domain — an unprivileged `peterfan doctor` can never
                // see a system-domain LaunchDaemon there, so that check always
                // reported "not loaded" even when the daemon was fine. The
                // actual IPC reachability check above is the real ground
                // truth (and already ran), so reuse it instead of guessing.
                print_check("  daemon responding over IPC", daemon);
                if !daemon {
                    println!(
                        "    {} run `peterfan install-daemon` to reload",
                        "→".dimmed()
                    );
                }
            } else {
                println!(
                    "    {} run `peterfan install-daemon` to set up persistent fan control",
                    "→".dimmed()
                );
            }

            // ── Menubar login item ────────────────────────────────────────
            let login_item_installed = peterfan_platform::login_item::is_installed();
            print_check("menubar login item installed", login_item_installed);
            if !login_item_installed {
                println!(
                    "    {} run `peterfan login-item install` to start at login",
                    "→".dimmed()
                );
            }

            // ── Config file ───────────────────────────────────────────────
            let cfg = peterfan_platform::config::load();
            let cfg_path = peterfan_platform::config::path();
            let cfg_exists = cfg_path.as_ref().is_some_and(|p| p.exists());
            print_check("config file present", cfg_exists);
            if cfg_exists {
                let bad_rules: Vec<_> = cfg
                    .rules
                    .iter()
                    .filter(|r| r.condition().is_none())
                    .collect();
                if bad_rules.is_empty() {
                    print_kv(
                        "  config",
                        &format!(
                            "profile={} interval={}s critical={:.0}°C rules={}",
                            cfg.profile.as_str(),
                            cfg.interval_secs,
                            cfg.critical_temp_c,
                            cfg.rules.len()
                        ),
                    );
                } else {
                    println!(
                        "  {} config has {} invalid rule(s):",
                        "⚠".yellow(),
                        bad_rules.len()
                    );
                    for r in &bad_rules {
                        println!("      unknown condition: '{}'", r.when);
                    }
                }
            } else {
                println!(
                    "    {} run `peterfan config --init` to create it",
                    "→".dimmed()
                );
            }

            // ── Daemon state file ─────────────────────────────────────────
            let state_file =
                std::path::Path::new("/Library/Application Support/peterfand/state.toml");
            if state_file.exists() {
                let content = std::fs::read_to_string(state_file).unwrap_or_default();
                let mode = content
                    .lines()
                    .find(|l| l.starts_with("mode"))
                    .and_then(|l| l.split('"').nth(1))
                    .unwrap_or("?");
                print_kv("  daemon state file", &format!("present (mode = {mode})"));
            } else {
                print_kv(
                    "  daemon state file",
                    "absent (reboot will use config profile)",
                );
            }

            // ── Log file ──────────────────────────────────────────────────
            let log = std::path::Path::new(DAEMON_LOG);
            if log.exists() {
                let meta = std::fs::metadata(log).ok();
                let size = meta
                    .map(|m| {
                        let kb = m.len() / 1024;
                        if kb > 1024 {
                            format!("{:.1} MB", kb as f32 / 1024.0)
                        } else {
                            format!("{kb} KB")
                        }
                    })
                    .unwrap_or_else(|| "?".into());
                let newsyslog_ok =
                    std::path::Path::new(peterfan_platform::daemon_install::NEWSYSLOG_CONF)
                        .exists();
                let rotation: &str = if newsyslog_ok {
                    "rotation configured"
                } else {
                    "no rotation — run `peterfan install-daemon`"
                };
                print_kv(
                    "  daemon log",
                    &format!("{DAEMON_LOG} ({size}, {rotation})"),
                );
            } else {
                print_kv("  daemon log", "absent (daemon not yet started)");
            }
        }
    }

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
        let io = if d.read_bytes_per_sec + d.write_bytes_per_sec > 0.0 {
            format!(
                "R {} W {}",
                render::rate(d.read_bytes_per_sec),
                render::rate(d.write_bytes_per_sec)
            )
        } else {
            String::new()
        };
        println!(
            "  {:<14} {} / {} ({})  {}  {} {}",
            d.mount,
            render::bytes(d.used),
            render::bytes(d.total),
            render::pct_colored(d.used_percent).trim(),
            render::load_bar(d.used_percent),
            d.kind.dimmed(),
            io.dimmed(),
        );
    }
}

fn print_networks<'a>(nets: impl Iterator<Item = &'a peterfan_core::metrics::NetInterface>) {
    for n in nets {
        let meta = format!(
            "{}total ↓{} ↑{}",
            n.ip.as_deref()
                .map(|ip| format!("{ip}  ·  "))
                .unwrap_or_default(),
            render::bytes(n.rx_total),
            render::bytes(n.tx_total)
        );
        println!(
            "  {:<14} ↓ {:>11}  ↑ {:>11}   {}",
            n.name,
            render::rate(n.rx_rate),
            render::rate(n.tx_rate),
            meta.dimmed(),
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

/// Try to get temperature and fan data from the running daemon's cache via IPC.
/// Returns `None` when the daemon is not running or has no cached readings yet.
/// When successful, the CLI can skip SMC initialisation entirely (~170 ms saved).
/// The returned Sensors also carries daemon_mode, daemon_power_w, and daemon_backend
/// so callers avoid a second `status` IPC round-trip.
#[cfg(unix)]
fn daemon_sensors() -> Option<Sensors> {
    let reply = ipc_send("temps")?;
    let json_str = reply.strip_prefix("ok ")?;
    let v: serde_json::Value = serde_json::from_str(json_str).ok()?;
    let temps: Vec<TempSensor> = serde_json::from_value(v["temps"].clone()).ok()?;
    let fans: Vec<peterfan_core::types::Fan> = serde_json::from_value(v["fans"].clone()).ok()?;
    // Only return a hit when we have at least some real data.
    if temps.is_empty() && fans.is_empty() {
        return None;
    }
    Some(Sensors {
        temps,
        fans,
        simulated: false,
        daemon_mode: v["mode"].as_str().map(str::to_string),
        daemon_power_w: v["power_w"].as_f64().map(|f| f as f32),
        daemon_backend: v["backend"].as_str().map(str::to_string),
    })
}
#[cfg(not(unix))]
fn daemon_sensors() -> Option<Sensors> {
    None
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

// ---------------------------------------------------------------------------
// `peterfan profile create/delete/list` — custom fan curves
// ---------------------------------------------------------------------------

fn cmd_profile_action(json: bool, action: ProfileAction) -> Result<()> {
    use peterfan_core::config::CustomCurveConfig;
    use peterfan_core::curve::CurvePoint;

    match action {
        ProfileAction::Create { name, points } => {
            // Parse "30:20,60:50,80:90,90:100"
            let raw: Vec<[f32; 2]> = points
                .split(',')
                .map(|s| {
                    let s = s.trim();
                    let (t, d) = s
                        .split_once(':')
                        .ok_or_else(|| anyhow::anyhow!("invalid point '{s}' — use temp:duty"))?;
                    let temp: f32 = t
                        .trim()
                        .parse()
                        .map_err(|_| anyhow::anyhow!("bad temp '{}'", t.trim()))?;
                    let duty: f32 = d
                        .trim()
                        .parse()
                        .map_err(|_| anyhow::anyhow!("bad duty '{}'", d.trim()))?;
                    Ok::<_, anyhow::Error>([temp, duty])
                })
                .collect::<Result<_>>()?;

            if raw.len() < 2 {
                anyhow::bail!("a curve needs at least 2 points");
            }
            // Validate by building a FanCurve.
            let pts: Vec<CurvePoint> = raw
                .iter()
                .map(|&[t, d]| CurvePoint::new(t, d as u8))
                .collect();
            peterfan_core::curve::FanCurve::new(pts)
                .map_err(|e| anyhow::anyhow!("invalid curve: {e}"))?;

            let curve = CustomCurveConfig {
                points: raw.clone(),
            };
            let mut cfg = peterfan_platform::config::load();

            if name == "custom" {
                cfg.custom_curve = Some(curve);
            } else {
                cfg.named_curves.insert(name.clone(), curve);
            }
            let path = peterfan_platform::config::save(&cfg)
                .map_err(|e| anyhow::anyhow!("could not write config: {e}"))?;

            if !json {
                println!(
                    "  {} curve '{}' saved ({} points)  ({})",
                    "✓".green(),
                    name.bold(),
                    raw.len(),
                    path.display()
                );
                println!(
                    "  Points: {}",
                    raw.iter()
                        .map(|&[t, d]| format!("{t:.0}°C→{d:.0}%"))
                        .collect::<Vec<_>>()
                        .join("  ")
                );
                println!(
                    "  Use with: {}",
                    format!("peterfan config --set profile {name}").cyan()
                );
            }
            notify_daemon_reload(json);
        }

        ProfileAction::Delete { name } => {
            let mut cfg = peterfan_platform::config::load();
            if name == "custom" {
                if cfg.custom_curve.is_none() {
                    anyhow::bail!("no custom curve defined");
                }
                cfg.custom_curve = None;
            } else if cfg.named_curves.remove(&name).is_none() {
                anyhow::bail!("no named curve '{}' found", name);
            }
            peterfan_platform::config::save(&cfg)
                .map_err(|e| anyhow::anyhow!("could not write config: {e}"))?;
            if !json {
                println!("  {} curve '{}' removed", "✓".green(), name.bold());
            }
            notify_daemon_reload(json);
        }

        ProfileAction::List => {
            let cfg = peterfan_platform::config::load();
            if json {
                let mut out = serde_json::json!({});
                if let Some(cc) = &cfg.custom_curve {
                    out["custom"] = serde_json::json!(cc.points);
                }
                for (k, v) in &cfg.named_curves {
                    out[k] = serde_json::json!(v.points);
                }
                println!("{}", serde_json::to_string_pretty(&out)?);
                return Ok(());
            }
            println!("{}", render::heading("Custom Curves"));
            if cfg.custom_curve.is_none() && cfg.named_curves.is_empty() {
                println!(
                    "  (none — use `peterfan profile create <name> --points \"30:20,60:50,...\"`)"
                );
                return Ok(());
            }
            if let Some(cc) = &cfg.custom_curve {
                let pts_str = cc
                    .points
                    .iter()
                    .map(|&[t, d]| format!("{t:.0}°C→{d:.0}%"))
                    .collect::<Vec<_>>()
                    .join("  ");
                println!("  {} custom  {}", "●".cyan(), pts_str.dimmed());
            }
            for (name, cc) in &cfg.named_curves {
                let pts_str = cc
                    .points
                    .iter()
                    .map(|&[t, d]| format!("{t:.0}°C→{d:.0}%"))
                    .collect::<Vec<_>>()
                    .join("  ");
                println!("  {} {:<12}  {}", "●".cyan(), name.bold(), pts_str.dimmed());
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// `peterfan watch` — live single-line display
// ---------------------------------------------------------------------------

fn cmd_watch(mock: bool, interval_secs: u64) -> Result<()> {
    use std::io::Write;
    use std::sync::atomic::{AtomicBool, Ordering};

    static STOP: AtomicBool = AtomicBool::new(false);

    #[cfg(unix)]
    {
        extern "C" fn on_sig(_: libc::c_int) {
            STOP.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        unsafe {
            libc::signal(libc::SIGINT, on_sig as *const () as libc::sighandler_t);
        }
    }

    let prov = provider(mock);
    let mut mon: Box<dyn peterfan_core::SystemMonitor> = if mock {
        peterfan_platform::mock_monitor()
    } else {
        peterfan_platform::system_monitor()
    };

    eprintln!("PeterFan watch — Ctrl-C to quit");

    while !STOP.load(Ordering::Relaxed) {
        mon.refresh();
        let cpu = mon.cpu();
        let mem = mon.memory();
        let temps = prov.temperatures().unwrap_or_default();
        let fans = prov.fans().unwrap_or_default();
        let hottest = temps.iter().map(|t| t.value.0).fold(0.0_f32, f32::max);
        let fastest_rpm = fans.iter().map(|f| f.rpm).fold(0u32, u32::max);
        let power = prov.power_watts();

        // Read daemon mode (strip backend qualifier).
        let mode = ipc_send("status")
            .as_deref()
            .and_then(|r| r.strip_prefix("ok "))
            .map(|s| s.split_once(" (").map_or(s, |(m, _)| m).to_string())
            .unwrap_or_default();

        let power_part = power.map(|w| format!("  {w:.0}W")).unwrap_or_default();
        let mode_part = if mode.is_empty() {
            String::new()
        } else {
            format!("  {}", mode.bold())
        };

        let temp_color = if hottest >= 80.0 {
            format!("{:.0}°C", hottest).red().to_string()
        } else if hottest >= 60.0 {
            format!("{:.0}°C", hottest).yellow().to_string()
        } else {
            format!("{:.0}°C", hottest).green().to_string()
        };

        let cpu_color = if cpu.usage_percent >= 80.0 {
            format!("{:.0}%", cpu.usage_percent).red().to_string()
        } else if cpu.usage_percent >= 50.0 {
            format!("{:.0}%", cpu.usage_percent).yellow().to_string()
        } else {
            format!("{:.0}%", cpu.usage_percent).green().to_string()
        };

        let line = format!(
            "CPU {}  MEM {:.0}%  {}  {} RPM{}{}   ",
            cpu_color, mem.used_percent, temp_color, fastest_rpm, power_part, mode_part,
        );

        print!("\r{line}");
        let _ = std::io::stdout().flush();

        let step = std::time::Duration::from_millis(100);
        let total = std::time::Duration::from_secs(interval_secs);
        let mut elapsed = std::time::Duration::ZERO;
        while elapsed < total && !STOP.load(Ordering::Relaxed) {
            std::thread::sleep(step);
            elapsed += step;
        }
    }

    println!(); // 커서를 새 줄로 이동
    Ok(())
}

// ---------------------------------------------------------------------------
// `peterfan update` — GitHub 최신 버전 확인
// ---------------------------------------------------------------------------

fn cmd_update() -> Result<()> {
    let current = env!("CARGO_PKG_VERSION");
    print!("Current: v{current}  Checking GitHub…");
    let _ = std::io::Write::flush(&mut std::io::stdout());

    let result = peterfan_platform::updater::fetch_latest_release();
    print!("\r");
    let _ = std::io::Write::flush(&mut std::io::stdout());

    let release = match result {
        Ok(r) => r,
        Err(e) => {
            println!("  {} could not check for updates: {e}", "✗".red());
            return Ok(());
        }
    };

    print_kv("Current version", &format!("v{current}"));
    print_kv("Latest version ", &format!("v{}", release.version));

    if peterfan_platform::updater::is_newer(current, &release.version) {
        println!(
            "\n  {} Update available → {}",
            "→".cyan(),
            release.html_url.bold()
        );
        if let Some(url) = &release.asset_url {
            println!("  {}", url.dimmed());
        }
    } else {
        println!("  {} already up to date", "✓".green());
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// `peterfan alert` — 임계값 초과 시 데스크탑 알림
// ---------------------------------------------------------------------------

/// Send a desktop notification. Tries osascript on macOS, notify-send on Linux,
/// and falls back to stderr on Windows or if both tools are absent.
fn send_notification(title: &str, body: &str) {
    #[cfg(target_os = "macos")]
    {
        let script = format!(
            "display notification {:?} with title {:?} sound name \"Funk\"",
            body, title
        );
        let _ = std::process::Command::new("osascript")
            .arg("-e")
            .arg(&script)
            .status();
    }
    #[cfg(target_os = "linux")]
    {
        let _ = std::process::Command::new("notify-send")
            .args(["-u", "critical", title, body])
            .status();
        return;
    }
    #[cfg(target_os = "windows")]
    {
        let script = format!(
            "[void][Windows.UI.Notifications.ToastNotificationManager, Windows.UI.Notifications, ContentType=WindowsRuntime]; \
             $t = [Windows.UI.Notifications.ToastNotificationManager]::GetTemplateContent([Windows.UI.Notifications.ToastTemplateType]::ToastText02); \
             $t.SelectSingleNode('//text[@id=\"1\"]').InnerText = '{title}'; \
             $t.SelectSingleNode('//text[@id=\"2\"]').InnerText = '{body}'; \
             [Windows.UI.Notifications.ToastNotificationManager]::CreateToastNotifier('PeterFan').Show([Windows.UI.Notifications.ToastNotification]::new($t))"
        );
        let _ = std::process::Command::new("powershell")
            .args(["-NoProfile", "-Command", &script])
            .status();
        return;
    }
    // Fallback for other Unix platforms (FreeBSD, etc.) — write to stderr.
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    eprintln!("[peterfan alert] {title}: {body}");
}

// Each parameter is an independent `peterfan alert` CLI flag; splitting them
// into a struct would just move the same fields elsewhere for no clarity gain.
#[allow(clippy::too_many_arguments)]
fn cmd_alert(
    mock: bool,
    cpu_flag: Option<f32>,
    mem_flag: Option<f32>,
    temp_flag: Option<f32>,
    interval_flag: Option<u64>,
    cooldown_flag: Option<u64>,
    once: bool,
    save: bool,
    sub: Option<AlertAction>,
) -> Result<()> {
    // Subcommand: install / status / remove LaunchAgent.
    if let Some(action) = sub {
        return cmd_alert_agent(action);
    }

    // Merge CLI flags with config defaults.
    let cfg = peterfan_platform::config::load();
    let alert_cfg = &cfg.alert;
    let cpu_thresh = cpu_flag.or(alert_cfg.cpu_pct);
    let mem_thresh = mem_flag.or(alert_cfg.memory_pct);
    let temp_thresh = temp_flag.or(alert_cfg.temp_c);
    let interval = interval_flag.unwrap_or(alert_cfg.interval_secs).max(1);
    let cooldown = cooldown_flag.unwrap_or(alert_cfg.cooldown_secs);

    if cpu_thresh.is_none() && mem_thresh.is_none() && temp_thresh.is_none() {
        anyhow::bail!(
            "no thresholds set — specify at least one of --cpu, --memory, --temp\n\
             or save defaults:  peterfan alert --cpu 85 --temp 90 --save\n\
             Example:           peterfan alert --cpu 85 --temp 90"
        );
    }

    // --save: write thresholds back to config, then exit.
    if save {
        let mut cfg_mut = cfg.clone();
        if let Some(v) = cpu_flag {
            cfg_mut.alert.cpu_pct = Some(v);
        }
        if let Some(v) = mem_flag {
            cfg_mut.alert.memory_pct = Some(v);
        }
        if let Some(v) = temp_flag {
            cfg_mut.alert.temp_c = Some(v);
        }
        if let Some(v) = interval_flag {
            cfg_mut.alert.interval_secs = v;
        }
        if let Some(v) = cooldown_flag {
            cfg_mut.alert.cooldown_secs = v;
        }
        let path = peterfan_platform::config::save(&cfg_mut)
            .map_err(|e| anyhow::anyhow!("could not write config: {e}"))?;
        println!(
            "  {} alert thresholds saved  ({})",
            "✓".green(),
            path.display()
        );
        if let Some(c) = cfg_mut.alert.cpu_pct {
            println!("  cpu  > {c}%");
        }
        if let Some(m) = cfg_mut.alert.memory_pct {
            println!("  mem  > {m}%");
        }
        if let Some(t) = cfg_mut.alert.temp_c {
            println!("  temp > {t}°C");
        }
        println!("  Run `peterfan alert` to start monitoring with these thresholds.");
        return Ok(());
    }

    run_alert_loop(
        mock,
        cpu_thresh,
        mem_thresh,
        temp_thresh,
        interval,
        cooldown,
        once,
    )
}

fn run_alert_loop(
    mock: bool,
    cpu_thresh: Option<f32>,
    mem_thresh: Option<f32>,
    temp_thresh: Option<f32>,
    interval: u64,
    cooldown: u64,
    once: bool,
) -> Result<()> {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Instant;

    if !once {
        println!(
            "{} {}",
            render::heading("PeterFan Alert"),
            "Ctrl-C to stop".dimmed()
        );
        if let Some(c) = cpu_thresh {
            println!("  {} CPU  > {c}%", "•".cyan());
        }
        if let Some(m) = mem_thresh {
            println!("  {} MEM  > {m}%", "•".cyan());
        }
        if let Some(t) = temp_thresh {
            println!("  {} TEMP > {t}°C", "•".cyan());
        }
        println!("  cooldown {cooldown}s  ·  interval {interval}s");
        println!();
    }

    static STOP: AtomicBool = AtomicBool::new(false);
    #[cfg(unix)]
    {
        extern "C" fn on_sig(_: libc::c_int) {
            STOP.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        unsafe {
            libc::signal(libc::SIGINT, on_sig as *const () as libc::sighandler_t);
        }
    }

    let mut last_cpu: Option<Instant> = None;
    let mut last_mem: Option<Instant> = None;
    let mut last_temp: Option<Instant> = None;

    let prov = provider(mock);
    let mut mon: Box<dyn peterfan_core::SystemMonitor> = if mock {
        peterfan_platform::mock_monitor()
    } else {
        peterfan_platform::system_monitor()
    };

    let cooldown_dur = Duration::from_secs(cooldown);
    let mut any_triggered = false;

    loop {
        if STOP.load(Ordering::Relaxed) {
            break;
        }

        mon.refresh();
        let cpu_val = mon.cpu().usage_percent;
        let mem_val = mon.memory().used_percent;
        let temp_val = prov
            .temperatures()
            .unwrap_or_default()
            .iter()
            .map(|t| t.value.0)
            .fold(0.0_f32, f32::max);

        let now = Instant::now();

        macro_rules! check {
            ($thresh:expr, $val:expr, $last:expr, $metric:literal, $unit:literal) => {
                if let Some(thresh) = $thresh {
                    if $val > thresh {
                        let in_cooldown =
                            $last.is_some_and(|t: Instant| now.duration_since(t) < cooldown_dur);
                        if !in_cooldown {
                            let msg = format!(
                                "{} is {:.1}{} (threshold: {}{})",
                                $metric, $val, $unit, thresh, $unit
                            );
                            if once {
                                println!("  {} {}", "!".red().bold(), msg.red());
                            } else {
                                println!("\r  {} alert: {}   ", "!".red().bold(), msg.red());
                                send_notification("PeterFan Alert", &msg);
                            }
                            $last = Some(now);
                            any_triggered = true;
                        }
                    }
                }
            };
        }

        check!(cpu_thresh, cpu_val, last_cpu, "CPU", "%");
        check!(mem_thresh, mem_val, last_mem, "Memory", "%");
        check!(temp_thresh, temp_val, last_temp, "Temperature", "°C");

        if once {
            break;
        }

        let cpu_col = if cpu_val >= cpu_thresh.unwrap_or(f32::MAX) {
            format!("{:.0}%", cpu_val).red().to_string()
        } else {
            format!("{:.0}%", cpu_val).green().to_string()
        };
        let mem_col = if mem_val >= mem_thresh.unwrap_or(f32::MAX) {
            format!("{:.0}%", mem_val).red().to_string()
        } else {
            format!("{:.0}%", mem_val).green().to_string()
        };
        let temp_col = if temp_val >= temp_thresh.unwrap_or(f32::MAX) {
            format!("{:.0}°C", temp_val).red().to_string()
        } else {
            format!("{:.0}°C", temp_val).green().to_string()
        };
        print!(
            "\r  watching  CPU {}  MEM {}  TEMP {}   ",
            cpu_col, mem_col, temp_col
        );
        let _ = std::io::Write::flush(&mut std::io::stdout());

        let step = Duration::from_millis(200);
        let total = Duration::from_secs(interval);
        let mut elapsed = Duration::ZERO;
        while elapsed < total && !STOP.load(Ordering::Relaxed) {
            std::thread::sleep(step);
            elapsed += step;
        }
    }

    if !once {
        println!();
    }
    if once && any_triggered {
        std::process::exit(1);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// `peterfan alert install/status/remove` — user LaunchAgent
// ---------------------------------------------------------------------------

const ALERT_AGENT_LABEL: &str = "dev.peterfan.alert";

fn alert_agent_plist_path() -> Option<std::path::PathBuf> {
    dirs::home_dir().map(|h| {
        h.join("Library")
            .join("LaunchAgents")
            .join(format!("{ALERT_AGENT_LABEL}.plist"))
    })
}

fn alert_agent_plist(bin: &std::path::Path, cfg: &peterfan_core::config::AlertConfig) -> String {
    let mut args = format!(
        "\n    <string>{}</string>\n    <string>alert</string>",
        bin.display()
    );
    if let Some(c) = cfg.cpu_pct {
        args.push_str(&format!(
            "\n    <string>--cpu</string>\n    <string>{c}</string>"
        ));
    }
    if let Some(m) = cfg.memory_pct {
        args.push_str(&format!(
            "\n    <string>--memory</string>\n    <string>{m}</string>"
        ));
    }
    if let Some(t) = cfg.temp_c {
        args.push_str(&format!(
            "\n    <string>--temp</string>\n    <string>{t}</string>"
        ));
    }
    args.push_str(&format!(
        "\n    <string>--interval</string>\n    <string>{}</string>",
        cfg.interval_secs
    ));
    args.push_str(&format!(
        "\n    <string>--cooldown</string>\n    <string>{}</string>",
        cfg.cooldown_secs
    ));
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
  "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>           <string>{ALERT_AGENT_LABEL}</string>
  <key>ProgramArguments</key>
  <array>{args}
  </array>
  <key>RunAtLoad</key>       <true/>
  <key>KeepAlive</key>       <true/>
  <key>StandardOutPath</key> <string>/tmp/peterfan-alert.log</string>
  <key>StandardErrorPath</key><string>/tmp/peterfan-alert.log</string>
</dict>
</plist>
"#
    )
}

fn cmd_alert_agent(action: AlertAction) -> Result<()> {
    let plist_path =
        alert_agent_plist_path().ok_or_else(|| anyhow::anyhow!("could not determine home dir"))?;

    match action {
        AlertAction::Status => {
            if plist_path.exists() {
                let loaded = std::process::Command::new("launchctl")
                    .args(["list", ALERT_AGENT_LABEL])
                    .output()
                    .map(|o| o.status.success())
                    .unwrap_or(false);
                println!(
                    "  {} alert agent installed{}",
                    "✓".green(),
                    if loaded {
                        " (running)"
                    } else {
                        " (not loaded)"
                    }
                );
                println!("  plist: {}", plist_path.display());
                println!("  log:   /tmp/peterfan-alert.log");
            } else {
                println!(
                    "  {} alert agent not installed — run `peterfan alert install`",
                    "✗".yellow()
                );
                println!("  Tip: set thresholds first: peterfan alert --cpu 85 --temp 90 --save");
            }
        }
        AlertAction::Install { binary } => {
            let cfg = peterfan_platform::config::load();
            if cfg.alert.is_empty() {
                anyhow::bail!(
                    "no alert thresholds in config — save them first:\n\
                     peterfan alert --cpu 85 --temp 90 --save"
                );
            }
            let bin = peterfan_platform::login_item::find_menubar_binary(binary.as_deref())
                .map_err(|e| anyhow::anyhow!(e))
                .or_else(|_| {
                    std::env::current_exe().map_err(|e| anyhow::anyhow!("cannot find self: {e}"))
                })
                .unwrap_or_else(|_| std::path::PathBuf::from("peterfan"));
            if let Some(dir) = plist_path.parent() {
                std::fs::create_dir_all(dir)?;
            }
            std::fs::write(&plist_path, alert_agent_plist(&bin, &cfg.alert))?;
            let _ = std::process::Command::new("launchctl")
                .args(["load", "-w", plist_path.to_str().unwrap_or("")])
                .status();
            println!(
                "  {} alert agent installed\n  binary: {}\n  plist:  {}",
                "✓".green(),
                bin.display().bold(),
                plist_path.display()
            );
            let alert = &cfg.alert;
            if let Some(c) = alert.cpu_pct {
                println!("  cpu  > {c}%");
            }
            if let Some(m) = alert.memory_pct {
                println!("  mem  > {m}%");
            }
            if let Some(t) = alert.temp_c {
                println!("  temp > {t}°C");
            }
            println!(
                "  {} peterfan will alert at login automatically",
                "→".dimmed()
            );
        }
        AlertAction::Remove => {
            if !plist_path.exists() {
                println!("  {} alert agent is not installed", "—".dimmed());
                return Ok(());
            }
            let _ = std::process::Command::new("launchctl")
                .args(["unload", "-w", plist_path.to_str().unwrap_or("")])
                .status();
            std::fs::remove_file(&plist_path)?;
            println!(
                "  {} alert agent removed  ({})",
                "✓".green(),
                plist_path.display()
            );
        }
    }
    Ok(())
}
