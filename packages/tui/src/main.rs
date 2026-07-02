//! `peterfan-tui` — a live terminal system dashboard built on ratatui.
//!
//! Polls the active [`SystemMonitor`] once a second and draws CPU (global +
//! per-core), memory, disk, network, battery, and a top-process table. Quit
//! with `q`, `Esc`, or `Ctrl-C`.
//!
//! Fan control (when daemon is running or process has root):
//! - `1`–`5` → apply profile: silent / balanced / gaming / performance / maximum
//! - `a`     → restore automatic (OS-managed) control
//! - `r`     → switch daemon to rules mode
//! - `h`     → hold fans at a typed duty % (0–100), Enter to confirm, Esc to cancel
//!
//! Pass `--mock` for the simulated machine.

use std::time::Duration;

use anyhow::Result;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyModifiers};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Gauge, Paragraph, Row, Sparkline, Table};
use ratatui::Frame;

use peterfan_core::metrics::{
    BatteryInfo, CpuMetrics, DiskInfo, MemoryMetrics, NetInterface, ProcSort, ProcessInfo,
    SystemInfo,
};
use peterfan_core::profile::Profile;
use peterfan_core::types::{Fan, TempSensor};
use peterfan_core::{HardwareProvider, SystemMonitor};

const HISTORY_LEN: usize = 120;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("peterfan-tui {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!(
            "peterfan-tui {}\n\n\
             Live terminal system dashboard (CPU, memory, disk, network, \
             battery, temps, fans, processes).\n\n\
             USAGE:\n    peterfan-tui [OPTIONS]\n\n\
             OPTIONS:\n    \
             --mock          Use simulated hardware instead of real sensors\n    \
             --version, -V   Print version and exit\n    \
             --help, -h      Print this help and exit\n\n\
             Keys once running: q/Esc quit · 1-5 apply profile · a auto · \
             r rules · h hold %",
            env!("CARGO_PKG_VERSION")
        );
        return Ok(());
    }
    let use_mock = args.iter().any(|a| a == "--mock");
    let monitor: Box<dyn SystemMonitor> = if use_mock {
        peterfan_platform::mock_monitor()
    } else {
        peterfan_platform::system_monitor()
    };
    let provider: Box<dyn HardwareProvider> = if use_mock {
        peterfan_platform::mock()
    } else {
        peterfan_platform::detect()
    };

    let mut terminal = ratatui::init();
    let result = run(&mut terminal, monitor, provider);
    ratatui::restore();
    result
}

/// All the data drawn in one frame.
struct Dashboard<'a> {
    backend: &'a str,
    system: SystemInfo,
    cpu: CpuMetrics,
    memory: MemoryMetrics,
    disks: Vec<DiskInfo>,
    nets: Vec<NetInterface>,
    battery: Option<BatteryInfo>,
    procs: Vec<ProcessInfo>,
    temps: Vec<TempSensor>,
    fans: Vec<Fan>,
    power: Option<f32>,
    cpu_history: &'a [u64],
    /// Current fan control status (daemon mode or last command reply).
    fan_status: String,
    /// Whether fan control is available (daemon reachable or local root).
    can_control: bool,
    /// When `Some`, we are in hold-input mode and this is the digits typed so far.
    hold_input: Option<String>,
}

fn run(
    terminal: &mut ratatui::DefaultTerminal,
    mut monitor: Box<dyn SystemMonitor>,
    provider: Box<dyn HardwareProvider>,
) -> Result<()> {
    let backend = monitor.name().to_string();
    let mut cpu_history: Vec<u64> = Vec::with_capacity(HISTORY_LEN);
    // Transient message shown for one tick after a key press (then replaced by daemon status).
    let mut pending_msg: Option<String> = None;
    // When Some, we're collecting digits for a hold-% command.
    let mut hold_input: Option<String> = None;

    loop {
        monitor.refresh();

        let cpu = monitor.cpu();
        cpu_history.push(cpu.usage_percent.round() as u64);
        if cpu_history.len() > HISTORY_LEN {
            cpu_history.remove(0);
        }

        // Query daemon status; fall back to checking local control capability.
        let daemon_st = ipc_status();
        let can_control = !daemon_st.is_empty() || provider.capabilities().control_fans;

        let fan_status = if let Some(msg) = pending_msg.take() {
            msg
        } else {
            daemon_st
        };

        let data = Dashboard {
            backend: &backend,
            system: monitor.system_info(),
            cpu,
            memory: monitor.memory(),
            disks: monitor.disks(),
            nets: monitor.networks(),
            battery: monitor.battery(),
            procs: monitor.processes(12, ProcSort::Cpu),
            temps: provider.temperatures().unwrap_or_default(),
            fans: provider.fans().unwrap_or_default(),
            power: provider.power_watts(),
            cpu_history: &cpu_history,
            fan_status,
            can_control,
            hold_input: hold_input.clone(),
        };

        terminal.draw(|f| ui(f, &data))?;

        // Poll with a short timeout so hold-input feels responsive.
        let poll_ms = if hold_input.is_some() { 100 } else { 1000 };
        if event::poll(Duration::from_millis(poll_ms))? {
            if let Event::Key(key) = event::read()? {
                let ctrl_c =
                    key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL);

                // Handle hold-input mode first.
                if hold_input.is_some() {
                    match key.code {
                        KeyCode::Esc => {
                            hold_input = None;
                        }
                        KeyCode::Backspace => {
                            if let Some(ref mut s) = hold_input {
                                s.pop();
                            }
                        }
                        KeyCode::Char(c) if c.is_ascii_digit() => {
                            if let Some(ref mut s) = hold_input {
                                if s.len() < 3 {
                                    s.push(c);
                                }
                            }
                        }
                        KeyCode::Enter => {
                            if let Some(s) = hold_input.take() {
                                let pct: u32 = s.parse().unwrap_or(0);
                                let pct = pct.min(100);
                                let msg = if let Some(reply) = ipc_send(&format!("hold {pct}")) {
                                    format!("→ hold {pct}%: {reply}")
                                } else {
                                    format!("→ hold {pct}%: daemon not reachable")
                                };
                                pending_msg = Some(msg);
                            }
                        }
                        _ => {}
                    }
                    continue;
                }

                if ctrl_c || matches!(key.code, KeyCode::Char('q') | KeyCode::Esc) {
                    return Ok(());
                }

                if can_control {
                    if key.code == KeyCode::Char('h') {
                        hold_input = Some(String::new());
                    } else {
                        pending_msg = handle_fan_key(key.code, provider.as_ref());
                    }
                }
            }
        }
    }
}

/// Query the daemon's current mode for the status line.
fn ipc_status() -> String {
    #[cfg(unix)]
    if let Some(reply) = peterfan_platform::ipc::send_command("status") {
        if let Some(rest) = reply.strip_prefix("ok ") {
            return rest.to_string();
        }
    }
    String::new()
}

/// Handle a fan-control key press. Routes through daemon IPC when available,
/// falls back to direct SMC writes (needs root). Returns a status string or `None`.
fn handle_fan_key(key: KeyCode, provider: &dyn HardwareProvider) -> Option<String> {
    let profile_name = match key {
        KeyCode::Char('1') => Some("silent"),
        KeyCode::Char('2') => Some("balanced"),
        KeyCode::Char('3') => Some("gaming"),
        KeyCode::Char('4') => Some("performance"),
        KeyCode::Char('5') => Some("maximum"),
        _ => None,
    };

    if let Some(name) = profile_name {
        // Try daemon first (no root needed).
        if let Some(reply) = ipc_send(&format!("profile {name}")) {
            return Some(format!("→ {name}: {reply}"));
        }
        // Direct fallback (needs root / mock).
        if let Some(p) = Profile::parse(name) {
            let temps = provider.temperatures().unwrap_or_default();
            let hot = temps.iter().map(|t| t.value.0).fold(0.0_f32, f32::max);
            let duty = p.default_curve().duty_at(hot);
            let fans: Vec<_> = provider
                .fans()
                .unwrap_or_default()
                .into_iter()
                .filter(|f| f.controllable)
                .collect();
            let ok = fans
                .iter()
                .all(|f| provider.set_fan_duty(&f.id, duty).is_ok());
            return Some(if ok {
                format!("→ {name} ({duty}%)")
            } else {
                format!("→ {name}: needs daemon or root")
            });
        }
    }

    if key == KeyCode::Char('a') {
        if let Some(reply) = ipc_send("auto") {
            return Some(format!("→ auto: {reply}"));
        }
        let fans: Vec<_> = provider
            .fans()
            .unwrap_or_default()
            .into_iter()
            .filter(|f| f.controllable)
            .collect();
        let ok = fans.iter().all(|f| provider.set_fan_auto(&f.id).is_ok());
        return Some(if ok {
            "→ auto".into()
        } else {
            "→ auto: needs daemon or root".into()
        });
    }

    if key == KeyCode::Char('r') {
        if let Some(reply) = ipc_send("rules") {
            return Some(format!("→ rules: {reply}"));
        }
        return Some("→ rules: daemon not reachable".into());
    }

    None
}

fn ipc_send(cmd: &str) -> Option<String> {
    #[cfg(unix)]
    return peterfan_platform::ipc::send_command(cmd);
    #[cfg(not(unix))]
    let _ = cmd;
    #[cfg(not(unix))]
    None
}

fn ui(f: &mut Frame, d: &Dashboard) {
    let rows = Layout::vertical([
        Constraint::Length(1), // title
        Constraint::Length(4), // cpu
        Constraint::Length(3), // memory
        Constraint::Length(4), // disk + network side by side
        Constraint::Length(3), // battery / cpu history
        Constraint::Length(4), // thermals (temps + fans + power)
        Constraint::Min(5),    // processes
        Constraint::Length(1), // footer
    ])
    .split(f.area());

    render_title(f, rows[0], d);
    render_cpu(f, rows[1], &d.cpu);
    render_memory(f, rows[2], &d.memory);

    let mid =
        Layout::horizontal([Constraint::Percentage(55), Constraint::Percentage(45)]).split(rows[3]);
    render_disk(f, mid[0], &d.disks);
    render_network(f, mid[1], &d.nets);

    let low =
        Layout::horizontal([Constraint::Percentage(55), Constraint::Percentage(45)]).split(rows[4]);
    render_history(f, low[0], d.cpu_history);
    render_battery(f, low[1], d.battery.as_ref());

    render_thermals(f, rows[5], d);
    render_processes(f, rows[6], &d.procs);
    render_footer(f, rows[7], d);
}

fn render_thermals(f: &mut Frame, area: Rect, d: &Dashboard) {
    // Show fan status (daemon mode or last command) in the block title.
    let fan_label = if d.fan_status.is_empty() {
        String::new()
    } else {
        format!(" · {}", d.fan_status)
    };
    let title = match d.power {
        Some(w) => format!(" Thermals · {w:.1} W{fan_label} "),
        None => format!(" Thermals{fan_label} "),
    };
    let block = Block::default().borders(Borders::ALL).title(title);
    let inner = block.inner(area);
    f.render_widget(block, area);

    let cols =
        Layout::horizontal([Constraint::Percentage(55), Constraint::Percentage(45)]).split(inner);

    // Temperatures (left): up to two rows.
    let temp_lines: Vec<Line> = d
        .temps
        .iter()
        .take(inner.height as usize)
        .map(|t| {
            Line::from(vec![
                Span::styled(
                    format!("{:<12} ", t.label),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(
                    format!("{:.0}°C", t.value.0),
                    Style::default().fg(temp_color(t.value.0)),
                ),
            ])
        })
        .collect();
    f.render_widget(Paragraph::new(temp_lines), cols[0]);

    // Fans (right).
    let fan_lines: Vec<Line> = if d.fans.is_empty() {
        vec![Line::from(Span::styled(
            "no fan data",
            Style::default().fg(Color::DarkGray),
        ))]
    } else {
        d.fans
            .iter()
            .take(inner.height as usize)
            .map(|fan| {
                let rpm_str = format!("{:>5} RPM", fan.rpm);
                let duty_str = fan
                    .duty_percent
                    .map(|d| format!(" {:>3}%", d))
                    .unwrap_or_default();
                Line::from(vec![
                    Span::styled(
                        format!("{:<8} ", fan.label),
                        Style::default().fg(Color::DarkGray),
                    ),
                    Span::styled(rpm_str, Style::default().fg(Color::Cyan)),
                    Span::styled(duty_str, Style::default().fg(Color::Yellow)),
                ])
            })
            .collect()
    };
    f.render_widget(Paragraph::new(fan_lines), cols[1]);
}

fn render_title(f: &mut Frame, area: Rect, d: &Dashboard) {
    let os = d
        .system
        .os_name
        .as_deref()
        .map(|n| match &d.system.os_version {
            Some(v) => format!("{n} {v}"),
            None => n.to_string(),
        })
        .unwrap_or_default();
    let spans = vec![
        Span::styled(
            "PeterFan",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(
                "   {} · {} · up {}",
                d.backend,
                os,
                humantime(d.system.uptime_secs)
            ),
            Style::default().fg(Color::DarkGray),
        ),
    ];
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn load_color(p: f32) -> Color {
    match p {
        x if x < 50.0 => Color::Green,
        x if x < 80.0 => Color::Yellow,
        _ => Color::Red,
    }
}

fn temp_color(c: f32) -> Color {
    match c {
        x if x < 50.0 => Color::Green,
        x if x < 70.0 => Color::Yellow,
        _ => Color::Red,
    }
}

fn render_cpu(f: &mut Frame, area: Rect, cpu: &CpuMetrics) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(" CPU · {} ", cpu.brand));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let rows = Layout::vertical([Constraint::Length(1), Constraint::Min(1)]).split(inner);
    let gauge = Gauge::default()
        .gauge_style(Style::default().fg(load_color(cpu.usage_percent)))
        .ratio((cpu.usage_percent / 100.0).clamp(0.0, 1.0) as f64)
        .label(format!(
            "{:.1}%   {} MHz{}",
            cpu.usage_percent,
            cpu.frequency_mhz,
            cpu.load_avg
                .map(|l| format!("   load {:.2} {:.2} {:.2}", l.one, l.five, l.fifteen))
                .unwrap_or_default()
        ));
    f.render_widget(gauge, rows[0]);

    let data: Vec<u64> = cpu.per_core.iter().map(|c| c.round() as u64).collect();
    let spark = Sparkline::default()
        .data(&data)
        .max(100)
        .style(Style::default().fg(Color::Cyan));
    f.render_widget(spark, rows[1]);
}

fn render_memory(f: &mut Frame, area: Rect, mem: &MemoryMetrics) {
    let block = Block::default().borders(Borders::ALL).title(" Memory ");
    let inner = block.inner(area);
    f.render_widget(block, area);
    let gauge = Gauge::default()
        .gauge_style(Style::default().fg(load_color(mem.used_percent)))
        .ratio((mem.used_percent / 100.0).clamp(0.0, 1.0) as f64)
        .label(format!(
            "{} / {} ({:.1}%)",
            bytes(mem.used),
            bytes(mem.total),
            mem.used_percent
        ));
    f.render_widget(gauge, inner);
}

fn render_disk(f: &mut Frame, area: Rect, disks: &[DiskInfo]) {
    let block = Block::default().borders(Borders::ALL).title(" Disk ");
    let inner = block.inner(area);
    f.render_widget(block, area);
    if disks.is_empty() {
        return;
    }
    let n = inner.height as usize;
    let shown: Vec<&DiskInfo> = disks.iter().take(n).collect();
    let rows = Layout::vertical(vec![Constraint::Length(1); shown.len()]).split(inner);
    for (disk, row) in shown.iter().zip(rows.iter()) {
        let gauge = Gauge::default()
            .gauge_style(Style::default().fg(load_color(disk.used_percent)))
            .ratio((disk.used_percent / 100.0).clamp(0.0, 1.0) as f64)
            .label(format!(
                "{} {} / {}",
                disk.mount,
                bytes(disk.used),
                bytes(disk.total)
            ));
        f.render_widget(gauge, *row);
    }
}

fn render_network(f: &mut Frame, area: Rect, nets: &[NetInterface]) {
    let block = Block::default().borders(Borders::ALL).title(" Network ");
    let inner = block.inner(area);
    f.render_widget(block, area);

    let rx: f64 = nets.iter().map(|n| n.rx_rate).sum();
    let tx: f64 = nets.iter().map(|n| n.tx_rate).sum();
    let lines = vec![
        Line::from(vec![
            Span::styled("↓ ", Style::default().fg(Color::Green)),
            Span::raw(format!("{}/s", bytes(rx as u64))),
        ]),
        Line::from(vec![
            Span::styled("↑ ", Style::default().fg(Color::Magenta)),
            Span::raw(format!("{}/s", bytes(tx as u64))),
        ]),
    ];
    f.render_widget(Paragraph::new(lines), inner);
}

fn render_history(f: &mut Frame, area: Rect, history: &[u64]) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" CPU history ");
    let spark = Sparkline::default()
        .block(block)
        .data(history)
        .max(100)
        .style(Style::default().fg(Color::Cyan));
    f.render_widget(spark, area);
}

fn render_battery(f: &mut Frame, area: Rect, battery: Option<&BatteryInfo>) {
    let block = Block::default().borders(Borders::ALL).title(" Battery ");
    let inner = block.inner(area);
    f.render_widget(block, area);
    match battery {
        None => {
            f.render_widget(
                Paragraph::new(Span::styled(
                    "no battery",
                    Style::default().fg(Color::DarkGray),
                )),
                inner,
            );
        }
        Some(b) => {
            let gauge = Gauge::default()
                .gauge_style(Style::default().fg(load_color(100.0 - b.charge_percent)))
                .ratio((b.charge_percent / 100.0).clamp(0.0, 1.0) as f64)
                .label(format!("{:.0}% {}", b.charge_percent, b.state));
            f.render_widget(gauge, inner);
        }
    }
}

fn render_processes(f: &mut Frame, area: Rect, procs: &[ProcessInfo]) {
    let header = Row::new(vec!["PID", "CPU%", "MEM", "NAME"]).style(
        Style::default()
            .fg(Color::DarkGray)
            .add_modifier(Modifier::BOLD),
    );
    let rows: Vec<Row> = procs
        .iter()
        .map(|p| {
            Row::new(vec![
                Cell::from(p.pid.to_string()),
                Cell::from(format!("{:.1}", p.cpu_percent)),
                Cell::from(bytes(p.memory)),
                Cell::from(p.name.clone()),
            ])
        })
        .collect();
    let table = Table::new(
        rows,
        [
            Constraint::Length(8),
            Constraint::Length(7),
            Constraint::Length(11),
            Constraint::Min(10),
        ],
    )
    .header(header)
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Top processes (by CPU) "),
    );
    f.render_widget(table, area);
}

fn render_footer(f: &mut Frame, area: Rect, d: &Dashboard) {
    // Hold-input mode: show a typed-input prompt instead of the keybinding hint.
    if let Some(ref digits) = d.hold_input {
        let cursor = if digits.len() < 3 { "_" } else { "" };
        let spans = vec![
            Span::styled(
                " Hold fans at: ",
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("{digits}{cursor}% "),
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "  Enter: confirm   Esc: cancel",
                Style::default().fg(Color::DarkGray),
            ),
        ];
        f.render_widget(Paragraph::new(Line::from(spans)), area);
        return;
    }

    let text = if d.can_control {
        "q/Esc: quit   ·   1-5: profile   ·   a: auto   ·   r: rules   ·   h: hold %"
    } else {
        "q / Esc: quit   ·   refreshing every 1s"
    };
    f.render_widget(
        Paragraph::new(Span::styled(text, Style::default().fg(Color::DarkGray))),
        area,
    );
}

// --- small formatting helpers (kept local to avoid a shared-crate dependency) ---

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

fn humantime(secs: u64) -> String {
    let d = secs / 86_400;
    let h = (secs % 86_400) / 3600;
    let m = (secs % 3600) / 60;
    if d > 0 {
        format!("{d}d {h}h {m}m")
    } else if h > 0 {
        format!("{h}h {m}m")
    } else {
        format!("{m}m")
    }
}
