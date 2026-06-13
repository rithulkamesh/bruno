use core_graphics::display::CGDisplay;
use objc2_app_kit::{NSFloatingWindowLevel, NSEvent, NSView, NSWindow, NSWindowCollectionBehavior};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::window::Window;

pub fn configure_window(
    window: &Window,
    all_spaces: bool,
    always_on_top: bool,
    click_through: bool,
) {
    use winit::platform::macos::WindowExtMacOS;
    window.set_has_shadow(false);

    let Some(ns_window) = ns_window_from(window) else {
        return;
    };

    if all_spaces {
        let mut behavior = ns_window.collectionBehavior();
        behavior |= NSWindowCollectionBehavior::CanJoinAllSpaces;
        behavior |= NSWindowCollectionBehavior::Stationary;
        ns_window.setCollectionBehavior(behavior);
    }

    if always_on_top {
        ns_window.setLevel(NSFloatingWindowLevel);
        ns_window.setHidesOnDeactivate(false);
    }

    ns_window.setIgnoresMouseEvents(click_through);
}

pub fn global_cursor_physical(scale_factor: f64) -> Option<(f32, f32)> {
    let pt = NSEvent::mouseLocation();
    let main_h = CGDisplay::main().bounds().size.height;
    let y_top = main_h - pt.y;
    let scale = scale_factor as f32;
    Some((pt.x as f32 * scale, y_top as f32 * scale))
}

pub fn window_center_physical(window: &Window) -> Option<(f32, f32)> {
    let outer = window.outer_position().ok()?;
    let size = window.outer_size();
    Some((
        outer.x as f32 + size.width as f32 * 0.5,
        outer.y as f32 + size.height as f32 * 0.5,
    ))
}

pub fn ns_window_from(window: &Window) -> Option<objc2::rc::Retained<NSWindow>> {
    let handle = window.window_handle().ok()?;
    let RawWindowHandle::AppKit(appkit) = handle.as_raw() else {
        return None;
    };
    let view_ptr = appkit.ns_view.as_ptr().cast::<NSView>();
    if view_ptr.is_null() {
        return None;
    }
    let view = unsafe { view_ptr.as_ref()? };
    view.window()
}
