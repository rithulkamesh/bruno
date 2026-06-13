//! Bruno activity daemon — window tracking, idle detection, screen capture, relevance.

mod capture;
mod capture_loop;
mod classify;
mod idle;
mod relevance;
mod window;

use std::sync::Arc;

use bruno_core::{BrunoBus, StartupGate};
use idle::IdleMonitor;
use tracing::info;

pub use capture::{CapturePollState, CaptureService, ScaledScreenshot, poll_capture_requests};
pub use idle::{IdleMonitor as IdleState, IDLE_THRESHOLD_SECS};
pub use window::WindowTracker;

pub async fn run(
    bus: BrunoBus,
    capture: CaptureService,
    startup: Arc<StartupGate>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    info!("bruno-daemon starting");
    let idle_monitor = IdleMonitor::new();
    let idle = idle_monitor.clone();

    tokio::spawn(idle::run_idle_monitor(bus.clone(), idle_monitor));
    tokio::spawn(window::run_window_tracker(bus.clone()));
    tokio::spawn(capture_loop::run_capture_loop(bus.clone(), idle, capture, startup.clone()));

    startup.mark_daemon_ready();
    info!("bruno-daemon ready");

    std::future::pending::<()>().await;
    Ok(())
}
