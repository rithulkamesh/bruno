//! Throwaway: one real streamed request against the configured provider.
//! Run with: cargo run -p bruno-ai --example smoke

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let mut client = bruno_ai::AiClient::from_default_config();
    println!("available: {}", client.is_available().await);

    let result = client
        .chat_stream("Say hello in exactly three words.", |full| {
            print!("\r\x1b[K{full}");
            use std::io::Write;
            let _ = std::io::stdout().flush();
        })
        .await;

    println!();
    match result {
        Ok(text) => println!("OK ({} chars): {text:?}", text.len()),
        Err(e) => println!("ERR: {e}"),
    }
}
