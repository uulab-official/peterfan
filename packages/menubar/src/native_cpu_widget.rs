//! Owned macOS CPU status item. No sampling or animation timer lives here.
use crate::cpu_widget::{self, CpuWidgetHistory, WIDGET_PIXEL_HEIGHT};
use objc2::rc::Retained;
use objc2::AllocAnyThread;
use objc2_app_kit::{
    NSBitmapFormat, NSBitmapImageRep, NSCellImagePosition, NSDeviceRGBColorSpace, NSFont, NSImage,
    NSImageCacheMode,
};
use objc2_foundation::{MainThreadMarker, NSSize, NSString};
use peterfan_core::config::{CpuWidgetConfig, CpuWidgetStyle, ResolvedLanguage};
use tray_icon::menu::{CheckMenuItem, Menu, MenuId, MenuItem, PredefinedMenuItem};
use tray_icon::{TrayIcon, TrayIconAttributes};

pub(crate) enum CpuWidgetAction {
    OpenDashboard,
    Enabled(bool),
    Style(CpuWidgetStyle),
    Percentage(bool),
}

pub(crate) struct NativeCpuWidget {
    tray: TrayIcon,
    open: MenuItem,
    styles: Vec<(CpuWidgetStyle, CheckMenuItem)>,
    percentage: CheckMenuItem,
    hide: MenuItem,
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
            "CPU 위젯 숨기기",
        ]
    } else {
        [
            "Open Dashboard",
            "CPU Bar",
            "History",
            "Number",
            "Per-core bars",
            "Show Percentage",
            "Hide CPU Widget",
        ]
    }
}

impl NativeCpuWidget {
    pub(crate) fn new(config: CpuWidgetConfig, language: ResolvedLanguage) -> Result<Self, String> {
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
        let hide = MenuItem::new(text[6], true, None);
        menu.append(&percentage).map_err(|e| e.to_string())?;
        menu.append(&PredefinedMenuItem::separator())
            .map_err(|e| e.to_string())?;
        menu.append(&hide).map_err(|e| e.to_string())?;
        let tray = TrayIcon::new(TrayIconAttributes {
            menu: Some(Box::new(menu)),
            menu_on_left_click: false,
            menu_on_right_click: true,
            icon_is_template: true,
            title: Some("CPU  --%".to_string()),
            ..Default::default()
        })
        .map_err(|e| e.to_string())?;
        // Stage creation inside the same event-loop turn. A failed preference
        // save drops this hidden owned item without changing the live layout.
        let item = tray
            .ns_status_item()
            .ok_or("CPU status item creation failed")?;
        item.setAutosaveName(Some(&NSString::from_str("PeterFan.CPUWidget")));
        // tray-icon::set_visible(false) removes its NSStatusItem entirely.
        // Native visibility preserves the owned button while staging a save.
        item.setVisible(false);
        Ok(Self {
            tray,
            open,
            styles,
            percentage,
            hide,
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
    pub(crate) fn set_visible(&self, visible: bool) -> Result<(), String> {
        let item = self
            .tray
            .ns_status_item()
            .ok_or("CPU status item is missing")?;
        item.setVisible(visible);
        if visible {
            self.tray.set_title(self.last_title.as_deref());
        }
        Ok(())
    }

    pub(crate) fn menu_action(&self, id: &MenuId) -> Option<CpuWidgetAction> {
        // Muda toggles check items before delivering their event. Restore the
        // persisted state even for a no-op selection or a subsequent save error.
        self.reconcile_menu();
        if self.open.id() == id {
            Some(CpuWidgetAction::OpenDashboard)
        } else if self.hide.id() == id {
            Some(CpuWidgetAction::Enabled(false))
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

    pub(crate) fn update(
        &mut self,
        config: CpuWidgetConfig,
        language: ResolvedLanguage,
        history: &CpuWidgetHistory,
    ) -> Result<(), String> {
        let marker =
            MainThreadMarker::new().ok_or("CPU status item update requires the main thread")?;
        let item = self
            .tray
            .ns_status_item()
            .ok_or("CPU status item is missing")?;
        let button = item.button(marker).ok_or("CPU status button is missing")?;
        let text = cpu_widget::title(config, history.current());
        let width = history.pixel_width(config.style);
        if self.config != config || self.last_title.is_none() || self.last_image_width != width {
            item.setLength(history.point_width(config));
            button.setImagePosition(image_position(config));
            button.setFont(Some(&NSFont::monospacedDigitSystemFontOfSize_weight(
                11., 0.,
            )));
            // Resize tray-icon's own click surface only when layout changes.
            self.tray.set_title(Some(&text));
        }
        if self.language != language {
            let names = labels(language);
            self.open.set_text(names[0]);
            for (index, (_, entry)) in self.styles.iter().enumerate() {
                entry.set_text(names[index + 1]);
            }
            self.percentage.set_text(names[5]);
            self.hide.set_text(names[6]);
        }
        if self.last_title.as_deref() != Some(text.as_str()) {
            button.setTitle(&NSString::from_str(&text));
            self.last_title = Some(text);
        }
        if config.style == CpuWidgetStyle::Number {
            button.setImage(None);
            self.last_pixels.clear();
        } else {
            let pixels = history.render_rgba(config.style);
            if self.last_pixels != pixels || self.last_image_width != width {
                let image = image_from_rgba(&pixels, width)?;
                button.setImage(Some(&image));
                self.last_pixels = pixels;
            }
        }
        self.last_image_width = width;
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
        let tip = if language == ResolvedLanguage::Ko {
            format!("{core_tip}CPU {value} · 0–100%\n클릭하여 대시보드 열기 · 우클릭하여 표시 설정")
        } else {
            format!("{core_tip}CPU {value} · 0–100%\nClick for dashboard · Right-click for display options")
        };
        if tip != self.last_tooltip {
            self.tray
                .set_tooltip(Some(&tip))
                .map_err(|e| e.to_string())?;
            self.last_tooltip = tip;
        }
        self.config = config;
        self.reconcile_menu();
        self.language = language;
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
