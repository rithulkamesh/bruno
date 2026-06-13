//! Active window title and app name polling.

use std::time::{SystemTime, UNIX_EPOCH};

use bruno_core::{ActivityEvent, BrunoBus};
use tracing::debug;

#[cfg(target_os = "macos")]
mod platform {
    use core_foundation::base::{CFType, TCFType};
    use core_foundation::dictionary::CFDictionary;
    use core_foundation::number::CFNumber;
    use core_foundation::string::CFString;
    use core_graphics::window::{
        kCGNullWindowID, kCGWindowListExcludeDesktopElements, kCGWindowListOptionOnScreenOnly,
        CGWindowListCopyWindowInfo,
    };

    pub fn active_window() -> Option<(String, String)> {
        unsafe {
            let info_list = CGWindowListCopyWindowInfo(
                kCGWindowListOptionOnScreenOnly | kCGWindowListExcludeDesktopElements,
                kCGNullWindowID,
            );
            if info_list.is_null() {
                return None;
            }
            let array = core_foundation::array::CFArray::<CFDictionary<CFString, CFType>>::wrap_under_create_rule(
                info_list as _,
            );
            let layer_key = CFString::from_static_string("kCGWindowLayer");
            let name_key = CFString::from_static_string("kCGWindowName");
            let owner_key = CFString::from_static_string("kCGWindowOwnerName");

            for dict in array.iter() {
                let layer = dict
                    .find(&layer_key)
                    .and_then(|v| v.downcast::<CFNumber>())
                    .and_then(|n| n.to_i32())
                    .unwrap_or(-1);
                if layer != 0 {
                    continue;
                }
                let app_name = dict
                    .find(&owner_key)
                    .and_then(|v| v.downcast::<CFString>())
                    .map(|s| s.to_string())
                    .filter(|s| !s.is_empty())?;
                let title = dict
                    .find(&name_key)
                    .and_then(|v| v.downcast::<CFString>())
                    .map(|s| s.to_string())
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| app_name.clone());
                return Some((app_name, title));
            }
            None
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod platform {
    pub fn active_window() -> Option<(String, String)> {
        None
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub struct WindowTracker {
    last_app: Option<String>,
    last_title: Option<String>,
}

impl WindowTracker {
    pub fn new() -> Self {
        Self {
            last_app: None,
            last_title: None,
        }
    }

    pub fn poll(&mut self, bus: &BrunoBus) -> Option<(String, String)> {
        let (app, title) = platform::active_window()?;
        let changed =
            self.last_app.as_ref() != Some(&app) || self.last_title.as_ref() != Some(&title);
        if changed {
            debug!(app = %app, title = %title, "window changed");
            bus.emit_activity(ActivityEvent::WindowChanged {
                app: app.clone(),
                title: title.clone(),
                timestamp: now_secs(),
            });
            self.last_app = Some(app.clone());
            self.last_title = Some(title.clone());
        }
        Some((app, title))
    }

    pub fn current(&self) -> Option<(String, String)> {
        match (&self.last_app, &self.last_title) {
            (Some(app), Some(title)) => Some((app.clone(), title.clone())),
            _ => None,
        }
    }
}

impl Default for WindowTracker {
    fn default() -> Self {
        Self::new()
    }
}

pub async fn run_window_tracker(bus: BrunoBus) {
    let mut tracker = WindowTracker::new();
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(1));
    loop {
        interval.tick().await;
        tracker.poll(&bus);
    }
}
