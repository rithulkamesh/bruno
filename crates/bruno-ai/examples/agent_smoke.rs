//! Exercise the tool-using agent against the configured provider.
//! Run: cargo run -p bruno-ai --example agent_smoke -- "your question"

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    let cfg = bruno_ai::Config::load();
    let Some(agent) = bruno_ai::Agent::from_config(&cfg) else {
        eprintln!("agent unavailable for this provider (need Azure/OpenAI)");
        return;
    };
    let q = std::env::args().nth(1).unwrap_or_else(|| {
        "Search the web for the current stable Rust compiler version and tell me what it is.".into()
    });
    println!("Q: {q}\n");
    match agent.run(&q).await {
        Ok(text) => println!("\nANSWER: {text}"),
        Err(e) => println!("\nERR: {e}"),
    }
}
