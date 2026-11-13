//! Offscreen AppKit fixture: no status item, window, mouse or keyboard.
#[cfg(target_os = "macos")]
#[path = "../src/native_runner.rs"]
mod native_runner;
#[cfg(target_os = "macos")]
#[path = "../../../vendor/tray-icon-0.24.1/src/platform_impl/macos/target_layout.rs"]
mod target_layout;

// Imported native renderer unit tests expect the application's RGBA image
// constructor; this local equivalent owns its bitmap and does not create an
// application window, status item, or user input event.
#[cfg(all(test, target_os = "macos"))]
fn make_runner_native_image_from_rgba(
    _: peterfan_core::config::RunnerCharacter,
    _: usize,
    rgba: &[u8],
) -> Result<objc2::rc::Retained<objc2_app_kit::NSImage>, String> {
    use objc2::AllocAnyThread;
    use objc2_app_kit::{NSBitmapFormat, NSBitmapImageRep, NSDeviceRGBColorSpace, NSImage};
    use objc2_foundation::NSSize;
    if rgba.len() != 48 * 32 * 4 {
        return Err("invalid RGBA size".into());
    }
    let rep = unsafe {
        NSBitmapImageRep::initWithBitmapDataPlanes_pixelsWide_pixelsHigh_bitsPerSample_samplesPerPixel_hasAlpha_isPlanar_colorSpaceName_bitmapFormat_bytesPerRow_bitsPerPixel(
            NSBitmapImageRep::alloc(), std::ptr::null_mut(), 48, 32, 8, 4, true, false,
            NSDeviceRGBColorSpace, NSBitmapFormat::AlphaNonpremultiplied, 48 * 4, 32,
        )
    }.ok_or("bitmap allocation failed")?;
    if rep.bitmapData().is_null() || rep.bytesPerRow() != 48 * 4 || rep.pixelsHigh() != 32 {
        return Err("invalid bitmap storage".into());
    }
    unsafe {
        std::ptr::copy_nonoverlapping(rgba.as_ptr(), rep.bitmapData(), rgba.len());
    }
    let size = NSSize::new(27., 18.);
    rep.setSize(size);
    let image = NSImage::initWithSize(NSImage::alloc(), size);
    image.addRepresentation(&rep);
    image.setTemplate(true);
    Ok(image)
}

#[cfg(all(test, target_os = "macos"))]
fn make_runner_native_image_with_color(
    character: peterfan_core::config::RunnerCharacter,
    index: usize,
    rgba: &[u8],
    color: bool,
) -> Result<objc2::rc::Retained<objc2_app_kit::NSImage>, String> {
    let image = make_runner_native_image_from_rgba(character, index, rgba)?;
    image.setTemplate(!color);
    Ok(image)
}

#[cfg(target_os = "macos")]
fn main() {
    use objc2::AllocAnyThread;
    use objc2::MainThreadOnly;
    use objc2_app_kit::{NSCellImagePosition, NSImage, NSStatusBarButton, NSView};
    use objc2_foundation::{MainThreadMarker, NSPoint, NSRect, NSSize};
    use peterfan_core::config::MenubarDisplay;

    let mtm = MainThreadMarker::new().expect("offscreen fixture must run on main thread");
    let button = NSStatusBarButton::initWithFrame(
        NSStatusBarButton::alloc(mtm),
        NSRect::new(NSPoint::ZERO, NSSize::new(110.0, 24.0)),
    );
    let click_target = NSView::initWithFrame(NSView::alloc(mtm), button.bounds());
    button.addSubview(&click_target);
    target_layout::align_target(&click_target, &button);
    for width in [165.0, 84.0, 52.0, 110.0] {
        button.setFrameSize(NSSize::new(width, 24.0));
        assert_eq!(
            click_target.frame(),
            button.bounds(),
            "click region must track delayed AppKit resize"
        );
    }
    button.setImagePosition(NSCellImagePosition::ImageOnly);
    let image = |width| NSImage::initWithSize(NSImage::alloc(), NSSize::new(width, 22.0));
    let mut painter = native_runner::NativeRunnerPainter::default();
    painter.reconcile(&button, MenubarDisplay::Graph, true, false, 1, || {
        Some(image(33.0))
    });
    let old = painter
        .attachment_geometry()
        .expect("runner-only layer")
        .0
        .size;
    assert!(
        old.width > 0.0 && old.height > 0.0,
        "invalid initial image rect {old:?}"
    );
    painter.image_size_changed(Some(NSSize::new(33.0, 22.0)), Some(NSSize::new(67.0, 22.0)));
    painter.reconcile(&button, MenubarDisplay::Graph, true, false, 1, || {
        Some(image(67.0))
    });
    let larger = painter
        .attachment_geometry()
        .expect("CPU plus runner layer")
        .0
        .size;
    assert!(
        larger.width > old.width,
        "group image did not expand: {old:?} -> {larger:?}"
    );
    painter.image_size_changed(Some(NSSize::new(67.0, 22.0)), Some(NSSize::new(33.0, 22.0)));
    painter.reconcile(&button, MenubarDisplay::Graph, true, false, 1, || {
        Some(image(33.0))
    });
    let restored = painter
        .attachment_geometry()
        .expect("runner-only layer restored")
        .0
        .size;
    assert_eq!(restored, old, "runner-only image rect did not recover");
    painter.reconcile(&button, MenubarDisplay::Graph, true, false, 2, || {
        Some(image(67.0))
    });
    assert!(!painter.active(), "multiple screens use the image fallback");
    painter.paint(&image(67.0));
    assert_eq!(button.image().expect("fallback image").size().width, 67.0);
    painter.reconcile(&button, MenubarDisplay::Graph, true, true, 1, || {
        Some(image(67.0))
    });
    assert!(!painter.active(), "Reduce Motion uses the image fallback");
    painter.reconcile(&button, MenubarDisplay::Number, false, false, 1, || {
        Some(image(32.0))
    });
    assert!(!painter.active(), "CPU-only must not attach a runner layer");
    println!("PASS offscreen AppKit composite painter geometry and fallback");
}

#[cfg(not(target_os = "macos"))]
fn main() {}
