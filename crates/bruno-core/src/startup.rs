//! Shared startup readiness for orb loading state and deferred heavy work.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Minimum time to show the loading mood so it does not flash.
pub const MIN_LOADING_SECS: f32 = 1.2;

pub struct StartupGate {
    started: Instant,
    daemon_ready: AtomicBool,
    ui_ready: AtomicBool,
}

impl StartupGate {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            started: Instant::now(),
            daemon_ready: AtomicBool::new(false),
            ui_ready: AtomicBool::new(false),
        })
    }

    pub fn mark_daemon_ready(&self) {
        self.daemon_ready.store(true, Ordering::Release);
    }

    pub fn mark_ui_ready(&self) {
        self.ui_ready.store(true, Ordering::Release);
    }

    pub fn is_ready(&self) -> bool {
        let daemon = self.daemon_ready.load(Ordering::Acquire);
        let ui = self.ui_ready.load(Ordering::Acquire);
        let min_elapsed = self.started.elapsed() >= Duration::from_secs_f32(MIN_LOADING_SECS);
        daemon && ui && min_elapsed
    }
}
