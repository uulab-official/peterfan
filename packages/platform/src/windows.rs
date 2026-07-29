//! Windows hardware backend.
//!
//! System metrics are provided separately by [`crate::system::SysinfoMonitor`].
//! This provider intentionally reports no thermal or fan capabilities until a
//! hardware-specific EC/WMI backend is available. Returning empty readings is
//! safer than falling back to simulated sensors in a production build.

use peterfan_core::error::Result;
use peterfan_core::provider::{Capabilities, HardwareProvider};
use peterfan_core::types::{Fan, HardwareInfo, TempSensor};
use sysinfo::{MemoryRefreshKind, RefreshKind, System};

pub struct WindowsProvider {
    info: HardwareInfo,
}

impl WindowsProvider {
    pub fn new() -> Self {
        let mut system = System::new_with_specifics(
            RefreshKind::nothing()
                .with_cpu(sysinfo::CpuRefreshKind::everything())
                .with_memory(MemoryRefreshKind::everything()),
        );
        system.refresh_cpu_all();
        system.refresh_memory();

        let cpu = system
            .cpus()
            .first()
            .map(|cpu| cpu.brand().trim().to_string())
            .filter(|brand| !brand.is_empty())
            .unwrap_or_else(|| "CPU".to_string());
        let memory = (system.total_memory() > 0).then(|| {
            let gib = system.total_memory() as f64 / (1024.0 * 1024.0 * 1024.0);
            format!("{gib:.0} GB")
        });
        let os = System::long_os_version()
            .or_else(System::name)
            .unwrap_or_else(|| "Windows".to_string());

        Self {
            info: HardwareInfo {
                cpu,
                gpu: None,
                motherboard: None,
                memory,
                os,
            },
        }
    }
}

impl Default for WindowsProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl HardwareProvider for WindowsProvider {
    fn name(&self) -> &str {
        "windows"
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities::default()
    }

    fn hardware_info(&self) -> Result<HardwareInfo> {
        Ok(self.info.clone())
    }

    fn temperatures(&self) -> Result<Vec<TempSensor>> {
        Ok(Vec::new())
    }

    fn fans(&self) -> Result<Vec<Fan>> {
        Ok(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn production_windows_backend_never_exposes_simulated_thermals() {
        let provider = WindowsProvider::new();
        assert_eq!(provider.name(), "windows");
        assert_eq!(provider.capabilities(), Capabilities::default());
        assert!(provider.temperatures().unwrap().is_empty());
        assert!(provider.fans().unwrap().is_empty());
        assert_ne!(provider.name(), "mock");
    }
}
