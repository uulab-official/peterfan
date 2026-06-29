//! `peterfan-tui` — a live terminal dashboard built on ratatui.
//!
//! Polls the active [`HardwareProvider`] once a second and draws temperature
//! and fan gauges plus a sparkline of recent CPU temperature. Quit with `q`,
//! `Esc`, or `Ctrl-C`.
//!
//! Like the CLI, it falls back to the simulated backend for sensor data when
//! the real backend can't read sensors yet.

use std::time::Duration;

use anyhow::Result;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyModifiers};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Gauge, Paragraph, Sparkline};
use ratatui::Frame;

use peterfan_core::types::{Fan, SensorKind, TempSensor};
use peterfan_core::HardwareProvider;

const HISTORY_LEN: usize = 80;

struct Sensors {
    temps: Vec<TempSensor>,
    fans: Vec<Fan>,
    simulated: bool,
}

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

fn main() -> Result<()> {
    let use_mock = std::env::args().any(|a| a == "--mock");
    let provider: Box<dyn HardwareProvider> = if use_mock {
        peterfan_platform::mock()
    } else {
        peterfan_platform::detect()
    };

    let mut terminal = ratatui::init();
    let result = run(&mut terminal, provider.as_ref());
    ratatui::restore();
    result
}

fn run(terminal: &mut ratatui::DefaultTerminal, provider: &dyn HardwareProvider) -> Result<()> {
    let backend_name = provider.name().to_string();
    let mut cpu_history: Vec<u64> = Vec::with_capacity(HISTORY_LEN);

    loop {
        let sensors = read_sensors(provider)?;

        if let Some(cpu) = sensors.temps.iter().find(|t| t.kind == SensorKind::Cpu) {
            cpu_history.push(cpu.value.0.round() as u64);
            if cpu_history.len() > HISTORY_LEN {
                cpu_history.remove(0);
            }
        }

        terminal.draw(|f| ui(f, &backend_name, &sensors, &cpu_history))?;

        // Poll for input for up to ~1s, then refresh.
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

fn ui(f: &mut Frame, backend: &str, sensors: &Sensors, cpu_history: &[u64]) {
    let chunks = Layout::vertical([
        Constraint::Length(1),                              // title
        Constraint::Length(sensors.temps.len() as u16 + 2), // temps
        Constraint::Length(sensors.fans.len() as u16 + 2),  // fans
        Constraint::Min(5),                                 // cpu sparkline
        Constraint::Length(1),                              // footer
    ])
    .split(f.area());

    render_title(f, chunks[0], backend, sensors.simulated);
    render_temps(f, chunks[1], &sensors.temps);
    render_fans(f, chunks[2], &sensors.fans);
    render_history(f, chunks[3], cpu_history);
    render_footer(f, chunks[4]);
}

fn render_title(f: &mut Frame, area: Rect, backend: &str, simulated: bool) {
    let mut spans = vec![
        Span::styled(
            "PeterFan",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(
            format!("backend: {backend}"),
            Style::default().fg(Color::DarkGray),
        ),
    ];
    if simulated {
        spans.push(Span::raw("  "));
        spans.push(Span::styled(
            "[simulated]",
            Style::default().fg(Color::Yellow),
        ));
    }
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn temp_color(c: f32) -> Color {
    match c {
        x if x < 50.0 => Color::Green,
        x if x < 70.0 => Color::Yellow,
        x if x < 85.0 => Color::LightRed,
        _ => Color::Red,
    }
}

fn render_temps(f: &mut Frame, area: Rect, temps: &[TempSensor]) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Temperatures ");
    let inner = block.inner(area);
    f.render_widget(block, area);

    if temps.is_empty() {
        return;
    }
    let rows = Layout::vertical(vec![Constraint::Length(1); temps.len()]).split(inner);
    for (t, row) in temps.iter().zip(rows.iter()) {
        let ratio = (t.value.0 / 100.0).clamp(0.0, 1.0) as f64;
        let gauge = Gauge::default()
            .gauge_style(Style::default().fg(temp_color(t.value.0)))
            .ratio(ratio)
            .label(format!("{:<3} {:<12} {}", t.kind.short(), t.label, t.value));
        f.render_widget(gauge, *row);
    }
}

fn render_fans(f: &mut Frame, area: Rect, fans: &[Fan]) {
    let block = Block::default().borders(Borders::ALL).title(" Fans ");
    let inner = block.inner(area);
    f.render_widget(block, area);

    if fans.is_empty() {
        return;
    }
    let rows = Layout::vertical(vec![Constraint::Length(1); fans.len()]).split(inner);
    for (fan, row) in fans.iter().zip(rows.iter()) {
        let duty = fan.duty_percent.unwrap_or(0);
        let gauge = Gauge::default()
            .gauge_style(Style::default().fg(Color::Cyan))
            .percent(duty as u16)
            .label(format!("{:<12} {:>5} RPM", fan.label, fan.rpm));
        f.render_widget(gauge, *row);
    }
}

fn render_history(f: &mut Frame, area: Rect, cpu_history: &[u64]) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" CPU temperature (live) ");
    let sparkline = Sparkline::default()
        .block(block)
        .data(cpu_history)
        .max(100)
        .style(Style::default().fg(Color::Cyan));
    f.render_widget(sparkline, area);
}

fn render_footer(f: &mut Frame, area: Rect) {
    let footer = Paragraph::new(Line::from(vec![Span::styled(
        "q / Esc: quit   ·   refreshing every 1s",
        Style::default().fg(Color::DarkGray),
    )]));
    f.render_widget(footer, area);
}
