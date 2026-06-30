//! # peterfan-platform
//!
//! Hardware backends that implement [`peterfan_core::HardwareProvider`].
//!
//! - [`mock`] — a fully simulated machine. Always available, used for the demo
//!   experience, for `--mock`, and as the substrate for tests.
//! - `macos` — a **real, read-only** backend that reports genuine hardware info
//!   via `sysctl`. Temperature/fan reading (SMC) and fan control are not yet
//!   implemented; see `docs/ROADMAP.md`.
//! - `windows` — placeholder; not yet implemented.
//!
//! Use [`detect`] to get the best backend for the current OS, or [`mock`] to
//! force the simulated one.

pub mod config;
#[cfg(unix)]
pub mod ipc;
pub mod mock;
pub mod mock_monitor;
pub mod system;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(all(target_os = "macos", feature = "experimental-gpu"))]
mod macos_gpu;
#[cfg(target_os = "macos")]
mod macos_hid;
#[cfg(target_os = "macos")]
mod smc_write;

/// Apple Silicon GPU active-residency (%), behind the off-by-default
/// `experimental-gpu` feature. Not exposed in the default build because the
/// IOReport `GPUPH` residency we can read does not match Activity Monitor's
/// GPU% definition (it counts low-power display-compositing states as "busy",
/// reading ~50% even at idle), and we won't ship an inaccurate number. Kept as
/// working reference plumbing — see `macos_gpu.rs`.
#[cfg(all(target_os = "macos", feature = "experimental-gpu"))]
pub fn gpu_usage_percent() -> Option<f32> {
    macos_gpu::gpu_usage_percent()
}

#[cfg(target_os = "macos")]
pub use smc_write::FanProbe;

/// Read-only probe of the SMC fan-control keys, for `peterfan doctor`.
/// `None` on platforms without this backend.
#[cfg(target_os = "macos")]
pub fn fan_control_probe() -> Option<FanProbe> {
    Some(smc_write::probe())
}
#[cfg(not(target_os = "macos"))]
pub fn fan_control_probe() -> Option<()> {
    None
}

/// Whether a `peterfand` daemon is currently reachable over the local IPC socket.
#[cfg(unix)]
pub fn daemon_reachable() -> bool {
    ipc::connect().is_some()
}
#[cfg(not(unix))]
pub fn daemon_reachable() -> bool {
    false
}

use peterfan_core::{HardwareProvider, SystemMonitor};

/// Return the best available backend for the current operating system.
///
/// Falls back to the [`mock::MockProvider`] when no real backend exists or the
/// real one fails to initialize, so callers always get a working provider.
pub fn detect() -> Box<dyn HardwareProvider> {
    #[cfg(target_os = "macos")]
    {
        if let Ok(p) = macos::MacosProvider::new() {
            return Box::new(p);
        }
    }

    Box::new(mock::MockProvider::new())
}

/// Return the simulated backend, regardless of OS (`peterfan --mock`).
pub fn mock() -> Box<dyn HardwareProvider> {
    Box::new(mock::MockProvider::new())
}

/// Return the real cross-platform system-metrics monitor (`sysinfo`-backed).
pub fn system_monitor() -> Box<dyn SystemMonitor> {
    Box::new(system::SysinfoMonitor::new())
}

/// Return the simulated system-metrics monitor (`peterfan --mock`).
pub fn mock_monitor() -> Box<dyn SystemMonitor> {
    Box::new(mock_monitor::MockMonitor::new())
}
