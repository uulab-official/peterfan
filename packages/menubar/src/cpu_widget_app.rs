//! GUI-owned lifecycle and request acknowledgements for the independent CPU item.
use super::{App, SettingsOperation, SettingsRequest, SETTINGS_RESULTS, SETTINGS_RESULTS_MAX};
use crate::cpu_widget::CpuWidgetHistory;
#[cfg(target_os = "macos")]
use crate::native_cpu_widget::NativeCpuWidget;
use peterfan_core::config::CpuWidgetConfig;

pub(super) fn parse_config(payload: &str) -> Result<CpuWidgetConfig, String> {
    let value: serde_json::Value = serde_json::from_str(payload).map_err(|e| e.to_string())?;
    let object = value
        .as_object()
        .ok_or("CPU widget settings must be an object")?;
    if object.len() != 3
        || !["enabled", "style", "show_percentage"]
            .iter()
            .all(|key| object.contains_key(*key))
    {
        return Err("CPU widget settings require enabled, style and show_percentage".into());
    }
    serde_json::from_value(value).map_err(|e| e.to_string())
}

fn commit_with_save<F>(
    current: &mut CpuWidgetConfig,
    requested: CpuWidgetConfig,
    save: F,
) -> Result<(), String>
where
    F: FnOnce() -> Result<(), String>,
{
    save()?;
    *current = requested;
    Ok(())
}

pub(super) fn apply(
    app: &mut App,
    requested: CpuWidgetConfig,
    persist: bool,
) -> Result<(), String> {
    #[cfg(not(target_os = "macos"))]
    if requested.enabled {
        return Err("CPU menu-bar widgets are supported on macOS".into());
    }
    #[cfg(target_os = "macos")]
    let candidate = if requested.enabled && app.cpu_widget_native.is_none() {
        let mut candidate = NativeCpuWidget::new(requested, app.language.resolve())?;
        candidate.update(requested, app.language.resolve(), &app.cpu_widget_history)?;
        Some(candidate)
    } else {
        None
    };
    let was_enabled = app.cpu_widget_config.enabled;
    commit_with_save(&mut app.cpu_widget_config, requested, || {
        if persist {
            peterfan_platform::config::update(|config| {
                config.menubar.cpu_widget = requested;
                Ok(())
            })
            .map_err(|e| e.to_string())?;
        }
        Ok(())
    })?;
    if was_enabled != requested.enabled {
        app.cpu_widget_history = CpuWidgetHistory::default();
    }
    #[cfg(target_os = "macos")]
    {
        if !requested.enabled {
            app.cpu_widget_native = None;
        } else {
            if let Some(candidate) = candidate {
                app.cpu_widget_native = Some(candidate);
            }
            if let Some(widget) = &mut app.cpu_widget_native {
                widget.update(requested, app.language.resolve(), &app.cpu_widget_history)?;
                widget.set_visible(true)?;
            }
        }
    }
    app.cpu_widget_error = None;
    Ok(())
}

pub(super) fn refresh(app: &mut App, usage: f32) {
    if !app.cpu_widget_config.enabled {
        return;
    }
    app.cpu_widget_history
        .sample_at(usage, std::time::Instant::now());
    #[cfg(target_os = "macos")]
    if let Some(widget) = &mut app.cpu_widget_native {
        match widget.update(
            app.cpu_widget_config,
            app.language.resolve(),
            &app.cpu_widget_history,
        ) {
            Ok(()) => app.cpu_widget_error = None,
            Err(error) => {
                if app.cpu_widget_error.as_deref() != Some(error.as_str()) {
                    super::report_preference_failure("CPU widget display", &error);
                }
                app.cpu_widget_error = Some(error);
            }
        }
    }
}

pub(super) fn request(app: &mut App, request: SettingsRequest) -> String {
    let result = parse_config(&request.payload).and_then(|config| apply(app, config, true));
    let (ok, message) = match result {
        Ok(()) => (true, "CPU widget settings saved".to_string()),
        Err(error) => (false, format!("CPU widget: {error}")),
    };
    let mut results = SETTINGS_RESULTS.lock().expect("settings results poisoned");
    results.push_back(serde_json::json!({"id":request.id,"operation":SettingsOperation::CpuWidget,"ok":ok,"daemon_reload":"not_applicable","message":message}));
    while results.len() > SETTINGS_RESULTS_MAX {
        results.pop_front();
    }
    message
}

#[cfg(test)]
mod tests {
    use super::*;
    use peterfan_core::config::CpuWidgetStyle;
    #[test]
    fn failed_persistence_preserves_live_widget_configuration_and_allows_retry() {
        let original = CpuWidgetConfig::default();
        let mut current = original;
        let requested = CpuWidgetConfig {
            enabled: true,
            style: CpuWidgetStyle::History,
            show_percentage: false,
        };
        assert!(commit_with_save(&mut current, requested, || Err("disk full".into())).is_err());
        assert_eq!(current, original);
        commit_with_save(&mut current, requested, || Ok(())).unwrap();
        assert_eq!(current, requested);
    }
    #[test]
    fn cpu_widget_request_rejects_unknown_missing_and_wrong_type_fields() {
        for value in [
            r#"{}"#,
            r#"{"enabled":true}"#,
            r#"{"enabled":true,"style":"graph","show_percentage":true}"#,
            r#"{"enabled":"yes","style":"bar","show_percentage":true}"#,
            r#"{"enabled":true,"style":"bar","show_percentage":true,"fan":"manual"}"#,
        ] {
            assert!(parse_config(value).is_err(), "{value}");
        }
        assert!(
            parse_config(r#"{"enabled":true,"style":"history","show_percentage":false}"#)
                .unwrap()
                .enabled
        );
    }
}
