//! User configuration (TOML) — pure data + (de)serialization.
//!
//! The [`Config`] type and its TOML parsing live here in the OS-agnostic core.
//! Resolving the file path and reading/writing it is the platform layer's job
//! (`peterfan_platform::config`), so `core` stays free of filesystem and
//! OS-specific path logic.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::curve::{CurvePoint, FanCurve};
use crate::profile::Profile;

/// A single control point as stored in config TOML: `[temp_c, duty_percent]`.
type RawPoint = [f32; 2];

/// A user-defined fan curve stored in the config file.
///
/// ```toml
/// [custom_curve]
/// points = [[30, 20], [60, 50], [80, 90], [90, 100]]
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomCurveConfig {
    pub points: Vec<RawPoint>,
}

impl CustomCurveConfig {
    /// Convert raw points into a validated [`FanCurve`], if valid.
    pub fn to_fan_curve(&self) -> Option<FanCurve> {
        let pts: Vec<CurvePoint> = self
            .points
            .iter()
            .map(|&[t, d]| CurvePoint::new(t, d.min(100.0) as u8))
            .collect();
        FanCurve::new(pts).ok()
    }
}

/// Named user-defined curves — each key is the curve name, usable as a profile
/// name in rules (e.g. `profile = "work"`).
///
/// ```toml
/// [named_curves.work]
/// points = [[30, 20], [55, 40], [75, 80], [88, 100]]
/// ```
pub type NamedCurves = BTreeMap<String, CustomCurveConfig>;

/// Threshold settings for `peterfan alert` — stored in the `[alert]` config section.
///
/// ```toml
/// [alert]
/// cpu_pct   = 85.0   # alert when CPU usage % exceeds this
/// memory_pct = 90.0  # alert when memory usage % exceeds this
/// temp_c    = 88.0   # alert when hottest sensor °C exceeds this
/// cooldown_secs = 300
/// interval_secs = 10
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AlertConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpu_pct: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_pct: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temp_c: Option<f32>,
    pub cooldown_secs: u64,
    pub interval_secs: u64,
}

impl Default for AlertConfig {
    fn default() -> Self {
        Self {
            cpu_pct: None,
            memory_pct: None,
            temp_c: None,
            cooldown_secs: 300,
            interval_secs: 10,
        }
    }
}

impl AlertConfig {
    /// True only when every field (including `cooldown_secs`/`interval_secs`)
    /// still matches the default — checking thresholds alone would drop a
    /// user's custom cooldown/interval on save just because no threshold was
    /// ever set.
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// Which live metric the menu-bar item shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MenubarMetric {
    Cpu,
    Memory,
    #[default]
    Temp,
    Fan,
    Network,
}

impl MenubarMetric {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "cpu" => Some(Self::Cpu),
            "memory" | "mem" => Some(Self::Memory),
            "temp" | "temperature" => Some(Self::Temp),
            "fan" | "rpm" => Some(Self::Fan),
            "network" | "net" => Some(Self::Network),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Cpu => "cpu",
            Self::Memory => "memory",
            Self::Temp => "temp",
            Self::Fan => "fan",
            Self::Network => "network",
        }
    }
}

/// How the menu-bar item renders the chosen metric.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MenubarDisplay {
    /// Text only, e.g. "42%" — the lightest, most compact look.
    Number,
    /// Animated RunCat-style character icon only, no text.
    #[serde(rename = "cat", alias = "graph")]
    Graph,
    /// Animated character icon plus text.
    #[default]
    Both,
}

impl MenubarDisplay {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "number" | "text" => Some(Self::Number),
            "cat" | "runner" | "graph" => Some(Self::Graph),
            "both" => Some(Self::Both),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Number => "number",
            Self::Graph => "cat",
            Self::Both => "both",
        }
    }
}

/// UI language for the menu-bar app (native menu labels + popover text).
/// `System` resolves from the `LANG`/`LC_ALL` environment variable at
/// startup; the explicit variants are a user override, persisted so it
/// survives a relaunch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    #[default]
    System,
    English,
    Korean,
}

impl Language {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "system" => Some(Self::System),
            "english" | "en" => Some(Self::English),
            "korean" | "ko" | "한국어" => Some(Self::Korean),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::System => "system",
            Self::English => "english",
            Self::Korean => "korean",
        }
    }

    /// Resolve `System` against the environment; explicit choices pass through.
    pub fn resolve(self) -> ResolvedLanguage {
        match self {
            Self::English => ResolvedLanguage::En,
            Self::Korean => ResolvedLanguage::Ko,
            Self::System => {
                let env_lang = std::env::var("LANG").unwrap_or_default();
                if env_lang.to_ascii_lowercase().starts_with("ko") {
                    ResolvedLanguage::Ko
                } else {
                    ResolvedLanguage::En
                }
            }
        }
    }
}

/// The concrete language to render, after resolving [`Language::System`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolvedLanguage {
    En,
    Ko,
}

/// Which CPU temperature family the menu-bar headline should show.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TemperatureSource {
    /// Live Apple Silicon CPU Core Average (`cpu.die`).
    #[default]
    CoreAverage,
    /// IOHID PMU tdie average, often close to iStat-style die readings.
    IohidTdie,
    /// SMC `TCDX` summary.
    SmcSummary,
    /// SMC `TV*` aggregate, matching some Macs Fan Control views.
    SmcAggregate,
    /// Hottest CPU-related reading.
    Hottest,
}

impl TemperatureSource {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "core" | "core-average" | "cpu" | "cpu-average" | "all-core" | "all-cores"
            | "p-core" | "pcore" => Some(Self::CoreAverage),
            "iohid" | "iohid-tdie" | "tdie" => Some(Self::IohidTdie),
            "summary" | "smc-summary" | "tcdx" => Some(Self::SmcSummary),
            "aggregate" | "smc-aggregate" | "tv" | "tv-aggregate" | "tv*" => {
                Some(Self::SmcAggregate)
            }
            "hot" | "hottest" | "hotspot" => Some(Self::Hottest),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::CoreAverage => "core-average",
            Self::IohidTdie => "iohid-tdie",
            Self::SmcSummary => "smc-summary",
            Self::SmcAggregate => "smc-aggregate",
            Self::Hottest => "hottest",
        }
    }
}

/// Menu-bar item appearance, persisted so it survives a relaunch.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct MenubarConfig {
    pub metric: MenubarMetric,
    pub display: MenubarDisplay,
    pub temperature_source: TemperatureSource,
    /// User picked "Don't Ask Again" on the first-run fan-control setup
    /// prompt — stop offering it automatically (still reachable via the
    /// right-click menu).
    pub setup_prompt_dismissed: bool,
    /// App version for which the user dismissed the stale-daemon update
    /// prompt. A future app version can ask again because the bundled helper
    /// has changed.
    pub daemon_update_prompt_dismissed_for: Option<String>,
    /// Unix timestamp before which "Not Now" suppresses the automatic stale
    /// daemon prompt. The manual Setup button remains available.
    pub daemon_update_prompt_snoozed_until_unix: Option<u64>,
    pub language: Language,
}

impl MenubarConfig {
    pub fn is_default(&self) -> bool {
        self.metric == MenubarMetric::Temp
            && self.display == MenubarDisplay::Both
            && self.temperature_source == TemperatureSource::CoreAverage
            && !self.setup_prompt_dismissed
            && self.daemon_update_prompt_dismissed_for.is_none()
            && self.daemon_update_prompt_snoozed_until_unix.is_none()
            && self.language == Language::System
    }
}

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
    /// User-defined curve for `profile = "custom"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub custom_curve: Option<CustomCurveConfig>,
    /// Named user-defined curves; keys are valid profile names in rules.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub named_curves: NamedCurves,
    /// Alert thresholds for `peterfan alert`.
    #[serde(default, skip_serializing_if = "AlertConfig::is_empty")]
    pub alert: AlertConfig,
    /// Menu-bar item appearance (metric shown + number/cat/both style).
    #[serde(default, skip_serializing_if = "MenubarConfig::is_default")]
    pub menubar: MenubarConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            profile: Profile::Balanced,
            interval_secs: 2,
            critical_temp_c: 90.0,
            rules: Vec::new(),
            custom_curve: None,
            named_curves: BTreeMap::new(),
            alert: AlertConfig::default(),
            menubar: MenubarConfig::default(),
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

    /// Resolve a fan curve for `profile`, using the config's `custom_curve`
    /// when the profile is `Custom` and a user curve is defined.
    /// Falls back to the profile's built-in default curve.
    pub fn curve_for(&self, profile: Profile) -> FanCurve {
        if profile == Profile::Custom {
            if let Some(cc) = &self.custom_curve {
                if let Some(curve) = cc.to_fan_curve() {
                    return curve;
                }
            }
        }
        profile.default_curve()
    }

    /// Look up a named custom curve by name, for use in rules or display.
    /// Returns `None` if no such curve is defined.
    pub fn named_curve(&self, name: &str) -> Option<FanCurve> {
        self.named_curves.get(name)?.to_fan_curve()
    }

    /// Check whether a string is a valid profile reference — either a built-in
    /// profile name or a named custom curve in this config.
    pub fn is_valid_profile_ref(&self, name: &str) -> bool {
        Profile::parse(name).is_some() || self.named_curves.contains_key(name)
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
    fn legacy_license_section_is_ignored_and_removed_on_save() {
        let cfg = Config::from_toml(
            "profile = \"balanced\"\n[license]\nkey = \"PFAN1-old\"\nfirst_run_unix = 1\n",
        )
        .unwrap();

        let saved = cfg.to_toml();
        assert!(!saved.contains("[license]"));
        assert!(!saved.contains("PFAN1-old"));
    }

    #[test]
    fn alert_config_with_only_cooldown_customized_is_not_empty() {
        // A user who sets only cooldown/interval (no threshold) must not
        // have the whole [alert] section silently dropped on save.
        let alert = AlertConfig {
            cooldown_secs: 600,
            ..Default::default()
        };
        assert!(!alert.is_empty());

        let cfg = Config {
            alert,
            ..Default::default()
        };
        let toml = cfg.to_toml();
        assert!(toml.contains("[alert]"));
        let back = Config::from_toml(&toml).unwrap();
        assert_eq!(back.alert.cooldown_secs, 600);
    }

    #[test]
    fn menubar_config_roundtrips_and_omits_when_default() {
        let mut cfg = Config::default();
        assert!(!cfg.to_toml().contains("[menubar]"));

        cfg.menubar.metric = MenubarMetric::Network;
        cfg.menubar.display = MenubarDisplay::Graph;
        cfg.menubar.temperature_source = TemperatureSource::IohidTdie;
        let toml = cfg.to_toml();
        assert!(toml.contains("[menubar]"));
        assert!(toml.contains("display = \"cat\""));
        assert!(toml.contains("temperature_source = \"iohid-tdie\""));
        let back = Config::from_toml(&toml).unwrap();
        assert_eq!(back.menubar.metric, MenubarMetric::Network);
        assert_eq!(back.menubar.display, MenubarDisplay::Graph);
        assert_eq!(
            back.menubar.temperature_source,
            TemperatureSource::IohidTdie
        );

        let legacy = Config::from_toml("[menubar]\ndisplay = \"graph\"\n").unwrap();
        assert_eq!(legacy.menubar.display, MenubarDisplay::Graph);
        assert_eq!(
            legacy.menubar.temperature_source,
            TemperatureSource::CoreAverage
        );
    }

    #[test]
    fn menubar_display_accepts_cat_alias_for_runner_icon() {
        assert_eq!(MenubarDisplay::parse("cat"), Some(MenubarDisplay::Graph));
        assert_eq!(MenubarDisplay::parse("runner"), Some(MenubarDisplay::Graph));
        assert_eq!(MenubarDisplay::parse("graph"), Some(MenubarDisplay::Graph));
        assert_eq!(MenubarDisplay::Graph.as_str(), "cat");
    }

    #[test]
    fn language_resolves_explicit_choices_and_system_env() {
        assert_eq!(Language::English.resolve(), ResolvedLanguage::En);
        assert_eq!(Language::Korean.resolve(), ResolvedLanguage::Ko);
        // `System` depends on $LANG — just check it doesn't panic and picks
        // one of the two, since CI's locale is out of our control.
        let resolved = Language::System.resolve();
        assert!(resolved == ResolvedLanguage::En || resolved == ResolvedLanguage::Ko);
    }

    #[test]
    fn language_roundtrips_through_config_toml() {
        let mut cfg = Config::default();
        cfg.menubar.language = Language::Korean;
        let toml = cfg.to_toml();
        assert!(toml.contains("[menubar]"));
        let back = Config::from_toml(&toml).unwrap();
        assert_eq!(back.menubar.language, Language::Korean);
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
