//! Idle time tracking via CGEventSource.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use bruno_core::{ActivityEvent, BrunoBus};
use tracing::debug;

pub const IDLE_THRESHOLD_SECS: f64 = 60.0;

#[cfg(target_os = "macos")]
mod platform {
    use objc2_core_graphics::{CGEventSource, CGEventSourceStateID, CGEventType};

    pub fn seconds_since_input() -> f64 {
        CGEventSource::seconds_since_last_event_type(
            CGEventSourceStateID::CombinedSessionState,
            CGEventType(u32::MAX),
        )
    }
}

#[cfg(not(target_os = "macos"))]
mod platform {
    pub fn seconds_since_input() -> f64 {
        0.0
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[derive(Clone)]
pub struct IdleMonitor {
    is_idle: Arc<AtomicBool>,
    idle_since: Arc<std::sync::Mutex<Option<u64>>>,
}

impl IdleMonitor {
    pub fn new() -> Self {
        Self {
            is_idle: Arc::new(AtomicBool::new(false)),
            idle_since: Arc::new(std::sync::Mutex::new(None)),
        }
    }

    pub fn is_idle(&self) -> bool {
        self.is_idle.load(Ordering::Relaxed)
    }

    pub fn poll(&self, bus: &BrunoBus) {
        let idle_secs = platform::seconds_since_input();
        let was_idle = self.is_idle.load(Ordering::Relaxed);
        let now_idle = idle_secs > IDLE_THRESHOLD_SECS;

        if now_idle && !was_idle {
            let since = now_secs().saturating_sub(idle_secs as u64);
            debug!(since, idle_secs, "idle started");
            *self.idle_since.lock().unwrap() = Some(since);
            self.is_idle.store(true, Ordering::Relaxed);
            bus.emit_activity(ActivityEvent::IdleStarted { since });
        } else if !now_idle && was_idle {
            let duration_secs = {
                let guard = self.idle_since.lock().unwrap();
                guard.map(|s| now_secs().saturating_sub(s)).unwrap_or(0)
            };
            debug!(duration_secs, "idle ended");
            *self.idle_since.lock().unwrap() = None;
            self.is_idle.store(false, Ordering::Relaxed);
            bus.emit_activity(ActivityEvent::IdleEnded { duration_secs });
        }
    }
}

impl Default for IdleMonitor {
    fn default() -> Self {
        Self::new()
    }
}

pub async fn run_idle_monitor(bus: BrunoBus, monitor: IdleMonitor) {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(1));
    loop {
        interval.tick().await;
        monitor.poll(&bus);
    }
}
