//! `peterfan-tui` — a live terminal system dashboard built on ratatui.
//!
//! Polls the active [`SystemMonitor`] once a second and draws CPU (global +
//! per-core), memory, disk, network, battery, and a top-process table. Quit
//! with `q`, `Esc`, or `Ctrl-C`. Pass `--mock` for the simulated machine.

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
use peterfan_core::types::{Fan, TempSensor};
use peterfan_core::{HardwareProvider, SystemMonitor};

const HISTORY_LEN: usize = 120;

fn main() -> Result<()> {
    let use_mock = std::env::args().any(|a| a == "--mock");
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
}

fn run(
    terminal: &mut ratatui::DefaultTerminal,
    mut monitor: Box<dyn SystemMonitor>,
    provider: Box<dyn HardwareProvider>,
) -> Result<()> {
    let backend = monitor.name().to_string();
    let mut cpu_history: Vec<u64> = Vec::with_capacity(HISTORY_LEN);

    loop {
        // The loop period (~1s) is the sampling interval for usage % and rates.
        monitor.refresh();

        let cpu = monitor.cpu();
        cpu_history.push(cpu.usage_percent.round() as u64);
        if cpu_history.len() > HISTORY_LEN {
            cpu_history.remove(0);
        }

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
        };

        terminal.draw(|f| ui(f, &data))?;

        if event::poll(Duration::from_millis(1000))? {
            if let Event::Key(key) = event::read()? {
                let ctrl_c =
                    key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL);
                if matches!(key.code, KeyCode::Char('q') | KeyCode::Esc) || ctrl_c {
                    return Ok(());
                }
            }
        }
    }
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
    render_footer(f, rows[7]);
}

fn render_thermals(f: &mut Frame, area: Rect, d: &Dashboard) {
    let title = match d.power {
        Some(w) => format!(" Thermals · {w:.1} W "),
        None => " Thermals ".to_string(),
    };
    let block = Block::default().borders(Borders::ALL).title(title);
    let inner = block.inner(area);
    f.render_widget(block, area);

    let cols = Layout::horizontal([Constraint::Percentage(55), Constraint::Percentage(45)]).split(inner);

    // Temperatures (left): up to two rows.
    let temp_lines: Vec<Line> = d
        .temps
        .iter()
        .take(inner.height as usize)
        .map(|t| {
            Line::from(vec![
                Span::styled(format!("{:<12} ", t.label), Style::default().fg(Color::DarkGray)),
                Span::styled(format!("{:.0}°C", t.value.0), Style::default().fg(temp_color(t.value.0))),
            ])
        })
        .collect();
    f.render_widget(Paragraph::new(temp_lines), cols[0]);

    // Fans (right).
    let fan_lines: Vec<Line> = if d.fans.is_empty() {
        vec![Line::from(Span::styled("no fan data", Style::default().fg(Color::DarkGray)))]
    } else {
        d.fans
            .iter()
            .take(inner.height as usize)
            .map(|fan| {
                Line::from(vec![
                    Span::styled(format!("{:<8} ", fan.label), Style::default().fg(Color::DarkGray)),
                    Span::styled(format!("{:>5} RPM", fan.rpm), Style::default().fg(Color::Cyan)),
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

fn render_footer(f: &mut Frame, area: Rect) {
    f.render_widget(
        Paragraph::new(Span::styled(
            "q / Esc: quit   ·   refreshing every 1s",
            Style::default().fg(Color::DarkGray),
        )),
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
