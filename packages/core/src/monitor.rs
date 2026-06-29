//! The [`SystemMonitor`] trait: the seam for OS-level system metrics.
//!
//! This is the counterpart to [`crate::provider::HardwareProvider`]. Where
//! `HardwareProvider` is about thermal hardware (temps, fans, control) that
//! needs per-OS native access, `SystemMonitor` is about general system metrics
//! (CPU, memory, disk, network, processes, battery).
//!
//! A monitor follows a **sample → wait → sample** model: usage percentages and
//! network rates are deltas, so callers [`refresh`](SystemMonitor::refresh),
//! wait a short interval, and `refresh` again before reading.

use crate::metrics::{
    BatteryInfo, CpuMetrics, DiskInfo, MemoryMetrics, NetInterface, ProcSort, ProcessInfo,
    SystemInfo,
};

/// Which metric families a monitor can actually report on this machine.
///
/// Cross-platform metrics (CPU/memory/disk/network/process/system) are always
/// real. `battery` depends on hardware; `temps`/`fans` are reported by the
/// separate [`HardwareProvider`](crate::provider::HardwareProvider) and tracked
/// here only so a front-end can present a single capability summary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MonitorCapabilities {
    pub cpu: bool,
    pub memory: bool,
    pub disks: bool,
    pub networks: bool,
    pub processes: bool,
    pub battery: bool,
}

/// A source of system metrics.
///
/// Implementations are not required to be `Send`/`Sync`: monitors are polled
/// from a single thread (the CLI command or the TUI loop) and may hold OS
/// handles with interior state that is cheaper kept thread-local.
pub trait SystemMonitor {
    /// Short backend name, e.g. `"sysinfo"`, `"mock"`.
    fn name(&self) -> &str;

    /// What this monitor can report here.
    fn capabilities(&self) -> MonitorCapabilities;

    /// Re-sample all metrics. Call twice with a short delay before reading
    /// usage percentages and network rates (they are interval deltas).
    fn refresh(&mut self);

    fn system_info(&self) -> SystemInfo;
    fn cpu(&self) -> CpuMetrics;
    fn memory(&self) -> MemoryMetrics;
    fn disks(&self) -> Vec<DiskInfo>;
    fn networks(&self) -> Vec<NetInterface>;

    /// Up to `limit` processes, ranked by `sort` (descending).
    fn processes(&self, limit: usize, sort: ProcSort) -> Vec<ProcessInfo>;

    /// Battery state, or `None` on a machine without one.
    fn battery(&self) -> Option<BatteryInfo>;
}
