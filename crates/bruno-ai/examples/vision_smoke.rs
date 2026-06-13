//! Vision check: send an image to the configured provider's complete() path.
//! Run: cargo run -p bruno-ai --example vision_smoke -- /tmp/shot.jpg

use base64::Engine;
use bruno_ai::provider;
use bruno_ai::{Config, Message};

const SYSTEM: &str =
    "Describe the screenshot in one sentence, then reply ONLY with JSON: {\"relevant\": bool, \
\"confidence\": f32, \"reason\": string}";

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    let path = std::env::args().nth(1).expect("usage: vision_smoke <image>");
    let bytes = std::fs::read(&path).expect("read image");
    let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);

    let cfg = Config::load();
    let p = provider::from_config(&cfg.ai);
    let msg = Message::user_with_images("Active window title: smoke test", vec![b64]);

    match p.complete(SYSTEM, &[msg]).await {
        Ok(text) => println!("RESPONSE:\n{text}"),
        Err(e) => println!("ERR: {e}"),
    }
}
