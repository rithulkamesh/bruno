//! Anthropic Claude (Messages API) streaming client.
//!
//! Rust has no official Anthropic SDK, so this calls the Messages API over raw
//! HTTP: `POST https://api.anthropic.com/v1/messages` with `x-api-key` +
//! `anthropic-version` headers, `stream: true`, and SSE `content_block_delta`
//! events carrying `delta.text`.

use async_trait::async_trait;
use reqwest::header::{HeaderMap, CONTENT_TYPE};
use reqwest::Client;
use serde::{Deserialize, Serialize};

use super::{sse, Provider};
use crate::config::ClaudeConfig;
use crate::error::AiError;
use crate::message::Message;

const API_URL: &str = "https://api.anthropic.com/v1/messages";
const ANTHROPIC_VERSION: &str = "2023-06-01";
const MAX_TOKENS: u32 = 1024;
const CHAT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

pub struct ClaudeProvider {
    client: Client,
    cfg: ClaudeConfig,
}

impl ClaudeProvider {
    pub fn new(cfg: ClaudeConfig) -> Self {
        Self {
            client: Client::builder()
                .timeout(CHAT_TIMEOUT)
                .build()
                .unwrap_or_else(|_| Client::new()),
            cfg,
        }
    }
}

#[derive(Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    max_tokens: u32,
    system: &'a str,
    messages: Vec<WireMessage<'a>>,
    stream: bool,
}

#[derive(Serialize)]
struct WireMessage<'a> {
    role: &'a str,
    content: &'a str,
}

/// We only care about `content_block_delta` events carrying text deltas.
#[derive(Deserialize)]
#[serde(tag = "type")]
enum StreamEvent {
    #[serde(rename = "content_block_delta")]
    ContentBlockDelta { delta: Delta },
    #[serde(other)]
    Other,
}

#[derive(Deserialize)]
#[serde(tag = "type")]
enum Delta {
    #[serde(rename = "text_delta")]
    TextDelta { text: String },
    #[serde(other)]
    Other,
}

#[async_trait]
impl Provider for ClaudeProvider {
    async fn chat_stream(
        &self,
        system: &str,
        messages: &[Message],
        on_delta: &mut (dyn FnMut(String) + Send),
    ) -> Result<String, AiError> {
        if self.cfg.api_key.is_empty() {
            return Err(AiError::Config("claude.api_key is not set".into()));
        }

        let wire: Vec<WireMessage> = messages
            .iter()
            .map(|m| WireMessage {
                role: m.role.as_str(),
                content: &m.content,
            })
            .collect();

        let request = ChatRequest {
            model: &self.cfg.model,
            max_tokens: MAX_TOKENS,
            system,
            messages: wire,
            stream: true,
        };

        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, "application/json".parse().unwrap());
        headers.insert("anthropic-version", ANTHROPIC_VERSION.parse().unwrap());
        headers.insert(
            "x-api-key",
            self.cfg
                .api_key
                .parse()
                .map_err(|_| AiError::Config("invalid claude.api_key".into()))?,
        );

        let response = self
            .client
            .post(API_URL)
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
            if let Ok(StreamEvent::ContentBlockDelta {
                delta: Delta::TextDelta { text },
            }) = serde_json::from_str::<StreamEvent>(payload)
            {
                if !text.is_empty() {
                    full.push_str(&text);
                    on_delta(text);
                }
            }
        })
        .await?;

        Ok(full)
    }

    async fn is_available(&self) -> bool {
        !self.cfg.api_key.is_empty()
    }
}
