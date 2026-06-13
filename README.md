# Bruno

A calm, minimal **desktop companion** for macOS — a floating orb that watches your
screen, notices when you drift off-task, and talks to you. Local-first voice in
and out, pluggable LLM providers, on-device speech recognition.

> ⚠️ **License: GPL-3.0-or-later.** Bruno can link [Piper](https://github.com/OHF-Voice/piper1-gpl)
> (and espeak-ng) for neural TTS, which are GPL-3.0.

## Workspace

| Crate | Role |
|-------|------|
| `bruno-app` | The desktop app: GPU orb (winit/wgpu), HUD, global hotkey, event wiring |
| `bruno-ai` | Multi-provider LLM layer — Ollama, OpenAI, Claude, Azure AI Foundry, LM Studio (streaming chat + vision) |
| `bruno-voice` | STT (whisper.cpp, Apple SFSpeech fallback) + TTS (Apple, optional Piper) + speaker verification |
| `bruno-daemon` | Screen capture + relevance classification (vision, via `bruno-ai`) |
| `bruno-orb` | The orb renderer (shaders, moods) |
| `bruno-core` | Shared event bus, intents, startup gating |

## Configuration

Everything lives in `~/.config/bruno/config.toml`:

```toml
[ai]
provider = "azure"            # ollama | openai | claude | azure | lmstudio

[ai.azure]
endpoint = "https://<resource>.cognitiveservices.azure.com"
api_key = "..."
deployment = "gpt-5-mini"
api_version = "2024-12-01-preview"

[stt]
backend = "auto"             # auto (whisper, Apple fallback) | whisper | apple
[stt.whisper]
model = "small.en"           # auto-downloaded to ~/.config/bruno/models/

[tts]
backend = "apple"            # apple | piper
[tts.piper]                  # only used with the `piper` cargo feature
install_dir = "/path/to/libpiper/install"   # has espeak-ng-data/ + dylibs
model_path = "/path/to/voice.onnx"
length_scale = 1.0
```

## Build

```sh
cargo build --workspace          # default: whisper STT + Apple TTS
```

### Optional: Piper neural TTS (GPL)

1. Build libpiper (its cmake downloads espeak-ng + onnxruntime):
   ```sh
   git clone https://github.com/OHF-Voice/piper1-gpl
   cd piper1-gpl/libpiper
   cmake -Bbuild -DCMAKE_BUILD_TYPE=Release -DCMAKE_INSTALL_PREFIX=$PWD/install
   cmake --build build && cmake --install build
   ```
2. Build Bruno against it and select the backend:
   ```sh
   PIPER_DIR=/abs/path/piper1-gpl/libpiper/install \
     cargo build -p bruno-app --features bruno-voice/piper
   ```
3. Set `[tts] backend = "piper"` + `[tts.piper]` paths in config.

## Controls

- **`⌘⇧B`** — toggle listening (global hotkey, works unfocused)
- **Click the orb** — toggle listening / HUD
- **`Esc`** — stop
- Voice: "enroll my voice" / "forget my voice"
