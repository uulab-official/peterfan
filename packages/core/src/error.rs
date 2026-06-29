//! Error types shared across PeterFan.

use thiserror::Error;

/// Errors returned by the core and by platform backends.
#[derive(Debug, Error)]
pub enum CoreError {
    /// A hardware read/write failed at the OS or driver level.
    #[error("hardware access failed: {0}")]
    Hardware(String),

    /// The requested operation is not implemented on this platform yet.
    ///
    /// This is a *normal*, expected outcome (e.g. fan control on a backend
    /// that is read-only) — not a bug. Callers should handle it gracefully.
    #[error("not supported on this platform: {0}")]
    Unsupported(String),

    /// The operation requires elevated privileges the process does not have.
    #[error("permission denied: {0} (try running with elevated privileges)")]
    PermissionDenied(String),

    /// A fan curve failed validation.
    #[error("invalid fan curve: {0}")]
    InvalidCurve(String),

    /// A sensor or fan id was not found.
    #[error("not found: {0}")]
    NotFound(String),
}

/// Convenience alias used throughout the workspace.
pub type Result<T> = std::result::Result<T, CoreError>;
