# Contributing to Bruno

Thanks for your interest! Bruno is a macOS-only desktop companion written in Rust.

## Building

```sh
cargo build --workspace      # default: whisper STT + Apple TTS, no native deps
cargo run -p bruno-app       # or: just run-lite
```

The default build needs nothing special — whisper.cpp is built automatically by
`whisper-rs`. macOS + a recent Xcode/CLT and `cmake` are required.

### Optional: Piper neural TTS (GPL)

Piper is opt-in because it links a native `libpiper`. Build it once:

```sh
git clone https://github.com/OHF-Voice/piper1-gpl
cd piper1-gpl/libpiper
cmake -Bbuild -DCMAKE_BUILD_TYPE=Release -DCMAKE_INSTALL_PREFIX="$HOME/.config/bruno/piper"
cmake --build build && cmake --install build
```

Then run with the feature (the build looks for libpiper at
`~/.config/bruno/piper`, or set `PIPER_DIR`):

```sh
cargo run -p bruno-app --features piper    # or: just run
```

## Layout

| Crate | Role |
|-------|------|
| `bruno-app` | Desktop app: GPU orb, glass HUD, global hotkey, event wiring, headless agent browser (WKWebView) |
| `bruno-ai` | LLM providers (Azure/OpenAI/Claude/Ollama/LM Studio) + the tool-using agent (RAG via Veclite, web tools) |
| `bruno-voice` | STT (whisper.cpp, Apple fallback), TTS (Apple, optional Piper), speaker verification |
| `bruno-daemon` | Screen capture + relevance classification |
| `bruno-orb` | Orb shader/renderer |
| `bruno-core` | Shared event bus, intents |

## Conventions

- There's no enforced `rustfmt` style yet — please match the surrounding code.
- Keep changes focused; one logical change per PR.
- `cargo build --workspace` and `cargo clippy` should be clean.

## Config

Runtime config lives at `~/.config/bruno/config.toml` (never commit it — it
holds API keys). See the README for the schema.

## License

By contributing you agree your work is licensed under **GPL-3.0-or-later**.
