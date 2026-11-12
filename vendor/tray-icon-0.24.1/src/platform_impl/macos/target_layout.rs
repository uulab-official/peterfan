use objc2_app_kit::{NSAutoresizingMaskOptions, NSView};

pub(crate) fn align_target(target: &NSView, button: &NSView) {
    target.setFrame(button.bounds());
    target.setAutoresizingMask(
        NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable,
    );
}
