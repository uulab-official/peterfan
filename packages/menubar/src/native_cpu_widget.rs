//! Owned macOS CPU status item. No sampling or animation timer lives here.
use crate::cpu_widget::{self, CpuWidgetHistory, WIDGET_PIXEL_HEIGHT};
use objc2::rc::Retained;
use objc2::AllocAnyThread;
use objc2_app_kit::{
    NSBitmapFormat, NSBitmapImageRep, NSCellImagePosition, NSDeviceRGBColorSpace, NSFont, NSImage,
    NSImageCacheMode, NSStatusBarButton, NSStatusItem,
};
use objc2_foundation::{MainThreadMarker, NSSize, NSString};
use peterfan_core::config::{
    CpuWidgetConfig, CpuWidgetStyle, MenuItemsVisibility, MenubarDisplay, ResolvedLanguage,
};
use tray_icon::menu::{CheckMenuItem, Menu, MenuId, MenuItem, PredefinedMenuItem, Submenu};
use tray_icon::{TrayIcon, TrayIconAttributes};

/// A validated retained native primary button; committing visibility cannot
/// remove its NSStatusItem or fail after a successful configuration save.
pub(crate) struct PreparedPrimaryItem {
    item: Retained<NSStatusItem>,
    button: Retained<NSStatusBarButton>,
}

impl PreparedPrimaryItem {
    pub(crate) fn new(tray: &TrayIcon) -> Result<Self, String> {
        let marker = MainThreadMarker::new().ok_or("Menu items require the GUI thread")?;
        let item = tray
            .ns_status_item()
            .ok_or("Primary status item is unavailable")?;
        let button = item
            .button(marker)
            .ok_or("Primary status button is unavailable")?;
        Ok(Self { item, button })
    }

    pub(crate) fn configure(&self, tray: &TrayIcon, display: MenubarDisplay) {
        self.item.setLength(primary_width(display));
        self.button
            .setImagePosition(primary_image_position(display));
        // Refresh tray-icon's click surface after installing the fixed width.
        tray.set_title(Some(if display == MenubarDisplay::Graph {
            ""
        } else {
            " --°C"
        }));
    }

    pub(crate) fn set_visible(&self, visible: bool) {
        self.item.setVisible(visible);
    }

    pub(crate) fn apply(&self, tray: &TrayIcon, display: MenubarDisplay, visible: bool) {
        self.configure(tray, display);
        self.set_visible(visible);
    }
}

pub(crate) fn primary_width(display: MenubarDisplay) -> f64 {
    match display {
        MenubarDisplay::Number => 50.,
        MenubarDisplay::Graph => 36.,
        MenubarDisplay::Both => 84.,
    }
}

pub(crate) fn primary_image_position(display: MenubarDisplay) -> NSCellImagePosition {
    match display {
        MenubarDisplay::Number => NSCellImagePosition::NoImage,
        MenubarDisplay::Graph => NSCellImagePosition::ImageOnly,
        MenubarDisplay::Both => NSCellImagePosition::ImageLeft,
    }
}

pub(crate) struct PreparedCpuWidgetUpdate {
    button: Retained<NSStatusBarButton>,
    config: CpuWidgetConfig,
    language: ResolvedLanguage,
    title: String,
    tooltip: String,
    width: u32,
    point_width: f64,
    pixels: Vec<u8>,
    image: Option<Retained<NSImage>>,
    image_changed: bool,
}

pub(crate) enum CpuWidgetAction {
    OpenDashboard,
    Visibility(MenuItemsVisibility),
    Style(CpuWidgetStyle),
    Percentage(bool),
}

pub(crate) struct NativeCpuWidget {
    tray: TrayIcon,
    open: MenuItem,
    styles: Vec<(CpuWidgetStyle, CheckMenuItem)>,
    percentage: CheckMenuItem,
    menu_items: MenuItemsMenu,
    item: Retained<NSStatusItem>,
    config: CpuWidgetConfig,
    language: ResolvedLanguage,
    last_pixels: Vec<u8>,
    last_image_width: u32,
    last_title: Option<String>,
    last_tooltip: String,
}

fn labels(language: ResolvedLanguage) -> [&'static str; 7] {
    if language == ResolvedLanguage::Ko {
        [
            "대시보드 열기",
            "CPU 바",
            "히스토리",
            "숫자",
            "코어별 바",
            "사용률 숫자 표시",
            "메뉴 막대 표시 항목",
        ]
    } else {
        [
            "Open Dashboard",
            "CPU Bar",
            "History",
            "Number",
            "Per-core bars",
            "Show Percentage",
            "Menu-bar items",
        ]
    }
}

/// Identical independent controls on both native item menus.
pub(crate) struct MenuItemsMenu {
    submenu: Submenu,
    entries: Vec<CheckMenuItem>,
    visibility: std::cell::Cell<MenuItemsVisibility>,
}

impl MenuItemsMenu {
    pub(crate) fn new(language: ResolvedLanguage, visibility: MenuItemsVisibility) -> Self {
        let submenu = Submenu::new(labels(language)[6], true);
        let entries = (0..3)
            .map(|_| {
                let item = CheckMenuItem::new("", true, false, None);
                let _ = submenu.append(&item);
                item
            })
            .collect();
        let result = Self {
            submenu,
            entries,
            visibility: std::cell::Cell::new(visibility),
        };
        result.update(visibility, language);
        result
    }
    pub(crate) fn submenu(&self) -> &Submenu {
        &self.submenu
    }
    pub(crate) fn update(&self, visibility: MenuItemsVisibility, language: ResolvedLanguage) {
        self.visibility.set(visibility);
        self.submenu.set_text(labels(language)[6]);
        let names = if language == ResolvedLanguage::Ko {
            ["CPU 위젯", "러너 캐릭터", "측정값 텍스트"]
        } else {
            ["CPU widget", "Runner character", "Metric text"]
        };
        for (entry, name) in self.entries.iter().zip(names) {
            entry.set_text(name);
        }
        self.reconcile();
    }
    fn reconcile(&self) {
        let v = self.visibility.get();
        let values = [v.cpu, v.runner, v.metric];
        let count = values.iter().filter(|&&value| value).count();
        for (entry, checked) in self.entries.iter().zip(values) {
            entry.set_checked(checked);
            entry.set_enabled(!checked || count > 1);
        }
    }
    pub(crate) fn action(&self, id: &MenuId) -> Option<MenuItemsVisibility> {
        self.reconcile();
        let index = self.entries.iter().position(|entry| entry.id() == id)?;
        let mut v = self.visibility.get();
        match index {
            0 => v.cpu = !v.cpu,
            1 => v.runner = !v.runner,
            _ => v.metric = !v.metric,
        }
        Some(v)
    }
}

impl NativeCpuWidget {
    pub(crate) fn new(config: CpuWidgetConfig, language: ResolvedLanguage) -> Result<Self, String> {
        let marker =
            MainThreadMarker::new().ok_or("CPU status item must be created on the main thread")?;
        let text = labels(language);
        let menu = Menu::new();
        let open = MenuItem::new(text[0], true, None);
        menu.append(&open).map_err(|e| e.to_string())?;
        menu.append(&PredefinedMenuItem::separator())
            .map_err(|e| e.to_string())?;
        let mut styles = Vec::new();
        for (index, style) in [
            CpuWidgetStyle::Bar,
            CpuWidgetStyle::History,
            CpuWidgetStyle::Number,
            CpuWidgetStyle::Cores,
        ]
        .into_iter()
        .enumerate()
        {
            let item = CheckMenuItem::new(text[index + 1], true, config.style == style, None);
            menu.append(&item).map_err(|e| e.to_string())?;
            styles.push((style, item));
        }
        let percentage = CheckMenuItem::new(
            text[5],
            config.style != CpuWidgetStyle::Number,
            config.show_percentage,
            None,
        );
        let menu_items = MenuItemsMenu::new(
            language,
            MenuItemsVisibility {
                cpu: config.enabled,
                runner: true,
                metric: true,
            },
        );
        menu.append(&percentage).map_err(|e| e.to_string())?;
        menu.append(&PredefinedMenuItem::separator())
            .map_err(|e| e.to_string())?;
        menu.append(menu_items.submenu())
            .map_err(|e| e.to_string())?;
        let tray = TrayIcon::new(TrayIconAttributes {
            menu: Some(Box::new(menu)),
            menu_on_left_click: false,
            menu_on_right_click: true,
            icon_is_template: true,
            title: Some("CPU  --%".to_string()),
            ..Default::default()
        })
        .map_err(|e| e.to_string())?;
        let item = tray
            .ns_status_item()
            .ok_or("CPU status item creation failed")?;
        item.button(marker)
            .ok_or("CPU status button creation failed")?;
        item.setAutosaveName(Some(&NSString::from_str("PeterFan.CPUWidget")));
        // Native visibility retains the button; tray-icon's setter removes it.
        item.setVisible(false);
        Ok(Self {
            tray,
            open,
            styles,
            percentage,
            menu_items,
            item,
            config,
            language,
            last_pixels: Vec::new(),
            last_image_width: 0,
            last_title: None,
            last_tooltip: String::new(),
        })
    }
    pub(crate) fn tray(&self) -> &TrayIcon {
        &self.tray
    }
    pub(crate) fn set_visible(&self, visible: bool) {
        self.item.setVisible(visible);
        if visible {
            self.tray.set_title(self.last_title.as_deref());
        }
    }
    pub(crate) fn set_item_visibility(
        &self,
        visibility: MenuItemsVisibility,
        language: ResolvedLanguage,
    ) {
        self.menu_items.update(visibility, language);
    }
    pub(crate) fn menu_action(&self, id: &MenuId) -> Option<CpuWidgetAction> {
        self.reconcile_menu();
        if self.open.id() == id {
            Some(CpuWidgetAction::OpenDashboard)
        } else if let Some(visibility) = self.menu_items.action(id) {
            Some(CpuWidgetAction::Visibility(visibility))
        } else if self.percentage.id() == id {
            Some(CpuWidgetAction::Percentage(!self.config.show_percentage))
        } else {
            self.styles
                .iter()
                .find(|(_, item)| item.id() == id)
                .map(|(style, _)| CpuWidgetAction::Style(*style))
        }
    }
    fn reconcile_menu(&self) {
        for (style, entry) in &self.styles {
            entry.set_checked(*style == self.config.style);
        }
        self.percentage.set_checked(
            self.config.show_percentage || self.config.style == CpuWidgetStyle::Number,
        );
        self.percentage
            .set_enabled(self.config.style != CpuWidgetStyle::Number);
    }

    /// Allocate images and validate the native button without touching the live
    /// presentation. Dropping this value rolls back a failed save naturally.
    pub(crate) fn prepare_update(
        &self,
        config: CpuWidgetConfig,
        language: ResolvedLanguage,
        history: &CpuWidgetHistory,
    ) -> Result<PreparedCpuWidgetUpdate, String> {
        let marker =
            MainThreadMarker::new().ok_or("CPU status item update requires the main thread")?;
        let button = self
            .item
            .button(marker)
            .ok_or("CPU status button is missing")?;
        let title = cpu_widget::title(config, history.current());
        let width = history.pixel_width(config.style);
        let pixels = if config.style == CpuWidgetStyle::Number {
            Vec::new()
        } else {
            history.render_rgba(config.style)
        };
        let image_changed = config.style == CpuWidgetStyle::Number
            || self.last_pixels != pixels
            || self.last_image_width != width;
        let image = if image_changed && config.style != CpuWidgetStyle::Number {
            Some(image_from_rgba(&pixels, width)?)
        } else {
            None
        };
        let value = history
            .current()
            .map_or_else(|| "--%".to_string(), |v| format!("{v:.0}%"));
        let core_tip = if config.style == CpuWidgetStyle::Cores {
            if language == ResolvedLanguage::Ko {
                format!(
                    "{}개 논리 코어 · 코어마다 0–100%\n",
                    history.core_values().len()
                )
            } else {
                format!(
                    "{} logical CPUs · 0–100% per CPU\n",
                    history.core_values().len()
                )
            }
        } else {
            String::new()
        };
        let tooltip = if language == ResolvedLanguage::Ko {
            format!("{core_tip}CPU {value} · 0–100%\n클릭하여 대시보드 열기 · 우클릭하여 표시 설정")
        } else {
            format!("{core_tip}CPU {value} · 0–100%\nClick for dashboard · Right-click for display options")
        };
        Ok(PreparedCpuWidgetUpdate {
            button,
            config,
            language,
            title,
            tooltip,
            width,
            point_width: history.point_width(config),
            pixels,
            image,
            image_changed,
        })
    }

    pub(crate) fn commit_update(&mut self, prepared: PreparedCpuWidgetUpdate) {
        let PreparedCpuWidgetUpdate {
            button,
            config,
            language,
            title,
            tooltip,
            width,
            point_width,
            pixels,
            image,
            image_changed,
        } = prepared;
        if self.config != config || self.last_title.is_none() || self.last_image_width != width {
            self.item.setLength(point_width);
            button.setImagePosition(image_position(config));
            button.setFont(Some(&NSFont::monospacedDigitSystemFontOfSize_weight(
                11., 0.,
            )));
            self.tray.set_title(Some(&title));
        }
        if self.language != language {
            let names = labels(language);
            self.open.set_text(names[0]);
            for (index, (_, entry)) in self.styles.iter().enumerate() {
                entry.set_text(names[index + 1]);
            }
            self.percentage.set_text(names[5]);
            self.menu_items
                .update(self.menu_items.visibility.get(), language);
        }
        if self.last_title.as_deref() != Some(title.as_str()) {
            button.setTitle(&NSString::from_str(&title));
            self.last_title = Some(title);
        }
        if image_changed {
            button.setImage(image.as_deref());
            self.last_pixels = pixels;
        }
        if tooltip != self.last_tooltip {
            button.setToolTip(Some(&NSString::from_str(&tooltip)));
            self.last_tooltip = tooltip;
        }
        self.last_image_width = width;
        self.config = config;
        self.language = language;
        self.reconcile_menu();
    }

    pub(crate) fn update(
        &mut self,
        config: CpuWidgetConfig,
        language: ResolvedLanguage,
        history: &CpuWidgetHistory,
    ) -> Result<(), String> {
        let prepared = self.prepare_update(config, language, history)?;
        self.commit_update(prepared);
        Ok(())
    }
}

fn image_position(config: CpuWidgetConfig) -> NSCellImagePosition {
    match (config.style, config.show_percentage) {
        (CpuWidgetStyle::Number, _) => NSCellImagePosition::NoImage,
        (_, true) => NSCellImagePosition::ImageLeft,
        (_, false) => NSCellImagePosition::ImageOnly,
    }
}

fn image_from_rgba(rgba: &[u8], width: u32) -> Result<Retained<NSImage>, String> {
    if width == 0 || rgba.len() != width as usize * WIDGET_PIXEL_HEIGHT as usize * 4 {
        return Err("invalid CPU widget raster size".into());
    }
    let bitmap = unsafe { NSBitmapImageRep::initWithBitmapDataPlanes_pixelsWide_pixelsHigh_bitsPerSample_samplesPerPixel_hasAlpha_isPlanar_colorSpaceName_bitmapFormat_bytesPerRow_bitsPerPixel(
        NSBitmapImageRep::alloc(), std::ptr::null_mut(), width as isize, WIDGET_PIXEL_HEIGHT as isize,
        8, 4, true, false, NSDeviceRGBColorSpace, NSBitmapFormat::AlphaNonpremultiplied, (width * 4) as isize, 32) }
        .ok_or("CPU widget bitmap allocation failed")?;
    let pixels = bitmap.bitmapData();
    if pixels.is_null()
        || bitmap.bytesPerRow() != (width * 4) as isize
        || bitmap.pixelsHigh() != WIDGET_PIXEL_HEIGHT as isize
    {
        return Err("invalid CPU widget bitmap storage".into());
    }
    // AppKit owns the checked RGBA8 allocation. No borrowed pointer escapes.
    unsafe { std::ptr::copy_nonoverlapping(rgba.as_ptr(), pixels, rgba.len()) };
    let size = NSSize::new(f64::from(width) / 2., 18.);
    bitmap.setSize(size);
    let image = NSImage::initWithSize(NSImage::alloc(), size);
    image.addRepresentation(&bitmap);
    image.setCacheMode(NSImageCacheMode::Always);
    image.setTemplate(true);
    Ok(image)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cpu_layout_preserves_a_visible_graph_and_number_only_has_no_image() {
        let mut config = CpuWidgetConfig::default();
        assert_eq!(image_position(config), NSCellImagePosition::ImageLeft);
        config.show_percentage = false;
        assert_eq!(image_position(config), NSCellImagePosition::ImageOnly);
        config.style = CpuWidgetStyle::Number;
        assert_eq!(image_position(config), NSCellImagePosition::NoImage);
    }
    #[test]
    fn native_cpu_image_has_owned_retina_storage_and_template_appearance() {
        assert!(image_from_rgba(&[0; 4], 64).is_err());
        let mut h = CpuWidgetHistory::default();
        h.push(50.);
        let image = image_from_rgba(&h.render_rgba(CpuWidgetStyle::Bar), 64).unwrap();
        assert_eq!(image.size(), NSSize::new(32., 18.));
        assert!(image.isTemplate());
    }

    #[test]
    fn native_core_images_preserve_all_columns_and_validate_storage() {
        let mut history = CpuWidgetHistory::default();
        let now = std::time::Instant::now();
        for count in [1, 8, 10, 18, 32, 64] {
            history.sample_cpu_at(
                50.,
                &vec![50.; count],
                now + std::time::Duration::from_secs(count as u64),
            );
            let width = history.pixel_width(CpuWidgetStyle::Cores);
            let image =
                image_from_rgba(&history.render_rgba(CpuWidgetStyle::Cores), width).unwrap();
            assert_eq!(image.size(), NSSize::new(f64::from(width) / 2., 18.));
            assert!(image.isTemplate());
            assert!(image_from_rgba(&[0; 4], width).is_err());
        }
        assert!(image_from_rgba(&[], 0).is_err());
        assert!(image_from_rgba(&[], u32::MAX).is_err());
    }
}
