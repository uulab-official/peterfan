//! # peterfan-platform
//!
//! Hardware backends that implement [`peterfan_core::HardwareProvider`].
//!
//! - [`mock`] — a fully simulated machine. Always available, used for the demo
//!   experience, for `--mock`, and as the substrate for tests.
//! - `macos` — real SMC/IOHID temperatures, fan RPM, and guarded fan control.
//! - `windows` — real system information and ACPI/WMI thermal zones where the
//!   firmware exposes them; fan RPM and fan control remain hardware-dependent.
//!
//! Use [`detect`] to get the best backend for the current OS, or [`mock`] to
//! force the simulated one.

pub mod config;
#[cfg(target_os = "macos")]
pub mod daemon_install;
#[cfg(unix)]
pub mod ipc;
#[cfg(target_os = "macos")]
pub mod login_item;
pub mod mock;
pub mod mock_monitor;
pub mod system;
pub mod updater;
#[cfg(target_os = "windows")]
pub mod windows_login_item;

/// Oldest installed root daemon this app version can safely keep using.
///
/// App-only UI releases should bump the app version without forcing users to
/// re-enter their macOS password. Raise this only when the daemon IPC contract
/// or fan-control behavior genuinely requires a newer `/usr/local/bin/peterfand`.
pub const MIN_REQUIRED_DAEMON_VERSION: &str = "1.27.29";

/// Oldest installed root daemon that can reinstall fan control from the
/// signed app bundle without another administrator-password prompt.
pub const MIN_SELF_REINSTALL_DAEMON_VERSION: &str = "1.26.37";

#[cfg(target_os = "macos")]
mod macos;
#[cfg(all(target_os = "macos", feature = "experimental-gpu"))]
mod macos_gpu;
#[cfg(target_os = "macos")]
mod macos_hid;
#[cfg(target_os = "macos")]
mod smc_write;
#[cfg(target_os = "windows")]
mod windows;

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

#[cfg(target_os = "macos")]
pub use macos::{CpuTemperatureCoreReading, CpuTemperatureKeyReading, CpuTemperatureProbe};

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

#[cfg(target_os = "macos")]
pub fn cpu_temperature_probe() -> Option<CpuTemperatureProbe> {
    macos::cpu_temperature_probe()
}
#[cfg(not(target_os = "macos"))]
pub fn cpu_temperature_probe() -> Option<()> {
    None
}

#[cfg(target_os = "macos")]
pub fn all_temperature_sensors() -> Vec<peterfan_core::types::TempSensor> {
    macos::all_temperature_sensors()
}
#[cfg(target_os = "windows")]
pub fn all_temperature_sensors() -> Vec<peterfan_core::types::TempSensor> {
    windows::WindowsProvider::new()
        .temperatures()
        .unwrap_or_default()
}
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub fn all_temperature_sensors() -> Vec<peterfan_core::types::TempSensor> {
    Vec::new()
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

pub fn parse_daemon_version_output(output: &str) -> Option<String> {
    output
        .split_whitespace()
        .find(|part| part.chars().next().is_some_and(|c| c.is_ascii_digit()))
        .map(|part| part.trim().to_string())
        .filter(|part| !part.is_empty())
}

pub fn daemon_update_required(installed_version: &str) -> bool {
    updater::is_newer(installed_version, MIN_REQUIRED_DAEMON_VERSION)
}

pub fn daemon_self_reinstall_supported(installed_version: &str) -> bool {
    !updater::is_newer(installed_version, MIN_SELF_REINSTALL_DAEMON_VERSION)
}

#[cfg(target_os = "macos")]
pub fn installed_daemon_version() -> Option<String> {
    let output = std::process::Command::new("/usr/local/bin/peterfand")
        .arg("--version")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    parse_daemon_version_output(&String::from_utf8_lossy(&output.stdout))
}

#[cfg(not(target_os = "macos"))]
pub fn installed_daemon_version() -> Option<String> {
    None
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
        Box::new(mock::MockProvider::new())
    }

    #[cfg(target_os = "windows")]
    {
        Box::new(windows::WindowsProvider::new())
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        Box::new(mock::MockProvider::new())
    }
}

/// Return the simulated backend, regardless of OS (`peterfan --mock`).
pub fn mock() -> Box<dyn HardwareProvider> {
    Box::new(mock::MockProvider::new())
}

/// Return the real cross-platform system-metrics monitor (`sysinfo`-backed).
pub fn system_monitor() -> Box<dyn SystemMonitor> {
    Box::new(system::SysinfoMonitor::new())
}

/// Return a light-weight monitor that skips process enumeration and disk/network
/// I/O on each refresh — suitable for commands that only need CPU% or memory.
/// About 150 ms faster per refresh on macOS than `system_monitor()`.
pub fn quick_monitor() -> Box<dyn SystemMonitor> {
    Box::new(system::SysinfoMonitor::new_quick())
}

/// Return the simulated system-metrics monitor (`peterfan --mock`).
pub fn mock_monitor() -> Box<dyn SystemMonitor> {
    Box::new(mock_monitor::MockMonitor::new())
}

#[cfg(test)]
mod tests {
    #[test]
    fn parse_daemon_version_output_finds_semver_token() {
        assert_eq!(
            super::parse_daemon_version_output("peterfand 1.26.13\n"),
            Some("1.26.13".to_string())
        );
        assert_eq!(
            super::parse_daemon_version_output("warning\npeterfand 1.26.8"),
            Some("1.26.8".to_string())
        );
        assert_eq!(super::parse_daemon_version_output("peterfand\n"), None);
    }

    #[test]
    fn daemon_update_uses_min_required_version_not_app_version() {
        assert!(super::daemon_update_required("1.27.10"));
        assert!(!super::daemon_update_required(
            super::MIN_REQUIRED_DAEMON_VERSION
        ));
        assert!(super::daemon_update_required("1.27.13"));
        assert!(super::daemon_update_required("1.27.15"));
        assert!(super::daemon_update_required("1.27.21"));
        assert!(super::daemon_update_required("1.27.22"));
    }

    #[test]
    fn daemon_self_reinstall_requires_new_enough_daemon() {
        assert!(!super::daemon_self_reinstall_supported("1.26.36"));
        assert!(super::daemon_self_reinstall_supported(
            super::MIN_SELF_REINSTALL_DAEMON_VERSION
        ));
        assert!(super::daemon_self_reinstall_supported("1.26.38"));
    }
}
