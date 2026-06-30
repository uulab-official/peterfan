//! # peterfan-core
//!
//! The OS-agnostic heart of PeterFan. This crate knows **nothing** about
//! Windows, macOS, SMC keys, or Embedded Controllers. It only knows about:
//!
//! - domain [`types`] (temperatures, fans, hardware info),
//! - system [`metrics`] (CPU, memory, disk, network, processes, battery),
//! - fan [`curve`]s and their interpolation,
//! - [`profile`]s (Silent / Balanced / Gaming / …),
//! - and the two backend seams: [`provider::HardwareProvider`] (thermal
//!   hardware) and [`monitor::SystemMonitor`] (system metrics).
//!
//! The dependency direction is strictly one-way:
//!
//! ```text
//! cli / tui / gui  →  core  →  HardwareProvider  ←  platform backends
//! ```
//!
//! Nothing in `core` may `use` a platform crate. This is what keeps the
//! architecture portable: adding Linux later means adding one backend, not
//! touching the core.

pub mod config;
pub mod curve;
pub mod error;
pub mod metrics;
pub mod monitor;
pub mod profile;
pub mod provider;
pub mod types;

pub use error::{CoreError, Result};
pub use monitor::{MonitorCapabilities, SystemMonitor};
pub use provider::{Capabilities, HardwareProvider};
