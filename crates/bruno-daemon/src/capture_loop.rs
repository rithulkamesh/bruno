//! Screen capture loop + Ollama relevance classification.

use std::sync::Arc;

use bruno_core::BrunoBus;
use bruno_core::StartupGate;
use tracing::{info, warn};

use crate::capture::CaptureService;
use crate::classify::Classifier;
use crate::idle::IdleMonitor;
use crate::relevance::RelevanceTracker;
use crate::window::WindowTracker;

pub async fn run_capture_loop(
    bus: BrunoBus,
    idle: IdleMonitor,
    capture: CaptureService,
    startup: Arc<StartupGate>,
) {
    while !startup.is_ready() {
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
    info!("capture loop active");

    let classifier = Classifier::new();
    let mut relevance = RelevanceTracker::new();
    let mut window_tracker = WindowTracker::new();
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(10));
    interval.tick().await;

    loop {
        interval.tick().await;

        if idle.is_idle() {
            continue;
        }

        let Some((app, title)) = window_tracker.poll(&bus) else {
            continue;
        };

        let Some(screenshot) = capture.capture_scaled().await else {
            warn!("screen capture failed");
            continue;
        };

        let Some(classification) = classifier.classify(&title, &screenshot.jpeg_base64).await else {
            continue;
        };

        relevance.process(&bus, classification, &app, &title);
    }
}
