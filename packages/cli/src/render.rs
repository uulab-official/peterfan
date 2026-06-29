//! Terminal rendering helpers: colored bars, temperature coloring, headings.
//!
//! Kept separate from command logic so the same primitives are reused across
//! `status`, `temps`, `fans`, and `curve`.

use owo_colors::OwoColorize;
use peterfan_core::types::{Celsius, SensorKind};

/// Width, in characters, of the inline bar graphs.
const BAR_WIDTH: usize = 12;

/// A filled/empty block bar for `value` within `[0, max]`.
pub fn bar(value: f32, max: f32) -> String {
    let frac = if max > 0.0 {
        (value / max).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let filled = (frac * BAR_WIDTH as f32).round() as usize;
    let mut s = String::with_capacity(BAR_WIDTH * 3);
    for i in 0..BAR_WIDTH {
        s.push(if i < filled { '█' } else { '░' });
    }
    s
}

/// Color a temperature string by severity (green → yellow → red).
pub fn temp_colored(t: Celsius) -> String {
    let s = t.to_string();
    match t.0 {
        x if x < 50.0 => s.green().to_string(),
        x if x < 70.0 => s.yellow().to_string(),
        x if x < 85.0 => s.bright_red().to_string(),
        _ => s.red().bold().to_string(),
    }
}

/// Color a bar by the same temperature thresholds as [`temp_colored`].
pub fn temp_bar_colored(t: Celsius) -> String {
    let b = bar(t.0, 100.0);
    match t.0 {
        x if x < 50.0 => b.green().to_string(),
        x if x < 70.0 => b.yellow().to_string(),
        x if x < 85.0 => b.bright_red().to_string(),
        _ => b.red().to_string(),
    }
}

/// A section heading, e.g. `── Temperatures ──`.
pub fn heading(title: &str) -> String {
    format!("{}", title.bold().cyan())
}

/// The product banner shown at the top of `status`.
pub fn banner(version: &str) -> String {
    format!(
        "{} {}",
        "PeterFan".bold().cyan(),
        format!("v{version}").dimmed()
    )
}

/// Right-pad a sensor's short kind label to a fixed column.
pub fn kind_label(kind: SensorKind) -> String {
    format!("{:<3}", kind.short())
}
