//! Windows hardware backend.
//!
//! System metrics are provided separately by [`crate::system::SysinfoMonitor`].
//! This provider reports firmware thermal zones when Windows exposes them
//! through ACPI/WMI. Those readings are intentionally classified as system
//! temperatures rather than CPU-core temperatures. Fan RPM and control remain
//! unavailable until a hardware-specific EC backend can be implemented safely.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use peterfan_core::error::{CoreError, Result};
use peterfan_core::provider::{Capabilities, HardwareProvider};
use peterfan_core::types::{Celsius, Fan, HardwareInfo, SensorKind, SensorSource, TempSensor};
use sysinfo::{Components, MemoryRefreshKind, Motherboard, Product, RefreshKind, System};

pub struct WindowsProvider {
    info: HardwareInfo,
    components: Mutex<Components>,
    temperatures_available: AtomicBool,
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
        let product = Product::name().filter(|name| useful_hardware_name(name));
        let motherboard = Motherboard::new().and_then(|board| {
            let vendor = board
                .vendor_name()
                .filter(|name| useful_hardware_name(name));
            let name = board.name().filter(|name| useful_hardware_name(name));
            match (vendor, name) {
                (Some(vendor), Some(name)) if !name.eq_ignore_ascii_case(&vendor) => {
                    Some(format!("{vendor} {name}"))
                }
                (Some(vendor), _) => Some(vendor),
                (_, Some(name)) => Some(name),
                _ => product.clone(),
            }
        });

        Self {
            info: HardwareInfo {
                cpu,
                gpu: None,
                motherboard: motherboard.or(product),
                memory,
                os,
            },
            // Discovery runs on the first background temperature read so app
            // startup is not held up by a slow or unavailable WMI provider.
            components: Mutex::new(Components::new()),
            temperatures_available: AtomicBool::new(false),
        }
    }
}

fn useful_hardware_name(name: &str) -> bool {
    let name = name.trim();
    !name.is_empty()
        && !matches!(
            name.to_ascii_lowercase().as_str(),
            "default string" | "system product name" | "to be filled by o.e.m."
        )
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
        Capabilities {
            read_temps: self.temperatures_available.load(Ordering::Acquire),
            read_fans: false,
            control_fans: false,
        }
    }

    fn hardware_info(&self) -> Result<HardwareInfo> {
        Ok(self.info.clone())
    }

    fn temperatures(&self) -> Result<Vec<TempSensor>> {
        let mut components = self
            .components
            .lock()
            .map_err(|_| CoreError::Hardware("Windows thermal state lock was poisoned".into()))?;
        components.refresh(true);
        let count = components.len();
        let temps = components
            .iter()
            .enumerate()
            .filter_map(|(index, component)| {
                let value = component.temperature()?;
                // ACPI firmware is inconsistent. Reject sentinels and
                // physically implausible values instead of presenting them.
                (value.is_finite() && (1.0..=125.0).contains(&value)).then(|| TempSensor {
                    id: format!("system.acpi.thermal_zone.{index}"),
                    label: if count == 1 {
                        "System Thermal Zone".to_string()
                    } else {
                        format!("System Thermal Zone {}", index + 1)
                    },
                    kind: SensorKind::Mainboard,
                    source: SensorSource::Acpi,
                    value: Celsius(value),
                })
            })
            .collect::<Vec<_>>();
        self.temperatures_available
            .store(!temps.is_empty(), Ordering::Release);
        Ok(temps)
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
        let temps = provider.temperatures().unwrap();
        assert_eq!(provider.capabilities().read_temps, !temps.is_empty());
        assert!(temps.iter().all(|temp| {
            temp.source == SensorSource::Acpi
                && temp.kind == SensorKind::Mainboard
                && (1.0..=125.0).contains(&temp.value.0)
        }));
        assert!(!provider.capabilities().read_fans);
        assert!(!provider.capabilities().control_fans);
        assert!(provider.fans().unwrap().is_empty());
        assert_ne!(provider.name(), "mock");
    }

    #[test]
    fn placeholder_smbios_names_are_not_presented_as_hardware() {
        assert!(!useful_hardware_name("Default string"));
        assert!(!useful_hardware_name("To Be Filled By O.E.M."));
        assert!(useful_hardware_name("ThinkPad T14 Gen 5"));
    }
}
