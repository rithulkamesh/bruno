//! The [`Provider`] abstraction and the per-backend implementations.

use async_trait::async_trait;

use crate::config::{AiConfig, ProviderKind};
use crate::error::AiError;
use crate::message::Message;

mod azure;
mod claude;
mod lmstudio;
mod ollama;
mod openai;
mod sse;

pub use azure::AzureProvider;
pub use claude::ClaudeProvider;
pub use lmstudio::LMStudioProvider;
pub use ollama::OllamaProvider;
pub use openai::OpenAiProvider;

/// A streaming chat backend. Implementations stream assistant text and invoke
/// `on_delta` with each incremental chunk (the delta, not the accumulated text).
#[async_trait]
pub trait Provider: Send + Sync {
    /// Stream a completion for `messages` under `system`. Returns the full
    /// assistant text on success. `on_delta` receives each new token chunk.
    async fn chat_stream(
        &self,
        system: &str,
        messages: &[Message],
        on_delta: &mut (dyn FnMut(String) + Send),
    ) -> Result<String, AiError>;

    /// Cheap reachability/credential check used to decide whether to attempt a chat.
    async fn is_available(&self) -> bool;
}

/// Build the provider selected by `cfg`.
pub fn from_config(cfg: &AiConfig) -> Box<dyn Provider> {
    match cfg.provider {
        ProviderKind::Ollama => Box::new(OllamaProvider::new(cfg.ollama.clone())),
        ProviderKind::OpenAi => Box::new(OpenAiProvider::new(cfg.openai.clone())),
        ProviderKind::Claude => Box::new(ClaudeProvider::new(cfg.claude.clone())),
        ProviderKind::Azure => Box::new(AzureProvider::new(cfg.azure.clone())),
        ProviderKind::LmStudio => Box::new(LMStudioProvider::new(cfg.lmstudio.clone())),
    }
}
