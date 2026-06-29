//! macOS backend (read-only, **real** hardware info).
//!
//! What works today, with no special privileges:
//! - [`HardwareProvider::hardware_info`] via `sysctl` (genuine CPU brand,
//!   installed memory, OS version).
//!
//! What does NOT work yet (returns [`CoreError::Unsupported`]):
//! - temperature & fan reading — requires talking to the SMC (`AppleSMC`) over
//!   IOKit and decoding per-key types. Apple Silicon and Intel expose different
//!   keys; doing this *correctly* is its own milestone (see `docs/ROADMAP.md`).
//! - fan control — SMC writes are privileged and can require SIP changes; this
//!   is deliberately deferred behind the safety design in the daemon.
//!
//! Because temps/fans are unsupported here, the CLI/TUI transparently fall back
//! to the mock backend for sensor data and label it as simulated.

use std::ffi::CString;
use std::mem;
use std::ptr;

use peterfan_core::error::{CoreError, Result};
use peterfan_core::provider::Capabilities;
use peterfan_core::types::{Fan, HardwareInfo, TempSensor};
use peterfan_core::HardwareProvider;

pub struct MacosProvider;

impl MacosProvider {
    pub fn new() -> Result<Self> {
        Ok(Self)
    }
}

impl HardwareProvider for MacosProvider {
    fn name(&self) -> &str {
        "macos"
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            // SMC reading not implemented yet — be honest about it.
            read_temps: false,
            read_fans: false,
            control_fans: false,
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
            // Apple integrates the GPU; we don't probe it via sysctl here.
            gpu: None,
            motherboard: None,
            memory,
            os,
        })
    }

    fn temperatures(&self) -> Result<Vec<TempSensor>> {
        Err(CoreError::Unsupported(
            "temperature reading on macOS (SMC) is not implemented yet".into(),
        ))
    }

    fn fans(&self) -> Result<Vec<Fan>> {
        Err(CoreError::Unsupported(
            "fan reading on macOS (SMC) is not implemented yet".into(),
        ))
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
