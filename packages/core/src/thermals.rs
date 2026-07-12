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

/// Hottest trustworthy reading for critical fan-control decisions.
///
/// `cpu.die.hot` is the platform backend's mapped CPU-core maximum. When it
/// exists, diagnostic CPU hotspot/aggregate feeds must not override it: some
/// Apple Silicon SMC hotspot keys remain near 100 °C while the actual cores
/// are cool. Non-CPU components are still considered so a genuinely hot GPU,
/// SSD, or board sensor can trigger protection.
pub fn safety_temperature_c(temps: &[TempSensor]) -> Option<f32> {
    fn max_value<'a>(sensors: impl Iterator<Item = &'a TempSensor>) -> Option<f32> {
        sensors
            .map(|sensor| sensor.value.0)
            .filter(|value| value.is_finite())
            .max_by(f32::total_cmp)
    }

    let cpu = temps
        .iter()
        .find(|sensor| sensor.id == "cpu.die.hot" && sensor.value.0.is_finite())
        .map(|sensor| sensor.value.0)
        .or_else(|| {
            max_value(
                temps
                    .iter()
                    .filter(|sensor| sensor.kind == SensorKind::Cpu)
                    .filter(|sensor| !sensor.id.contains("hotspot")),
            )
        });
    let non_cpu = max_value(temps.iter().filter(|sensor| sensor.kind != SensorKind::Cpu));

    cpu.into_iter()
        .chain(non_cpu)
        .max_by(f32::total_cmp)
        .or_else(|| hottest_temperature_c(temps))
}

/// Human-facing representative temperature.
///
/// PeterFan publishes `cpu.die` as the calibrated CPU headline temperature
/// selected by the platform backend. Prefer that value for status/menu/log
/// output so Apple Silicon machines use the same representative temperature
/// everywhere.
pub fn representative_temperature_c(temps: &[TempSensor]) -> Option<f32> {
    fn is_valid_cpu_average_reading(sensor: &TempSensor) -> bool {
        if sensor.kind != SensorKind::Cpu || sensor.id.contains("hot") || sensor.value.0.is_nan() {
            return false;
        }

        if sensor.id.contains("proximity")
            || sensor.id.contains("airflow")
            || sensor.id.contains("ambient")
            || sensor.id.contains("board")
            || sensor.id.contains("memory")
        {
            return false;
        }
        true
    }

    if let Some(value) = temps
        .iter()
        .find(|t| t.id == "cpu.die" && is_valid_cpu_average_reading(t))
        .map(|t| t.value.0)
    {
        return Some(value);
    }

    let stable_average_values: Vec<f32> = temps
        .iter()
        .filter(|t| is_valid_cpu_average_reading(t))
        .filter(|t| {
            matches!(
                t.id.as_str(),
                "cpu.smc.die" | "cpu.smc.aggregate" | "cpu.smc.summary"
            )
        })
        .map(|t| t.value.0)
        .collect();

    if let Some(value) = stable_average_values
        .iter()
        .copied()
        .max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
    {
        return Some(value);
    }

    if let Some(value) = temps
        .iter()
        .find(|t| {
            matches!(t.id.as_str(), "cpu.iohid.tdie" | "cpu.iohid.cpu")
                && is_valid_cpu_average_reading(t)
        })
        .map(|t| t.value.0)
    {
        return Some(value);
    }

    let cpu_average_values: Vec<f32> = temps
        .iter()
        .filter(|t| is_valid_cpu_average_reading(t))
        .map(|t| t.value.0)
        .collect();

    if !cpu_average_values.is_empty() {
        return Some(cpu_average_values.iter().sum::<f32>() / cpu_average_values.len() as f32);
    }

    let cpu_values: Vec<f32> = temps
        .iter()
        .filter(|t| t.kind == SensorKind::Cpu && !t.id.contains("hot"))
        .map(|t| t.value.0)
        .collect();

    if !cpu_values.is_empty() {
        return Some(cpu_values.iter().sum::<f32>() / cpu_values.len() as f32);
    }

    if let Some(best) = temps
        .iter()
        .map(|t| t.value.0)
        .max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
    {
        return Some(best);
    }

    None
}

#[cfg(test)]
mod tests {
    use crate::types::{Celsius, SensorKind, SensorSource, TempSensor};

    fn temp(id: &str, kind: SensorKind, value: f32) -> TempSensor {
        TempSensor {
            id: id.to_string(),
            label: id.to_string(),
            kind,
            source: SensorSource::Unknown,
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
    fn representative_temperature_prefers_synthetic_cpu_die_before_other_candidates() {
        let temps = vec![
            temp("cpu.die", SensorKind::Cpu, 52.0),
            temp("cpu.smc.die", SensorKind::Cpu, 72.0),
            temp("cpu.iohid.tdie", SensorKind::Cpu, 74.0),
            temp("cpu.smc.summary", SensorKind::Cpu, 58.0),
            temp("cpu.die.hot", SensorKind::Cpu, 80.0),
            temp("ssd", SensorKind::Storage, 81.0),
        ];

        assert_eq!(super::representative_temperature_c(&temps), Some(52.0));
    }

    #[test]
    fn representative_temperature_uses_hottest_stable_cpu_average_without_synthetic_die() {
        let temps = vec![
            temp("cpu.smc.aggregate", SensorKind::Cpu, 72.0),
            temp("cpu.smc.summary", SensorKind::Cpu, 83.0),
            temp("cpu.iohid.tdie", SensorKind::Cpu, 50.0),
            temp("cpu.die.hot", SensorKind::Cpu, 101.0),
        ];

        assert_eq!(super::representative_temperature_c(&temps), Some(83.0));
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

    #[test]
    fn safety_temperature_prefers_mapped_core_hottest_over_diagnostic_hotspot() {
        let temps = vec![
            temp("cpu.die", SensorKind::Cpu, 65.0),
            temp("cpu.die.hot", SensorKind::Cpu, 69.0),
            temp("cpu.smc.aggregate", SensorKind::Cpu, 73.0),
            temp("cpu.smc.hotspot.hot", SensorKind::Cpu, 101.0),
            temp("ssd", SensorKind::Storage, 34.0),
        ];

        assert_eq!(super::safety_temperature_c(&temps), Some(69.0));
    }

    #[test]
    fn safety_temperature_still_protects_hot_non_cpu_components() {
        let temps = vec![
            temp("cpu.die.hot", SensorKind::Cpu, 69.0),
            temp("gpu.die", SensorKind::Gpu, 94.0),
        ];

        assert_eq!(super::safety_temperature_c(&temps), Some(94.0));
    }

    #[test]
    fn safety_temperature_falls_back_to_raw_cpu_without_mapped_core_hottest() {
        let temps = vec![
            temp("cpu.performance-core.1", SensorKind::Cpu, 72.0),
            temp("cpu.performance-core.2", SensorKind::Cpu, 76.0),
            temp("cpu.smc.hotspot.hot", SensorKind::Cpu, 101.0),
        ];

        assert_eq!(super::safety_temperature_c(&temps), Some(76.0));
    }
}
