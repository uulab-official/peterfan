//! Shared temperature selection rules for UI, logging, and automation.

use crate::types::{SensorKind, TempSensor};

/// Hottest reading across every available sensor.
///
/// Use this for safety decisions such as critical-temperature overrides.
pub fn hottest_temperature_c(temps: &[TempSensor]) -> Option<f32> {
    temps
        .iter()
        .map(|t| t.value.0)
        .max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
}

/// Human-facing representative temperature.
///
/// PeterFan publishes `cpu.die` as the calibrated CPU headline temperature
/// selected by the platform backend. Prefer that value for status/menu/log
/// output so Apple Silicon machines use the same representative temperature
/// everywhere.
pub fn representative_temperature_c(temps: &[TempSensor]) -> Option<f32> {
    temps
        .iter()
        .find(|t| t.id == "cpu.die")
        .map(|t| t.value.0)
        .or_else(|| {
            let cpu_values: Vec<f32> = temps
                .iter()
                .filter(|t| t.kind == SensorKind::Cpu && !t.id.contains("hot"))
                .map(|t| t.value.0)
                .collect();
            (!cpu_values.is_empty())
                .then(|| cpu_values.iter().sum::<f32>() / cpu_values.len() as f32)
        })
        .or_else(|| hottest_temperature_c(temps))
}

#[cfg(test)]
mod tests {
    use crate::types::{Celsius, SensorKind, TempSensor};

    fn temp(id: &str, kind: SensorKind, value: f32) -> TempSensor {
        TempSensor {
            id: id.to_string(),
            label: id.to_string(),
            kind,
            value: Celsius(value),
        }
    }

    #[test]
    fn representative_temperature_prefers_cpu_average_over_hottest_sensor() {
        let temps = vec![
            temp("cpu.die", SensorKind::Cpu, 52.0),
            temp("cpu.die.hot", SensorKind::Cpu, 67.0),
            temp("ssd", SensorKind::Storage, 71.0),
        ];

        assert_eq!(super::representative_temperature_c(&temps), Some(52.0));
    }

    #[test]
    fn representative_temperature_averages_raw_cpu_sensors_when_no_synthetic_average_exists() {
        let temps = vec![
            temp("cpu.core.1", SensorKind::Cpu, 40.0),
            temp("cpu.core.2", SensorKind::Cpu, 60.0),
            temp("cpu.die.hot", SensorKind::Cpu, 70.0),
            temp("ssd", SensorKind::Storage, 80.0),
        ];

        assert_eq!(super::representative_temperature_c(&temps), Some(50.0));
    }

    #[test]
    fn representative_temperature_falls_back_to_hottest_without_cpu_average() {
        let temps = vec![
            temp("battery", SensorKind::Battery, 33.0),
            temp("airport", SensorKind::Other, 45.0),
        ];

        assert_eq!(super::representative_temperature_c(&temps), Some(45.0));
    }

    #[test]
    fn hottest_temperature_ignores_representative_preference() {
        let temps = vec![
            temp("cpu.die", SensorKind::Cpu, 52.0),
            temp("cpu.die.hot", SensorKind::Cpu, 67.0),
            temp("ssd", SensorKind::Storage, 71.0),
        ];

        assert_eq!(super::hottest_temperature_c(&temps), Some(71.0));
    }
}
