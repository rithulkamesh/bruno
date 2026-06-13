//! OpenAI Chat Completions client (streaming + non-streaming, with vision).
//!
//! The wire format here is also used by Azure AI Foundry ([`super::azure`]) and
//! LM Studio ([`super::lmstudio`]), so the request building and request loops are
//! exposed as [`stream_completions`] / [`complete_completions`] for reuse.

use async_trait::async_trait;
use reqwest::header::{HeaderMap, AUTHORIZATION, CONTENT_TYPE};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::{sse, Provider};
use crate::config::OpenAiConfig;
use crate::error::AiError;
use crate::message::Message;

const CHAT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

pub struct OpenAiProvider {
    client: Client,
    cfg: OpenAiConfig,
}

impl OpenAiProvider {
    pub fn new(cfg: OpenAiConfig) -> Self {
        Self {
            client: Client::builder()
                .timeout(CHAT_TIMEOUT)
                .build()
                .unwrap_or_else(|_| Client::new()),
            cfg,
        }
    }

    fn headers(&self) -> Result<HeaderMap, AiError> {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, "application/json".parse().unwrap());
        headers.insert(
            AUTHORIZATION,
            format!("Bearer {}", self.cfg.api_key)
                .parse()
                .map_err(|_| AiError::Config("invalid openai.api_key".into()))?,
        );
        Ok(headers)
    }
}

#[derive(Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    messages: Vec<WireMessage<'a>>,
    stream: bool,
}

#[derive(Serialize)]
struct WireMessage<'a> {
    role: &'a str,
    content: Value,
}

// --- streaming response shape ---
#[derive(Deserialize)]
struct StreamChunk {
    choices: Vec<StreamChoice>,
}
#[derive(Deserialize)]
struct StreamChoice {
    delta: Delta,
}
#[derive(Deserialize)]
struct Delta {
    content: Option<String>,
}

// --- non-streaming response shape ---
#[derive(Deserialize)]
struct CompletionResponse {
    choices: Vec<CompletionChoice>,
}
#[derive(Deserialize)]
struct CompletionChoice {
    message: CompletionMessage,
}
#[derive(Deserialize)]
struct CompletionMessage {
    content: Option<String>,
}

/// Build the OpenAI message array, prepending `system` and encoding any image
/// attachments as `image_url` data-URL parts.
fn wire_messages<'a>(system: &'a str, messages: &'a [Message]) -> Vec<WireMessage<'a>> {
    let mut out = Vec::with_capacity(messages.len() + 1);
    out.push(WireMessage {
        role: "system",
        content: Value::String(system.to_string()),
    });
    for m in messages {
        let content = if m.images.is_empty() {
            Value::String(m.content.clone())
        } else {
            let mut parts = vec![json!({ "type": "text", "text": m.content })];
            for img in &m.images {
                parts.push(json!({
                    "type": "image_url",
                    "image_url": { "url": format!("data:image/jpeg;base64,{img}") }
                }));
            }
            Value::Array(parts)
        };
        out.push(WireMessage {
            role: m.role.as_str(),
            content,
        });
    }
    out
}

/// Streaming Chat Completions request, parsing OpenAI delta events.
pub(crate) async fn stream_completions(
    client: &Client,
    url: &str,
    headers: HeaderMap,
    model: &str,
    system: &str,
    messages: &[Message],
    on_delta: &mut (dyn FnMut(String) + Send),
) -> Result<String, AiError> {
    let request = ChatRequest {
        model,
        messages: wire_messages(system, messages),
        stream: true,
    };

    let response = client
        .post(url)
        .headers(headers)
        .json(&request)
        .send()
        .await
        .map_err(|e| AiError::Request(e.to_string()))?;

    if !response.status().is_success() {
        return Err(AiError::Status(response.status().as_u16()));
    }

    let mut full = String::new();
    sse::read(response, |payload| {
        if let Ok(chunk) = serde_json::from_str::<StreamChunk>(payload) {
            if let Some(content) = chunk.choices.into_iter().next().and_then(|c| c.delta.content) {
                if !content.is_empty() {
                    full.push_str(&content);
                    on_delta(content);
                }
            }
        }
    })
    .await?;

    Ok(full)
}

/// Non-streaming Chat Completions request, returning the full message content.
pub(crate) async fn complete_completions(
    client: &Client,
    url: &str,
    headers: HeaderMap,
    model: &str,
    system: &str,
    messages: &[Message],
) -> Result<String, AiError> {
    let request = ChatRequest {
        model,
        messages: wire_messages(system, messages),
        stream: false,
    };

    let response = client
        .post(url)
        .headers(headers)
        .json(&request)
        .send()
        .await
        .map_err(|e| AiError::Request(e.to_string()))?;

    if !response.status().is_success() {
        return Err(AiError::Status(response.status().as_u16()));
    }

    let body: CompletionResponse = response
        .json()
        .await
        .map_err(|e| AiError::Decode(e.to_string()))?;
    Ok(body
        .choices
        .into_iter()
        .next()
        .and_then(|c| c.message.content)
        .unwrap_or_default())
}

#[async_trait]
impl Provider for OpenAiProvider {
    async fn chat_stream(
        &self,
        system: &str,
        messages: &[Message],
        on_delta: &mut (dyn FnMut(String) + Send),
    ) -> Result<String, AiError> {
        if self.cfg.api_key.is_empty() {
            return Err(AiError::Config("openai.api_key is not set".into()));
        }
        let url = format!("{}/chat/completions", self.cfg.base_url.trim_end_matches('/'));
        stream_completions(
            &self.client,
            &url,
            self.headers()?,
            &self.cfg.model,
            system,
            messages,
            on_delta,
        )
        .await
    }

    async fn complete(&self, system: &str, messages: &[Message]) -> Result<String, AiError> {
        if self.cfg.api_key.is_empty() {
            return Err(AiError::Config("openai.api_key is not set".into()));
        }
        let url = format!("{}/chat/completions", self.cfg.base_url.trim_end_matches('/'));
        complete_completions(
            &self.client,
            &url,
            self.headers()?,
            &self.cfg.model,
            system,
            messages,
        )
        .await
    }

    async fn is_available(&self) -> bool {
        !self.cfg.api_key.is_empty()
    }
}
