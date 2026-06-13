//! STT configuration from `~/.config/bruno/config.toml` (the `[stt]` table).
//!
//! Shares the file with `bruno-ai`; unknown tables (e.g. `[ai]`) are ignored.

use serde::Deserialize;
use tracing::{info, warn};

#[derive(Debug, Clone, Deserialize, Default)]
struct RootConfig {
    #[serde(default)]
    stt: SttConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SttConfig {
    #[serde(default)]
    pub backend: SttBackend,
    #[serde(default)]
    pub whisper: WhisperConfig,
}

impl Default for SttConfig {
    fn default() -> Self {
        Self {
            backend: SttBackend::default(),
            whisper: WhisperConfig::default(),
        }
    }
}

/// Which speech recogniser to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum SttBackend {
    /// Whisper, falling back to Apple SFSpeech if the model can't be loaded.
    #[default]
    Auto,
    /// Whisper only.
    Whisper,
    /// Apple SFSpeechRecognizer only (the original backend).
    Apple,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WhisperConfig {
    /// ggml model name, e.g. `small.en`, `base.en`, `tiny.en`, `medium.en`.
    #[serde(default = "default_model")]
    pub model: String,
    /// Explicit path to a ggml `.bin`. When empty the model is resolved (and
    /// auto-downloaded) under `~/.config/bruno/models/`.
    #[serde(default)]
    pub model_path: String,
    /// Transcription language hint.
    #[serde(default = "default_language")]
    pub language: String,
}

impl Default for WhisperConfig {
    fn default() -> Self {
        Self {
            model: default_model(),
            model_path: String::new(),
            language: default_language(),
        }
    }
}

fn default_model() -> String {
    "small.en".to_string()
}
fn default_language() -> String {
    "en".to_string()
}

impl SttConfig {
    pub fn load() -> Self {
        let Some(path) = config_path() else {
            warn!("could not resolve home dir; using default STT config");
            return Self::default();
        };
        let raw = match std::fs::read_to_string(&path) {
            Ok(raw) => raw,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Self::default(),
            Err(e) => {
                warn!("failed to read {}: {e}; default STT config", path.display());
                return Self::default();
            }
        };
        match toml::from_str::<RootConfig>(&raw) {
            Ok(cfg) => {
                info!(backend = ?cfg.stt.backend, model = %cfg.stt.whisper.model, "loaded STT config");
                cfg.stt
            }
            Err(e) => {
                warn!("failed to parse {}: {e}; default STT config", path.display());
                Self::default()
            }
        }
    }
}

fn config_path() -> Option<std::path::PathBuf> {
    dirs::home_dir().map(|h| h.join(".config").join("bruno").join("config.toml"))
}
