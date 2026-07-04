//! macOS backend — **real** hardware info, temperatures, and fan speeds.
//!
//! With no special privileges:
//! - [`HardwareProvider::hardware_info`] via `sysctl` (CPU brand, RAM, OS).
//! - [`HardwareProvider::temperatures`] and [`HardwareProvider::fans`] via the
//!   SMC (`AppleSMC` over IOKit), using the `macsmc` crate.
//!
//! Honesty notes:
//! - We only report temperature sensors that return a plausible (non-zero)
//!   reading. On Apple Silicon the SMC does **not** expose the classic CPU/GPU
//!   die-temperature keys (they read 0), so we read Apple Silicon CPU die
//!   temperatures through IOHID and show the remaining SMC board/ambient
//!   sensors that return plausible values.
//! - Fan **control** (SMC writes) is not implemented yet, so fans report
//!   `controllable: false`.

use std::ffi::CString;
use std::mem;
use std::ptr;
use std::sync::Mutex;

use macsmc::Smc;

use crate::smc_write::Conn;

use peterfan_core::error::{CoreError, Result};
use peterfan_core::provider::Capabilities;
use peterfan_core::types::{Celsius, Fan, HardwareInfo, SensorKind, TempSensor};
use peterfan_core::HardwareProvider;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CpuCoreClass {
    Efficiency,
    Performance,
}

struct CpuCoreTempKey {
    key: &'static str,
    class: CpuCoreClass,
}

struct CpuCoreTemp {
    class: CpuCoreClass,
    temp: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CpuTemperatureKeyReading {
    pub key: String,
    pub value_c: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CpuTemperatureCoreReading {
    pub key: String,
    pub class: &'static str,
    pub value_c: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CpuTemperatureProbe {
    pub selected_average_c: Option<f32>,
    pub selected_hottest_c: Option<f32>,
    pub summary_average_c: Option<f32>,
    pub aggregate_average_c: Option<f32>,
    pub hotspot_average_c: Option<f32>,
    pub hotspot_hottest_c: Option<f32>,
    pub performance_core_average_c: Option<f32>,
    pub all_core_average_c: Option<f32>,
    pub core_hottest_c: Option<f32>,
    pub summary_keys: Vec<CpuTemperatureKeyReading>,
    pub aggregate_keys: Vec<CpuTemperatureKeyReading>,
    pub hotspot_keys: Vec<CpuTemperatureKeyReading>,
    pub core_keys: Vec<CpuTemperatureCoreReading>,
}

const M3_CPU_CORE_TEMP_KEYS: &[CpuCoreTempKey] = &[
    CpuCoreTempKey {
        key: "Te05",
        class: CpuCoreClass::Efficiency,
    },
    CpuCoreTempKey {
        key: "Te0L",
        class: CpuCoreClass::Efficiency,
    },
    CpuCoreTempKey {
        key: "Te0P",
        class: CpuCoreClass::Efficiency,
    },
    CpuCoreTempKey {
        key: "Te0S",
        class: CpuCoreClass::Efficiency,
    },
    CpuCoreTempKey {
        key: "Tf04",
        class: CpuCoreClass::Performance,
    },
    CpuCoreTempKey {
        key: "Tf09",
        class: CpuCoreClass::Performance,
    },
    CpuCoreTempKey {
        key: "Tf0A",
        class: CpuCoreClass::Performance,
    },
    CpuCoreTempKey {
        key: "Tf0B",
        class: CpuCoreClass::Performance,
    },
    CpuCoreTempKey {
        key: "Tf0D",
        class: CpuCoreClass::Performance,
    },
    CpuCoreTempKey {
        key: "Tf0E",
        class: CpuCoreClass::Performance,
    },
    CpuCoreTempKey {
        key: "Tf44",
        class: CpuCoreClass::Performance,
    },
    CpuCoreTempKey {
        key: "Tf49",
        class: CpuCoreClass::Performance,
    },
    CpuCoreTempKey {
        key: "Tf4A",
        class: CpuCoreClass::Performance,
    },
    CpuCoreTempKey {
        key: "Tf4B",
        class: CpuCoreClass::Performance,
    },
    CpuCoreTempKey {
        key: "Tf4D",
        class: CpuCoreClass::Performance,
    },
    CpuCoreTempKey {
        key: "Tf4E",
        class: CpuCoreClass::Performance,
    },
];

// CPU summary/die key observed in local M3 Max samples. It moves with short
// load changes, so it is useful as a live floor when the aggregate lags.
const M3_CPU_SUMMARY_TEMP_KEYS: &[&str] = &["TCDX"];

// Apple Silicon aggregate CPU keys observed on M3 Pro/Max. Macs Fan Control's
// "CPU Core Average" on the local Mac15,10 tracks these keys closely, while
// summary and hotspot keys cover short rises that the aggregate can smooth out.
const M3_CPU_CORE_AVERAGE_TEMP_KEYS: &[&str] = &["TV0s", "TV1s", "TVsa", "TVss"];

// CPU die/hotspot keys observed on the local Mac15,10 M3 Max. These track
// sustained load into the 80s, but can sit above live core readings at idle, so
// they are reported as "CPU Hottest" only; the headline stays a core average.
const M3_CPU_HOTSPOT_TEMP_KEYS: &[&str] = &["TVD0", "TCMb"];
const CPU_HOTSPOT_ACTIVE_FLOOR_C: f32 = 70.0;

fn deduped_name_average_max<'a, I>(temps: I) -> Option<f32>
where
    I: IntoIterator<Item = (&'a str, f32)>,
{
    let mut by_name = std::collections::BTreeMap::<&str, f32>::new();
    for (name, temp) in temps {
        by_name
            .entry(name)
            .and_modify(|existing| *existing = existing.max(temp))
            .or_insert(temp);
    }
    (!by_name.is_empty()).then(|| by_name.values().sum::<f32>() / by_name.len() as f32)
}

pub struct MacosProvider {
    /// Whether the SMC could be opened on this machine (probed once at startup).
    has_smc: bool,
    /// A persistent SMC write connection, opened on first control use and kept
    /// open so forced fan state holds (it reverts when the connection closes).
    force_conn: Mutex<Option<Conn>>,
}

impl MacosProvider {
    pub fn new() -> Result<Self> {
        let has_smc = Smc::connect().is_ok();
        Ok(Self {
            has_smc,
            force_conn: Mutex::new(None),
        })
    }
}

fn average_and_hot(values: &[f32]) -> Option<(f32, f32)> {
    (!values.is_empty()).then(|| {
        let avg = values.iter().sum::<f32>() / values.len() as f32;
        let hot = values.iter().copied().fold(0.0, f32::max);
        (avg, hot)
    })
}

fn apple_silicon_cpu_core_temperatures() -> Vec<CpuCoreTemp> {
    let keys: Vec<&str> = M3_CPU_CORE_TEMP_KEYS
        .iter()
        .map(|sensor| sensor.key)
        .collect();
    let raw = crate::smc_write::read_temperature_keys(&keys);
    raw.into_iter()
        .filter_map(|(key, temp)| {
            let sensor = M3_CPU_CORE_TEMP_KEYS
                .iter()
                .find(|known| known.key == key)?;
            Some(CpuCoreTemp {
                class: sensor.class,
                temp,
            })
        })
        .collect()
}

fn apple_silicon_cpu_average_temperatures() -> Vec<f32> {
    crate::smc_write::read_temperature_keys(M3_CPU_CORE_AVERAGE_TEMP_KEYS)
        .into_iter()
        .map(|(_, temp)| temp)
        .collect()
}

fn apple_silicon_cpu_summary_temperatures() -> Vec<f32> {
    crate::smc_write::read_temperature_keys(M3_CPU_SUMMARY_TEMP_KEYS)
        .into_iter()
        .map(|(_, temp)| temp)
        .collect()
}

fn apple_silicon_cpu_hotspot_temperatures() -> Vec<f32> {
    crate::smc_write::read_temperature_keys(M3_CPU_HOTSPOT_TEMP_KEYS)
        .into_iter()
        .map(|(_, temp)| temp)
        .collect()
}

fn cpu_core_class_label(class: CpuCoreClass) -> &'static str {
    match class {
        CpuCoreClass::Efficiency => "efficiency",
        CpuCoreClass::Performance => "performance",
    }
}

fn temp_sensor_id_fragment(raw: &str) -> String {
    let mut out: String = raw
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '.'
            }
        })
        .collect();
    while out.contains("..") {
        out = out.replace("..", ".");
    }
    out.trim_matches('.').to_string()
}

fn hid_sensor_kind(name: &str) -> SensorKind {
    let lower = name.to_lowercase();
    if lower.contains("battery") || lower.contains("gas gauge") {
        SensorKind::Battery
    } else if lower.contains("nand") || lower.contains("ssd") {
        SensorKind::Storage
    } else if lower.contains("gpu") {
        SensorKind::Gpu
    } else if lower.contains("memory") || lower.contains("dram") {
        SensorKind::Memory
    } else if lower.contains("tdie") || lower.contains("cpu") {
        SensorKind::Cpu
    } else {
        SensorKind::Other
    }
}

fn cpu_temperature_probe_from_readings(
    summary_raw: Vec<(String, f32)>,
    aggregate_raw: Vec<(String, f32)>,
    hotspot_raw: Vec<(String, f32)>,
    core_raw: Vec<(String, f32)>,
) -> Option<CpuTemperatureProbe> {
    let summary_values: Vec<f32> = summary_raw.iter().map(|(_, temp)| *temp).collect();
    let summary_keys: Vec<_> = summary_raw
        .into_iter()
        .map(|(key, value_c)| CpuTemperatureKeyReading { key, value_c })
        .collect();
    let aggregate_values: Vec<f32> = aggregate_raw.iter().map(|(_, temp)| *temp).collect();
    let aggregate_keys: Vec<_> = aggregate_raw
        .into_iter()
        .map(|(key, value_c)| CpuTemperatureKeyReading { key, value_c })
        .collect();
    let hotspot_values: Vec<f32> = hotspot_raw.iter().map(|(_, temp)| *temp).collect();
    let hotspot_keys: Vec<_> = hotspot_raw
        .into_iter()
        .map(|(key, value_c)| CpuTemperatureKeyReading { key, value_c })
        .collect();

    let mut cores = Vec::new();
    let mut core_keys = Vec::new();
    for (key, value_c) in core_raw {
        let Some(sensor) = M3_CPU_CORE_TEMP_KEYS.iter().find(|known| known.key == key) else {
            continue;
        };
        cores.push(CpuCoreTemp {
            class: sensor.class,
            temp: value_c,
        });
        core_keys.push(CpuTemperatureCoreReading {
            key,
            class: cpu_core_class_label(sensor.class),
            value_c,
        });
    }

    let (selected_average_c, selected_hottest_c) =
        match apple_silicon_cpu_average_and_hot_from_values(
            &cores,
            &summary_values,
            &aggregate_values,
            &hotspot_values,
        ) {
            Some((avg, hot)) => (Some(avg), Some(hot)),
            None => (None, None),
        };
    let summary_average_c = average_and_hot(&summary_values).map(|(avg, _)| avg);
    let aggregate_average_c = average_and_hot(&aggregate_values).map(|(avg, _)| avg);
    let (hotspot_average_c, hotspot_hottest_c) = match average_and_hot(&hotspot_values) {
        Some((avg, hot)) => (Some(avg), Some(hot)),
        None => (None, None),
    };
    let all_core_values: Vec<f32> = cores.iter().map(|sensor| sensor.temp).collect();
    let performance_values: Vec<f32> = cores
        .iter()
        .filter(|sensor| sensor.class == CpuCoreClass::Performance)
        .map(|sensor| sensor.temp)
        .collect();
    let performance_core_average_c = average_and_hot(&performance_values).map(|(avg, _)| avg);
    let all_core_average_c = average_and_hot(&all_core_values).map(|(avg, _)| avg);
    let core_hottest_c = average_and_hot(&all_core_values).map(|(_, hot)| hot);

    if summary_keys.is_empty()
        && aggregate_keys.is_empty()
        && hotspot_keys.is_empty()
        && core_keys.is_empty()
    {
        return None;
    }

    Some(CpuTemperatureProbe {
        selected_average_c,
        selected_hottest_c,
        summary_average_c,
        aggregate_average_c,
        hotspot_average_c,
        hotspot_hottest_c,
        performance_core_average_c,
        all_core_average_c,
        core_hottest_c,
        summary_keys,
        aggregate_keys,
        hotspot_keys,
        core_keys,
    })
}

pub fn cpu_temperature_probe() -> Option<CpuTemperatureProbe> {
    let summary_raw = crate::smc_write::read_temperature_keys(M3_CPU_SUMMARY_TEMP_KEYS);
    let aggregate_raw = crate::smc_write::read_temperature_keys(M3_CPU_CORE_AVERAGE_TEMP_KEYS);
    let hotspot_raw = crate::smc_write::read_temperature_keys(M3_CPU_HOTSPOT_TEMP_KEYS);
    let core_keys: Vec<&str> = M3_CPU_CORE_TEMP_KEYS
        .iter()
        .map(|sensor| sensor.key)
        .collect();
    let core_raw = crate::smc_write::read_temperature_keys(&core_keys);
    cpu_temperature_probe_from_readings(summary_raw, aggregate_raw, hotspot_raw, core_raw)
}

pub fn all_temperature_sensors() -> Vec<TempSensor> {
    let mut temps = MacosProvider::new()
        .and_then(|provider| provider.temperatures())
        .unwrap_or_default();

    if let Ok(mut smc) = Smc::connect() {
        if let Ok(iter) = smc.all_data() {
            let mut raw_smc = Vec::new();
            for data in iter {
                let Ok(data) = data else { continue };
                if !data.key.starts_with('T') {
                    continue;
                }
                let Ok(Some(macsmc::DataValue::Float(value))) = data.value else {
                    continue;
                };
                if !(1.0..=130.0).contains(&value) {
                    continue;
                }
                raw_smc.push(TempSensor {
                    id: format!("smc.raw.{}", data.key),
                    label: format!("SMC {}", data.key),
                    kind: SensorKind::Other,
                    value: Celsius(value),
                });
            }
            raw_smc.sort_by(|a, b| {
                b.value
                    .0
                    .partial_cmp(&a.value.0)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| a.id.cmp(&b.id))
            });
            temps.extend(raw_smc);
        }
    }

    let mut hid = crate::macos_hid::read_temps()
        .into_iter()
        .enumerate()
        .map(|(idx, (name, value))| {
            let label = if name.is_empty() {
                format!("IOHID sensor {}", idx + 1)
            } else {
                format!("IOHID {name}")
            };
            let fragment = if name.is_empty() {
                format!("sensor.{}", idx + 1)
            } else {
                temp_sensor_id_fragment(&name)
            };
            TempSensor {
                id: format!("hid.raw.{fragment}.{idx}"),
                label,
                kind: hid_sensor_kind(&name),
                value: Celsius(value),
            }
        })
        .collect::<Vec<_>>();
    hid.sort_by(|a, b| {
        b.value
            .0
            .partial_cmp(&a.value.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.id.cmp(&b.id))
    });
    temps.extend(hid);

    temps
}

fn apple_silicon_cpu_average_and_hot_from_values(
    cores: &[CpuCoreTemp],
    summary_values: &[f32],
    aggregate_values: &[f32],
    hotspot_values: &[f32],
) -> Option<(f32, f32)> {
    let all_core_values: Vec<f32> = cores.iter().map(|sensor| sensor.temp).collect();
    let (summary_avg, summary_hot) = match average_and_hot(summary_values) {
        Some((avg, hot)) => (Some(avg), Some(hot)),
        None => (None, None),
    };
    let (all_core_avg, computed_hot) = match average_and_hot(&all_core_values) {
        Some((avg, hot)) => (Some(avg), Some(hot)),
        None => (None, None),
    };
    let performance_values: Vec<f32> = cores
        .iter()
        .filter(|sensor| sensor.class == CpuCoreClass::Performance)
        .map(|sensor| sensor.temp)
        .collect();
    let performance_avg = average_and_hot(&performance_values).map(|(avg, _)| avg);
    let aggregate_avg = average_and_hot(aggregate_values).map(|(avg, _)| avg);
    let (hotspot_avg, hotspot_hot) = match average_and_hot(hotspot_values) {
        Some((avg, hot)) => (Some(avg), Some(hot)),
        None => (None, None),
    };
    let avg = [
        performance_avg,
        all_core_avg,
        aggregate_avg,
        summary_avg,
        hotspot_avg,
    ]
    .into_iter()
    .flatten()
    .next()?;
    let active_hotspot_hot = hotspot_hot.filter(|value| *value >= CPU_HOTSPOT_ACTIVE_FLOOR_C);
    let hot = [computed_hot, summary_hot, active_hotspot_hot]
        .into_iter()
        .flatten()
        .fold(avg, f32::max);
    Some((avg, hot))
}

fn apple_silicon_cpu_average_and_hot(cores: &[CpuCoreTemp]) -> Option<(f32, f32)> {
    let summary_values = apple_silicon_cpu_summary_temperatures();
    let aggregate_values = apple_silicon_cpu_average_temperatures();
    let hotspot_values = apple_silicon_cpu_hotspot_temperatures();
    apple_silicon_cpu_average_and_hot_from_values(
        cores,
        &summary_values,
        &aggregate_values,
        &hotspot_values,
    )
}

impl HardwareProvider for MacosProvider {
    fn name(&self) -> &str {
        "macos"
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            read_temps: self.has_smc,
            read_fans: self.has_smc,
            // Fan-speed control via SMC writes works on Intel Macs. On Apple
            // Silicon the fans are governed by the system: the same SMC writes
            // are accepted but have no effect, so we honestly report no control
            // rather than offer a dead knob.
            // Attempted wherever the SMC is present. On Apple Silicon the write
            // may be ignored by firmware, so callers should verify the RPM
            // actually changed rather than trust a non-error as success.
            control_fans: self.has_smc,
        }
    }

    fn hardware_info(&self) -> Result<HardwareInfo> {
        let cpu = sysctl_string("machdep.cpu.brand_string")
            .unwrap_or_else(|| "Apple Silicon".to_string());

        let memory = sysctl_u64("hw.memsize").map(|bytes| {
            let gib = bytes as f64 / (1024.0 * 1024.0 * 1024.0);
            format!("{:.0} GB", gib.round())
        });

        let os_version = sysctl_string("kern.osproductversion");
        let darwin = sysctl_string("kern.osrelease");
        let os = match (os_version, darwin) {
            (Some(v), Some(d)) => format!("macOS {v} (Darwin {d})"),
            (Some(v), None) => format!("macOS {v}"),
            _ => "macOS".to_string(),
        };

        Ok(HardwareInfo {
            cpu,
            gpu: None,
            motherboard: None,
            memory,
            os,
        })
    }

    fn temperatures(&self) -> Result<Vec<TempSensor>> {
        if !self.has_smc {
            return Err(CoreError::Unsupported("SMC not available".into()));
        }
        let mut temps: Vec<TempSensor> = Vec::new();

        // M3 Pro/Max expose per-core keys, `TV*` aggregate keys, `TCDX`
        // summary, and die hotspot keys. The headline is a CPU core average;
        // hotspot readings are listed separately as CPU Hottest.
        let smc_cpu_cores = apple_silicon_cpu_core_temperatures();
        if let Some((avg, hot)) = apple_silicon_cpu_average_and_hot(&smc_cpu_cores) {
            temps.push(TempSensor {
                id: "cpu.die".into(),
                label: "CPU Core Average".into(),
                kind: SensorKind::Cpu,
                value: Celsius(avg),
            });
            temps.push(TempSensor {
                id: "cpu.die.hot".into(),
                label: "CPU Core Hottest".into(),
                kind: SensorKind::Cpu,
                value: Celsius(hot),
            });
        }

        // Fallback CPU die temperatures via IOHID. `tcal` is a calibration
        // reading, not a die sensor; duplicate service entries are collapsed by
        // sensor name.
        let hid = crate::macos_hid::read_temps();
        let dies: Vec<(&str, f32)> = hid
            .iter()
            .filter(|(n, _)| n.contains("tdie"))
            .map(|(n, t)| (n.as_str(), *t))
            .collect();
        if temps.iter().all(|t| t.id != "cpu.die") {
            if let Some(avg) = deduped_name_average_max(dies.iter().copied()) {
                let hot = dies.iter().map(|(_, t)| *t).fold(0.0, f32::max);
                temps.push(TempSensor {
                    id: "cpu.die".into(),
                    label: "CPU Core Average".into(),
                    kind: SensorKind::Cpu,
                    value: Celsius(avg),
                });
                temps.push(TempSensor {
                    id: "cpu.die.hot".into(),
                    label: "CPU hottest".into(),
                    kind: SensorKind::Cpu,
                    value: Celsius(hot),
                });
            }
        }
        let nand: Vec<f32> = hid
            .iter()
            .filter(|(n, _)| n.contains("NAND"))
            .map(|(_, t)| *t)
            .collect();
        if let Some(ssd) = nand.iter().cloned().reduce(f32::max) {
            temps.push(TempSensor {
                id: "ssd".into(),
                label: "SSD".into(),
                kind: SensorKind::Storage,
                value: Celsius(ssd),
            });
        }

        // Battery pack temperature (the gas-gauge IC reports one reading per
        // cell; average them). Real on Apple Silicon laptops — verified against
        // plausible values (high 20s to low 30s °C at idle) on an M3 Max.
        let batt: Vec<f32> = hid
            .iter()
            .filter(|(n, _)| {
                n.to_lowercase().contains("gas gauge") || n.to_lowercase().contains("battery")
            })
            .map(|(_, t)| *t)
            .collect();
        if !batt.is_empty() {
            let avg = batt.iter().sum::<f32>() / batt.len() as f32;
            temps.push(TempSensor {
                id: "battery".into(),
                label: "Battery".into(),
                kind: SensorKind::Battery,
                value: Celsius(avg),
            });
        }

        let mut smc = Smc::connect().map_err(|e| CoreError::Hardware(format!("SMC: {e:?}")))?;

        // Ambient/board SMC sensors (id, label, kind, °C); zeros filtered below.
        let mut cand: Vec<(&str, &str, SensorKind, f32)> = Vec::new();
        if let Ok(t) = smc.cpu_temperature() {
            cand.push(("cpu.smc.die", "CPU die", SensorKind::Cpu, t.die.0));
            cand.push(("cpu.smc.proximity", "CPU", SensorKind::Cpu, t.proximity.0));
        }
        if let Ok(t) = smc.gpu_temperature() {
            cand.push(("gpu.die", "GPU die", SensorKind::Gpu, t.die.0));
            cand.push(("gpu.proximity", "GPU", SensorKind::Gpu, t.proximity.0));
        }
        if let Ok(t) = smc.other_temperatures() {
            cand.push((
                "mem.proximity",
                "Memory",
                SensorKind::Memory,
                t.memory_bank_proximity.0,
            ));
            cand.push((
                "mainboard.proximity",
                "Mainboard",
                SensorKind::Mainboard,
                t.mainboard_proximity.0,
            ));
            cand.push(("airport", "Airport", SensorKind::Other, t.airport.0));
            cand.push((
                "airflow.left",
                "Airflow left",
                SensorKind::Other,
                t.airflow_left.0,
            ));
            cand.push((
                "airflow.right",
                "Airflow right",
                SensorKind::Other,
                t.airflow_right.0,
            ));
            cand.push((
                "heatpipe.1",
                "Heatpipe 1",
                SensorKind::Other,
                t.heatpipe_1.0,
            ));
            cand.push((
                "heatpipe.2",
                "Heatpipe 2",
                SensorKind::Other,
                t.heatpipe_2.0,
            ));
            cand.push((
                "palmrest.1",
                "Palm rest 1",
                SensorKind::Other,
                t.palm_rest_1.0,
            ));
            cand.push((
                "palmrest.2",
                "Palm rest 2",
                SensorKind::Other,
                t.palm_rest_2.0,
            ));
        }

        // Add the SMC ambient sensors that returned a plausible value. On
        // Apple Silicon the SMC CPU/GPU die keys read 0 (filtered) — the real
        // die temps came from IOHID above; on Intel the SMC ones provide them.
        temps.extend(cand.into_iter().filter(|&(_, _, _, c)| c > 1.0).map(
            |(id, label, kind, c)| TempSensor {
                id: id.into(),
                label: label.into(),
                kind,
                value: Celsius(c),
            },
        ));
        Ok(temps)
    }

    fn fans(&self) -> Result<Vec<Fan>> {
        if !self.has_smc {
            return Err(CoreError::Unsupported("SMC not available".into()));
        }
        let mut smc = Smc::connect().map_err(|e| CoreError::Hardware(format!("SMC: {e:?}")))?;
        let fans = smc
            .fans()
            .map_err(|e| CoreError::Hardware(format!("SMC fans: {e:?}")))?;

        let mut out = Vec::new();
        for (i, fan) in fans.enumerate() {
            let Ok(f) = fan else { continue };
            out.push(Fan {
                id: format!("fan.{i}"),
                label: format!("Fan {}", i + 1),
                rpm: f.actual.0.round() as u32,
                min_rpm: Some(f.min.0.round() as u32),
                max_rpm: Some(f.max.0.round() as u32),
                duty_percent: Some(f.percentage().clamp(0.0, 100.0).round() as u8),
                controllable: self.has_smc,
            });
        }
        Ok(out)
    }

    fn set_fan_duty(&self, fan_id: &str, duty_percent: u8) -> Result<()> {
        let idx = fan_index(fan_id)?;
        // Map duty% onto the fan's real [min, max] RPM range.
        let mut smc = Smc::connect().map_err(|e| CoreError::Hardware(format!("SMC: {e:?}")))?;
        let fan = smc
            .fans()
            .map_err(|e| CoreError::Hardware(format!("SMC fans: {e:?}")))?
            .nth(idx as usize)
            .and_then(|f| f.ok())
            .ok_or_else(|| CoreError::NotFound(format!("fan '{fan_id}'")))?;
        let (min, max) = (fan.min.0, fan.max.0);
        let rpm = (min + (duty_percent as f32 / 100.0) * (max - min)).clamp(min, max);

        self.with_conn(|c| c.force(idx, rpm))
    }

    fn set_fan_auto(&self, fan_id: &str) -> Result<()> {
        let idx = fan_index(fan_id)?;
        self.with_conn(|c| c.auto(idx))
    }

    fn power_watts(&self) -> Option<f32> {
        if !self.has_smc {
            return None;
        }
        let mut smc = Smc::connect().ok()?;
        let w = smc.power_system_total().ok()?.0;
        (w > 0.0).then_some(w)
    }
}

impl MacosProvider {
    /// Run `f` against the persistent SMC write connection, opening it once.
    fn with_conn(
        &self,
        f: impl FnOnce(&Conn) -> std::result::Result<(), crate::smc_write::FanCtlError>,
    ) -> Result<()> {
        let mut guard = self.force_conn.lock().expect("smc conn poisoned");
        if guard.is_none() {
            *guard = Some(Conn::open().map_err(map_fan_err)?);
        }
        f(guard.as_ref().expect("conn present")).map_err(map_fan_err)
    }
}

/// Parse `"fan.N"` (or a bare index) into a fan index.
fn fan_index(fan_id: &str) -> Result<u8> {
    fan_id
        .rsplit('.')
        .next()
        .and_then(|s| s.parse::<u8>().ok())
        .filter(|&n| n < 10)
        .ok_or_else(|| CoreError::NotFound(format!("fan id '{fan_id}'")))
}

fn map_fan_err(e: crate::smc_write::FanCtlError) -> CoreError {
    use crate::smc_write::FanCtlError as F;
    match e {
        F::NotPrivileged => {
            CoreError::PermissionDenied("SMC fan control requires root — re-run with `sudo`".into())
        }
        F::Open => CoreError::Hardware("could not open AppleSMC".into()),
        F::Smc(code) => CoreError::Hardware(format!("SMC write failed (code {code})")),
    }
}

/// Read a string-valued sysctl by name, e.g. `machdep.cpu.brand_string`.
fn sysctl_string(name: &str) -> Option<String> {
    let cname = CString::new(name).ok()?;
    let mut size: libc::size_t = 0;

    // First call with a null buffer to learn the required size.
    let rc = unsafe {
        libc::sysctlbyname(
            cname.as_ptr(),
            ptr::null_mut(),
            &mut size,
            ptr::null_mut(),
            0,
        )
    };
    if rc != 0 || size == 0 {
        return None;
    }

    let mut buf = vec![0u8; size];
    let rc = unsafe {
        libc::sysctlbyname(
            cname.as_ptr(),
            buf.as_mut_ptr() as *mut libc::c_void,
            &mut size,
            ptr::null_mut(),
            0,
        )
    };
    if rc != 0 {
        return None;
    }

    // sysctl strings are NUL-terminated; drop the trailing NUL if present.
    if buf.last() == Some(&0) {
        buf.pop();
    }
    String::from_utf8(buf).ok()
}

/// Read an unsigned-integer sysctl by name, e.g. `hw.memsize`.
fn sysctl_u64(name: &str) -> Option<u64> {
    let cname = CString::new(name).ok()?;
    let mut val: u64 = 0;
    let mut size = mem::size_of::<u64>() as libc::size_t;
    let rc = unsafe {
        libc::sysctlbyname(
            cname.as_ptr(),
            &mut val as *mut u64 as *mut libc::c_void,
            &mut size,
            ptr::null_mut(),
            0,
        )
    };
    if rc != 0 {
        return None;
    }
    Some(val)
}

#[cfg(test)]
mod tests {
    #[test]
    fn deduped_name_average_uses_one_value_per_sensor() {
        let avg = super::deduped_name_average_max([
            ("PMU tdie1", 50.0),
            ("PMU tdie1", 52.0),
            ("PMU tdie2", 56.0),
        ])
        .unwrap();

        assert!((avg - 54.0).abs() < f32::EPSILON);
    }

    #[test]
    fn deduped_name_average_empty_input_is_none() {
        assert!(super::deduped_name_average_max([]).is_none());
    }

    #[test]
    fn temp_sensor_id_fragment_normalizes_for_stable_raw_ids() {
        assert_eq!(
            super::temp_sensor_id_fragment("PMU tdie 1 / CPU"),
            "pmu.tdie.1.cpu"
        );
        assert_eq!(super::temp_sensor_id_fragment(""), "");
    }

    #[test]
    fn hid_sensor_kind_classifies_common_raw_sensor_names() {
        assert_eq!(
            super::hid_sensor_kind("CPU Performance Core 1"),
            peterfan_core::types::SensorKind::Cpu
        );
        assert_eq!(
            super::hid_sensor_kind("GPU Cluster 1"),
            peterfan_core::types::SensorKind::Gpu
        );
        assert_eq!(
            super::hid_sensor_kind("APPLE SSD AP1024Z"),
            peterfan_core::types::SensorKind::Storage
        );
        assert_eq!(
            super::hid_sensor_kind("Battery Gas Gauge"),
            peterfan_core::types::SensorKind::Battery
        );
    }

    #[test]
    fn average_and_hot_summarizes_core_temperatures() {
        assert_eq!(super::average_and_hot(&[]), None);
        assert_eq!(
            super::average_and_hot(&[70.0, 74.0, 68.0]),
            Some((70.666664, 74.0))
        );
    }

    #[test]
    fn apple_silicon_cpu_temperature_uses_live_core_when_hotspot_is_inactive() {
        let cores = vec![
            super::CpuCoreTemp {
                class: super::CpuCoreClass::Efficiency,
                temp: 60.0,
            },
            super::CpuCoreTemp {
                class: super::CpuCoreClass::Efficiency,
                temp: 62.0,
            },
            super::CpuCoreTemp {
                class: super::CpuCoreClass::Performance,
                temp: 74.0,
            },
            super::CpuCoreTemp {
                class: super::CpuCoreClass::Performance,
                temp: 78.0,
            },
        ];

        assert_eq!(
            super::apple_silicon_cpu_average_and_hot_from_values(
                &cores,
                &[72.0],
                &[74.0, 76.0],
                &[64.0, 66.0]
            ),
            Some((76.0, 78.0))
        );
    }

    #[test]
    fn apple_silicon_cpu_average_stays_on_core_average_when_summary_is_hotter() {
        let cores = vec![
            super::CpuCoreTemp {
                class: super::CpuCoreClass::Efficiency,
                temp: 60.0,
            },
            super::CpuCoreTemp {
                class: super::CpuCoreClass::Performance,
                temp: 78.0,
            },
        ];

        assert_eq!(
            super::apple_silicon_cpu_average_and_hot_from_values(
                &cores,
                &[82.0],
                &[74.0, 76.0],
                &[]
            ),
            Some((78.0, 82.0))
        );
    }

    #[test]
    fn apple_silicon_cpu_temperature_keeps_hotspot_out_of_average() {
        let cores = vec![
            super::CpuCoreTemp {
                class: super::CpuCoreClass::Efficiency,
                temp: 60.0,
            },
            super::CpuCoreTemp {
                class: super::CpuCoreClass::Performance,
                temp: 78.0,
            },
        ];

        assert_eq!(
            super::apple_silicon_cpu_average_and_hot_from_values(
                &cores,
                &[72.0],
                &[74.0, 76.0],
                &[84.0, 86.0]
            ),
            Some((78.0, 86.0))
        );
    }

    #[test]
    fn apple_silicon_cpu_average_uses_aggregate_without_summary_key() {
        let cores = vec![
            super::CpuCoreTemp {
                class: super::CpuCoreClass::Efficiency,
                temp: 60.0,
            },
            super::CpuCoreTemp {
                class: super::CpuCoreClass::Efficiency,
                temp: 62.0,
            },
            super::CpuCoreTemp {
                class: super::CpuCoreClass::Performance,
                temp: 74.0,
            },
            super::CpuCoreTemp {
                class: super::CpuCoreClass::Performance,
                temp: 78.0,
            },
        ];

        assert_eq!(
            super::apple_silicon_cpu_average_and_hot_from_values(&cores, &[], &[74.0, 76.0], &[]),
            Some((76.0, 78.0))
        );
    }

    #[test]
    fn apple_silicon_cpu_average_falls_back_to_aggregate_without_core_keys() {
        let cores = vec![];

        assert_eq!(
            super::apple_silicon_cpu_average_and_hot_from_values(&cores, &[], &[74.0], &[]),
            Some((74.0, 74.0))
        );
    }

    #[test]
    fn apple_silicon_cpu_average_uses_all_cores_without_aggregate_keys() {
        let cores = vec![
            super::CpuCoreTemp {
                class: super::CpuCoreClass::Efficiency,
                temp: 60.0,
            },
            super::CpuCoreTemp {
                class: super::CpuCoreClass::Efficiency,
                temp: 62.0,
            },
            super::CpuCoreTemp {
                class: super::CpuCoreClass::Performance,
                temp: 74.0,
            },
            super::CpuCoreTemp {
                class: super::CpuCoreClass::Performance,
                temp: 78.0,
            },
        ];

        assert_eq!(
            super::apple_silicon_cpu_average_and_hot_from_values(&cores, &[], &[], &[]),
            Some((76.0, 78.0))
        );
    }

    #[test]
    fn apple_silicon_cpu_average_falls_back_to_all_cores_without_performance_keys() {
        let cores = vec![
            super::CpuCoreTemp {
                class: super::CpuCoreClass::Efficiency,
                temp: 60.0,
            },
            super::CpuCoreTemp {
                class: super::CpuCoreClass::Efficiency,
                temp: 62.0,
            },
        ];

        assert_eq!(
            super::apple_silicon_cpu_average_and_hot_from_values(&cores, &[], &[], &[]),
            Some((61.0, 62.0))
        );
    }

    #[test]
    fn m3_cpu_core_key_map_includes_efficiency_and_performance_cores() {
        let keys: Vec<&str> = super::M3_CPU_CORE_TEMP_KEYS
            .iter()
            .map(|sensor| sensor.key)
            .collect();

        assert!(keys.contains(&"Te05"));
        assert!(keys.contains(&"Te0S"));
        assert!(keys.contains(&"Tf04"));
        assert!(keys.contains(&"Tf4E"));
    }

    #[test]
    fn m3_cpu_core_key_map_marks_core_classes() {
        let efficiency = super::M3_CPU_CORE_TEMP_KEYS
            .iter()
            .filter(|sensor| sensor.class == super::CpuCoreClass::Efficiency)
            .count();
        let performance = super::M3_CPU_CORE_TEMP_KEYS
            .iter()
            .filter(|sensor| sensor.class == super::CpuCoreClass::Performance)
            .count();

        assert_eq!(efficiency, 4);
        assert_eq!(performance, 12);
    }

    #[test]
    fn m3_cpu_average_key_map_includes_aggregate_keys() {
        assert_eq!(
            super::M3_CPU_CORE_AVERAGE_TEMP_KEYS,
            &["TV0s", "TV1s", "TVsa", "TVss"]
        );
    }

    #[test]
    fn m3_cpu_hotspot_key_map_includes_cpu_die_keys() {
        assert_eq!(super::M3_CPU_HOTSPOT_TEMP_KEYS, &["TVD0", "TCMb"]);
    }

    #[test]
    fn cpu_temperature_probe_reports_selection_and_candidates() {
        let probe = super::cpu_temperature_probe_from_readings(
            vec![("TCDX".to_string(), 72.0)],
            vec![("TV0s".to_string(), 74.0), ("TVss".to_string(), 76.0)],
            vec![("TVD0".to_string(), 84.0), ("TCMb".to_string(), 86.0)],
            vec![
                ("Te05".to_string(), 62.0),
                ("Tf04".to_string(), 64.0),
                ("Tf09".to_string(), 66.0),
            ],
        )
        .unwrap();

        assert_eq!(probe.selected_average_c, Some(65.0));
        assert_eq!(probe.selected_hottest_c, Some(86.0));
        assert_eq!(probe.summary_average_c, Some(72.0));
        assert_eq!(probe.aggregate_average_c, Some(75.0));
        assert_eq!(probe.hotspot_average_c, Some(85.0));
        assert_eq!(probe.hotspot_hottest_c, Some(86.0));
        assert_eq!(probe.performance_core_average_c, Some(65.0));
        assert_eq!(probe.all_core_average_c, Some(64.0));
        assert_eq!(probe.core_hottest_c, Some(66.0));
        assert_eq!(probe.aggregate_keys.len(), 2);
        assert_eq!(probe.hotspot_keys.len(), 2);
        assert_eq!(
            probe
                .core_keys
                .iter()
                .filter(|sensor| sensor.class == "performance")
                .count(),
            2
        );
    }

    #[test]
    fn cpu_temperature_probe_returns_none_without_readings() {
        assert!(super::cpu_temperature_probe_from_readings(
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new()
        )
        .is_none());
    }

    #[test]
    #[ignore = "prints local SMC CPU core temperatures for manual debugging"]
    fn print_smc_cpu_core_temperatures() {
        let mut smc = macsmc::Smc::connect().expect("SMC should open");
        let iter = smc.cpu_core_temps().expect("CPU core temps should read");
        for (idx, temp) in iter.enumerate() {
            match temp {
                Ok(temp) => println!("{:02}: {:.2}", idx + 1, temp.0),
                Err(e) => println!("{:02}: error {e:?}", idx + 1),
            }
        }
    }

    #[test]
    #[ignore = "prints local SMC temperature-like keys for manual debugging"]
    fn print_smc_temperature_like_keys() {
        let mut smc = macsmc::Smc::connect().expect("SMC should open");
        for data in smc.all_data().expect("SMC data should iterate") {
            let Ok(data) = data else { continue };
            if !data.key.starts_with('T') {
                continue;
            }
            let Ok(Some(macsmc::DataValue::Float(value))) = data.value else {
                continue;
            };
            if (1.0..=130.0).contains(&value) {
                println!("{:4}  {:6.2}", data.key, value);
            }
        }
    }

    #[test]
    #[ignore = "prints PeterFan's selected Apple Silicon CPU core SMC keys"]
    fn print_selected_cpu_core_temperature_keys() {
        for (idx, sensor) in super::apple_silicon_cpu_core_temperatures()
            .iter()
            .enumerate()
        {
            println!("{:?} {:02}: {:.2}", sensor.class, idx + 1, sensor.temp);
        }
    }

    #[test]
    #[ignore = "prints PeterFan's Apple Silicon CPU temperature candidates"]
    fn print_cpu_temperature_candidates() {
        let aggregate_values = super::apple_silicon_cpu_average_temperatures();
        let summary_values = super::apple_silicon_cpu_summary_temperatures();
        let core_values = super::apple_silicon_cpu_core_temperatures();
        if let Some((avg, hot)) = super::apple_silicon_cpu_average_and_hot_from_values(
            &core_values,
            &summary_values,
            &aggregate_values,
            &super::apple_silicon_cpu_hotspot_temperatures(),
        ) {
            println!("selected avg/hot: {avg:.2} / {hot:.2}");
        }
        if let Some((avg, hot)) = super::average_and_hot(&summary_values) {
            println!("summary avg/hot: {avg:.2} / {hot:.2}");
        }
        if let Some((avg, hot)) = super::average_and_hot(&aggregate_values) {
            println!("aggregate avg/hot: {avg:.2} / {hot:.2}");
        }
        let performance: Vec<f32> = core_values
            .iter()
            .filter(|sensor| sensor.class == super::CpuCoreClass::Performance)
            .map(|sensor| sensor.temp)
            .collect();
        if let Some((avg, hot)) = super::average_and_hot(&performance) {
            println!("p-core avg/hot: {avg:.2} / {hot:.2}");
        }
        for (idx, temp) in aggregate_values.iter().enumerate() {
            println!(
                "aggregate {} {}: {:.2}",
                super::M3_CPU_CORE_AVERAGE_TEMP_KEYS[idx],
                idx + 1,
                temp
            );
        }
    }
}
