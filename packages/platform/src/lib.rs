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

pub mod mock;
pub mod mock_monitor;
pub mod system;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
mod smc_write;

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
