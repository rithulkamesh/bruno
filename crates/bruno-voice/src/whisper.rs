//! whisper.cpp transcription via the `whisper-rs` bindings.
//!
//! Resolves (and auto-downloads) a ggml model under `~/.config/bruno/models/`,
//! then transcribes 16 kHz mono PCM. Metal-accelerated on macOS.

use std::path::{Path, PathBuf};
use std::sync::Once;
use std::time::Duration;

use tracing::{info, warn};
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

use crate::config::WhisperConfig;

/// Skip transcription for buffers shorter than this (avoids whisper warnings on
/// near-empty input). 16 kHz × 0.1 s.
const MIN_SAMPLES: usize = 1_600;

pub struct WhisperEngine {
    ctx: WhisperContext,
    language: String,
}

impl WhisperEngine {
    /// Load the configured model, downloading it on first use if necessary.
    /// Blocking — call off the main thread.
    pub fn load(cfg: &WhisperConfig) -> Result<Self, String> {
        // Route whisper.cpp/GGML stderr spam through the `log` facade (silenced
        // unless trace logging is enabled). Safe to call once.
        static LOG_HOOK: Once = Once::new();
        LOG_HOOK.call_once(whisper_rs::install_logging_hooks);

        let path = resolve_model_path(cfg)?;
        let path_str = path.to_str().ok_or("model path is not valid UTF-8")?;
        info!("loading whisper model: {path_str}");
        let ctx = WhisperContext::new_with_params(path_str, WhisperContextParameters::default())
            .map_err(|e| format!("whisper context load failed: {e}"))?;
        Ok(Self {
            ctx,
            language: cfg.language.clone(),
        })
    }

    /// Transcribe 16 kHz mono PCM. Returns `None` for empty/too-short input or on error.
    pub fn transcribe(&self, samples_16k: &[f32]) -> Option<String> {
        if samples_16k.len() < MIN_SAMPLES {
            return None;
        }

        let mut state = self.ctx.create_state().ok()?;

        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_language(Some(&self.language));
        params.set_translate(false);
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        params.set_suppress_blank(true);
        // Non-speech annotations like "(clapping)" / "[keyboard clicking]" that
        // whisper emits on noise (e.g. typing) are stripped from the output
        // below via strip_annotations().
        let threads = std::thread::available_parallelism()
            .map(|n| n.get() as i32)
            .unwrap_or(4);
        params.set_n_threads(threads);

        if let Err(e) = state.full(params, samples_16k) {
            warn!("whisper transcription failed: {e}");
            return None;
        }

        let n = state.full_n_segments().ok()?;
        let mut out = String::new();
        for i in 0..n {
            if let Ok(seg) = state.full_get_segment_text(i) {
                out.push_str(&seg);
            }
        }
        // Belt-and-suspenders: drop any residual bracketed/parenthesised
        // sound annotations whisper still slips through on noise.
        let cleaned = strip_annotations(&out);
        let cleaned = cleaned.trim();
        if cleaned.is_empty() {
            return None;
        }
        Some(cleaned.to_string())
    }
}

/// Remove `(...)` and `[...]` groups (whisper's non-speech annotations) and
/// collapse the surrounding whitespace.
fn strip_annotations(s: &str) -> String {
    let mut out = String::new();
    let mut paren = 0i32;
    let mut brack = 0i32;
    for c in s.chars() {
        match c {
            '(' => paren += 1,
            ')' => paren = (paren - 1).max(0),
            '[' => brack += 1,
            ']' => brack = (brack - 1).max(0),
            _ if paren == 0 && brack == 0 => out.push(c),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn resolve_model_path(cfg: &WhisperConfig) -> Result<PathBuf, String> {
    if !cfg.model_path.is_empty() {
        let p = PathBuf::from(&cfg.model_path);
        return if p.exists() {
            Ok(p)
        } else {
            Err(format!("configured model_path does not exist: {}", p.display()))
        };
    }

    let dir = models_dir().ok_or("could not resolve home directory")?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("create models dir: {e}"))?;
    let file = dir.join(format!("ggml-{}.bin", cfg.model));
    if file.exists() {
        return Ok(file);
    }
    download_model(&cfg.model, &file)?;
    Ok(file)
}

fn models_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".config").join("bruno").join("models"))
}

fn download_model(model: &str, dest: &Path) -> Result<(), String> {
    let url = format!(
        "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-{model}.bin"
    );
    info!("downloading whisper model '{model}' (one-time) from {url}");

    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(600))
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client.get(&url).send().map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("download failed: HTTP {}", resp.status()));
    }
    let bytes = resp.bytes().map_err(|e| e.to_string())?;

    // Write to a temp path then rename, so an interrupted download can't be
    // mistaken for a complete model on the next run.
    let tmp = dest.with_extension("bin.part");
    std::fs::write(&tmp, &bytes).map_err(|e| format!("write model: {e}"))?;
    std::fs::rename(&tmp, dest).map_err(|e| format!("finalize model: {e}"))?;
    info!("whisper model saved to {}", dest.display());
    Ok(())
}
