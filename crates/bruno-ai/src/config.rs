//! Configuration loaded from `~/.config/bruno/config.toml`.
//!
//! Everything is optional. A missing file (or a missing `[ai]` table) falls back
//! to a local Ollama provider so the app works out of the box with no config.

use serde::Deserialize;
use tracing::{info, warn};

/// Default system prompt — Bruno's persona. Overridable via `ai.system_prompt`.
pub const DEFAULT_SYSTEM_PROMPT: &str =
    "You are Bruno, a calm desktop companion who talks to a developer with ADHD out loud — your \
words are spoken aloud, so write like a real person speaking, not like a chatbot. Keep it to one \
or two short, natural sentences. Be warm but understated, a little dry, never peppy or coachy. \
Talk like a friend sitting next to them, not an assistant: contractions, plain words, no filler \
like \"Sure!\" or \"Of course!\". Never use markdown, bullet points, emoji, code, JSON, headings, \
or quotation marks around your reply — just say the thing. Don't narrate that you're an AI or \
explain your reasoning. When they drift off-task, nudge gently and briefly; otherwise just be \
good company.";

#[derive(Debug, Clone, Deserialize, Default)]
pub struct Config {
    #[serde(default)]
    pub ai: AiConfig,
    /// Neurodivergence-aware behaviour: how (and how often) Bruno nudges.
    #[serde(default)]
    pub neuro: NeuroConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AiConfig {
    #[serde(default)]
    pub provider: ProviderKind,
    /// Overrides [`DEFAULT_SYSTEM_PROMPT`] when set.
    pub system_prompt: Option<String>,
    #[serde(default)]
    pub ollama: OllamaConfig,
    #[serde(default)]
    pub openai: OpenAiConfig,
    #[serde(default)]
    pub claude: ClaudeConfig,
    #[serde(default)]
    pub azure: AzureConfig,
    #[serde(default)]
    pub lmstudio: LMStudioConfig,
}

impl Default for AiConfig {
    fn default() -> Self {
        Self {
            provider: ProviderKind::default(),
            system_prompt: None,
            ollama: OllamaConfig::default(),
            openai: OpenAiConfig::default(),
            claude: ClaudeConfig::default(),
            azure: AzureConfig::default(),
            lmstudio: LMStudioConfig::default(),
        }
    }
}

impl AiConfig {
    pub fn system_prompt(&self) -> &str {
        self.system_prompt
            .as_deref()
            .unwrap_or(DEFAULT_SYSTEM_PROMPT)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ProviderKind {
    #[default]
    Ollama,
    OpenAi,
    Claude,
    Azure,
    #[serde(rename = "lmstudio")]
    LmStudio,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OllamaConfig {
    #[serde(default = "ollama_url")]
    pub url: String,
    #[serde(default = "ollama_model")]
    pub model: String,
}

impl Default for OllamaConfig {
    fn default() -> Self {
        Self {
            url: ollama_url(),
            model: ollama_model(),
        }
    }
}

fn ollama_url() -> String {
    "http://localhost:11434".to_string()
}
fn ollama_model() -> String {
    "gemma4".to_string()
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct OpenAiConfig {
    #[serde(default)]
    pub api_key: String,
    #[serde(default = "openai_model")]
    pub model: String,
    /// Defaults to the public OpenAI endpoint; override for compatible gateways.
    #[serde(default = "openai_base_url")]
    pub base_url: String,
}

fn openai_model() -> String {
    "gpt-4o".to_string()
}
fn openai_base_url() -> String {
    "https://api.openai.com/v1".to_string()
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ClaudeConfig {
    #[serde(default)]
    pub api_key: String,
    #[serde(default = "claude_model")]
    pub model: String,
}

fn claude_model() -> String {
    "claude-opus-4-8".to_string()
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct AzureConfig {
    /// e.g. `https://my-resource.services.ai.azure.com`
    #[serde(default)]
    pub endpoint: String,
    #[serde(default)]
    pub api_key: String,
    /// The deployed model/deployment name.
    #[serde(default)]
    pub deployment: String,
    #[serde(default = "azure_api_version")]
    pub api_version: String,
    /// Deployment name for the embeddings model (used by the agent's RAG memory).
    #[serde(default = "azure_embedding_deployment")]
    pub embedding_deployment: String,
}

fn azure_embedding_deployment() -> String {
    "text-embedding-3-small".to_string()
}

fn azure_api_version() -> String {
    "2024-10-21".to_string()
}

#[derive(Debug, Clone, Deserialize)]
pub struct LMStudioConfig {
    /// LM Studio's local OpenAI-compatible server base URL.
    #[serde(default = "lmstudio_base_url")]
    pub base_url: String,
    #[serde(default)]
    pub model: String,
    /// LM Studio ignores auth, but a key can be supplied for proxied setups.
    #[serde(default)]
    pub api_key: String,
}

impl Default for LMStudioConfig {
    fn default() -> Self {
        Self {
            base_url: lmstudio_base_url(),
            model: String::new(),
            api_key: String::new(),
        }
    }
}

fn lmstudio_base_url() -> String {
    "http://localhost:1234/v1".to_string()
}

/// Neurodivergence-aware nudging, after Deshmukh, *"Toward Neurodivergent-Aware
/// Productivity"* (CHItaly 2025, doi:10.1145/3750069.3750114): an adaptive,
/// privacy-first, human-in-the-loop feedback engine. The paper's core finding is
/// that for ADHD-affected users, *how* and *when* you interrupt matters more than
/// *that* you interrupt — nudges must be infrequent, shame-free, and respectful of
/// attention rhythms (hyperfocus, fatigue), with the user always in control.
///
/// All state stays on-device; these knobs shape the [`crate`] nudge gate.
#[derive(Debug, Clone, Deserialize)]
pub struct NeuroConfig {
    /// Master switch. When false, Bruno nudges with no adaptive gating.
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Communication profile — tunes tone and pacing of every nudge.
    #[serde(default)]
    pub profile: NeuroProfile,
    /// Minimum gap between two nudges (alarm-fatigue guard).
    #[serde(default = "neuro_cooldown")]
    pub nudge_cooldown_secs: u64,
    /// Hard ceiling on nudges within any rolling hour.
    #[serde(default = "neuro_max_per_hour")]
    pub max_nudges_per_hour: u32,
    /// How long "snooze"/"go away"/"taking a break" silences nudges, in minutes.
    #[serde(default = "neuro_snooze")]
    pub snooze_minutes: u64,
    /// Never interrupt while the user is in sustained focus (hyperfocus).
    #[serde(default = "default_true")]
    pub hyperfocus_protection: bool,
    /// Local-time window where nudges are suppressed, `"HH:MM-HH:MM"`. Empty = off.
    #[serde(default)]
    pub quiet_hours: String,
    /// Phrasing style for nudges. Always shame-free; this picks gentle vs. plain.
    #[serde(default)]
    pub tone: NudgeTone,
}

impl Default for NeuroConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            profile: NeuroProfile::default(),
            nudge_cooldown_secs: neuro_cooldown(),
            max_nudges_per_hour: neuro_max_per_hour(),
            snooze_minutes: neuro_snooze(),
            hyperfocus_protection: true,
            quiet_hours: String::new(),
            tone: NudgeTone::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum NeuroProfile {
    /// ADHD-affected: warm, low-pressure, one tiny next step (the paper's focus).
    #[default]
    Adhd,
    /// Autistic: clear, literal, predictable phrasing; no ambiguity or idiom.
    Autistic,
    /// Neurotypical / unspecified: neutral, concise.
    Generic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum NudgeTone {
    /// Soft, reassuring check-in.
    #[default]
    Gentle,
    /// Plain and brief — still non-judgmental, just less cushioning.
    Direct,
}

fn default_true() -> bool {
    true
}
fn neuro_cooldown() -> u64 {
    600
}
fn neuro_max_per_hour() -> u32 {
    4
}
fn neuro_snooze() -> u64 {
    30
}

impl Config {
    /// Load `~/.config/bruno/config.toml`. On any failure (missing file, parse
    /// error) returns defaults so the app still runs.
    pub fn load() -> Self {
        let Some(path) = config_path() else {
            warn!("could not resolve home dir; using default AI config");
            return Self::default();
        };

        let raw = match std::fs::read_to_string(&path) {
            Ok(raw) => raw,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                info!("no config at {}; using default AI config", path.display());
                return Self::default();
            }
            Err(e) => {
                warn!("failed to read {}: {e}; using default AI config", path.display());
                return Self::default();
            }
        };

        match toml::from_str::<Config>(&raw) {
            Ok(cfg) => {
                info!("loaded config from {}", path.display());
                cfg
            }
            Err(e) => {
                warn!("failed to parse {}: {e}; using default AI config", path.display());
                Self::default()
            }
        }
    }
}

fn config_path() -> Option<std::path::PathBuf> {
    dirs::home_dir().map(|h| h.join(".config").join("bruno").join("config.toml"))
}
