#[cfg(target_os = "macos")]
mod macos;

#[cfg(target_os = "macos")]
pub use macos::{configure_window, global_cursor_physical, window_center_physical};

#[cfg(not(target_os = "macos"))]
pub fn configure_window(
    _window: &winit::window::Window,
    _all_spaces: bool,
    _always_on_top: bool,
    _click_through: bool,
) {
}

#[cfg(not(target_os = "macos"))]
pub fn global_cursor_physical(_scale: f64) -> Option<(f32, f32)> {
    None
}

#[cfg(not(target_os = "macos"))]
pub fn window_center_physical(_window: &winit::window::Window) -> Option<(f32, f32)> {
    None
}
