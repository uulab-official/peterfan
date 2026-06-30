//! Config file location and IO (the OS-specific half of configuration).
//!
//! The [`Config`](peterfan_core::config::Config) type lives in the core; here we
//! resolve its path (`~/.config/peterfan/config.toml` and the platform
//! equivalents, via `dirs`) and read/write it.

use std::io;
use std::path::PathBuf;

use peterfan_core::config::Config;

/// The config file path, e.g. `~/.config/peterfan/config.toml`.
pub fn path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("peterfan").join("config.toml"))
}

/// Load the config, falling back to defaults if it's missing or unparseable.
pub fn load() -> Config {
    path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| Config::from_toml(&s).ok())
        .unwrap_or_default()
}

/// Create the config file with defaults if it doesn't exist; return its path.
pub fn init_default() -> io::Result<PathBuf> {
    let p = path().ok_or_else(|| io::Error::other("no config directory"))?;
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir)?;
    }
    if !p.exists() {
        std::fs::write(&p, Config::default().to_toml())?;
    }
    Ok(p)
}

/// Write `cfg` back to the config file, creating it if needed.
pub fn save(cfg: &Config) -> io::Result<PathBuf> {
    let p = path().ok_or_else(|| io::Error::other("no config directory"))?;
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&p, cfg.to_toml())?;
    Ok(p)
}
