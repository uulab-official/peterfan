//! GUI-owned lifecycle and request acknowledgements for the independent CPU item.
use super::{App, SettingsOperation, SettingsRequest, SETTINGS_RESULTS, SETTINGS_RESULTS_MAX};
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

pub(super) fn apply(
    app: &mut App,
    requested: CpuWidgetConfig,
    persist: bool,
) -> Result<(), String> {
    super::menu_items_app::apply_cpu(app, requested, persist)
}

pub(super) fn refresh(app: &mut App, usage: f32, per_core: &[f32]) {
    if !app.cpu_widget_config.enabled {
        return;
    }
    app.cpu_widget_history
        .sample_cpu_at(usage, per_core, std::time::Instant::now());
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

#[cfg(target_os = "macos")]
pub(super) fn native_appearance(
    mut config: CpuWidgetConfig,
    action: crate::native_cpu_widget::CpuWidgetAction,
) -> Option<CpuWidgetConfig> {
    use crate::native_cpu_widget::CpuWidgetAction;
    match action {
        CpuWidgetAction::Style(style) => config = config.select_style(style),
        CpuWidgetAction::Percentage(show) => config.show_percentage = show,
        CpuWidgetAction::OpenDashboard | CpuWidgetAction::Visibility(_) => return None,
    }
    Some(config)
}

pub(super) fn request(app: &mut App, request: SettingsRequest) -> String {
    complete_request(request, |config| apply(app, config, true))
}

pub(super) fn complete_request(
    request: SettingsRequest,
    apply: impl FnOnce(CpuWidgetConfig) -> Result<(), String>,
) -> String {
    let result = parse_config(&request.payload).and_then(apply);
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
