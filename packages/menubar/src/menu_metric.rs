//! Metric selection and formatting for the menu-bar title.
use peterfan_core::config::MenubarMetric;

#[derive(Clone, Copy, Debug)]
pub(crate) struct MetricSelection {
    pub(crate) active: MenubarMetric,
    persisted: MenubarMetric,
}

impl MetricSelection {
    pub(crate) fn new_for_launch(
        saved: MenubarMetric,
        argument: Option<&str>,
        service: Option<&str>,
        valid_config: bool,
    ) -> Result<Self, String> {
        // Validate even a superseded legacy LaunchAgent argument: silently
        // accepting malformed explicit input would hide a broken launch file.
        let mut selection = Self::new(saved, argument)?;
        if valid_config && service == Some("kr.co.uulab.peterfan.menubar") {
            selection.active = saved;
        }
        Ok(selection)
    }

    pub(crate) fn new(saved: MenubarMetric, argument: Option<&str>) -> Result<Self, String> {
        let active = match argument {
            Some(value) => MenubarMetric::parse(value)
                .ok_or_else(|| format!("unknown menu-bar metric: {value}"))?,
            None => saved,
        };
        Ok(Self {
            active,
            persisted: saved,
        })
    }

    pub(crate) fn reload(&mut self, saved: MenubarMetric) {
        if self.persisted != saved {
            self.persisted = saved;
            self.active = saved;
        }
    }

    pub(crate) fn reload_if_valid(&mut self, saved: Option<MenubarMetric>) {
        if let Some(saved) = saved {
            self.reload(saved);
        }
    }

    pub(crate) fn saved(&mut self, value: MenubarMetric) {
        self.active = value;
        self.persisted = value;
    }
}

/// Fixed widths avoid shifting adjacent menu items as sensor values change.
#[cfg(any(target_os = "macos", test))]
pub(crate) fn reserved_width(metric: MenubarMetric) -> f64 {
    match metric {
        MenubarMetric::Cpu | MenubarMetric::Memory => 50.0,
        MenubarMetric::Temp => 64.0,
        MenubarMetric::Fan | MenubarMetric::Network => 80.0,
    }
}

pub(crate) fn should_refresh_network(
    metric: MenubarMetric,
    visible: bool,
    slow_refreshed: bool,
    due: bool,
) -> bool {
    metric == MenubarMetric::Network && visible && !slow_refreshed && due
}

pub(crate) fn apply_valid_config<T>(config: Option<T>, apply: impl FnOnce(T)) {
    if let Some(config) = config {
        apply(config);
    }
}

pub(crate) fn last_good_config(
    cached: &mut peterfan_core::config::Config,
    candidate: Option<peterfan_core::config::Config>,
) -> peterfan_core::config::Config {
    if let Some(config) = candidate {
        *cached = config.clone();
        config
    } else {
        cached.clone()
    }
}

pub(crate) struct MetricReadings {
    pub(crate) cpu: Option<f32>,
    pub(crate) memory: Option<f32>,
    pub(crate) temperature: Option<f32>,
    pub(crate) fan_rpm: Option<u32>,
    pub(crate) network_bytes_per_sec: Option<f64>,
}

pub(crate) fn title(metric: MenubarMetric, readings: &MetricReadings) -> String {
    fn percentage(value: Option<f32>) -> String {
        value
            .filter(|n| n.is_finite() && (0.0..=100.0).contains(n))
            .map_or_else(|| " --%".to_string(), |n| format!("{n:>3.0}%"))
    }
    match metric {
        MenubarMetric::Cpu => percentage(readings.cpu),
        MenubarMetric::Memory => percentage(readings.memory),
        MenubarMetric::Temp => readings
            .temperature
            .filter(|n| n.is_finite() && *n > 0.0)
            .map_or_else(
                || " --°C".to_string(),
                |n| {
                    if n > 999.0 {
                        ">999°C".into()
                    } else {
                        format!("{n:>3.0}°C")
                    }
                },
            ),
        MenubarMetric::Fan => readings.fan_rpm.map_or_else(
            || " --rpm".to_string(),
            |rpm| {
                if rpm >= 100_000 {
                    ">99krpm".into()
                } else if rpm < 10_000 {
                    format!("{rpm:>4}rpm")
                } else {
                    format!("{:.0}krpm", rpm as f64 / 1000.0)
                }
            },
        ),
        MenubarMetric::Network => readings
            .network_bytes_per_sec
            .filter(|n| n.is_finite() && *n >= 0.0)
            .map_or_else(
                || " --/s".to_string(),
                |bytes| {
                    if bytes >= 1000.0 * 1024.0 * 1024.0 * 1024.0 {
                        return ">999G/s".into();
                    }
                    let (value, unit) = if bytes >= 1024.0 * 1024.0 * 1024.0 {
                        (bytes / (1024.0 * 1024.0 * 1024.0), "G")
                    } else if bytes >= 1024.0 * 1024.0 {
                        (bytes / (1024.0 * 1024.0), "M")
                    } else if bytes >= 1024.0 {
                        (bytes / 1024.0, "K")
                    } else {
                        (bytes, "B")
                    };
                    if value < 10.0 {
                        format!("{value:.1}{unit}/s")
                    } else {
                        format!("{value:.0}{unit}/s")
                    }
                },
            ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_are_bounded_and_metric_width_is_stable_across_sample_changes() {
        let metrics = [
            MenubarMetric::Cpu,
            MenubarMetric::Memory,
            MenubarMetric::Temp,
            MenubarMetric::Fan,
            MenubarMetric::Network,
        ];
        for metric in metrics {
            assert!(reserved_width(metric) >= 50.0);
        }
        let extreme = MetricReadings {
            cpu: Some(100.0),
            memory: Some(100.0),
            temperature: Some(f32::MAX),
            fan_rpm: Some(u32::MAX),
            network_bytes_per_sec: Some(f64::MAX),
        };
        assert_eq!(title(MenubarMetric::Temp, &extreme), ">999°C");
        assert_eq!(title(MenubarMetric::Fan, &extreme), ">99krpm");
        assert_eq!(title(MenubarMetric::Network, &extreme), ">999G/s");
    }

    #[test]
    fn unreadable_or_malformed_config_keeps_the_current_metric_until_valid_reload() {
        let mut selection = MetricSelection::new(MenubarMetric::Fan, None).unwrap();
        selection.reload_if_valid(None);
        assert_eq!(selection.active, MenubarMetric::Fan);
        selection.reload_if_valid(Some(MenubarMetric::Memory));
        assert_eq!(selection.active, MenubarMetric::Memory);
    }

    #[test]
    fn invalid_reload_preserves_all_live_preferences_until_a_valid_reload() {
        use peterfan_core::config::{Config, CpuWidgetStyle, UpdateChannel};
        let mut cached = Config::default();
        cached.menubar.metric = MenubarMetric::Fan;
        cached.menubar.primary_visible = false;
        cached.menubar.cpu_widget.style = CpuWidgetStyle::Cores;
        cached.menubar.update_channel = UpdateChannel::PreRelease;
        let invalid = Config::from_toml("[broken").ok();
        let mut applied = false;
        apply_valid_config(invalid.as_ref(), |_| applied = true);
        assert!(!applied);
        let kept = last_good_config(&mut cached, invalid);
        assert_eq!(kept.menubar.metric, MenubarMetric::Fan);
        assert!(!kept.menubar.primary_visible);
        assert_eq!(kept.menubar.cpu_widget.style, CpuWidgetStyle::Cores);
        assert_eq!(kept.menubar.update_channel, UpdateChannel::PreRelease);
        let replacement = Config::default();
        apply_valid_config(Some(&replacement), |_| applied = true);
        assert!(applied);
        let loaded = last_good_config(&mut cached, Some(replacement));
        assert_eq!(loaded.menubar.update_channel, UpdateChannel::Stable);
        assert_eq!(cached.menubar.update_channel, UpdateChannel::Stable);
    }

    #[test]
    fn network_sampling_only_runs_for_visible_network_without_a_slow_refresh() {
        assert!(should_refresh_network(
            MenubarMetric::Network,
            true,
            false,
            true
        ));
        assert!(!should_refresh_network(
            MenubarMetric::Network,
            false,
            false,
            true
        ));
        assert!(!should_refresh_network(
            MenubarMetric::Network,
            true,
            true,
            true
        ));
        assert!(!should_refresh_network(
            MenubarMetric::Network,
            true,
            false,
            false
        ));
        assert!(!should_refresh_network(
            MenubarMetric::Fan,
            true,
            false,
            true
        ));
    }

    #[test]
    fn launch_agent_prefers_valid_saved_choice_but_direct_cli_remains_authoritative() {
        let label = Some("kr.co.uulab.peterfan.menubar");
        assert_eq!(
            MetricSelection::new_for_launch(MenubarMetric::Fan, Some("cpu"), label, true)
                .unwrap()
                .active,
            MenubarMetric::Fan
        );
        assert_eq!(
            MetricSelection::new_for_launch(MenubarMetric::Fan, Some("cpu"), label, false)
                .unwrap()
                .active,
            MenubarMetric::Cpu
        );
        assert_eq!(
            MetricSelection::new_for_launch(MenubarMetric::Fan, Some("cpu"), None, true)
                .unwrap()
                .active,
            MenubarMetric::Cpu
        );
        assert_eq!(
            MetricSelection::new_for_launch(
                MenubarMetric::Fan,
                Some("cpu"),
                Some("kr.co.uulab.peterfan.other"),
                true
            )
            .unwrap()
            .active,
            MenubarMetric::Cpu
        );
        assert!(
            MetricSelection::new_for_launch(MenubarMetric::Fan, Some("invalid"), label, true)
                .is_err()
        );
    }

    #[test]
    fn argument_overrides_saved_only_until_persisted_setting_changes() {
        let mut state = MetricSelection::new(MenubarMetric::Temp, Some("cpu")).unwrap();
        assert_eq!(state.active, MenubarMetric::Cpu);
        state.reload(MenubarMetric::Temp);
        assert_eq!(state.active, MenubarMetric::Cpu);
        state.reload(MenubarMetric::Memory);
        assert_eq!(state.active, MenubarMetric::Memory);
        state.saved(MenubarMetric::Fan);
        assert_eq!(state.active, MenubarMetric::Fan);
        // A normal launch restores the user's last choice. An explicit CLI
        // argument is deliberately authoritative again on every new launch.
        assert_eq!(
            MetricSelection::new(MenubarMetric::Fan, None)
                .unwrap()
                .active,
            MenubarMetric::Fan
        );
        assert_eq!(
            MetricSelection::new(MenubarMetric::Fan, Some("cpu"))
                .unwrap()
                .active,
            MenubarMetric::Cpu
        );
        assert!(MetricSelection::new(MenubarMetric::Temp, Some("invalid")).is_err());
    }

    #[test]
    fn distinct_metrics_display_real_values_and_missing_data_is_not_zero() {
        let readings = MetricReadings {
            cpu: Some(48.0),
            memory: Some(76.0),
            temperature: Some(61.0),
            fan_rpm: Some(2300),
            network_bytes_per_sec: Some(2048.0),
        };
        assert!(title(MenubarMetric::Cpu, &readings).contains("48%"));
        assert!(title(MenubarMetric::Memory, &readings).contains("76%"));
        assert!(title(MenubarMetric::Temp, &readings).contains("61°C"));
        assert!(title(MenubarMetric::Fan, &readings).contains("2300"));
        assert!(title(MenubarMetric::Network, &readings).contains("2.0K"));
        let missing = MetricReadings {
            cpu: Some(f32::NAN),
            memory: None,
            temperature: None,
            fan_rpm: None,
            network_bytes_per_sec: Some(f64::NAN),
        };
        for metric in [
            MenubarMetric::Cpu,
            MenubarMetric::Memory,
            MenubarMetric::Temp,
            MenubarMetric::Fan,
            MenubarMetric::Network,
        ] {
            assert!(title(metric, &missing).contains("--"));
        }
    }
}
