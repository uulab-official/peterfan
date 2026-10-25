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

/// Native menu-bar notification preferences.
///
/// The daemon's critical-temperature safety notification is intentionally
/// separate and always enabled. These preferences control earlier,
/// user-facing warnings from the unprivileged app.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct NotificationConfig {
    /// CPU Core Average threshold. `None` keeps the optional early warning off.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature_c: Option<f32>,
    /// Notify when fan write/readback failure counters increase.
    pub fan_failures: bool,
    /// Notify when the silent launch check discovers a newer signed release.
    pub updates: bool,
}

impl Default for NotificationConfig {
    fn default() -> Self {
        Self {
            temperature_c: None,
            fan_failures: true,
            updates: true,
        }
    }
}

impl NotificationConfig {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

/// Which GitHub release stream the app should check for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum UpdateChannel {
    #[default]
    Stable,
    PreRelease,
}

impl UpdateChannel {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "stable" | "release" | "releases" => Some(Self::Stable),
            "pre-release" | "prerelease" | "preview" | "beta" => Some(Self::PreRelease),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Stable => "stable",
            Self::PreRelease => "pre-release",
        }
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

/// Appearance of the independent CPU menu-bar widget.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CpuWidgetStyle {
    #[default]
    Bar,
    History,
    Number,
    Cores,
}

impl CpuWidgetStyle {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "bar" => Some(Self::Bar),
            "history" => Some(Self::History),
            "number" => Some(Self::Number),
            "cores" => Some(Self::Cores),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Bar => "bar",
            Self::History => "history",
            Self::Number => "number",
            Self::Cores => "cores",
        }
    }
}

/// A separately enabled CPU item; existing installations keep their layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CpuWidgetConfig {
    pub enabled: bool,
    pub style: CpuWidgetStyle,
    pub show_percentage: bool,
}

impl Default for CpuWidgetConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            style: CpuWidgetStyle::Bar,
            show_percentage: true,
        }
    }
}

impl CpuWidgetConfig {
    pub fn select_style(mut self, style: CpuWidgetStyle) -> Self {
        if style == CpuWidgetStyle::Cores && self.style != style {
            self.show_percentage = false;
        }
        self.style = style;
        self
    }

    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

/// Animated character used by the CPU-responsive menu-bar runner.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RunnerCharacter {
    #[default]
    Cat,
    Dog,
    Rabbit,
    Fox,
    Penguin,
    Dinosaur,
    Robot,
    Ghost,
    Bird,
    Duck,
    Sheep,
    Fish,
    Slime,
    Human,
    Pushup,
    Orbit,
    Sushi,
    City,
    Sausage,
    Party,
}

impl RunnerCharacter {
    /// Authored, quality-checked production picker roster.
    pub const FEATURED: [Self; 20] = Self::ALL;

    /// Every value understood by the persisted configuration format.
    pub const ALL: [Self; 20] = [
        Self::Cat,
        Self::Dog,
        Self::Rabbit,
        Self::Fox,
        Self::Penguin,
        Self::Dinosaur,
        Self::Robot,
        Self::Ghost,
        Self::Bird,
        Self::Duck,
        Self::Sheep,
        Self::Fish,
        Self::Slime,
        Self::Human,
        Self::Pushup,
        Self::Orbit,
        Self::Sushi,
        Self::City,
        Self::Sausage,
        Self::Party,
    ];

    pub fn is_featured(self) -> bool {
        Self::FEATURED.contains(&self)
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "cat" => Some(Self::Cat),
            "dog" => Some(Self::Dog),
            "rabbit" | "bunny" => Some(Self::Rabbit),
            "fox" => Some(Self::Fox),
            "penguin" => Some(Self::Penguin),
            "dinosaur" | "dino" => Some(Self::Dinosaur),
            "robot" | "bot" => Some(Self::Robot),
            "ghost" => Some(Self::Ghost),
            "bird" => Some(Self::Bird),
            "duck" => Some(Self::Duck),
            "sheep" => Some(Self::Sheep),
            "fish" => Some(Self::Fish),
            "slime" => Some(Self::Slime),
            "human" | "runner" => Some(Self::Human),
            "pushup" | "push-up" => Some(Self::Pushup),
            "orbit" | "reactor" => Some(Self::Orbit),
            "sushi" => Some(Self::Sushi),
            "city" => Some(Self::City),
            "sausage" | "hotdog" => Some(Self::Sausage),
            "party" => Some(Self::Party),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Cat => "cat",
            Self::Dog => "dog",
            Self::Rabbit => "rabbit",
            Self::Fox => "fox",
            Self::Penguin => "penguin",
            Self::Dinosaur => "dinosaur",
            Self::Robot => "robot",
            Self::Ghost => "ghost",
            Self::Bird => "bird",
            Self::Duck => "duck",
            Self::Sheep => "sheep",
            Self::Fish => "fish",
            Self::Slime => "slime",
            Self::Human => "human",
            Self::Pushup => "pushup",
            Self::Orbit => "orbit",
            Self::Sushi => "sushi",
            Self::City => "city",
            Self::Sausage => "sausage",
            Self::Party => "party",
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
fn default_show_cpu_core_graph() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct MenubarConfig {
    pub metric: MenubarMetric,
    pub display: MenubarDisplay,
    pub character: RunnerCharacter,
    /// Rotate through a shuffled character roster once per minute.
    pub random_character: bool,
    pub temperature_source: TemperatureSource,
    pub update_channel: UpdateChannel,
    /// Show the compact per-core CPU activity graph in the overview header.
    #[serde(default = "default_show_cpu_core_graph")]
    pub show_cpu_core_graph: bool,
    pub cpu_widget: CpuWidgetConfig,
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

impl Default for MenubarConfig {
    fn default() -> Self {
        Self {
            metric: MenubarMetric::default(),
            display: MenubarDisplay::default(),
            character: RunnerCharacter::default(),
            random_character: false,
            temperature_source: TemperatureSource::default(),
            update_channel: UpdateChannel::default(),
            show_cpu_core_graph: default_show_cpu_core_graph(),
            cpu_widget: CpuWidgetConfig::default(),
            setup_prompt_dismissed: false,
            daemon_update_prompt_dismissed_for: None,
            daemon_update_prompt_snoozed_until_unix: None,
            language: Language::default(),
        }
    }
}

impl MenubarConfig {
    pub fn is_default(&self) -> bool {
        self.metric == MenubarMetric::Temp
            && self.display == MenubarDisplay::Both
            && self.character == RunnerCharacter::Cat
            && !self.random_character
            && self.temperature_source == TemperatureSource::CoreAverage
            && self.update_channel == UpdateChannel::Stable
            && self.show_cpu_core_graph
            && self.cpu_widget.is_default()
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
    /// Native menu-bar notifications.
    #[serde(default, skip_serializing_if = "NotificationConfig::is_default")]
    pub notifications: NotificationConfig,
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
            notifications: NotificationConfig::default(),
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
            Some(("cpu_above", v)) => {
                let value = v.trim().parse::<f32>().ok()?;
                (value.is_finite() && (0.0..=120.0).contains(&value))
                    .then_some(Condition::CpuAbove(value))
            }
            Some(("time", v)) => {
                let (a, b) = v.split_once('-')?;
                let start = a.trim().parse().ok()?;
                let end = b.trim().parse().ok()?;
                (start < 24 && end < 24).then_some(Condition::Time(start, end))
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
    /// Validate the subset of settings that can affect the privileged fan
    /// control loop. The daemon calls this before accepting a user's config,
    /// so a malformed file cannot disable sensible polling or thermal limits.
    pub fn validate_for_control(&self) -> Result<(), String> {
        if !(1..=60).contains(&self.interval_secs) {
            return Err("interval_secs must be between 1 and 60".to_string());
        }
        if !self.critical_temp_c.is_finite() || !(70.0..=110.0).contains(&self.critical_temp_c) {
            return Err("critical_temp_c must be between 70 and 110".to_string());
        }
        if self
            .custom_curve
            .as_ref()
            .is_some_and(|curve| curve.to_fan_curve().is_none())
        {
            return Err("custom_curve is invalid".to_string());
        }
        if let Some((name, _)) = self
            .named_curves
            .iter()
            .find(|(_, curve)| curve.to_fan_curve().is_none())
        {
            return Err(format!("named curve '{name}' is invalid"));
        }
        if let Some(rule) = self.rules.iter().find(|rule| rule.condition().is_none()) {
            return Err(format!("automation rule '{}' is invalid", rule.when));
        }
        if self.rules.len() > 32 {
            return Err("automation rules cannot contain more than 32 entries".to_string());
        }
        Ok(())
    }

    /// Parse a config from TOML text.
    pub fn from_toml(s: &str) -> Result<Self, String> {
        let mut value: toml::Value = toml::from_str(s).map_err(|e| e.to_string())?;
        if let Some(widget) = value
            .get_mut("menubar")
            .and_then(|v| v.get_mut("cpu_widget"))
            .and_then(toml::Value::as_table_mut)
        {
            if let Some(flag) = widget.get("per_core_bars") {
                let enabled = flag
                    .as_bool()
                    .ok_or("menubar.cpu_widget.per_core_bars must be a boolean")?;
                if enabled
                    && widget
                        .get("style")
                        .is_none_or(|v| v.as_str() == Some("bar"))
                {
                    widget.insert("style".into(), toml::Value::String("cores".into()));
                }
            }
        }
        value.try_into().map_err(|e: toml::de::Error| e.to_string())
    }

    /// Render this config as pretty TOML (with a header comment).
    pub fn to_toml(&self) -> String {
        let body = if self.menubar.cpu_widget.style == CpuWidgetStyle::Cores {
            // Helpers from v1.32.8 deserialize the full Config. Keep their
            // known enum value on disk and use an additive UI-only flag; JSON
            // settings requests/payloads continue to use the `cores` mode.
            let mut value = toml::Value::try_from(self).expect("valid configuration serializes");
            let widget = value["menubar"]["cpu_widget"]
                .as_table_mut()
                .expect("non-default CPU widget is serialized");
            widget.insert("style".into(), toml::Value::String("bar".into()));
            widget.insert("per_core_bars".into(), toml::Value::Boolean(true));
            toml::to_string_pretty(&value).unwrap_or_default()
        } else {
            toml::to_string_pretty(self).unwrap_or_default()
        };
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
    fn privileged_control_config_rejects_unsafe_values() {
        let mut cfg = Config::default();
        assert!(cfg.validate_for_control().is_ok());

        cfg.interval_secs = 0;
        assert!(cfg.validate_for_control().is_err());
        cfg.interval_secs = 2;
        cfg.critical_temp_c = 150.0;
        assert!(cfg.validate_for_control().is_err());
        cfg.critical_temp_c = 90.0;
        cfg.rules.push(Rule {
            when: "sometimes".to_string(),
            profile: Profile::Silent,
        });
        assert!(cfg.validate_for_control().is_err());
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
    fn notification_config_roundtrips_and_defaults_to_safety_signals() {
        let cfg = Config::default();
        assert!(!cfg.to_toml().contains("[notifications]"));
        assert!(cfg.notifications.fan_failures);
        assert!(cfg.notifications.updates);
        assert_eq!(cfg.notifications.temperature_c, None);

        let mut configured = cfg;
        configured.notifications.temperature_c = Some(82.0);
        configured.notifications.updates = false;
        let toml = configured.to_toml();
        assert!(toml.contains("[notifications]"));
        let back = Config::from_toml(&toml).unwrap();
        assert_eq!(back.notifications.temperature_c, Some(82.0));
        assert!(back.notifications.fan_failures);
        assert!(!back.notifications.updates);
    }

    #[test]
    fn cpu_widget_preferences_survive_legacy_partial_and_full_roundtrip() {
        let legacy = Config::from_toml("[menubar]\ncharacter = \"fox\"\n").unwrap();
        let value = serde_json::to_value(&legacy.menubar).unwrap();
        assert_eq!(value["cpu_widget"]["enabled"], false);
        assert_eq!(value["cpu_widget"]["style"], "bar");
        assert_eq!(value["cpu_widget"]["show_percentage"], true);
        for style in ["bar", "history", "number", "cores"] {
            let original = format!("interval_secs = 7\n[menubar]\ncharacter = \"fox\"\n[menubar.cpu_widget]\nenabled = true\nstyle = \"{style}\"\nshow_percentage = false\n");
            let cfg = Config::from_toml(&original).unwrap();
            let saved = cfg.to_toml();
            assert!(saved.contains("[menubar.cpu_widget]"));
            let restored = Config::from_toml(&saved).unwrap();
            let value = serde_json::to_value(&restored.menubar).unwrap();
            assert_eq!(value["cpu_widget"]["enabled"], true);
            assert_eq!(value["cpu_widget"]["style"], style);
            assert_eq!(value["cpu_widget"]["show_percentage"], false);
            assert_eq!(restored.interval_secs, 7);
            assert_eq!(restored.menubar.character, RunnerCharacter::Fox);
        }
        assert!(!Config::default().to_toml().contains("[menubar]"));
        let partial = Config::from_toml("[menubar.cpu_widget]\nenabled = true\n").unwrap();
        assert_eq!(
            serde_json::to_value(partial.menubar).unwrap()["cpu_widget"]["style"],
            "bar"
        );
    }

    #[test]
    fn cpu_widget_core_mode_hides_number_only_when_entering() {
        let config = CpuWidgetConfig::default().select_style(CpuWidgetStyle::Cores);
        assert_eq!(CpuWidgetStyle::parse("cores"), Some(CpuWidgetStyle::Cores));
        assert_eq!(CpuWidgetStyle::Cores.as_str(), "cores");
        assert!(!config.show_percentage);
        let with_number = CpuWidgetConfig {
            show_percentage: true,
            ..config
        };
        assert!(
            with_number
                .select_style(CpuWidgetStyle::Cores)
                .show_percentage
        );
        assert!(
            with_number
                .select_style(CpuWidgetStyle::History)
                .show_percentage
        );
    }

    #[test]
    fn cpu_widget_core_mode_disk_format_remains_readable_by_v1328_helpers() {
        // Frozen widget enum/schema from the immutable v1.32.8 release. Older
        // helpers deserialize the entire user Config, including this UI block.
        #[derive(Debug, Deserialize, PartialEq)]
        #[serde(rename_all = "lowercase")]
        enum LegacyStyle {
            Bar,
            History,
            Number,
        }
        #[derive(Deserialize)]
        struct LegacyWidget {
            enabled: bool,
            style: LegacyStyle,
            show_percentage: bool,
        }
        for enabled in [false, true] {
            for show_percentage in [false, true] {
                let config = Config {
                    interval_secs: 7,
                    profile: Profile::Gaming,
                    menubar: MenubarConfig {
                        cpu_widget: CpuWidgetConfig {
                            enabled,
                            style: CpuWidgetStyle::Cores,
                            show_percentage,
                        },
                        ..MenubarConfig::default()
                    },
                    ..Config::default()
                };
                let stored = config.to_toml();
                let value: toml::Value = toml::from_str(&stored).unwrap();
                let widget = value["menubar"]["cpu_widget"].clone();
                let old: LegacyWidget = widget
                    .try_into()
                    .expect("existing helpers must still parse UI preferences");
                assert_eq!(old.style, LegacyStyle::Bar);
                assert_eq!(old.enabled, enabled);
                assert_eq!(old.show_percentage, show_percentage);
                let restored = Config::from_toml(&stored).unwrap();
                assert_eq!(restored.menubar.cpu_widget.style, CpuWidgetStyle::Cores);
                assert_eq!(restored.interval_secs, 7);
                assert_eq!(restored.profile, Profile::Gaming);
                assert_eq!(
                    serde_json::to_value(restored.menubar.cpu_widget).unwrap()["style"],
                    "cores"
                );
            }
        }
        assert!(Config::from_toml(
            "[menubar.cpu_widget]\nstyle=\"bar\"\nper_core_bars=\"invalid\"\n"
        )
        .is_err());
        assert_eq!(
            Config::from_toml("[menubar.cpu_widget]\nstyle=\"history\"\nper_core_bars=true\n")
                .unwrap()
                .menubar
                .cpu_widget
                .style,
            CpuWidgetStyle::History
        );
    }

    #[test]
    fn menubar_config_roundtrips_and_omits_when_default() {
        let mut cfg = Config::default();
        assert!(!cfg.to_toml().contains("[menubar]"));
        assert!(cfg.menubar.show_cpu_core_graph);

        cfg.menubar.metric = MenubarMetric::Network;
        cfg.menubar.display = MenubarDisplay::Graph;
        cfg.menubar.character = RunnerCharacter::Fox;
        cfg.menubar.temperature_source = TemperatureSource::IohidTdie;
        cfg.menubar.show_cpu_core_graph = false;
        let toml = cfg.to_toml();
        assert!(toml.contains("[menubar]"));
        assert!(toml.contains("display = \"cat\""));
        assert!(toml.contains("character = \"fox\""));
        assert!(toml.contains("temperature_source = \"iohid-tdie\""));
        let back = Config::from_toml(&toml).unwrap();
        assert_eq!(back.menubar.metric, MenubarMetric::Network);
        assert_eq!(back.menubar.display, MenubarDisplay::Graph);
        assert_eq!(back.menubar.character, RunnerCharacter::Fox);
        assert_eq!(
            back.menubar.temperature_source,
            TemperatureSource::IohidTdie
        );
        assert!(!back.menubar.show_cpu_core_graph);

        let legacy = Config::from_toml("[menubar]\ndisplay = \"graph\"\n").unwrap();
        assert_eq!(legacy.menubar.display, MenubarDisplay::Graph);
        assert_eq!(legacy.menubar.character, RunnerCharacter::Cat);
        assert_eq!(
            legacy.menubar.temperature_source,
            TemperatureSource::CoreAverage
        );
        assert!(legacy.menubar.show_cpu_core_graph);
    }

    #[test]
    fn random_runner_preference_survives_roundtrip_without_changing_fixed_character() {
        let config =
            Config::from_toml("[menubar]\ncharacter = \"fox\"\nrandom_character = true\n").unwrap();
        let encoded = config.to_toml();
        assert!(encoded.contains("random_character = true"));
        let restored = Config::from_toml(&encoded).unwrap();
        assert_eq!(restored.menubar.character, RunnerCharacter::Fox);
        assert!(restored.to_toml().contains("random_character = true"));
        let legacy = Config::from_toml("[menubar]\ncharacter = \"pushup\"\n").unwrap();
        assert!(!legacy.to_toml().contains("random_character = true"));
    }

    #[test]
    fn menubar_display_accepts_cat_alias_for_runner_icon() {
        assert_eq!(MenubarDisplay::parse("cat"), Some(MenubarDisplay::Graph));
        assert_eq!(MenubarDisplay::parse("runner"), Some(MenubarDisplay::Graph));
        assert_eq!(MenubarDisplay::parse("graph"), Some(MenubarDisplay::Graph));
        assert_eq!(MenubarDisplay::Graph.as_str(), "cat");
    }

    #[test]
    fn update_channel_parses_and_roundtrips() {
        assert_eq!(UpdateChannel::parse("stable"), Some(UpdateChannel::Stable));
        assert_eq!(
            UpdateChannel::parse("preview"),
            Some(UpdateChannel::PreRelease)
        );
        assert_eq!(UpdateChannel::Stable.as_str(), "stable");
        assert_eq!(UpdateChannel::PreRelease.as_str(), "pre-release");

        let mut cfg = Config::default();
        cfg.menubar.update_channel = UpdateChannel::PreRelease;
        let toml = cfg.to_toml();
        assert!(toml.contains("update_channel = \"pre-release\""));
        let back = Config::from_toml(&toml).unwrap();
        assert_eq!(back.menubar.update_channel, UpdateChannel::PreRelease);
    }

    #[test]
    fn runner_character_parses_supported_choices() {
        assert_eq!(
            RunnerCharacter::ALL.map(|character| character.as_str()),
            [
                "cat", "dog", "rabbit", "fox", "penguin", "dinosaur", "robot", "ghost", "bird",
                "duck", "sheep", "fish", "slime", "human", "pushup", "orbit", "sushi", "city",
                "sausage", "party"
            ]
        );
        assert_eq!(
            RunnerCharacter::parse("bunny"),
            Some(RunnerCharacter::Rabbit)
        );
        assert_eq!(
            RunnerCharacter::parse("dino"),
            Some(RunnerCharacter::Dinosaur)
        );
        assert_eq!(RunnerCharacter::parse("bot"), Some(RunnerCharacter::Robot));
        assert_eq!(
            RunnerCharacter::parse("push-up"),
            Some(RunnerCharacter::Pushup)
        );
        assert_eq!(
            RunnerCharacter::parse("hotdog"),
            Some(RunnerCharacter::Sausage)
        );
        assert_eq!(RunnerCharacter::parse("unknown"), None);
        for character in RunnerCharacter::ALL {
            assert_eq!(RunnerCharacter::parse(character.as_str()), Some(character));
        }
    }

    #[test]
    fn every_authored_character_is_selectable_and_round_trips() {
        assert_eq!(RunnerCharacter::FEATURED.len(), 20);
        for character in RunnerCharacter::ALL {
            assert!(character.is_featured(), "missing {}", character.as_str());
            assert_eq!(RunnerCharacter::parse(character.as_str()), Some(character));
        }
        let names = RunnerCharacter::FEATURED.map(|c| c.as_str());
        assert_eq!(
            names
                .into_iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            20
        );
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
        assert_eq!(Condition::parse("cpu_above:NaN"), None);
        assert_eq!(Condition::parse("cpu_above:121"), None);
        assert_eq!(Condition::parse("time:24-7"), None);
        assert_eq!(Condition::parse("time:22-24"), None);
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
