//! # peterfan-core
//!
//! The OS-agnostic heart of PeterFan. This crate knows **nothing** about
//! Windows, macOS, SMC keys, or Embedded Controllers. It only knows about:
//!
//! - domain [`types`] (temperatures, fans, hardware info),
//! - fan [`curve`]s and their interpolation,
//! - [`profile`]s (Silent / Balanced / Gaming / …),
//! - and the single [`provider::HardwareProvider`] trait that platform
//!   backends implement.
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

pub mod curve;
pub mod error;
pub mod profile;
pub mod provider;
pub mod types;

pub use error::{CoreError, Result};
pub use provider::{Capabilities, HardwareProvider};
