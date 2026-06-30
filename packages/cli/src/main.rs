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
        /// What to show in the menu bar: cpu (default), temp, fan.
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
        Command::Config { init, set, get } => cmd_config(json, init, set, get),
        Command::Rule { action } => cmd_rule(json, action.unwrap_or(RuleAction::List)),
        Command::Daemon { action } => cmd_daemon(json, action),
        Command::Serve { port } => cmd_serve(mock, port),
        Command::Benchmark { secs } => cmd_benchmark(mock, json, secs),
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
    }
}

/// LaunchDaemon label + paths (kept in sync with `packaging/…plist`).
const DAEMON_LABEL: &str = "com.uulab.peterfan.daemon";

/// The LaunchDaemon plist, generated so the install needs no extra files.
fn daemon_plist() -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>{DAEMON_LABEL}</string>
  <key>ProgramArguments</key>
  <array>
    <string>/usr/local/bin/peterfand</string>
    <string>--profile</string><string>balanced</string>
  </array>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><true/>
  <key>StandardOutPath</key><string>/var/log/peterfand.log</string>
  <key>StandardErrorPath</key><string>/var/log/peterfand.err</string>
</dict>
</plist>
"#
    )
}

/// Find the `peterfand` binary shipped next to this `peterfan` executable.
#[cfg(target_os = "macos")]
fn find_peterfand() -> Result<std::path::PathBuf> {
    let mut cands = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            cands.push(dir.join("peterfand"));
        }
    }
    cands.push(std::path::PathBuf::from("./peterfand"));
    cands.push(std::path::PathBuf::from("target/release/peterfand"));
    cands.into_iter().find(|p| p.exists()).ok_or_else(|| {
        anyhow::anyhow!("peterfand not found next to peterfan (it ships in the same archive)")
    })
}

/// Run a privileged shell script via one macOS admin-password GUI prompt.
#[cfg(target_os = "macos")]
fn run_privileged(script: &str, dry_run: bool) -> Result<()> {
    let path = std::env::temp_dir().join("peterfan-daemon-install.sh");
    if path.to_string_lossy().contains('\'') {
        anyhow::bail!("temp path contains a quote; aborting");
    }
    std::fs::write(&path, script)?;
    let apple = format!(
        "do shell script \"/bin/bash '{}'\" with administrator privileges",
        path.display()
    );
    if dry_run {
        println!(
            "--- script ({}) ---\n{script}\n--- osascript ---\n{apple}",
            path.display()
        );
        return Ok(());
    }
    let status = std::process::Command::new("osascript")
        .arg("-e")
        .arg(&apple)
        .status()?;
    let _ = std::fs::remove_file(&path);
    if !status.success() {
        anyhow::bail!("privileged step was cancelled or failed");
    }
    Ok(())
}

const LOGIN_ITEM_LABEL: &str = "dev.peterfan.menubar";
const LOGIN_ITEM_BINARY: &str = "peterfan-menubar";

fn login_item_plist_path() -> Option<std::path::PathBuf> {
    dirs::home_dir().map(|h: std::path::PathBuf| {
        h.join("Library")
            .join("LaunchAgents")
            .join(format!("{LOGIN_ITEM_LABEL}.plist"))
    })
}

fn find_menubar_binary(override_path: Option<&str>) -> Result<std::path::PathBuf> {
    if let Some(p) = override_path {
        let path = std::path::PathBuf::from(p);
        if path.exists() {
            return Ok(path);
        }
        anyhow::bail!("binary not found at '{p}'");
    }
    // Look next to the current executable.
    if let Ok(exe) = std::env::current_exe() {
        let sibling = exe.parent().map(|d| d.join(LOGIN_ITEM_BINARY));
        if let Some(s) = sibling.filter(|p| p.exists()) {
            return Ok(s);
        }
    }
    // Fall back to $PATH.
    if let Ok(out) = std::process::Command::new("which")
        .arg(LOGIN_ITEM_BINARY)
        .output()
    {
        let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !s.is_empty() {
            return Ok(std::path::PathBuf::from(s));
        }
    }
    anyhow::bail!(
        "could not find '{LOGIN_ITEM_BINARY}' — use --binary <path> to specify its location"
    )
}

fn login_item_plist(bin: &std::path::Path, metric: &str) -> String {
    let metric_arg = if metric == "cpu" {
        String::new()
    } else {
        format!("\n    <string>--metric</string>\n    <string>{metric}</string>")
    };
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
  "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>       <string>{LOGIN_ITEM_LABEL}</string>
  <key>ProgramArguments</key>
  <array>
    <string>{}</string>{metric_arg}
  </array>
  <key>RunAtLoad</key>   <true/>
  <key>KeepAlive</key>   <false/>
  <key>StandardOutPath</key> <string>/tmp/peterfan-menubar.log</string>
  <key>StandardErrorPath</key> <string>/tmp/peterfan-menubar.log</string>
</dict>
</plist>
"#,
        bin.display()
    )
}

#[cfg(target_os = "macos")]
fn cmd_login_item(action: LoginItemAction) -> Result<()> {
    use owo_colors::OwoColorize;
    let plist_path =
        login_item_plist_path().ok_or_else(|| anyhow::anyhow!("could not determine home dir"))?;

    match action {
        LoginItemAction::Status => {
            if plist_path.exists() {
                let content = std::fs::read_to_string(&plist_path).unwrap_or_default();
                let bin_line = content
                    .lines()
                    .skip_while(|l| !l.contains("ProgramArguments"))
                    .nth(2)
                    .map(|l| l.trim().trim_start_matches("<string>").trim_end_matches("</string>"))
                    .unwrap_or("?");
                println!(
                    "  {} login item installed\n  binary: {}\n  plist:  {}",
                    "✓".green(),
                    bin_line.bold(),
                    plist_path.display()
                );
            } else {
                println!(
                    "  {} login item not installed — run `peterfan login-item install`",
                    "✗".yellow()
                );
            }
        }
        LoginItemAction::Install { binary, metric } => {
            let bin = find_menubar_binary(binary.as_deref())?;
            if let Some(dir) = plist_path.parent() {
                std::fs::create_dir_all(dir)?;
            }
            std::fs::write(&plist_path, login_item_plist(&bin, &metric))?;
            // Load it immediately so the user doesn't have to log out/in.
            let _ = std::process::Command::new("launchctl")
                .args(["load", "-w", plist_path.to_str().unwrap_or("")])
                .status();
            println!(
                "  {} login item installed\n  binary: {}\n  plist:  {}\n  {} peterfan-menubar will start at login",
                "✓".green(),
                bin.display().bold(),
                plist_path.display(),
                "→".dimmed()
            );
        }
        LoginItemAction::Remove => {
            if !plist_path.exists() {
                println!("  {} login item is not installed", "—".dimmed());
                return Ok(());
            }
            let _ = std::process::Command::new("launchctl")
                .args(["unload", "-w", plist_path.to_str().unwrap_or("")])
                .status();
            std::fs::remove_file(&plist_path)?;
            println!(
                "  {} login item removed  ({})",
                "✓".green(),
                plist_path.display()
            );
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
    let bin = find_peterfand()?;
    let plist_dst = format!("/Library/LaunchDaemons/{DAEMON_LABEL}.plist");
    let script = format!(
        "set -e\n\
         install -m 755 '{bin}' /usr/local/bin/peterfand\n\
         cat > '{plist_dst}' <<'PLIST'\n{plist}PLIST\n\
         chown root:wheel '{plist_dst}'\n\
         chmod 644 '{plist_dst}'\n\
         launchctl bootout system '{plist_dst}' 2>/dev/null || true\n\
         launchctl bootstrap system '{plist_dst}'\n",
        bin = bin.display(),
        plist = daemon_plist(),
    );
    println!(
        "Installing the PeterFan fan-control daemon as root.\n\
         macOS will ask for your password once — after that the menu-bar app and \
         `peterfan fan …` work without sudo (just like Macs Fan Control)."
    );
    run_privileged(&script, dry_run)?;
    if dry_run {
        return Ok(());
    }
    std::thread::sleep(Duration::from_millis(800));
    if peterfan_platform::daemon_reachable() {
        println!("{}", "✓ daemon installed and running (root)".green());
        println!("  it starts at every boot; logs at /var/log/peterfand.log");
    } else {
        println!(
            "{}",
            "installed, but the daemon isn't answering yet — check /var/log/peterfand.err".yellow()
        );
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn cmd_uninstall_daemon(dry_run: bool) -> Result<()> {
    let plist_dst = format!("/Library/LaunchDaemons/{DAEMON_LABEL}.plist");
    let script = format!(
        "launchctl bootout system '{plist_dst}' 2>/dev/null || true\n\
         rm -f '{plist_dst}' /usr/local/bin/peterfand\n"
    );
    println!("Removing the PeterFan fan-control daemon (one admin prompt)…");
    run_privileged(&script, dry_run)?;
    if !dry_run {
        println!("{}", "✓ daemon removed".green());
    }
    Ok(())
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

fn cmd_benchmark(mock: bool, json: bool, secs: u64) -> Result<()> {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    let secs = secs.max(1);
    let mut monitor = if mock {
        peterfan_platform::mock_monitor()
    } else {
        peterfan_platform::system_monitor()
    };
    let provider = provider(mock);

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

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "duration_secs": secs,
                "threads": workers,
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
            _ => anyhow::bail!("unknown key '{key}'; valid keys: profile, interval, critical"),
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
            _ => anyhow::bail!("unknown key '{key}'; valid keys: profile, interval, critical"),
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

    // Show daemon status below fans if reachable.
    if !mock {
        if let Some(daemon_st) = ipc_send("status")
            .as_deref()
            .and_then(|r| r.strip_prefix("ok "))
            .map(str::to_string)
        {
            println!(
                "  {} {}",
                "fan daemon:".dimmed(),
                daemon_st.bold()
            );
        }
    }

    if let Some(w) = provider.power_watts() {
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
    let daemon_mode = ipc_send("status")
        .as_deref()
        .and_then(|r| r.strip_prefix("ok "))
        .map(str::to_string);
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
    if let Some(mode) = &daemon_mode {
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
        if can_control { "available" } else { "unavailable (read-only backend)" },
    );
    match &daemon_mode {
        Some(mode) => print_kv("Daemon mode", mode),
        None => print_kv("Daemon", "not running — install with `peterfan install-daemon`"),
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
