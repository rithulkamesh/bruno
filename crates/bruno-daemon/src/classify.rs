//! Multimodal relevance classification via the configured AI provider
//! (Ollama / OpenAI / Claude / Azure / LM Studio — see `~/.config/bruno/config.toml`).

use bruno_ai::provider::{self, Provider};
use bruno_ai::{Config, Message};
use serde::{Deserialize, Serialize};
use tracing::warn;

const SYSTEM_PROMPT: &str = "You are Bruno's attention monitor. The user is a developer \
building Bruno, a Rust desktop app. Determine if the current screen content is relevant to \
their work context.

RELEVANT: coding, terminals, documentation, GitHub, Claude, design tools, research, writing, \
any productivity tool.

IRRELEVANT: social media feeds, YouTube videos unrelated to work, games, entertainment streaming.

Respond ONLY with valid JSON:
{
  relevant: bool,
  confidence: f32,
  reason: string (max 8 words)
}";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Classification {
    pub relevant: bool,
    pub confidence: f32,
    pub reason: String,
}

pub struct Classifier {
    provider: Box<dyn Provider>,
}

impl Classifier {
    pub fn new() -> Self {
        let config = Config::load();
        Self {
            provider: provider::from_config(&config.ai),
        }
    }

    pub async fn classify(&self, window_title: &str, image_base64: &str) -> Option<Classification> {
        let user = Message::user_with_images(
            format!("Active window title: {window_title}"),
            vec![image_base64.to_string()],
        );

        let content = match self.provider.complete(SYSTEM_PROMPT, &[user]).await {
            Ok(content) => content,
            Err(e) => {
                warn!("classify request failed: {e}");
                return None;
            }
        };

        parse_classification(&content)
    }
}

impl Default for Classifier {
    fn default() -> Self {
        Self::new()
    }
}

fn parse_classification(content: &str) -> Option<Classification> {
    let trimmed = content.trim();
    if let Ok(c) = serde_json::from_str::<Classification>(trimmed) {
        return Some(c);
    }
    let start = trimmed.find('{')?;
    let end = trimmed.rfind('}')?;
    serde_json::from_str(&trimmed[start..=end]).ok()
}
