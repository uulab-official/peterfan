//! Minimal SMC **write** client for fan control on macOS.
//!
//! `macsmc` (used for reading) is read-only, so this module implements the
//! small slice of the IOKit `AppleSMC` user-client needed to *write* fan keys.
//! The struct layout mirrors the one `macsmc` uses for reads (which is known to
//! work on this hardware); we only add the `WRITE_BYTES` command.
//!
//! Fan control keys (per fan index `n`):
//! - `FnMd` (uppercase; `Fnmd` on M5) — mode: `1` = forced, `0` = auto.
//! - `FnTg` — target speed (a 4-byte little-endian `flt ` RPM on Apple Silicon).
//! - `Ftst` — diagnostic/force-test unlock (ui8).
//!
//! ## Apple Silicon unlock sequence
//!
//! On Apple Silicon, `thermalmonitord` enforces a "System Mode" and will revert
//! a bare `FnMd = 1` back to `0` after a few seconds, so a direct write looks
//! like it succeeds but has no effect. The working sequence (reverse-engineered
//! by the community; see `docs/RESEARCH.md`) is:
//!
//! 1. Try `FnMd = 1` directly (enough on M1 and M5).
//! 2. If it doesn't stick, write `Ftst = 1` to inhibit the thermal servo, then
//!    re-write `FnMd = 1`, polling for a few seconds until it holds.
//! 3. Write the target RPM to `FnTg`.
//! 4. To restore, set `FnMd = 0` and `Ftst = 0` so the daemon takes back over.
//!
//! Even with the right sequence, **some firmware revisions ignore manual
//! control entirely**, which is why callers verify by reading RPM back rather
//! than trusting a successful write.
//!
//! SMC writes are privileged: without root the kernel returns
//! `kIOReturnNotPrivileged`, surfaced here as [`FanCtlError::NotPrivileged`].

use std::mem::size_of;
use std::os::raw::{c_char, c_void};

type KernReturn = i32;
type MachPort = u32;

const MASTER_PORT_DEFAULT: MachPort = 0;
const KERN_SUCCESS: KernReturn = 0;
const RETURN_NOT_PRIVILEGED: KernReturn = 0xe000_02c1u32 as KernReturn;
const KERNEL_INDEX_SMC: u32 = 2;
const CMD_READ_KEYINFO: u8 = 9;
const CMD_READ_BYTES: u8 = 5;
const CMD_WRITE_BYTES: u8 = 6;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Version {
    major: u8,
    minor: u8,
    build: u8,
    reserved: u8,
    release: u16,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct LimitData {
    version: u16,
    length: u16,
    cpu: u32,
    gpu: u32,
    mem: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct KeyInfo {
    data_size: u32,
    data_type: u32,
    data_attributes: u8,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Bytes([u8; 32]);

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct KeyData {
    key: u32,
    version: Version,
    p_limit: LimitData,
    key_info: KeyInfo,
    result: u8,
    status: u8,
    data8: u8,
    data32: u32,
    bytes: Bytes,
}

#[link(name = "IOKit", kind = "framework")]
extern "C" {
    fn IOServiceMatching(name: *const c_char) -> *mut c_void;
    fn IOServiceGetMatchingService(master: MachPort, matching: *const c_void) -> MachPort;
    fn IOServiceOpen(
        service: MachPort,
        owning: MachPort,
        typ: u32,
        connect: *mut MachPort,
    ) -> KernReturn;
    fn IOServiceClose(conn: MachPort) -> KernReturn;
    fn IOConnectCallStructMethod(
        conn: MachPort,
        selector: u32,
        input: *const c_void,
        input_size: usize,
        output: *mut c_void,
        output_size: *mut usize,
    ) -> KernReturn;
    fn IOObjectRelease(obj: MachPort) -> KernReturn;
    fn mach_task_self() -> MachPort;
}

/// Read-only snapshot of the SMC keys that matter for fan control, for
/// `peterfan doctor`. None of this needs root (it only reads key-info).
#[derive(Debug, Clone, Copy)]
pub struct FanProbe {
    /// The `AppleSMC` user-client could be opened at all.
    pub opened: bool,
    /// The fan-0 mode key that exists, if any (`F0Md` on M1–M4, `F0md` on M5).
    pub mode_key: Option<&'static str>,
    /// The `Ftst` Apple-Silicon unlock key is present.
    pub ftst: bool,
    /// The Intel `FS! ` force bitmask key is present.
    pub fs: bool,
}

/// Probe the fan-control SMC keys (read-only; no root required).
pub fn probe() -> FanProbe {
    match Conn::open() {
        Ok(c) => {
            let mode_key = if c.key_exists(fan_key(0, *b"Md")) {
                Some("F0Md")
            } else if c.key_exists(fan_key(0, *b"md")) {
                Some("F0md")
            } else {
                None
            };
            FanProbe {
                opened: true,
                mode_key,
                ftst: c.key_exists(ftst_key()),
                fs: c.key_exists(fs_key()),
            }
        }
        Err(_) => FanProbe {
            opened: false,
            mode_key: None,
            ftst: false,
            fs: false,
        },
    }
}

/// Read selected SMC temperature keys as Celsius.
///
/// This is intentionally tiny and key-list driven. Apple Silicon exposes many
/// opaque temperature keys, and reading the full table every second is too
/// expensive for the menu-bar app; callers should pass only keys they trust.
pub fn read_temperature_keys(keys: &[&str]) -> Vec<(String, f32)> {
    let Ok(conn) = Conn::open() else {
        return Vec::new();
    };
    keys.iter()
        .filter_map(|key| {
            let value = conn.read_temperature_key_str(key).ok()?;
            (1.0..=130.0)
                .contains(&value)
                .then(|| ((*key).to_string(), value))
        })
        .collect()
}

/// Errors from a fan-control write.
#[derive(Debug)]
pub enum FanCtlError {
    /// SMC writes require elevated privileges (run with `sudo`).
    NotPrivileged,
    /// Could not open the `AppleSMC` service.
    Open,
    /// The SMC returned an error for the operation.
    Smc(i32),
}

/// A four-character SMC key as a big-endian u32, e.g. `F0Md`.
fn fan_key(idx: u8, suffix: [u8; 2]) -> u32 {
    u32::from_be_bytes([b'F', b'0' + idx, suffix[0], suffix[1]])
}

/// The `FS! ` fan-force bitmask key (one bit per fan = manual mode).
fn fs_key() -> u32 {
    u32::from_be_bytes(*b"FS! ")
}

/// The `Ftst` diagnostic/force-test unlock key (Apple Silicon).
fn ftst_key() -> u32 {
    u32::from_be_bytes(*b"Ftst")
}

#[cfg(any(target_arch = "aarch64", test))]
#[derive(Debug)]
enum AutoProbe {
    Released,
    Manual,
    Legacy,
}

#[cfg(any(target_arch = "aarch64", test))]
fn safe_auto_handoff(
    mut probe: impl FnMut() -> Result<AutoProbe, FanCtlError>,
    mut legacy: impl FnMut() -> Result<(), FanCtlError>,
) -> Result<(), FanCtlError> {
    match probe() {
        Ok(AutoProbe::Released) => Ok(()),
        Ok(AutoProbe::Manual) => {
            legacy()?;
            match probe() {
                Ok(AutoProbe::Released) => Ok(()),
                Ok(_) => Err(FanCtlError::Smc(-3)),
                Err(error) => Err(error),
            }
        }
        Ok(AutoProbe::Legacy) => legacy(),
        // When observation fails, still attempt the existing safety restore.
        // Refusing to write here could leave a previously forced fan pinned low.
        // A successful transport write cannot prove the handoff while the mode
        // is unreadable: report the observation error so callers retry.
        Err(error) => {
            legacy()?;
            Err(error)
        }
    }
}

#[cfg(any(target_arch = "aarch64", test))]
fn classify_auto_probe(
    mut key_present: impl FnMut(u32) -> Result<bool, FanCtlError>,
    mut mode: impl FnMut() -> Result<u8, FanCtlError>,
) -> Result<AutoProbe, FanCtlError> {
    // Global force mechanisms require the original cross-fan behavior.
    if key_present(ftst_key())? || key_present(fs_key())? {
        return Ok(AutoProbe::Legacy);
    }
    match mode()? {
        0 | 3 => Ok(AutoProbe::Released),
        1 => Ok(AutoProbe::Manual),
        _ => Err(FanCtlError::Smc(-1)),
    }
}

/// Flip fan `idx`'s bit in the `FS! ` manual-mode bitmask (best-effort; the key
/// is absent on some machines, in which case we rely on `Fn Md` alone).
fn set_force_bit(conn: &Conn, idx: u8, manual: bool) {
    let mut buf = [0u8; 2];
    if conn.read_key(fs_key(), &mut buf).is_ok() {
        let mut mask = u16::from_be_bytes(buf);
        if manual {
            mask |= 1 << idx;
        } else {
            mask &= !(1u16 << idx);
        }
        let _ = conn.write_key(fs_key(), &mask.to_be_bytes());
    }
}

/// An open SMC user-client connection. **Kept open** for the lifetime of the
/// owner: on Apple Silicon a forced fan reverts to automatic when the SMC
/// connection closes, so the controller must hold this open and re-assert.
pub struct Conn {
    port: MachPort,
    /// Whether the slow `Ftst` unlock + poll has already been attempted, so the
    /// daemon (which re-asserts every tick) doesn't burn ~10s per tick polling
    /// when this machine's firmware simply ignores manual control.
    unlock_tried: std::sync::atomic::AtomicBool,
}

// The handle is a plain mach port (u32); safe to move and to guard with a Mutex.
unsafe impl Send for Conn {}

impl Conn {
    pub fn open() -> Result<Self, FanCtlError> {
        unsafe {
            let matching = IOServiceMatching(c"AppleSMC".as_ptr());
            let device = IOServiceGetMatchingService(MASTER_PORT_DEFAULT, matching);
            if device == 0 {
                return Err(FanCtlError::Open);
            }
            let mut conn: MachPort = 0;
            let rc = IOServiceOpen(device, mach_task_self(), 0, &mut conn);
            IOObjectRelease(device);
            if rc != KERN_SUCCESS {
                return Err(FanCtlError::Open);
            }
            Ok(Conn {
                port: conn,
                unlock_tried: std::sync::atomic::AtomicBool::new(false),
            })
        }
    }

    fn call_raw(&self, input: &KeyData, output: &mut KeyData) -> (KernReturn, usize) {
        let mut osize = size_of::<KeyData>();
        let rc = unsafe {
            IOConnectCallStructMethod(
                self.port,
                KERNEL_INDEX_SMC,
                input as *const _ as *const c_void,
                size_of::<KeyData>(),
                output as *mut _ as *mut c_void,
                &mut osize,
            )
        };
        (rc, osize)
    }

    fn call(&self, input: &KeyData, output: &mut KeyData) -> Result<(), FanCtlError> {
        let (rc, osize) = self.call_raw(input, output);
        check_smc_response(rc, output.result, osize)
    }

    /// Read up to `out.len()` bytes of `key` into `out`; returns the key size.
    fn read_key(&self, key: u32, out: &mut [u8]) -> Result<usize, FanCtlError> {
        let (_, size) = self.read_key_typed(key, out)?;
        Ok(size)
    }

    fn read_key_typed(&self, key: u32, out: &mut [u8]) -> Result<(u32, usize), FanCtlError> {
        let mut input = KeyData {
            key,
            data8: CMD_READ_KEYINFO,
            ..Default::default()
        };
        let mut info = KeyData::default();
        self.call(&input, &mut info)?;
        let size = info.key_info.data_size as usize;
        if size == 0 || size > 32 {
            return Err(FanCtlError::Smc(-1));
        }
        input.key_info.data_size = info.key_info.data_size;
        input.data8 = CMD_READ_BYTES;
        let mut data = KeyData::default();
        self.call(&input, &mut data)?;
        let n = size.min(out.len());
        out[..n].copy_from_slice(&data.bytes.0[..n]);
        Ok((info.key_info.data_type, size))
    }

    fn read_temperature_key_str(&self, key: &str) -> Result<f32, FanCtlError> {
        if key.len() != 4 {
            return Err(FanCtlError::Smc(-1));
        }
        let mut raw = [0u8; 4];
        let key = u32::from_be_bytes(
            key.as_bytes()
                .try_into()
                .map_err(|_| FanCtlError::Smc(-1))?,
        );
        let (data_type, size) = self.read_key_typed(key, &mut raw)?;
        decode_temperature(data_type, &raw[..size.min(raw.len())])
    }

    /// Write `data` to `key` (after reading its declared size from the SMC).
    fn write_key(&self, key: u32, data: &[u8]) -> Result<(), FanCtlError> {
        let mut input = KeyData {
            key,
            data8: CMD_READ_KEYINFO,
            ..Default::default()
        };
        let mut output = KeyData::default();
        self.call(&input, &mut output)?;

        let size = output.key_info.data_size as usize;
        if size == 0 || size > 32 || data.len() > size {
            return Err(FanCtlError::Smc(-1));
        }

        input.key_info.data_size = output.key_info.data_size;
        input.data8 = CMD_WRITE_BYTES;
        input.bytes = Bytes([0; 32]);
        input.bytes.0[..data.len()].copy_from_slice(data);

        let mut out2 = KeyData::default();
        self.call(&input, &mut out2)
    }
}

/// IOKit transport success does not imply that AppleSMC accepted a command.
/// For example, missing keys can return KERN_SUCCESS with result 132. The
/// firmware result is preserved as a positive SMC error code; a truncated
/// reply uses a distinct negative internal error. This is shared by key-info,
/// read and write requests.
fn check_smc_response(
    rc: KernReturn,
    firmware_result: u8,
    response_size: usize,
) -> Result<(), FanCtlError> {
    match rc {
        KERN_SUCCESS if response_size < size_of::<KeyData>() => Err(FanCtlError::Smc(-2)),
        KERN_SUCCESS if firmware_result != 0 => Err(FanCtlError::Smc(i32::from(firmware_result))),
        KERN_SUCCESS => Ok(()),
        RETURN_NOT_PRIVILEGED => Err(FanCtlError::NotPrivileged),
        other => Err(FanCtlError::Smc(other)),
    }
}

#[cfg(any(target_arch = "aarch64", test))]
fn check_optional_key_info(
    rc: KernReturn,
    firmware_result: u8,
    response_size: usize,
) -> Result<bool, FanCtlError> {
    if rc == KERN_SUCCESS && response_size >= size_of::<KeyData>() && firmware_result == 132 {
        return Ok(false);
    }
    check_smc_response(rc, firmware_result, response_size)?;
    Ok(true)
}

fn decode_temperature(data_type: u32, data: &[u8]) -> Result<f32, FanCtlError> {
    match &data_type.to_be_bytes() {
        b"sp78" if data.len() >= 2 => Ok(i16::from_be_bytes([data[0], data[1]]) as f32 / 256.0),
        b"fp88" if data.len() >= 2 => Ok(u16::from_be_bytes([data[0], data[1]]) as f32 / 256.0),
        b"flt " if data.len() >= 4 => Ok(f32::from_ne_bytes([data[0], data[1], data[2], data[3]])),
        _ => Err(FanCtlError::Smc(-1)),
    }
}

impl Drop for Conn {
    fn drop(&mut self) {
        unsafe {
            IOServiceClose(self.port);
        }
    }
}

impl Conn {
    /// Does `key` exist on this SMC? (Reads only its key-info / size.)
    fn key_exists(&self, key: u32) -> bool {
        let input = KeyData {
            key,
            data8: CMD_READ_KEYINFO,
            ..Default::default()
        };
        let mut info = KeyData::default();
        self.call(&input, &mut info).is_ok() && info.key_info.data_size > 0
    }

    /// Distinguish an absent optional key from a failed or malformed probe.
    #[cfg(target_arch = "aarch64")]
    fn optional_key_present(&self, key: u32) -> Result<bool, FanCtlError> {
        let input = KeyData {
            key,
            data8: CMD_READ_KEYINFO,
            ..Default::default()
        };
        let mut info = KeyData::default();
        let (rc, osize) = self.call_raw(&input, &mut info);
        if !check_optional_key_info(rc, info.result, osize)? {
            return Ok(false);
        }
        if info.key_info.data_size > 0 && info.key_info.data_size <= 32 {
            Ok(true)
        } else {
            Err(FanCtlError::Smc(-1))
        }
    }

    #[cfg(target_arch = "aarch64")]
    fn auto_probe(&self, idx: u8) -> Result<AutoProbe, FanCtlError> {
        classify_auto_probe(
            |key| self.optional_key_present(key),
            || {
                let mut mode = [0u8; 1];
                if self.read_key(self.mode_key(idx), &mut mode)? != mode.len() {
                    return Err(FanCtlError::Smc(-1));
                }
                Ok(mode[0])
            },
        )
    }

    /// Resolve the mode key for fan `idx`: uppercase `FnMd` on M1–M4, lowercase
    /// `Fnmd` on M5. Falls back to uppercase if neither probe succeeds.
    fn mode_key(&self, idx: u8) -> u32 {
        let upper = fan_key(idx, *b"Md");
        if self.key_exists(upper) {
            return upper;
        }
        let lower = fan_key(idx, *b"md");
        if self.key_exists(lower) {
            return lower;
        }
        upper
    }

    /// Read a one-byte mode key; `true` if it currently reads `1` (manual).
    fn mode_is_manual(&self, mode_key: u32) -> bool {
        let mut b = [0u8; 1];
        self.read_key(mode_key, &mut b).is_ok() && b[0] == 1
    }

    /// Force fan `idx` to `rpm`. Enables manual mode — using the `Ftst` unlock
    /// sequence if a direct mode write doesn't stick (Apple Silicon) — then sets
    /// the target speed. Called every tick by the daemon to re-assert; the slow
    /// unlock/poll path only runs while manual mode isn't yet established.
    pub fn force(&self, idx: u8, rpm: f32) -> Result<(), FanCtlError> {
        set_force_bit(self, idx, true); // Intel `FS! ` bitmask; no-op where absent
        let mode_key = self.mode_key(idx);

        // 1) Direct attempt (sufficient on M1/M5 and on Intel).
        let _ = self.write_key(mode_key, &[1u8]);

        // 2) If it didn't stick, unlock via Ftst and poll the mode key until it
        //    holds (thermalmonitord reverts it for the first few seconds). The
        //    settle delay + poll window match the known-working reference
        //    implementation (Ftst settle ~0.5s, retry up to ~10s). Done at most
        //    once per connection so the daemon doesn't poll 10s every tick on
        //    firmware that ignores manual control.
        use std::sync::atomic::Ordering;
        if !self.mode_is_manual(mode_key) && !self.unlock_tried.swap(true, Ordering::Relaxed) {
            let _ = self.write_key(ftst_key(), &[1u8]);
            std::thread::sleep(std::time::Duration::from_millis(500));
            for _ in 0..100 {
                let _ = self.write_key(mode_key, &[1u8]);
                if self.mode_is_manual(mode_key) {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        }

        // 3) Set the target RPM (little-endian `flt` on Apple Silicon).
        self.write_key(fan_key(idx, *b"Tg"), &rpm.to_le_bytes())
    }

    /// Return fan `idx` to automatic (OS-managed) control and hand the thermal
    /// servo back by clearing the `Ftst` unlock.
    pub fn auto(&self, idx: u8) -> Result<(), FanCtlError> {
        #[cfg(target_arch = "aarch64")]
        {
            let result = safe_auto_handoff(|| self.auto_probe(idx), || self.auto_legacy(idx));
            // As on the legacy path, permit a new unlock attempt after auto.
            self.unlock_tried
                .store(false, std::sync::atomic::Ordering::Relaxed);
            result
        }
        #[cfg(not(target_arch = "aarch64"))]
        {
            self.auto_legacy(idx)
        }
    }

    fn auto_legacy(&self, idx: u8) -> Result<(), FanCtlError> {
        set_force_bit(self, idx, false);
        let r = self.write_key(self.mode_key(idx), &[0u8]);
        let _ = self.write_key(ftst_key(), &[0u8]); // best-effort; absent on M5
                                                    // Allow a fresh unlock attempt next time control is forced.
        self.unlock_tried
            .store(false, std::sync::atomic::Ordering::Relaxed);
        r
    }
}

#[cfg(test)]
mod response_tests {
    use super::*;

    #[test]
    fn automatic_handoff_skips_confirmed_released_mode_without_weakening_fallback() {
        use std::cell::Cell;
        let writes = Cell::new(0);
        safe_auto_handoff(
            || Ok(AutoProbe::Released),
            || {
                writes.set(writes.get() + 1);
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(
            writes.get(),
            0,
            "confirmed released mode must not be rewritten"
        );
        safe_auto_handoff(
            || Ok(AutoProbe::Legacy),
            || {
                writes.set(writes.get() + 1);
                Ok(())
            },
        )
        .unwrap();
        assert!(safe_auto_handoff(
            || Err(FanCtlError::Smc(-1)),
            || {
                writes.set(writes.get() + 1);
                Ok(())
            },
        )
        .is_err());
        assert_eq!(
            writes.get(),
            2,
            "unsupported or unreadable states must keep the legacy recovery attempt"
        );
    }

    #[test]
    fn automatic_handoff_verifies_manual_mode_was_released() {
        let mut probes = [Ok(AutoProbe::Manual), Ok(AutoProbe::Released)].into_iter();
        safe_auto_handoff(|| probes.next().unwrap(), || Ok(())).unwrap();
        let mut probes = [Ok(AutoProbe::Manual), Ok(AutoProbe::Manual)].into_iter();
        assert!(safe_auto_handoff(|| probes.next().unwrap(), || Ok(())).is_err());
        let mut probes = [Ok(AutoProbe::Manual), Err(FanCtlError::Smc(-1))].into_iter();
        assert!(safe_auto_handoff(|| probes.next().unwrap(), || Ok(())).is_err());
        assert!(
            safe_auto_handoff(|| Ok(AutoProbe::Manual), || Err(FanCtlError::NotPrivileged))
                .is_err()
        );
    }

    #[test]
    fn auto_probe_error_attempts_restore_then_retries_until_readback_succeeds() {
        use std::cell::Cell;
        let writes = Cell::new(0);
        let mut probes = [
            Err(FanCtlError::Smc(-1)),
            Ok(AutoProbe::Manual),
            Ok(AutoProbe::Released),
        ]
        .into_iter();
        let mut probe = || probes.next().unwrap();
        let mut restore = || {
            writes.set(writes.get() + 1);
            Ok(())
        };
        assert!(safe_auto_handoff(&mut probe, &mut restore).is_err());
        assert_eq!(writes.get(), 1);
        safe_auto_handoff(&mut probe, &mut restore).unwrap();
        assert_eq!(writes.get(), 2);
    }

    #[test]
    fn auto_probe_only_elides_writes_when_global_keys_are_absent_and_mode_is_known() {
        use std::cell::Cell;
        for raw_mode in [0, 3] {
            assert!(matches!(
                classify_auto_probe(|_| Ok(false), || Ok(raw_mode)),
                Ok(AutoProbe::Released)
            ));
        }
        assert!(matches!(
            classify_auto_probe(|_| Ok(false), || Ok(1)),
            Ok(AutoProbe::Manual)
        ));
        assert!(classify_auto_probe(|_| Ok(false), || Ok(2)).is_err());
        assert!(classify_auto_probe(|_| Ok(false), || Err(FanCtlError::Smc(-1))).is_err());
        for present_key in [ftst_key(), fs_key()] {
            assert!(matches!(
                classify_auto_probe(
                    |key| Ok(key == present_key),
                    || panic!("global-key path must remain legacy")
                ),
                Ok(AutoProbe::Legacy)
            ));
        }
        let reads = Cell::new(0);
        assert!(classify_auto_probe(
            |_| {
                reads.set(reads.get() + 1);
                if reads.get() == 2 {
                    Err(FanCtlError::Smc(-1))
                } else {
                    Ok(false)
                }
            },
            || Ok(0)
        )
        .is_err());
    }

    #[test]
    fn firmware_error_is_not_treated_as_a_successful_read_or_write() {
        // For missing keys, real hardware can return kern_success while the
        // SMC reports an error in KeyData.result. The same response parser is
        // used for key-info, reads, and writes.
        assert!(matches!(
            check_smc_response(KERN_SUCCESS, 132, size_of::<KeyData>()),
            Err(FanCtlError::Smc(132))
        ));
        assert!(matches!(
            check_smc_response(KERN_SUCCESS, 1, size_of::<KeyData>()),
            Err(FanCtlError::Smc(1))
        ));
    }

    #[test]
    fn optional_key_absence_requires_a_complete_successful_transport_response() {
        assert!(matches!(
            check_optional_key_info(KERN_SUCCESS, 132, size_of::<KeyData>()),
            Ok(false)
        ));
        assert!(check_optional_key_info(132, 132, size_of::<KeyData>()).is_err());
        assert!(check_optional_key_info(KERN_SUCCESS, 132, size_of::<KeyData>() - 1).is_err());
        assert!(check_optional_key_info(KERN_SUCCESS, 1, size_of::<KeyData>()).is_err());
        assert!(matches!(
            check_optional_key_info(KERN_SUCCESS, 0, size_of::<KeyData>()),
            Ok(true)
        ));
    }

    #[test]
    fn transport_failures_and_truncated_replies_do_not_claim_success() {
        assert!(matches!(
            check_smc_response(RETURN_NOT_PRIVILEGED, 0, size_of::<KeyData>()),
            Err(FanCtlError::NotPrivileged)
        ));
        assert!(matches!(
            check_smc_response(-42, 0, size_of::<KeyData>()),
            Err(FanCtlError::Smc(-42))
        ));
        assert!(matches!(
            check_smc_response(KERN_SUCCESS, 0, size_of::<KeyData>() - 1),
            Err(FanCtlError::Smc(_))
        ));
        assert!(check_smc_response(KERN_SUCCESS, 0, size_of::<KeyData>()).is_ok());
    }
}
