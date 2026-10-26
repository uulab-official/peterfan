//! One staged, atomic owner for menu-item visibility and CPU appearance.
use super::{App, SettingsOperation, SettingsRequest, SETTINGS_RESULTS, SETTINGS_RESULTS_MAX};
use peterfan_core::config::{CpuWidgetConfig, MenuItemsVisibility, MenubarConfig, MenubarDisplay};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ItemState {
    display: MenubarDisplay,
    primary_visible: bool,
    cpu: CpuWidgetConfig,
}

impl ItemState {
    fn from_app(app: &App) -> Self {
        Self {
            display: app.display,
            primary_visible: app.primary_visible,
            cpu: app.cpu_widget_config,
        }
    }

    fn from_config(config: &MenubarConfig) -> Self {
        Self {
            display: config.display,
            primary_visible: config.primary_visible,
            cpu: config.cpu_widget,
        }
    }

    fn config(self) -> MenubarConfig {
        MenubarConfig {
            display: self.display,
            primary_visible: self.primary_visible,
            cpu_widget: self.cpu,
            ..Default::default()
        }
    }

    fn visibility(self) -> MenuItemsVisibility {
        MenuItemsVisibility {
            cpu: self.cpu.enabled,
            runner: self.primary_visible && self.display != MenubarDisplay::Number,
            metric: self.primary_visible && self.display != MenubarDisplay::Graph,
        }
    }

    fn with_visibility(self, visibility: MenuItemsVisibility) -> Result<Self, String> {
        let mut config = MenubarConfig {
            display: self.display,
            primary_visible: self.primary_visible,
            cpu_widget: self.cpu,
            ..Default::default()
        };
        config.set_item_visibility(visibility)?;
        Ok(Self {
            display: config.display,
            primary_visible: config.primary_visible,
            cpu: config.cpu_widget,
        })
    }

    fn with_cpu(self, cpu: CpuWidgetConfig) -> Result<Self, String> {
        let result = Self { cpu, ..self };
        if !result.visibility().any() {
            return Err("Keep at least one menu-bar item visible".into());
        }
        Ok(result)
    }

    fn with_display(self, display: MenubarDisplay) -> Self {
        Self {
            display,
            primary_visible: true,
            ..self
        }
    }
}

pub(super) fn visibility(app: &App) -> MenuItemsVisibility {
    ItemState::from_app(app).visibility()
}

pub(super) fn parse_visibility(payload: &str) -> Result<MenuItemsVisibility, String> {
    let value: MenuItemsVisibility = serde_json::from_str(payload).map_err(|e| e.to_string())?;
    if !value.any() {
        return Err("Keep at least one menu-bar item visible".into());
    }
    Ok(value)
}

#[derive(Clone, Copy)]
enum ItemEdit {
    Visibility(MenuItemsVisibility),
    Cpu(CpuWidgetConfig),
    Display(MenubarDisplay),
    Reload(ItemState),
}

impl ItemEdit {
    fn resolve(self, latest: ItemState) -> Result<ItemState, String> {
        match self {
            Self::Visibility(value) => latest.with_visibility(value),
            Self::Cpu(value) => latest.with_cpu(value),
            Self::Display(value) => Ok(latest.with_display(value)),
            Self::Reload(value) => Ok(value),
        }
    }
}

// Resolve intent and prepare native resources inside the existing configuration
// transaction. A newer cooperating writer's unedited fields therefore feed both
// the saved configuration and the native/live commit, from the same snapshot.
fn commit_staged<T>(
    current: &mut ItemState,
    edit: ItemEdit,
    stage: impl FnOnce(ItemState) -> Result<T, String>,
    transaction: impl FnOnce(
        &mut dyn FnMut(&mut MenubarConfig) -> Result<(), String>,
    ) -> Result<(), String>,
) -> Result<T, String> {
    let mut stage = Some(stage);
    let mut prepared = None;
    let mut committed = None;
    transaction(&mut |config| {
        let requested = edit.resolve(ItemState::from_config(config))?;
        if !requested.visibility().any() {
            return Err("Keep at least one menu-bar item visible".into());
        }
        prepared = Some(stage.take().expect("one configuration edit")(requested)?);
        write_state(config, requested);
        committed = Some(requested);
        Ok(())
    })?;
    *current = committed.expect("successful configuration edit");
    Ok(prepared.expect("successful native preparation"))
}

#[cfg(target_os = "macos")]
struct PreparedItems {
    primary: crate::native_cpu_widget::PreparedPrimaryItem,
    cpu_update: Option<crate::native_cpu_widget::PreparedCpuWidgetUpdate>,
    cpu: Option<crate::native_cpu_widget::NativeCpuWidget>,
}

#[cfg(target_os = "macos")]
fn prepare(app: &App, requested: ItemState) -> Result<PreparedItems, String> {
    use crate::native_cpu_widget::{NativeCpuWidget, PreparedPrimaryItem};
    let primary =
        PreparedPrimaryItem::new(app.tray.as_ref().ok_or("Primary tray is unavailable")?)?;
    let empty_history = Default::default();
    let history = if app.cpu_widget_config.enabled != requested.cpu.enabled {
        &empty_history
    } else {
        &app.cpu_widget_history
    };
    let mut cpu_update = None;
    let cpu = if requested.cpu.enabled {
        if let Some(widget) = &app.cpu_widget_native {
            cpu_update =
                Some(widget.prepare_update(requested.cpu, app.language.resolve(), history)?);
            None
        } else {
            let mut candidate = NativeCpuWidget::new(requested.cpu, app.language.resolve())?;
            candidate.update(requested.cpu, app.language.resolve(), history)?;
            candidate.set_item_visibility(requested.visibility(), app.language.resolve());
            Some(candidate)
        }
    } else {
        None
    };
    Ok(PreparedItems {
        primary,
        cpu,
        cpu_update,
    })
}

fn apply_state(app: &mut App, edit: ItemEdit, persist: bool) -> Result<(), String> {
    let mut live = ItemState::from_app(app);
    let mut fallback = live.config();
    let transaction = |edit: &mut dyn FnMut(&mut MenubarConfig) -> Result<(), String>| {
        if persist {
            peterfan_platform::config::update(|config| {
                edit(&mut config.menubar).map_err(std::io::Error::other)
            })
            .map(|_| ())
            .map_err(|error| error.to_string())
        } else {
            edit(&mut fallback)
        }
    };
    #[cfg(target_os = "macos")]
    let staged = commit_staged(&mut live, edit, |state| prepare(app, state), transaction)?;
    #[cfg(not(target_os = "macos"))]
    commit_staged(
        &mut live,
        edit,
        |requested| {
            if requested.cpu.enabled || !requested.primary_visible {
                Err("Menu item visibility is supported on macOS".into())
            } else {
                Ok(())
            }
        },
        transaction,
    )?;
    let requested = live;
    // No fallible native operations after save. Never remove the primary item:
    // native visibility preserves its identity, button and click target.
    #[cfg(target_os = "macos")]
    {
        super::stop_native_runner_timer(app);
        if let Some(candidate) = staged.cpu {
            candidate.set_visible(true);
            app.cpu_widget_native = Some(candidate);
        }
        if let Some(widget) = &mut app.cpu_widget_native {
            if let Some(update) = staged.cpu_update {
                widget.commit_update(update);
            }
            widget.set_item_visibility(requested.visibility(), app.language.resolve());
            widget.set_visible(requested.cpu.enabled);
        }
    }
    if app.cpu_widget_config.enabled != requested.cpu.enabled {
        app.cpu_widget_history = Default::default();
    }
    app.display = live.display;
    app.primary_visible = live.primary_visible;
    app.cpu_widget_config = live.cpu;
    app.last_menubar_text = None;
    app.last_menubar_tooltip = None;
    super::invalidate_runner_icon(&mut app.last_runner_icon);
    #[cfg(target_os = "macos")]
    {
        if let Some(tray) = &app.tray {
            staged.primary.configure(tray, app.display);
        }
        super::apply_runner_icon(app);
        staged.primary.set_visible(app.primary_visible);
        super::sync_native_runner_timer(app);
        if let Some(menu) = &app.tray_menu {
            menu.menu_items
                .update(visibility(app), app.language.resolve());
        }
    }
    if let Some(menu) = &app.tray_menu {
        for (display, item) in &menu.display_items {
            item.set_checked(*display == app.display);
        }
    }
    app.cpu_widget_error = None;
    Ok(())
}

fn write_state(config: &mut MenubarConfig, state: ItemState) {
    config.display = state.display;
    config.primary_visible = state.primary_visible;
    config.cpu_widget = state.cpu;
}

pub(super) fn apply(
    app: &mut App,
    requested: MenuItemsVisibility,
    persist: bool,
) -> Result<(), String> {
    apply_state(app, ItemEdit::Visibility(requested), persist)
}

pub(super) fn apply_cpu(
    app: &mut App,
    requested: CpuWidgetConfig,
    persist: bool,
) -> Result<(), String> {
    apply_state(app, ItemEdit::Cpu(requested), persist)
}

pub(super) fn apply_display(
    app: &mut App,
    display: MenubarDisplay,
    persist: bool,
) -> Result<(), String> {
    apply_state(app, ItemEdit::Display(display), persist)
}

fn external_state(config: &MenubarConfig, macos: bool) -> ItemState {
    let mut effective = config.clone();
    if macos {
        effective
            .set_item_visibility(config.item_visibility())
            .expect("effective state is nonempty");
    } else {
        effective.primary_visible = true;
        effective.cpu_widget.enabled = false;
    }
    ItemState {
        display: effective.display,
        primary_visible: effective.primary_visible,
        cpu: effective.cpu_widget,
    }
}

pub(super) fn reload(app: &mut App, config: &MenubarConfig) {
    let requested = external_state(config, cfg!(target_os = "macos"));
    if requested == ItemState::from_app(app) {
        return;
    }
    if let Err(error) = apply_state(app, ItemEdit::Reload(requested), false) {
        // Native preparation failed: recover an accessible primary using the
        // same owned item; never rewrite the external or fan configuration.
        #[cfg(target_os = "macos")]
        {
            app.primary_visible = true;
            app.display = MenubarDisplay::Number;
            super::stop_native_runner_timer(app);
            if let Some(tray) = &app.tray {
                if let Ok(primary) = crate::native_cpu_widget::PreparedPrimaryItem::new(tray) {
                    primary.apply(tray, app.display, true);
                }
            }
            app.last_menubar_text = None;
            super::invalidate_runner_icon(&mut app.last_runner_icon);
            super::apply_runner_icon(app);
        }
        if app.cpu_widget_error.as_deref() != Some(error.as_str()) {
            super::report_preference_failure("Menu items", &error);
        }
        app.cpu_widget_error = Some(error);
    }
}

pub(super) fn request(app: &mut App, request: SettingsRequest) -> String {
    let result = parse_visibility(&request.payload).and_then(|value| apply(app, value, true));
    let (ok, message) = match result {
        Ok(()) => (true, "Menu-bar items saved".to_string()),
        Err(error) => (false, format!("Menu-bar items: {error}")),
    };
    let mut results = SETTINGS_RESULTS.lock().expect("settings results poisoned");
    results.push_back(serde_json::json!({"id":request.id,"operation":SettingsOperation::MenuItems,"ok":ok,"daemon_reload":"not_applicable","message":message}));
    while results.len() > SETTINGS_RESULTS_MAX {
        results.pop_front();
    }
    message
}

#[cfg(test)]
mod tests {
    use super::*;
    use peterfan_core::config::{
        CpuWidgetConfig, CpuWidgetStyle, MenuItemsVisibility, MenubarDisplay,
    };
    use std::cell::Cell;

    fn original() -> ItemState {
        ItemState {
            display: MenubarDisplay::Graph,
            primary_visible: true,
            cpu: CpuWidgetConfig::default(),
        }
    }

    #[test]
    fn visibility_save_rebases_external_cpu_appearance_and_commits_matching_live_state() {
        assert_external_cpu_appearance_survives(false);
    }

    #[test]
    fn legacy_display_save_rebases_external_cpu_appearance_and_commits_matching_live_state() {
        assert_external_cpu_appearance_survives(true);
    }

    fn assert_external_cpu_appearance_survives(legacy_display: bool) {
        let mut live = original();
        let mut latest = MenubarConfig {
            display: MenubarDisplay::Graph,
            cpu_widget: CpuWidgetConfig {
                enabled: true,
                style: CpuWidgetStyle::History,
                show_percentage: false,
            },
            ..Default::default()
        };
        let edit = if legacy_display {
            ItemEdit::Display(MenubarDisplay::Both)
        } else {
            ItemEdit::Visibility(MenuItemsVisibility {
                cpu: true,
                runner: false,
                metric: true,
            })
        };
        let staged = commit_staged(
            &mut live,
            edit,
            |state| Ok(state.cpu),
            |change| change(&mut latest),
        )
        .unwrap();
        assert_eq!(
            staged, latest.cpu_widget,
            "native preparation used stale appearance"
        );
        assert_eq!(
            latest.cpu_widget.style,
            CpuWidgetStyle::History,
            "new external CPU style was overwritten"
        );
        assert!(
            !latest.cpu_widget.show_percentage,
            "new external percentage was overwritten"
        );
        assert_eq!(
            live.cpu, latest.cpu_widget,
            "live/native state must match the committed rebase"
        );
        assert_eq!(
            latest.display,
            if legacy_display {
                MenubarDisplay::Both
            } else {
                MenubarDisplay::Number
            }
        );
    }

    #[test]
    fn strict_visibility_payload_rejects_missing_unknown_duplicates_types_and_all_off() {
        for value in [
            "{}",
            "null",
            r#"{"cpu":true,"runner":false}"#,
            r#"{"cpu":1,"runner":false,"metric":false}"#,
            r#"{"cpu":true,"runner":false,"metric":false,"fan":true}"#,
            r#"{"cpu":true,"cpu":false,"runner":true,"metric":false}"#,
            r#"{"cpu":false,"runner":false,"metric":false}"#,
        ] {
            assert!(parse_visibility(value).is_err(), "{value}");
        }
        assert_eq!(
            parse_visibility(r#"{"cpu":true,"runner":false,"metric":false}"#).unwrap(),
            MenuItemsVisibility {
                cpu: true,
                runner: false,
                metric: false
            }
        );
    }

    #[test]
    fn staged_failure_never_saves_or_changes_live_state() {
        let mut live = original();
        let edit = ItemEdit::Visibility(MenuItemsVisibility {
            cpu: true,
            runner: false,
            metric: false,
        });
        let mut config = live.config();
        let saved = Cell::new(false);
        let outcome = commit_staged(
            &mut live,
            edit,
            |_| Err::<(), _>("native unavailable".into()),
            |change| {
                change(&mut config)?;
                saved.set(true);
                Ok(())
            },
        );
        assert!(outcome.is_err());
        assert!(!saved.get());
        assert_eq!(live, original());
    }

    #[test]
    fn failed_save_keeps_live_items_and_retry_commits_prepared_state() {
        let mut live = original();
        let edit = ItemEdit::Visibility(MenuItemsVisibility {
            cpu: true,
            runner: false,
            metric: false,
        });
        let desired = edit.resolve(live).unwrap();
        let mut config = live.config();
        let staged = Cell::new(false);
        assert!(commit_staged(
            &mut live,
            edit,
            |_| {
                staged.set(true);
                Ok(42)
            },
            |change| {
                change(&mut config)?;
                assert!(staged.get());
                Err("disk full".into())
            }
        )
        .is_err());
        assert_eq!(live, original());
        assert_eq!(
            commit_staged(&mut live, edit, |_| Ok(42), |change| change(&mut config)).unwrap(),
            42
        );
        assert_eq!(live, desired);
        assert!(!live.primary_visible);
        assert!(live.cpu.enabled);
    }

    #[test]
    fn external_all_off_recovers_metric_and_windows_always_keeps_its_primary() {
        let config = MenubarConfig {
            display: MenubarDisplay::Graph,
            primary_visible: false,
            ..Default::default()
        };
        assert_eq!(
            external_state(&config, true).visibility(),
            MenuItemsVisibility {
                cpu: false,
                runner: false,
                metric: true
            }
        );
        let mut cpu_only = config.clone();
        cpu_only.cpu_widget.enabled = true;
        assert_eq!(
            external_state(&cpu_only, true).visibility(),
            MenuItemsVisibility {
                cpu: true,
                runner: false,
                metric: false
            }
        );
        for config in [config, cpu_only] {
            let windows = external_state(&config, false);
            assert!(windows.primary_visible);
            assert!(!windows.cpu.enabled);
            assert_eq!(windows.display, MenubarDisplay::Graph);
        }
    }

    #[test]
    fn cpu_and_legacy_display_entry_points_share_last_item_guard_and_preferences() {
        let only_cpu = original()
            .with_visibility(MenuItemsVisibility {
                cpu: true,
                runner: false,
                metric: false,
            })
            .unwrap();
        let mut style = only_cpu.cpu;
        style.enabled = false;
        assert!(only_cpu.with_cpu(style).is_err());
        style.enabled = true;
        style.style = CpuWidgetStyle::History;
        assert!(!only_cpu.with_cpu(style).unwrap().primary_visible);
        let restored = only_cpu.with_display(MenubarDisplay::Number);
        assert!(restored.primary_visible);
        assert!(restored.cpu.enabled);
        assert_eq!(
            restored.visibility(),
            MenuItemsVisibility {
                cpu: true,
                runner: false,
                metric: true
            }
        );
    }
}
