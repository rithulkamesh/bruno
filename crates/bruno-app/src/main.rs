mod browser;
mod calendar;
mod commands;
mod hud;
mod nudge;
mod orb_config;
mod orb_window;
mod platform;
mod wiring;

use std::sync::atomic::AtomicBool;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

use bruno_core::{BrunoBus, StartupGate};
use bruno_voice::VoiceService;
use tracing::info;

use commands::{HudCommand, MoodCommand, OrbGeometry};
use wiring::WiringState;

fn main() {
    tracing_subscriber::fmt::init();
    info!("bruno starting");

    let bus = BrunoBus::new(256);
    let (hud_tx, hud_rx) = mpsc::channel::<HudCommand>();
    let (mood_tx, mood_rx) = mpsc::channel::<MoodCommand>();
    let geometry = Arc::new(Mutex::new(OrbGeometry::default()));
    let voice_enabled = Arc::new(AtomicBool::new(false));
    let wiring_state = Arc::new(Mutex::new(WiringState::default()));

    let voice = VoiceService::new(bus.clone());
    let tts = voice.tts.clone();
    let tts_runtime = voice.tts_runtime;
    let stt = voice.stt.clone();

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");

    let (capture, capture_rx) = bruno_daemon::CaptureService::pair();
    let startup = StartupGate::new();

    rt.spawn(bruno_daemon::run(bus.clone(), capture, startup.clone()));

    // Headless in-app browser: handle goes to the agent (worker thread), the
    // receiver drives a WKWebView on the main thread (in orb_window).
    let (browser_handle, browser_rx) = browser::channel();
    let browser: Arc<dyn bruno_ai::Browser> = Arc::new(browser_handle);

    let hud_tx_w = hud_tx.clone();
    let mood_tx_w = mood_tx.clone();
    let state_w = wiring_state.clone();
    rt.spawn(wiring::run(
        bus,
        hud_tx_w,
        mood_tx_w,
        tts,
        state_w,
        browser,
    ));

    // Single winit event loop on the main thread (required on macOS).
    orb_window::run(orb_window::OrbParams {
        mood_rx,
        hud_rx,
        hud_tx,
        geometry,
        voice_enabled,
        wiring_state,
        tts_runtime,
        stt,
        capture_rx,
        startup,
        browser_rx,
    });
}
