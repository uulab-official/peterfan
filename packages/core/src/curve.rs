//! Fan curves: a mapping from temperature to fan duty cycle.
//!
//! A curve is a sorted list of `(temperature, duty%)` control points. The duty
//! at an arbitrary temperature is found by **linear interpolation** between the
//! two surrounding points, and is **clamped** to the first/last point outside
//! the defined range. This is the same model the GUI's drag-to-edit curve
//! editor produces.

use serde::{Deserialize, Serialize};

use crate::error::{CoreError, Result};

/// One control point on a [`FanCurve`].
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CurvePoint {
    pub temp_c: f32,
    /// Duty cycle `0..=100`.
    pub duty_percent: u8,
}

impl CurvePoint {
    pub fn new(temp_c: f32, duty_percent: u8) -> Self {
        Self {
            temp_c,
            duty_percent: duty_percent.min(100),
        }
    }
}

/// A validated, sorted fan curve.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FanCurve {
    points: Vec<CurvePoint>,
}

impl FanCurve {
    /// Build a curve from control points.
    ///
    /// Points are sorted by temperature and duty cycles are clamped to
    /// `0..=100`. A curve needs at least two points to interpolate between.
    pub fn new(mut points: Vec<CurvePoint>) -> Result<Self> {
        if points.len() < 2 {
            return Err(CoreError::InvalidCurve(
                "a curve needs at least 2 points".into(),
            ));
        }
        points.sort_by(|a, b| {
            a.temp_c
                .partial_cmp(&b.temp_c)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        for p in &mut points {
            p.duty_percent = p.duty_percent.min(100);
        }
        Ok(Self { points })
    }

    /// The control points, sorted ascending by temperature.
    pub fn points(&self) -> &[CurvePoint] {
        &self.points
    }

    /// The duty cycle this curve prescribes at `temp_c`.
    ///
    /// Linear interpolation between control points; clamped to the endpoints
    /// outside the defined temperature range.
    pub fn duty_at(&self, temp_c: f32) -> u8 {
        let pts = &self.points;
        let first = pts[0];
        let last = pts[pts.len() - 1];

        if temp_c <= first.temp_c {
            return first.duty_percent;
        }
        if temp_c >= last.temp_c {
            return last.duty_percent;
        }

        for w in pts.windows(2) {
            let (a, b) = (w[0], w[1]);
            if temp_c >= a.temp_c && temp_c <= b.temp_c {
                let span = b.temp_c - a.temp_c;
                if span <= f32::EPSILON {
                    return b.duty_percent;
                }
                let t = (temp_c - a.temp_c) / span;
                let duty =
                    a.duty_percent as f32 + t * (b.duty_percent as f32 - a.duty_percent as f32);
                return duty.round().clamp(0.0, 100.0) as u8;
            }
        }
        last.duty_percent
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn curve() -> FanCurve {
        FanCurve::new(vec![
            CurvePoint::new(20.0, 20),
            CurvePoint::new(40.0, 35),
            CurvePoint::new(60.0, 60),
            CurvePoint::new(80.0, 100),
        ])
        .unwrap()
    }

    #[test]
    fn clamps_below_and_above_range() {
        let c = curve();
        assert_eq!(c.duty_at(0.0), 20);
        assert_eq!(c.duty_at(20.0), 20);
        assert_eq!(c.duty_at(80.0), 100);
        assert_eq!(c.duty_at(120.0), 100);
    }

    #[test]
    fn interpolates_between_points() {
        let c = curve();
        // halfway between (40,35) and (60,60) → 47.5 → 48
        assert_eq!(c.duty_at(50.0), 48);
    }

    #[test]
    fn unsorted_input_is_sorted() {
        let c = FanCurve::new(vec![CurvePoint::new(80.0, 100), CurvePoint::new(20.0, 20)]).unwrap();
        assert_eq!(c.points()[0].temp_c, 20.0);
    }

    #[test]
    fn rejects_too_few_points() {
        assert!(FanCurve::new(vec![CurvePoint::new(20.0, 20)]).is_err());
    }
}
