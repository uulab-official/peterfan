//! Fan profiles: named presets that map to a default [`FanCurve`].
//!
//! Profiles are the user-facing knob (`peterfan profile gaming`). Each built-in
//! profile resolves to a curve; `Custom` is a placeholder for a user-defined
//! curve loaded from config.

use serde::{Deserialize, Serialize};

use crate::curve::{CurvePoint, FanCurve};

/// A named fan behavior preset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Profile {
    Silent,
    Balanced,
    Gaming,
    Performance,
    Maximum,
    Custom,
}

impl Profile {
    /// The selectable built-in profiles, in increasing aggressiveness.
    ///
    /// `Custom` is excluded because it has no built-in curve of its own.
    pub fn all() -> &'static [Profile] {
        &[
            Profile::Silent,
            Profile::Balanced,
            Profile::Gaming,
            Profile::Performance,
            Profile::Maximum,
        ]
    }

    /// Canonical lowercase name, e.g. `"balanced"`.
    pub fn as_str(&self) -> &'static str {
        match self {
            Profile::Silent => "silent",
            Profile::Balanced => "balanced",
            Profile::Gaming => "gaming",
            Profile::Performance => "performance",
            Profile::Maximum => "maximum",
            Profile::Custom => "custom",
        }
    }

    /// Parse a profile name, case-insensitively.
    pub fn parse(name: &str) -> Option<Profile> {
        match name.trim().to_ascii_lowercase().as_str() {
            "silent" => Some(Profile::Silent),
            "balanced" => Some(Profile::Balanced),
            "gaming" => Some(Profile::Gaming),
            "performance" | "perf" => Some(Profile::Performance),
            "maximum" | "max" => Some(Profile::Maximum),
            "custom" => Some(Profile::Custom),
            _ => None,
        }
    }

    /// One-line description of the profile's intent.
    pub fn description(&self) -> &'static str {
        match self {
            Profile::Silent => "Quietest. Fans stay low; accepts higher temps.",
            Profile::Balanced => "Sensible default trade-off between noise and cooling.",
            Profile::Gaming => "Ramps earlier to keep sustained loads cool.",
            Profile::Performance => "Aggressive cooling for heavy workloads.",
            Profile::Maximum => "Fans pinned to 100%. Loud but coolest.",
            Profile::Custom => "User-defined curve loaded from config.",
        }
    }

    /// The built-in fan curve for this profile.
    ///
    /// `Custom` returns the same curve as `Balanced` as a safe fallback until a
    /// user curve is loaded from config.
    pub fn default_curve(&self) -> FanCurve {
        let pts = match self {
            Profile::Silent => vec![(30.0, 0), (50.0, 20), (70.0, 40), (85.0, 70)],
            Profile::Balanced | Profile::Custom => {
                vec![(30.0, 20), (50.0, 35), (70.0, 60), (85.0, 100)]
            }
            Profile::Gaming => vec![(30.0, 30), (50.0, 50), (70.0, 75), (85.0, 100)],
            Profile::Performance => vec![(30.0, 40), (50.0, 60), (75.0, 90), (85.0, 100)],
            Profile::Maximum => vec![(0.0, 100), (100.0, 100)],
        };
        // The point sets above are authored valid (>= 2 points), so this
        // unwrap cannot fire; expressed as expect() to document the invariant.
        FanCurve::new(
            pts.into_iter()
                .map(|(t, d)| CurvePoint::new(t, d))
                .collect(),
        )
        .expect("built-in profile curves are always valid")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_is_case_insensitive_with_aliases() {
        assert_eq!(Profile::parse("GAMING"), Some(Profile::Gaming));
        assert_eq!(Profile::parse("max"), Some(Profile::Maximum));
        assert_eq!(Profile::parse("perf"), Some(Profile::Performance));
        assert_eq!(Profile::parse("nope"), None);
    }

    #[test]
    fn every_builtin_profile_has_a_valid_curve() {
        for p in Profile::all() {
            // Must not panic and must be monotonic-ish at the extremes.
            let c = p.default_curve();
            assert!(c.duty_at(95.0) >= c.duty_at(25.0));
        }
    }

    #[test]
    fn maximum_is_always_full() {
        assert_eq!(Profile::Maximum.default_curve().duty_at(10.0), 100);
    }
}
