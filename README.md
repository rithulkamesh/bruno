# Bruno

[![CI](https://github.com/rithulkamesh/bruno/actions/workflows/ci.yml/badge.svg)](https://github.com/rithulkamesh/bruno/actions/workflows/ci.yml)
[![License: GPL v3](https://img.shields.io/badge/License-GPLv3-blue.svg)](LICENSE)
![Platform: macOS](https://img.shields.io/badge/platform-macOS-lightgrey)

A calm, minimal **desktop companion** for macOS — a floating orb that watches your
screen, notices when you drift off-task, and talks to you. Local-first voice in
and out, pluggable LLM providers, on-device speech recognition, and a tool-using
agent with web browsing and Veclite-backed RAG memory.

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

[neuro]                      # neurodivergence-aware nudging (all on-device)
enabled = true
profile = "adhd"             # adhd | autistic | generic — tunes tone & pacing
nudge_cooldown_secs = 600    # min gap between nudges (alarm-fatigue guard)
max_nudges_per_hour = 4      # hard rolling-hour ceiling
snooze_minutes = 30          # "snooze"/"go away"/"take a break" silence window
hyperfocus_protection = true # never interrupt sustained focus
quiet_hours = "22:00-08:00"  # local-time window with no nudges ("" = off)
tone = "gentle"              # gentle | direct (both shame-free)
```

The `[neuro]` defaults are sane, so this whole table is optional — leave it out
and Bruno still nudges gently, infrequently, and shame-free.

## Build

```sh
cargo build --workspace      # default: whisper STT + Apple TTS, no native deps
cargo run -p bruno-app       # or: just run-lite
```

Needs macOS, a recent Xcode/Command Line Tools, and `cmake` (for whisper.cpp).
No API keys or extra setup required for the default build.

### Optional: Piper neural TTS (GPL)

Piper is opt-in (it links a native `libpiper`). Build it once into the path
Bruno looks for:

```sh
git clone https://github.com/OHF-Voice/piper1-gpl
cd piper1-gpl/libpiper
cmake -Bbuild -DCMAKE_BUILD_TYPE=Release -DCMAKE_INSTALL_PREFIX="$HOME/.config/bruno/piper"
cmake --build build && cmake --install build
```

Then run with the feature (set `[tts] backend = "piper"` in config first):

```sh
just run                     # = cargo run -p bruno-app --features piper
```

The build auto-discovers libpiper at `~/.config/bruno/piper`; override with
`PIPER_DIR`.

## Agent & memory

Bruno's conversational replies go through a tool-using agent (Azure/OpenAI
function-calling). It can **search the web** and **read pages** via a headless
in-app `WKWebView` (no window, full JS), and **remember/recall** facts in a
local [Veclite](https://crates.io/crates/veclite-db) vector store at
`~/.config/bruno/memory.vlt` (embeddings via Azure `text-embedding-3-small`).
So *"pull up research on X"* actually searches, reads, and answers.

## Neurodivergence-aware nudging

Bruno is built for the way ADHD attention actually works, following Deshmukh's
human-in-the-loop framework (see [Research](#research)). The daemon decides
*whether* you've drifted; a [nudge policy](crates/bruno-app/src/nudge.rs) decides
whether interrupting *right now* is actually kind:

- **Non-disruptive** — a cooldown plus an hourly cap stop alarm fatigue. A
  suppressed nudge is fully silent (no HUD, no voice, no mood flicker).
- **Respects attention rhythms** — sustained focus (hyperfocus) and quiet hours
  suppress nudges entirely.
- **You're in control** — "snooze" / "go away" / "take a break" silences Bruno
  for a configurable window; `Intent::Break` does too.
- **Shame-free, adaptive tone** — the spoken nudge adapts to your `profile`
  (ADHD / autistic / generic) and `tone`, never judging you for drifting.

All of this is in-memory and on-device — no behavioral data leaves your Mac.

## Controls

- **`⌘⇧B`** — toggle listening (global hotkey, works unfocused)
- **Click the orb** — toggle listening / HUD
- **`Esc`** — stop
- Voice: "enroll my voice" / "forget my voice" / "snooze"

## Research

Bruno's nudging design follows:

> Raghavendra Deshmukh. 2025. *Toward Neurodivergent-Aware Productivity: A
> Systems and AI-Based Human-in-the-Loop Framework for ADHD-Affected
> Professionals.* In Proceedings of the 16th Biannual Conference of the Italian
> SIGCHI Chapter (CHItaly 2025). ACM.
> [doi:10.1145/3750069.3750114](https://doi.org/10.1145/3750069.3750114)
> ([open-access preprint](https://arxiv.org/abs/2507.06864))

The paper proposes a privacy-first, on-device assistant with three parts —
behavior sensing, a voice interface, and an adaptive feedback engine — that
adapts the *tone, timing, and content* of nudges to a user's fluctuating
attention. Bruno maps those onto `bruno-daemon` (sensing), `bruno-voice` (voice),
and the `neuro` nudge policy (adaptive feedback).
