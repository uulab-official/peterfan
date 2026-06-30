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
    u32::from_be_bytes([b'F', b'S', b'!', b' '])
}

/// The `Ftst` diagnostic/force-test unlock key (Apple Silicon).
fn ftst_key() -> u32 {
    u32::from_be_bytes([b'F', b't', b's', b't'])
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
pub struct Conn(MachPort);

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
            Ok(Conn(conn))
        }
    }

    fn call(&self, input: &KeyData, output: &mut KeyData) -> Result<(), FanCtlError> {
        let mut osize = size_of::<KeyData>();
        let rc = unsafe {
            IOConnectCallStructMethod(
                self.0,
                KERNEL_INDEX_SMC,
                input as *const _ as *const c_void,
                size_of::<KeyData>(),
                output as *mut _ as *mut c_void,
                &mut osize,
            )
        };
        match rc {
            KERN_SUCCESS => Ok(()),
            RETURN_NOT_PRIVILEGED => Err(FanCtlError::NotPrivileged),
            other => Err(FanCtlError::Smc(other)),
        }
    }

    /// Read up to `out.len()` bytes of `key` into `out`; returns the key size.
    fn read_key(&self, key: u32, out: &mut [u8]) -> Result<usize, FanCtlError> {
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
        Ok(size)
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

impl Drop for Conn {
    fn drop(&mut self) {
        unsafe {
            IOServiceClose(self.0);
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

    /// Resolve the mode key for fan `idx`: uppercase `FnMd` on M1–M4, lowercase
    /// `Fnmd` on M5. Falls back to uppercase if neither probe succeeds.
    fn mode_key(&self, idx: u8) -> u32 {
        let upper = fan_key(idx, [b'M', b'd']);
        if self.key_exists(upper) {
            return upper;
        }
        let lower = fan_key(idx, [b'm', b'd']);
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
        //    holds (thermalmonitord reverts it for the first few seconds).
        if !self.mode_is_manual(mode_key) {
            let _ = self.write_key(ftst_key(), &[1u8]);
            for _ in 0..40 {
                let _ = self.write_key(mode_key, &[1u8]);
                if self.mode_is_manual(mode_key) {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        }

        // 3) Set the target RPM (little-endian `flt` on Apple Silicon).
        self.write_key(fan_key(idx, [b'T', b'g']), &rpm.to_le_bytes())
    }

    /// Return fan `idx` to automatic (OS-managed) control and hand the thermal
    /// servo back by clearing the `Ftst` unlock.
    pub fn auto(&self, idx: u8) -> Result<(), FanCtlError> {
        set_force_bit(self, idx, false);
        let r = self.write_key(self.mode_key(idx), &[0u8]);
        let _ = self.write_key(ftst_key(), &[0u8]); // best-effort; absent on M5
        r
    }
}
