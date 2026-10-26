//! A single-screen renderer that keeps animation out of NSStatusItem's
//! expensive cross-appearance replicant snapshot path. Multiple screens use
//! the normal image renderer so every menu bar retains its animated glyph.

use block2::RcBlock;
use objc2::rc::Retained;
use objc2_app_kit::{NSAppearanceCustomization, NSColor, NSImage, NSStatusBarButton};
use objc2_foundation::{NSPoint, NSRect};
use objc2_quartz_core::{CALayer, CATransaction};
use peterfan_core::config::MenubarDisplay;

pub(crate) fn eligible(
    display: MenubarDisplay,
    primary_visible: bool,
    reduce_motion: bool,
    screens: usize,
) -> bool {
    primary_visible
        && screens == 1
        && !reduce_motion
        && matches!(display, MenubarDisplay::Graph | MenubarDisplay::Both)
}

#[derive(Default)]
pub(crate) struct NativeRunnerPainter {
    layer: Option<RunnerLayer>,
    button: Option<Retained<NSStatusBarButton>>,
}

impl NativeRunnerPainter {
    pub(crate) fn active(&self) -> bool {
        self.layer.is_some()
    }

    pub(crate) fn attachment_geometry(&self) -> Option<(NSRect, f64)> {
        self.layer
            .as_ref()
            .map(|layer| (layer.color.frame(), layer.mask.contentsScale()))
    }

    pub(crate) fn bind(&mut self, button: &Retained<NSStatusBarButton>) -> bool {
        if self
            .button
            .as_ref()
            .is_some_and(|current| std::ptr::eq(&**current, &**button))
        {
            return false;
        }
        self.detach();
        self.button = Some(button.clone());
        true
    }

    /// Own the attachment only while the primary runner can be displayed.
    /// Reattaching reinstalls the layout image before synchronizing geometry.
    pub(crate) fn reconcile(
        &mut self,
        button: &Retained<NSStatusBarButton>,
        display: MenubarDisplay,
        primary_visible: bool,
        reduce_motion: bool,
        screens: usize,
        placeholder: impl FnOnce() -> Option<Retained<NSImage>>,
    ) -> bool {
        let was_active = self.active();
        let button_changed = self.bind(button);
        if eligible(display, primary_visible, reduce_motion, screens) {
            if !self.active() {
                if let Some(image) = placeholder() {
                    self.attach(&image);
                }
            }
        } else {
            self.detach();
        }
        self.sync();
        button_changed || was_active != self.active()
    }

    pub(crate) fn attach(&mut self, placeholder: &NSImage) -> bool {
        let Some(button) = self.button.as_ref() else {
            return false;
        };
        button.setWantsLayer(true);
        let Some(parent) = button.layer() else {
            return false;
        };
        let layer = RunnerLayer::new();
        // The button still owns layout, clicks, accessibility and title. A
        // transparent image reserves exactly its existing image rectangle.
        button.setImage(Some(placeholder));
        parent.addSublayer(&layer.color);
        self.layer = Some(layer);
        self.sync();
        if !self
            .attachment_geometry()
            .is_some_and(|(rect, _)| rect.size.width > 0.0 && rect.size.height > 0.0)
        {
            self.detach();
            return false;
        }
        true
    }

    pub(crate) fn detach(&mut self) {
        if let Some(layer) = self.layer.take() {
            layer.color.removeFromSuperlayer();
            if let Some(button) = &self.button {
                button.setImage(None);
            }
        }
    }

    pub(crate) fn sync(&self) {
        let Some(button) = self.button.as_ref() else {
            return;
        };
        let Some(layer) = &self.layer else {
            return;
        };
        let Some(cell) = button.cell() else {
            return;
        };
        let bounds = button.bounds();
        let image_rect = cell.imageRectForBounds(bounds);
        let parent_flipped = button
            .layer()
            .is_some_and(|parent| parent.isGeometryFlipped());
        let rect = image_rect_in_layer(bounds, image_rect, button.isFlipped(), parent_flipped);
        CATransaction::begin();
        CATransaction::setDisableActions(true);
        layer.color.setFrame(rect);
        layer.mask.setFrame(NSRect::new(NSPoint::ZERO, rect.size));
        let scale = button
            .window()
            .map_or(1.0, |window| window.backingScaleFactor());
        layer.color.setContentsScale(scale);
        layer.mask.setContentsScale(scale);
        // Dynamic NSColors must resolve in this menu bar's actual appearance,
        // which can differ from the application's appearance or system theme.
        let appearance = button.effectiveAppearance();
        let resolve = RcBlock::new(|| {
            let color = if cell.isHighlighted() {
                NSColor::selectedMenuItemTextColor()
            } else {
                NSColor::labelColor()
            }
            .CGColor();
            layer.color.setBackgroundColor(Some(&color));
        });
        appearance.performAsCurrentDrawingAppearance(&resolve);
        CATransaction::commit();
    }

    pub(crate) fn paint(&self, image: &NSImage) {
        let Some(button) = self.button.as_ref() else {
            return;
        };
        if let Some(layer) = &self.layer {
            layer.paint(image);
        } else {
            button.setImage(Some(image));
        }
    }
}

impl Drop for NativeRunnerPainter {
    fn drop(&mut self) {
        self.detach();
    }
}

struct RunnerLayer {
    color: Retained<CALayer>,
    mask: Retained<CALayer>,
}

impl RunnerLayer {
    fn new() -> Self {
        let color = CALayer::new();
        let mask = CALayer::new();
        // Both layers are ours. The mask has no superlayer, as CALayer requires.
        unsafe { color.setMask(Some(&mask)) };
        Self { color, mask }
    }

    fn paint(&self, image: &NSImage) {
        CATransaction::begin();
        CATransaction::setDisableActions(true);
        // CALayer.contents explicitly supports NSImage on macOS. It retains
        // the immutable image; no temporary pixel pointer crosses this call.
        unsafe { self.mask.setContents(Some(image)) };
        CATransaction::commit();
    }
}

fn image_rect_in_layer(
    bounds: NSRect,
    mut image: NSRect,
    view_flipped: bool,
    layer_flipped: bool,
) -> NSRect {
    if view_flipped != layer_flipped {
        image.origin.y = bounds.origin.y + bounds.size.height
            - (image.origin.y - bounds.origin.y)
            - image.size.height;
    }
    image
}

#[cfg(test)]
mod tests {
    use super::*;
    use objc2::AllocAnyThread;
    use objc2_core_graphics::{
        CGBitmapContextCreate, CGColorSpace, CGContext, CGImageAlphaInfo, CGImageByteOrderInfo,
    };
    use objc2_foundation::NSSize;

    fn composite(layer: &CALayer) -> Vec<u8> {
        const WIDTH: usize = 48;
        const HEIGHT: usize = 32;
        let mut rgba = vec![0; WIDTH * HEIGHT * 4];
        let space = CGColorSpace::new_device_rgb().expect("RGB color space");
        // The context has exactly WIDTH * HEIGHT RGBA8 bytes and is dropped
        // before the owned buffer is read or moved.
        let context = unsafe {
            CGBitmapContextCreate(
                rgba.as_mut_ptr().cast(),
                WIDTH,
                HEIGHT,
                8,
                WIDTH * 4,
                Some(&space),
                CGImageAlphaInfo::PremultipliedLast.0 | CGImageByteOrderInfo::Order32Big.0,
            )
        }
        .expect("owned RGBA bitmap context");
        CGContext::scale_ctm(Some(&context), WIDTH as f64 / 27.0, HEIGHT as f64 / 18.0);
        layer.renderInContext(&context);
        drop(context);
        rgba
    }

    #[test]
    fn native_mask_composites_visible_upright_frames_with_preserved_alpha() {
        let layer = RunnerLayer::new();
        let rect = NSRect::new(NSPoint::ZERO, NSSize::new(27.0, 18.0));
        layer.color.setFrame(rect);
        layer.mask.setFrame(rect);
        layer.mask.setContentsScale(48.0 / 27.0);
        let white = NSColor::whiteColor().CGColor();
        layer.color.setBackgroundColor(Some(&white));
        let mut rendered = Vec::new();
        for (left, top, alpha) in [(4, 3, 255), (30, 21, 128)] {
            let mut source = vec![0; 48 * 32 * 4];
            for y in top..top + 7 {
                for x in left..left + 10 {
                    source[(y * 48 + x) * 4..(y * 48 + x) * 4 + 4]
                        .copy_from_slice(&[255, 255, 255, alpha]);
                }
            }
            let image = crate::make_runner_native_image_from_rgba(
                peterfan_core::config::RunnerCharacter::Cat,
                0,
                &source,
            )
            .expect("asymmetric native source frame");
            layer.paint(&image);
            let pixels = composite(&layer.color);
            assert!(
                pixels.as_chunks::<4>().0.iter().any(|pixel| pixel[3] > 0),
                "native mask must produce visible pixels"
            );
            let center = ((top + 3) * 48 + left + 5) * 4;
            assert!(
                (i16::from(pixels[center + 3]) - i16::from(alpha)).abs() <= 2,
                "upright source center alpha: wanted {alpha}, got {}",
                pixels[center + 3]
            );
            assert!(
                pixels[center] >= alpha.saturating_sub(2),
                "white tint preserves premultiplied alpha"
            );
            let opposite = ((31 - top - 3) * 48 + (47 - left - 5)) * 4;
            assert_eq!(
                pixels[opposite + 3],
                0,
                "opposite quadrant stays transparent"
            );
            rendered.push(pixels);
        }
        assert_ne!(
            rendered[0], rendered[1],
            "advancing frames must change native pixels"
        );
    }

    #[test]
    fn scoped_menu_appearance_resolves_contrasting_native_tints() {
        use objc2_app_kit::{NSAppearance, NSAppearanceNameAqua, NSAppearanceNameDarkAqua};
        let mut tints = Vec::new();
        for name in unsafe { [NSAppearanceNameAqua, NSAppearanceNameDarkAqua] } {
            let appearance =
                NSAppearance::appearanceNamed(name).expect("standard system appearance");
            let color_layer = CALayer::new();
            color_layer.setFrame(NSRect::new(NSPoint::ZERO, NSSize::new(27.0, 18.0)));
            let resolve = RcBlock::new(|| {
                color_layer.setBackgroundColor(Some(&NSColor::labelColor().CGColor()))
            });
            appearance.performAsCurrentDrawingAppearance(&resolve);
            tints.push(composite(&color_layer));
        }
        assert!(tints[0][3] > 200);
        assert!(tints[1][3] > 200);
        let brightness =
            |pixels: &[u8]| u16::from(pixels[0]) + u16::from(pixels[1]) + u16::from(pixels[2]);
        assert!(
            brightness(&tints[0]) + 200 < brightness(&tints[1]),
            "Aqua and DarkAqua need contrasting glyph colors"
        );
    }

    #[test]
    fn detaching_and_dropping_painter_remove_owned_layer_from_native_parent() {
        let parent = CALayer::new();
        let layer = RunnerLayer::new();
        let retained_color = layer.color.clone();
        parent.addSublayer(&retained_color);
        let mut painter = NativeRunnerPainter {
            layer: Some(layer),
            button: None,
        };
        assert!(retained_color.superlayer().is_some());
        painter.detach();
        assert!(!painter.active());
        assert!(retained_color.superlayer().is_none());
        painter.detach();

        let next = RunnerLayer::new();
        let retained_next = next.color.clone();
        parent.addSublayer(&retained_next);
        painter.layer = Some(next);
        drop(painter);
        assert!(retained_next.superlayer().is_none());
    }

    #[test]
    fn layer_policy_keeps_every_display_animated_and_respects_reduce_motion() {
        for display in [
            MenubarDisplay::Number,
            MenubarDisplay::Graph,
            MenubarDisplay::Both,
        ] {
            for screens in [0, 1, 2, 3] {
                for reduce in [false, true] {
                    for visible in [false, true] {
                        assert_eq!(
                            eligible(display, visible, reduce, screens),
                            visible && screens == 1 && !reduce && display != MenubarDisplay::Number
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn image_rectangle_tracks_cell_layout_without_changing_width_or_scale() {
        let bounds = NSRect::new(NSPoint::ZERO, NSSize::new(84.0, 24.0));
        let image = NSRect::new(NSPoint::new(4.0, 2.0), NSSize::new(27.0, 18.0));
        assert_eq!(image_rect_in_layer(bounds, image, false, false), image);
        assert_eq!(image_rect_in_layer(bounds, image, true, true), image);
        assert_eq!(
            image_rect_in_layer(bounds, image, true, false),
            NSRect::new(NSPoint::new(4.0, 4.0), image.size)
        );
        assert_eq!(
            image_rect_in_layer(bounds, image, false, true),
            NSRect::new(NSPoint::new(4.0, 4.0), image.size)
        );
    }

    #[test]
    fn mask_frame_swaps_preserve_image_identity_without_implicit_animation() {
        let layer = RunnerLayer::new();
        let first = NSImage::initWithSize(NSImage::alloc(), NSSize::new(27.0, 18.0));
        let second = NSImage::initWithSize(NSImage::alloc(), NSSize::new(27.0, 18.0));
        layer.paint(&first);
        // Only NSImage values created above were assigned to this owned layer.
        let first_contents = unsafe { layer.mask.contents() }.expect("first contents");
        assert!(std::ptr::eq(
            &*first_contents,
            first.as_ref() as &objc2::runtime::AnyObject
        ));
        layer.paint(&second);
        let second_contents = unsafe { layer.mask.contents() }.expect("second contents");
        assert!(std::ptr::eq(
            &*second_contents,
            second.as_ref() as &objc2::runtime::AnyObject
        ));
        assert!(layer
            .mask
            .animationKeys()
            .is_none_or(|keys| keys.is_empty()));
        assert!(std::ptr::eq(
            &*layer.color.mask().expect("owned alpha mask"),
            layer.mask.as_ref()
        ));
    }
}
