//! [`AiClient`] — a stateful, provider-agnostic chat client.
//!
//! Holds conversation history and the system prompt, and delegates streaming to
//! the configured [`Provider`]. Drop-in replacement for the app's old
//! `OllamaClient`: `chat_stream` invokes `on_token` with the *accumulated* text
//! (not the delta), matching the previous HUD behaviour.

use crate::config::Config;
use crate::error::AiError;
use crate::message::Message;
use crate::provider::{self, Provider};

pub struct AiClient {
    provider: Box<dyn Provider>,
    system_prompt: String,
    history: Vec<Message>,
}

impl AiClient {
    /// Build a client from loaded [`Config`] (provider, credentials, persona).
    pub fn from_config(cfg: &Config) -> Self {
        Self {
            provider: provider::from_config(&cfg.ai),
            system_prompt: cfg.ai.system_prompt().to_string(),
            history: Vec::new(),
        }
    }

    /// Convenience: load `~/.config/bruno/config.toml` and build a client.
    pub fn from_default_config() -> Self {
        Self::from_config(&Config::load())
    }

    /// Send `user_message`, streaming the reply. `on_token` receives the full
    /// accumulated response so far on each update. The exchange is appended to
    /// history on success.
    pub async fn chat_stream<F>(
        &mut self,
        user_message: &str,
        mut on_token: F,
    ) -> Result<String, AiError>
    where
        F: FnMut(&str) + Send,
    {
        self.history.push(Message::user(user_message));

        let mut full = String::new();
        let result = self
            .provider
            .chat_stream(&self.system_prompt, &self.history, &mut |delta: String| {
                full.push_str(&delta);
                on_token(&full);
            })
            .await;

        match result {
            Ok(text) => {
                self.history.push(Message::assistant(text.clone()));
                Ok(text)
            }
            Err(e) => {
                // Roll back the user turn so a failed exchange doesn't poison history.
                self.history.pop();
                Err(e)
            }
        }
    }

    /// Whether the configured provider is reachable / has credentials.
    pub async fn is_available(&self) -> bool {
        self.provider.is_available().await
    }

    /// Clear conversation history (keeps the system prompt and provider).
    pub fn reset(&mut self) {
        self.history.clear();
    }
}
