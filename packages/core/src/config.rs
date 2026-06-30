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
    /// Automation rules, evaluated in order by the daemon (first match wins).
    pub rules: Vec<Rule>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            profile: Profile::Balanced,
            interval_secs: 2,
            critical_temp_c: 90.0,
            rules: Vec::new(),
        }
    }
}

/// An automation rule: when `when` holds, switch to `profile`.
///
/// In TOML:
/// ```toml
/// [[rules]]
/// when = "on_battery"
/// profile = "silent"
/// [[rules]]
/// when = "cpu_above:85"
/// profile = "maximum"
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rule {
    pub when: String,
    pub profile: Profile,
}

impl Rule {
    /// Parse the `when` string into a [`Condition`], if valid.
    pub fn condition(&self) -> Option<Condition> {
        Condition::parse(&self.when)
    }
}

/// A condition an automation [`Rule`] tests against the live [`RuleContext`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Condition {
    /// Running on battery power.
    OnBattery,
    /// Running on AC / wall power (or no battery).
    OnAc,
    /// Hottest CPU temperature is at or above this many °C.
    CpuAbove(f32),
    /// Local hour is within `[start, end)` (wraps past midnight if start > end).
    Time(u8, u8),
}

impl Condition {
    /// Parse `"on_battery"`, `"on_ac"`, `"cpu_above:85"`, or `"time:22-7"`.
    pub fn parse(s: &str) -> Option<Condition> {
        let s = s.trim().to_ascii_lowercase();
        match s.split_once(':') {
            None => match s.as_str() {
                "on_battery" => Some(Condition::OnBattery),
                "on_ac" => Some(Condition::OnAc),
                _ => None,
            },
            Some(("cpu_above", v)) => v.trim().parse::<f32>().ok().map(Condition::CpuAbove),
            Some(("time", v)) => {
                let (a, b) = v.split_once('-')?;
                Some(Condition::Time(
                    a.trim().parse().ok()?,
                    b.trim().parse().ok()?,
                ))
            }
            _ => None,
        }
    }

    /// Whether this condition holds for the given live context.
    pub fn matches(&self, ctx: &RuleContext) -> bool {
        match *self {
            Condition::OnBattery => !ctx.on_ac,
            Condition::OnAc => ctx.on_ac,
            Condition::CpuAbove(c) => ctx.cpu_temp_c >= c,
            Condition::Time(start, end) => {
                if start <= end {
                    ctx.hour >= start && ctx.hour < end
                } else {
                    ctx.hour >= start || ctx.hour < end
                }
            }
        }
    }
}

/// Live inputs the daemon evaluates rules against.
#[derive(Debug, Clone, Copy)]
pub struct RuleContext {
    pub on_ac: bool,
    pub cpu_temp_c: f32,
    pub hour: u8,
}

impl Config {
    /// Parse a config from TOML text.
    pub fn from_toml(s: &str) -> Result<Self, String> {
        toml::from_str(s).map_err(|e| e.to_string())
    }

    /// Render this config as pretty TOML (with a header comment).
    pub fn to_toml(&self) -> String {
        let body = toml::to_string_pretty(self).unwrap_or_default();
        let example = if self.rules.is_empty() {
            "\n# Automation rules (evaluated in order; first match wins). Examples:\n\
             # [[rules]]\n# when = \"cpu_above:85\"   # on_battery | on_ac | cpu_above:N | time:22-7\n\
             # profile = \"maximum\"\n"
        } else {
            ""
        };
        format!(
            "# PeterFan configuration\n# https://github.com/uulab-official/peterfan\n\n{body}{example}"
        )
    }

    /// The profile selected by the first matching automation rule, if any.
    pub fn active_profile(&self, ctx: &RuleContext) -> Option<Profile> {
        self.rules
            .iter()
            .find(|r| r.condition().is_some_and(|c| c.matches(ctx)))
            .map(|r| r.profile)
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
        assert!(cfg.rules.is_empty());
        let back = Config::from_toml(&cfg.to_toml()).unwrap();
        assert_eq!(back.profile, Profile::Gaming);
    }

    #[test]
    fn condition_parsing_and_matching() {
        let ctx = RuleContext {
            on_ac: false,
            cpu_temp_c: 70.0,
            hour: 23,
        };
        assert!(Condition::parse("on_battery").unwrap().matches(&ctx));
        assert!(!Condition::parse("on_ac").unwrap().matches(&ctx));
        assert!(Condition::parse("cpu_above:65").unwrap().matches(&ctx));
        assert!(!Condition::parse("cpu_above:80").unwrap().matches(&ctx));
        // night wraps past midnight
        assert!(Condition::parse("time:22-7").unwrap().matches(&ctx));
        assert_eq!(Condition::parse("bogus"), None);
    }

    #[test]
    fn rules_pick_first_match() {
        let toml = r#"
            [[rules]]
            when = "cpu_above:85"
            profile = "maximum"
            [[rules]]
            when = "on_battery"
            profile = "silent"
        "#;
        let cfg = Config::from_toml(toml).unwrap();
        let cool_battery = RuleContext {
            on_ac: false,
            cpu_temp_c: 40.0,
            hour: 12,
        };
        assert_eq!(cfg.active_profile(&cool_battery), Some(Profile::Silent));
        let hot = RuleContext {
            on_ac: true,
            cpu_temp_c: 90.0,
            hour: 12,
        };
        assert_eq!(cfg.active_profile(&hot), Some(Profile::Maximum));
        let plugged_cool = RuleContext {
            on_ac: true,
            cpu_temp_c: 40.0,
            hour: 12,
        };
        assert_eq!(cfg.active_profile(&plugged_cool), None);
    }
}
