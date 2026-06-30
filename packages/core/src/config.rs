//! User configuration (TOML) — pure data + (de)serialization.
//!
//! The [`Config`] type and its TOML parsing live here in the OS-agnostic core.
//! Resolving the file path and reading/writing it is the platform layer's job
//! (`peterfan_platform::config`), so `core` stays free of filesystem and
//! OS-specific path logic.

use serde::{Deserialize, Serialize};

use crate::profile::Profile;

/// PeterFan settings, with sensible defaults for every field. Missing fields in
/// a partial config file fall back to [`Config::default`].
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Default fan profile (used by the daemon and `profile` with no argument).
    pub profile: Profile,
    /// Refresh interval (seconds) for `--watch` and the daemon.
    pub interval_secs: u64,
    /// Temperature (°C) above which the daemon forces fans to 100%.
    pub critical_temp_c: f32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            profile: Profile::Balanced,
            interval_secs: 2,
            critical_temp_c: 90.0,
        }
    }
}

impl Config {
    /// Parse a config from TOML text.
    pub fn from_toml(s: &str) -> Result<Self, String> {
        toml::from_str(s).map_err(|e| e.to_string())
    }

    /// Render this config as pretty TOML (with a header comment).
    pub fn to_toml(&self) -> String {
        let body = toml::to_string_pretty(self).unwrap_or_default();
        format!("# PeterFan configuration\n# https://github.com/uulab-official/peterfan\n\n{body}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrips_and_fills_defaults() {
        let cfg = Config::from_toml("profile = \"gaming\"").unwrap();
        assert_eq!(cfg.profile, Profile::Gaming);
        assert_eq!(cfg.interval_secs, 2); // default filled
        let back = Config::from_toml(&cfg.to_toml()).unwrap();
        assert_eq!(back.profile, Profile::Gaming);
    }
}
