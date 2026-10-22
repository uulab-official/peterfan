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
    Battery,
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
            SensorKind::Battery => "BATT",
            SensorKind::Other => "—",
        }
    }
}

/// A more precise named group for temperature diagnostics.
///
/// `SensorKind` remains the broad component classification used by existing
/// clients. This finer group is derived from the stable sensor id/label so it
/// can be added to JSON and UI payloads without changing the wire shape of
/// `TempSensor` or breaking older daemons.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SensorGroup {
    CpuAverage,
    CpuHottest,
    CpuCore,
    CpuDie,
    GpuDie,
    Gpu,
    Memory,
    Storage,
    Battery,
    Mainboard,
    Other,
}

impl SensorGroup {
    /// Derive the most useful named group from a sensor's stable identity.
    pub fn from_sensor(sensor: &TempSensor) -> Self {
        let id = sensor.id.to_ascii_lowercase();
        let label = sensor.label.to_ascii_lowercase();

        match sensor.kind {
            SensorKind::Cpu => {
                if id.ends_with(".hot")
                    || id.contains("hotspot")
                    || label.contains("hottest")
                    || label.contains("hotspot")
                {
                    SensorGroup::CpuHottest
                } else if id == "cpu.die"
                    || id == "cpu.smc.die"
                    || id.contains("smc.aggregate")
                    || id.contains("smc.summary")
                    || id.contains("iohid.tdie")
                    || id.contains("iohid.cpu")
                    || label.contains("average")
                    || label.contains("aggregate")
                    || label.contains("summary")
                {
                    SensorGroup::CpuAverage
                } else if id.contains("core") || label.contains("core") {
                    SensorGroup::CpuCore
                } else {
                    SensorGroup::CpuDie
                }
            }
            SensorKind::Gpu => {
                if id == "gpu.die" || id.contains("tdie") || label.contains("die") {
                    SensorGroup::GpuDie
                } else {
                    SensorGroup::Gpu
                }
            }
            SensorKind::Memory => SensorGroup::Memory,
            SensorKind::Storage => SensorGroup::Storage,
            SensorKind::Battery => SensorGroup::Battery,
            SensorKind::Mainboard => SensorGroup::Mainboard,
            SensorKind::Other if is_board_like_label(&label) => SensorGroup::Mainboard,
            SensorKind::Other => SensorGroup::Other,
        }
    }

    pub fn short(&self) -> &'static str {
        match self {
            SensorGroup::CpuAverage => "CPU average",
            SensorGroup::CpuHottest => "CPU hottest",
            SensorGroup::CpuCore => "CPU core",
            SensorGroup::CpuDie => "CPU die",
            SensorGroup::GpuDie => "GPU die",
            SensorGroup::Gpu => "GPU",
            SensorGroup::Memory => "Memory",
            SensorGroup::Storage => "Storage",
            SensorGroup::Battery => "Battery",
            SensorGroup::Mainboard => "Mainboard",
            SensorGroup::Other => "Other",
        }
    }
}

fn is_board_like_label(label: &str) -> bool {
    [
        "ambient",
        "airport",
        "board",
        "heatpipe",
        "heatsink",
        "palm rest",
        "power supply",
        "proximity",
        "wireless",
    ]
    .iter()
    .any(|part| label.contains(part))
}

/// Hardware or subsystem that produced a temperature reading.
///
/// `Unknown` is the serde default so a new app can still read snapshots from
/// an older daemon that predates sensor provenance metadata.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SensorSource {
    Smc,
    Iohid,
    Acpi,
    Battery,
    Simulated,
    #[default]
    Unknown,
}

impl SensorSource {
    pub fn short(&self) -> &'static str {
        match self {
            SensorSource::Smc => "SMC",
            SensorSource::Iohid => "IOHID",
            SensorSource::Acpi => "ACPI",
            SensorSource::Battery => "Battery",
            SensorSource::Simulated => "Simulated",
            SensorSource::Unknown => "Unknown",
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
    #[serde(default)]
    pub source: SensorSource,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn temperature_source_defaults_for_legacy_json() {
        let sensor: TempSensor = serde_json::from_value(serde_json::json!({
            "id": "cpu.die",
            "label": "CPU Core Average",
            "kind": "cpu",
            "value": 52.5
        }))
        .expect("legacy sensor JSON should remain readable");

        assert_eq!(sensor.source, SensorSource::Unknown);
    }

    #[test]
    fn temperature_source_serializes_as_stable_lowercase_value() {
        let sensor = TempSensor {
            id: "cpu.die".into(),
            label: "CPU Core Average".into(),
            kind: SensorKind::Cpu,
            source: SensorSource::Smc,
            value: Celsius(52.5),
        };
        let value = serde_json::to_value(sensor).expect("sensor should serialize");

        assert_eq!(value["source"], "smc");
    }

    #[test]
    fn sensor_groups_separate_cpu_and_gpu_die_roles() {
        let sensor = |id: &str, label: &str, kind| TempSensor {
            id: id.into(),
            label: label.into(),
            kind,
            source: SensorSource::Smc,
            value: Celsius(50.0),
        };

        assert_eq!(
            SensorGroup::from_sensor(&sensor("cpu.die", "CPU Core Average", SensorKind::Cpu)),
            SensorGroup::CpuAverage
        );
        assert_eq!(
            SensorGroup::from_sensor(&sensor("cpu.die.hot", "CPU Core Hottest", SensorKind::Cpu)),
            SensorGroup::CpuHottest
        );
        assert_eq!(
            SensorGroup::from_sensor(&sensor(
                "cpu.performance-core.1",
                "CPU Performance Core 1",
                SensorKind::Cpu
            )),
            SensorGroup::CpuCore
        );
        assert_eq!(
            SensorGroup::from_sensor(&sensor("cpu.smc.die", "CPU die", SensorKind::Cpu)),
            SensorGroup::CpuAverage
        );
        assert_eq!(
            SensorGroup::from_sensor(&sensor("gpu.die", "GPU die", SensorKind::Gpu)),
            SensorGroup::GpuDie
        );
    }

    #[test]
    fn sensor_groups_name_storage_battery_and_board_readings() {
        let sensor = |id: &str, label: &str, kind| TempSensor {
            id: id.into(),
            label: label.into(),
            kind,
            source: SensorSource::Iohid,
            value: Celsius(35.0),
        };

        assert_eq!(
            SensorGroup::from_sensor(&sensor("ssd", "SSD", SensorKind::Storage)),
            SensorGroup::Storage
        );
        assert_eq!(
            SensorGroup::from_sensor(&sensor("battery", "Battery", SensorKind::Battery)),
            SensorGroup::Battery
        );
        assert_eq!(
            SensorGroup::from_sensor(&sensor("smc.raw.Ta0", "Ambient sensor", SensorKind::Other)),
            SensorGroup::Mainboard
        );
    }
}
