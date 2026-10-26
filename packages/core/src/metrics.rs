//! System metrics: the data a hardware/system monitor reports.
//!
//! These types describe CPU, memory, disk, network, process, battery, and
//! general system state. Like the rest of [`crate`], they are plain,
//! serializable data with no OS-specific behavior — a backend produces them,
//! every front-end (CLI, TUI, GUI, API) consumes them.

use serde::{Deserialize, Serialize};

/// Static, slow-changing information about the running system.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemInfo {
    pub host_name: Option<String>,
    /// OS family, e.g. `"macOS"`, `"Windows"`.
    pub os_name: Option<String>,
    /// OS version string, e.g. `"26.1"`.
    pub os_version: Option<String>,
    pub kernel_version: Option<String>,
    /// CPU architecture, e.g. `"aarch64"`, `"x86_64"`.
    pub arch: String,
    pub uptime_secs: u64,
    pub logical_cores: usize,
    pub physical_cores: Option<usize>,
}

/// Unix-style load average over 1/5/15 minutes (not meaningful on Windows).
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct LoadAvg {
    pub one: f64,
    pub five: f64,
    pub fifteen: f64,
}

/// CPU usage and clock state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CpuMetrics {
    pub brand: String,
    /// Aggregate usage across all cores, `0..=100`.
    pub usage_percent: f32,
    /// Per-logical-core usage, each `0..=100`.
    pub per_core: Vec<f32>,
    /// Current (or representative) frequency in MHz, if known.
    pub frequency_mhz: u64,
    /// Load average, where the platform provides it.
    pub load_avg: Option<LoadAvg>,
}

/// Physical and swap memory, in bytes.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct MemoryMetrics {
    pub total: u64,
    pub used: u64,
    pub available: u64,
    pub used_percent: f32,
    pub swap_total: u64,
    pub swap_used: u64,
    /// macOS-style breakdown (bytes), where the platform can report it.
    /// `None` on platforms without a virtual-memory breakdown.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub breakdown: Option<MemoryBreakdown>,
}

/// macOS virtual-memory breakdown, mirroring Activity Monitor's categories.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct MemoryBreakdown {
    /// Memory that cannot be paged out (kernel, drivers).
    pub wired: u64,
    /// In-use, recently-accessed pages (roughly "App Memory").
    pub active: u64,
    /// In-use but not recently accessed (reclaimable under pressure).
    pub inactive: u64,
    /// Memory held by the compressor (macOS memory compression).
    pub compressed: u64,
}

/// A mounted disk / volume, sizes in bytes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiskInfo {
    pub name: String,
    pub mount: String,
    pub fs: String,
    pub total: u64,
    pub available: u64,
    pub used: u64,
    pub used_percent: f32,
    pub removable: bool,
    /// `"SSD"`, `"HDD"`, or `"—"` when unknown.
    pub kind: String,
    /// Read throughput in bytes/second over the last refresh interval.
    pub read_bytes_per_sec: f64,
    /// Write throughput in bytes/second over the last refresh interval.
    pub write_bytes_per_sec: f64,
}

/// A network interface with cumulative counters and instantaneous rates.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetInterface {
    pub name: String,
    /// First non-loopback IPv4 address on this interface, if any.
    pub ip: Option<String>,
    pub rx_total: u64,
    pub tx_total: u64,
    /// Receive rate in bytes/second, measured over the last refresh interval.
    pub rx_rate: f64,
    /// Transmit rate in bytes/second, measured over the last refresh interval.
    pub tx_rate: f64,
}

/// A single process, as shown in "top consumers" lists.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessInfo {
    pub pid: u32,
    pub name: String,
    /// CPU usage `0..=100*ncores` as reported by the OS.
    pub cpu_percent: f32,
    /// Resident memory in bytes.
    pub memory: u64,
    /// Unix process start time used to detect PID reuse before control actions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_time_unix_secs: Option<u64>,
    /// Whether the backend proved that this process belongs to PeterFan's user.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owned_by_current_user: Option<bool>,
}

/// Normalize a process query consistently across native filtering and UI IPC.
pub fn normalize_process_query(query: &str) -> String {
    query
        .trim()
        .chars()
        .take(64)
        .collect::<String>()
        .to_lowercase()
}

/// How to rank processes in a "top" listing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcSort {
    Cpu,
    Memory,
}

/// Whether health is OS reported or derived from compatible physical capacities.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BatteryHealthSource {
    Reported,
    Estimated,
}

/// Battery state. Absent on desktops without a battery.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatteryInfo {
    /// Charge `0..=100`.
    pub charge_percent: f32,
    /// `"charging"`, `"discharging"`, `"full"`, `"empty"`, `"unknown"`.
    pub state: String,
    /// State of health `0..=100`, if reported.
    pub health_percent: Option<f32>,
    #[serde(default)]
    pub health_source: Option<BatteryHealthSource>,
    /// Original rated physical capacity, in mAh. Unknown provenance stays absent.
    #[serde(default)]
    pub design_capacity_mah: Option<u32>,
    /// Physical capacity when fully charged, in the same mAh unit as design.
    #[serde(default)]
    pub full_charge_capacity_mah: Option<u32>,
    pub cycle_count: Option<u32>,
    pub time_to_full_secs: Option<u64>,
    pub time_to_empty_secs: Option<u64>,
    pub vendor: Option<String>,
    pub model: Option<String>,
    /// Instantaneous energy flow in watts (positive = charging), if reported.
    pub energy_rate_w: Option<f32>,
}

#[cfg(test)]
mod battery_tests {
    use super::*;

    #[test]
    fn legacy_battery_json_defaults_new_details_to_unavailable() {
        let battery: BatteryInfo = serde_json::from_value(serde_json::json!({
            "charge_percent": 76.0, "state": "discharging",
            "health_percent": 94.0, "cycle_count": 0
        }))
        .unwrap();
        let serialized = serde_json::to_value(battery).unwrap();
        for field in [
            "health_source",
            "design_capacity_mah",
            "full_charge_capacity_mah",
        ] {
            assert_eq!(
                serialized.get(field),
                Some(&serde_json::Value::Null),
                "{field}"
            );
        }
        assert_eq!(serialized["cycle_count"], 0);
    }

    #[test]
    fn battery_health_source_roundtrips_reported_and_estimated() {
        for source in ["reported", "estimated"] {
            let battery: BatteryInfo = serde_json::from_value(serde_json::json!({
                "charge_percent": 76.0, "state": "discharging",
                "health_percent": 96.0, "health_source": source,
                "design_capacity_mah": 6249, "full_charge_capacity_mah": 5655
            }))
            .unwrap();
            let serialized = serde_json::to_value(battery).unwrap();
            assert_eq!(serialized["health_source"], source);
            assert_eq!(serialized["design_capacity_mah"], 6249);
            assert_eq!(serialized["full_charge_capacity_mah"], 5655);
            assert_eq!(serialized["charge_percent"], 76.0);
        }
    }
}
