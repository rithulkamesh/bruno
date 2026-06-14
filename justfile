# Bruno dev tasks — run `just <task>`. Install just: `brew install just`

# List tasks
default:
    @just --list

# Run the app with Apple TTS (no native deps — works on a fresh clone)
run-lite:
    cargo run -p bruno-app

# Run with Piper neural TTS (needs libpiper at ~/.config/bruno/piper — see README)
run:
    cargo run -p bruno-app --features piper

# Build the whole workspace (default features)
build:
    cargo build --workspace

# Run tests
test:
    cargo test --workspace

# Lint
clippy:
    cargo clippy --workspace

# Verify the headless WKWebView agent browser end-to-end
browser-test:
    BRUNO_BROWSER_TEST=1 cargo run -p bruno-app --features piper
