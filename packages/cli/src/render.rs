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

/// Format a byte count as a human-readable size (base-1024).
pub fn bytes(n: u64) -> String {
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

/// Format a transfer rate (bytes/second).
pub fn rate(bytes_per_sec: f64) -> String {
    format!("{}/s", bytes(bytes_per_sec.max(0.0) as u64))
}

/// Format a duration in seconds as e.g. `3h 20m` or `12m 4s`.
pub fn duration(secs: u64) -> String {
    let d = secs / 86_400;
    let h = (secs % 86_400) / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    if d > 0 {
        format!("{d}d {h}h {m}m")
    } else if h > 0 {
        format!("{h}h {m}m")
    } else if m > 0 {
        format!("{m}m {s}s")
    } else {
        format!("{s}s")
    }
}

/// Color a percentage string green/yellow/red by load thresholds.
pub fn pct_colored(p: f32) -> String {
    let s = format!("{p:>5.1}%");
    match p {
        x if x < 50.0 => s.green().to_string(),
        x if x < 80.0 => s.yellow().to_string(),
        _ => s.red().to_string(),
    }
}

/// A bar colored by load thresholds (for usage/utilization percentages).
pub fn load_bar(p: f32) -> String {
    let b = bar(p, 100.0);
    match p {
        x if x < 50.0 => b.green().to_string(),
        x if x < 80.0 => b.yellow().to_string(),
        _ => b.red().to_string(),
    }
}

/// A compact per-core load sparkline using block characters.
pub fn core_spark(per_core: &[f32]) -> String {
    const BLOCKS: [char; 9] = [' ', '▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    per_core
        .iter()
        .map(|&p| {
            let idx = ((p / 100.0).clamp(0.0, 1.0) * 8.0).round() as usize;
            BLOCKS[idx.min(8)]
        })
        .collect()
}
