use bruno_core::{BrunoBus, StartupGate};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    let bus = BrunoBus::new(256);
    let (capture, _capture_rx) = bruno_daemon::CaptureService::pair();
    let startup = StartupGate::new();
    if let Err(e) = bruno_daemon::run(bus, capture, startup).await {
        eprintln!("daemon error: {e}");
    }
}
