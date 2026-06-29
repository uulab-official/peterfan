//! A fully simulated machine.
//!
//! The mock backend models two thermal zones (CPU, GPU) plus RAM/SSD sensors
//! and two controllable fans. Temperatures drift over time so the TUI's live
//! graph has something to draw, and `set_fan_duty` actually changes the
//! simulated RPM — making it a faithful stand-in for end-to-end testing and
//! for the demo experience on machines without a real backend.

use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use peterfan_core::error::{CoreError, Result};
use peterfan_core::provider::Capabilities;
use peterfan_core::types::{Celsius, Fan, HardwareInfo, SensorKind, TempSensor};
use peterfan_core::HardwareProvider;

struct FanState {
    id: &'static str,
    label: &'static str,
    min_rpm: u32,
    max_rpm: u32,
    duty: u8,
}

struct State {
    fans: Vec<FanState>,
}

pub struct MockProvider {
    state: Mutex<State>,
}

impl Default for MockProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl MockProvider {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(State {
                fans: vec![
                    FanState {
                        id: "fan.cpu",
                        label: "CPU Fan",
                        min_rpm: 600,
                        max_rpm: 2400,
                        duty: 45,
                    },
                    FanState {
                        id: "fan.gpu",
                        label: "GPU Fan",
                        min_rpm: 500,
                        max_rpm: 2200,
                        duty: 38,
                    },
                ],
            }),
        }
    }

    /// A slow [0,1) triangle wave so simulated values drift visibly but
    /// smoothly over ~30s, without needing a RNG dependency.
    fn wobble(phase: f32) -> f32 {
        let secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs_f32())
            .unwrap_or(0.0);
        let x = ((secs / 30.0) + phase).fract();
        // triangle wave 0→1→0
        1.0 - (2.0 * x - 1.0).abs()
    }
}

impl HardwareProvider for MockProvider {
    fn name(&self) -> &str {
        "mock"
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            read_temps: true,
            read_fans: true,
            control_fans: true,
        }
    }

    fn hardware_info(&self) -> Result<HardwareInfo> {
        Ok(HardwareInfo {
            cpu: "Mock CPU (8C/16T @ 4.5GHz)".into(),
            gpu: Some("Mock GPU 16GB".into()),
            motherboard: Some("Mock Mainboard X1".into()),
            memory: Some("32 GB".into()),
            os: "Simulated".into(),
        })
    }

    fn temperatures(&self) -> Result<Vec<TempSensor>> {
        let cpu = 42.0 + Self::wobble(0.0) * 28.0; // 42..70
        let gpu = 38.0 + Self::wobble(0.4) * 26.0; // 38..64
        let ram = 36.0 + Self::wobble(0.7) * 8.0; // 36..44
        let ssd = 34.0 + Self::wobble(0.2) * 6.0; // 34..40
        Ok(vec![
            TempSensor {
                id: "cpu.package".into(),
                label: "CPU Package".into(),
                kind: SensorKind::Cpu,
                value: Celsius(cpu),
            },
            TempSensor {
                id: "gpu.core".into(),
                label: "GPU Core".into(),
                kind: SensorKind::Gpu,
                value: Celsius(gpu),
            },
            TempSensor {
                id: "mem.dimm".into(),
                label: "Memory".into(),
                kind: SensorKind::Memory,
                value: Celsius(ram),
            },
            TempSensor {
                id: "ssd.0".into(),
                label: "NVMe SSD".into(),
                kind: SensorKind::Storage,
                value: Celsius(ssd),
            },
        ])
    }

    fn fans(&self) -> Result<Vec<Fan>> {
        let state = self.state.lock().expect("mock state poisoned");
        Ok(state
            .fans
            .iter()
            .map(|f| {
                let span = (f.max_rpm - f.min_rpm) as f32;
                let rpm = f.min_rpm + (span * f.duty as f32 / 100.0) as u32;
                Fan {
                    id: f.id.into(),
                    label: f.label.into(),
                    rpm,
                    min_rpm: Some(f.min_rpm),
                    max_rpm: Some(f.max_rpm),
                    duty_percent: Some(f.duty),
                    controllable: true,
                }
            })
            .collect())
    }

    fn set_fan_duty(&self, fan_id: &str, duty_percent: u8) -> Result<()> {
        let mut state = self.state.lock().expect("mock state poisoned");
        let fan = state
            .fans
            .iter_mut()
            .find(|f| f.id == fan_id)
            .ok_or_else(|| CoreError::NotFound(format!("fan id '{fan_id}'")))?;
        fan.duty = duty_percent.min(100);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_changes_reported_rpm() {
        let p = MockProvider::new();
        p.set_fan_duty("fan.cpu", 100).unwrap();
        let fan = p
            .fans()
            .unwrap()
            .into_iter()
            .find(|f| f.id == "fan.cpu")
            .unwrap();
        assert_eq!(fan.duty_percent, Some(100));
        assert_eq!(fan.rpm, fan.max_rpm.unwrap());
    }

    #[test]
    fn unknown_fan_is_not_found() {
        let p = MockProvider::new();
        assert!(p.set_fan_duty("fan.nope", 50).is_err());
    }
}
