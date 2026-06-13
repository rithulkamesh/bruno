//! Ollama multimodal relevance classification.

use reqwest::Client;
use serde::{Deserialize, Serialize};
use tracing::warn;

const OLLAMA_URL: &str = "http://localhost:11434/api/chat";
const MODEL: &str = "gemma4";

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

#[derive(Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    messages: Vec<ChatMessage<'a>>,
    stream: bool,
}

#[derive(Serialize)]
struct ChatMessage<'a> {
    role: &'a str,
    content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    images: Option<Vec<&'a str>>,
}

#[derive(Deserialize)]
struct ChatResponse {
    message: Option<ResponseMessage>,
}

#[derive(Deserialize)]
struct ResponseMessage {
    content: String,
}

pub struct Classifier {
    client: Client,
}

impl Classifier {
    pub fn new() -> Self {
        Self {
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .unwrap_or_else(|_| Client::new()),
        }
    }

    pub async fn classify(&self, window_title: &str, image_base64: &str) -> Option<Classification> {
        let request = ChatRequest {
            model: MODEL,
            messages: vec![
                ChatMessage {
                    role: "system",
                    content: SYSTEM_PROMPT.to_string(),
                    images: None,
                },
                ChatMessage {
                    role: "user",
                    content: format!("Active window title: {window_title}"),
                    images: Some(vec![image_base64]),
                },
            ],
            stream: false,
        };

        let response = match self.client.post(OLLAMA_URL).json(&request).send().await {
            Ok(r) => r,
            Err(e) => {
                warn!("ollama classify request failed: {e}");
                return None;
            }
        };

        if !response.status().is_success() {
            warn!("ollama classify bad status: {}", response.status());
            return None;
        }

        let body: ChatResponse = match response.json().await {
            Ok(b) => b,
            Err(e) => {
                warn!("ollama classify parse failed: {e}");
                return None;
            }
        };

        let content = body.message?.content;
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
