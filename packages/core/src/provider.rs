//! The [`HardwareProvider`] trait: the single seam between PeterFan's
//! OS-agnostic core and the actual hardware.
//!
//! ```text
//!   core  ──depends on──▶  HardwareProvider  ◀──implements──  platform backends
//! ```
//!
//! Everything above this trait (CLI, TUI, GUI, API) is portable. Everything
//! below it (SMC on macOS, EC on Windows, sysfs on Linux later) is swappable.

use crate::error::Result;
use crate::types::{Fan, HardwareInfo, Snapshot, TempSensor};

/// What a backend can actually do on the current machine.
///
/// Backends advertise capabilities up front so the UI can disable controls and
/// show honest status (e.g. "monitoring only") instead of failing on use.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Capabilities {
    /// Can read temperature sensors.
    pub read_temps: bool,
    /// Can read fan speeds.
    pub read_fans: bool,
    /// Can set fan duty cycles. **Write access — privileged & risky.**
    pub control_fans: bool,
}

/// A source of hardware readings (and, optionally, fan control).
///
/// Implementations must be cheap to call repeatedly: the TUI and GUI poll
/// `temperatures()`/`fans()` on a timer.
///
/// ## Safety contract for control
///
/// `set_fan_duty` writes to hardware. Implementations MUST refuse to set a
/// duty that would be unsafe for the specific fan, and the higher layers are
/// responsible for restoring OS-default control on crash/exit (see the daemon).
pub trait HardwareProvider: Send + Sync {
    /// Short backend name for diagnostics, e.g. `"macos"`, `"mock"`.
    fn name(&self) -> &str;

    /// What this backend can do on this machine right now.
    fn capabilities(&self) -> Capabilities;

    /// Static machine description (CPU, RAM, OS, …).
    fn hardware_info(&self) -> Result<HardwareInfo>;

    /// Current temperature readings.
    fn temperatures(&self) -> Result<Vec<TempSensor>>;

    /// Current fan states.
    fn fans(&self) -> Result<Vec<Fan>>;

    /// A combined snapshot. Default implementation reads temps then fans.
    fn snapshot(&self) -> Result<Snapshot> {
        Ok(Snapshot {
            temps: self.temperatures()?,
            fans: self.fans()?,
        })
    }

    /// Drive a fan to `duty_percent` (`0..=100`), forcing manual control.
    ///
    /// Defaults to [`CoreError::Unsupported`](crate::error::CoreError::Unsupported)
    /// so read-only backends get correct behavior for free. Forced control
    /// persists until [`set_fan_auto`](Self::set_fan_auto) is called.
    fn set_fan_duty(&self, _fan_id: &str, _duty_percent: u8) -> Result<()> {
        Err(crate::error::CoreError::Unsupported("fan control".into()))
    }

    /// Return a fan to automatic (OS-managed) control.
    fn set_fan_auto(&self, _fan_id: &str) -> Result<()> {
        Err(crate::error::CoreError::Unsupported("fan control".into()))
    }
}
