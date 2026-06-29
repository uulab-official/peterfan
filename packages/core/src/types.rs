//! Domain types: what PeterFan measures and controls.
//!
//! These are intentionally plain data structures. They carry no behavior tied
//! to a specific OS so that they can travel freely between the core, the CLI,
//! the TUI, and (over the wire) the desktop GUI and the public HTTP API.

use serde::{Deserialize, Serialize};

/// A temperature in degrees Celsius.
///
/// A newtype rather than a bare `f32` so a temperature can never be silently
/// confused with a duty-cycle percentage or an RPM value at a call site.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct Celsius(pub f32);

impl std::fmt::Display for Celsius {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:.0}°C", self.0)
    }
}

/// What kind of component a temperature sensor is attached to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SensorKind {
    Cpu,
    Gpu,
    Memory,
    Storage,
    Mainboard,
    Other,
}

impl SensorKind {
    /// Short uppercase label used in compact UIs.
    pub fn short(&self) -> &'static str {
        match self {
            SensorKind::Cpu => "CPU",
            SensorKind::Gpu => "GPU",
            SensorKind::Memory => "RAM",
            SensorKind::Storage => "SSD",
            SensorKind::Mainboard => "MB",
            SensorKind::Other => "—",
        }
    }
}

/// A single temperature reading.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TempSensor {
    /// Stable machine id, e.g. `"cpu.package"`. Used by config & the API.
    pub id: String,
    /// Human-readable label, e.g. `"CPU Package"`.
    pub label: String,
    pub kind: SensorKind,
    pub value: Celsius,
}

/// A fan and its current state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fan {
    /// Stable machine id, e.g. `"fan.cpu"`.
    pub id: String,
    /// Human-readable label, e.g. `"CPU Fan"`.
    pub label: String,
    /// Current speed in revolutions per minute.
    pub rpm: u32,
    /// Lowest non-zero RPM the fan is rated for, if known.
    pub min_rpm: Option<u32>,
    /// Maximum RPM the fan is rated for, if known.
    pub max_rpm: Option<u32>,
    /// Current duty cycle as a percentage `0..=100`, if the backend exposes it.
    pub duty_percent: Option<u8>,
    /// Whether this fan can be driven by PeterFan on the current backend.
    pub controllable: bool,
}

/// Static information about the machine PeterFan is running on.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HardwareInfo {
    pub cpu: String,
    pub gpu: Option<String>,
    pub motherboard: Option<String>,
    pub memory: Option<String>,
    pub os: String,
}

/// A point-in-time reading of every sensor and fan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    pub temps: Vec<TempSensor>,
    pub fans: Vec<Fan>,
}
