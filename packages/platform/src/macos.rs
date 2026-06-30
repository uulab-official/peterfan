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
//!   die-temperature keys (they read 0), so those are filtered out; sensors the
//!   chip *does* expose (e.g. airflow/airport, palm rest, memory) are shown.
//!   Reading CPU/GPU die temps on Apple Silicon needs the IOHID thermal-sensor
//!   API — a separate milestone (see `docs/ROADMAP.md`).
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

impl HardwareProvider for MacosProvider {
    fn name(&self) -> &str {
        "macos"
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            read_temps: self.has_smc,
            read_fans: self.has_smc,
            // Fan control via SMC writes is implemented; the write itself is
            // privileged (returns PermissionDenied without root).
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

        // Real CPU/GPU die temperatures via IOHID (the SMC doesn't expose these
        // on Apple Silicon). Aggregate the per-cluster die sensors.
        let hid = crate::macos_hid::read_temps();
        let dies: Vec<f32> = hid
            .iter()
            .filter(|(n, _)| n.contains("tdie") || n.contains("tcal"))
            .map(|(_, t)| *t)
            .collect();
        if !dies.is_empty() {
            let avg = dies.iter().sum::<f32>() / dies.len() as f32;
            let hot = dies.iter().cloned().fold(0.0, f32::max);
            temps.push(TempSensor {
                id: "cpu.die".into(),
                label: "CPU".into(),
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
            cand.push(("mem.proximity", "Memory", SensorKind::Memory, t.memory_bank_proximity.0));
            cand.push(("mainboard.proximity", "Mainboard", SensorKind::Mainboard, t.mainboard_proximity.0));
            cand.push(("airport", "Airport", SensorKind::Other, t.airport.0));
            cand.push(("airflow.left", "Airflow left", SensorKind::Other, t.airflow_left.0));
            cand.push(("airflow.right", "Airflow right", SensorKind::Other, t.airflow_right.0));
            cand.push(("heatpipe.1", "Heatpipe 1", SensorKind::Other, t.heatpipe_1.0));
            cand.push(("heatpipe.2", "Heatpipe 2", SensorKind::Other, t.heatpipe_2.0));
            cand.push(("palmrest.1", "Palm rest 1", SensorKind::Other, t.palm_rest_1.0));
            cand.push(("palmrest.2", "Palm rest 2", SensorKind::Other, t.palm_rest_2.0));
        }

        // Add the SMC ambient sensors that returned a plausible value. On
        // Apple Silicon the SMC CPU/GPU die keys read 0 (filtered) — the real
        // die temps came from IOHID above; on Intel the SMC ones provide them.
        temps.extend(
            cand.into_iter()
                .filter(|&(_, _, _, c)| c > 1.0)
                .map(|(id, label, kind, c)| TempSensor {
                    id: id.into(),
                    label: label.into(),
                    kind,
                    value: Celsius(c),
                }),
        );
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
}

impl MacosProvider {
    /// Run `f` against the persistent SMC write connection, opening it once.
    fn with_conn(&self, f: impl FnOnce(&Conn) -> std::result::Result<(), crate::smc_write::FanCtlError>) -> Result<()> {
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
        F::NotPrivileged => CoreError::PermissionDenied(
            "SMC fan control requires root — re-run with `sudo`".into(),
        ),
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
