//! OpenAI Chat Completions streaming client.
//!
//! The wire format here is also used by Azure AI Foundry (see [`super::azure`]),
//! so the request/response types and streaming loop are exposed as
//! [`stream_completions`] for reuse.

use async_trait::async_trait;
use reqwest::header::{HeaderMap, AUTHORIZATION, CONTENT_TYPE};
use reqwest::Client;
use serde::{Deserialize, Serialize};

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
}

#[derive(Serialize)]
pub(crate) struct ChatRequest<'a> {
    pub model: &'a str,
    pub messages: Vec<WireMessage<'a>>,
    pub stream: bool,
}

#[derive(Serialize)]
pub(crate) struct WireMessage<'a> {
    pub role: &'a str,
    pub content: &'a str,
}

#[derive(Deserialize)]
struct StreamChunk {
    choices: Vec<Choice>,
}

#[derive(Deserialize)]
struct Choice {
    delta: Delta,
}

#[derive(Deserialize)]
struct Delta {
    content: Option<String>,
}

/// Build the OpenAI-style message array with the system prompt prepended.
pub(crate) fn wire_messages<'a>(system: &'a str, messages: &'a [Message]) -> Vec<WireMessage<'a>> {
    std::iter::once(WireMessage {
        role: "system",
        content: system,
    })
    .chain(messages.iter().map(|m| WireMessage {
        role: m.role.as_str(),
        content: &m.content,
    }))
    .collect()
}

/// Issue a streaming Chat Completions request to `url` with `headers` and parse
/// the OpenAI delta format. Shared by the OpenAI and Azure providers.
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

        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, "application/json".parse().unwrap());
        headers.insert(
            AUTHORIZATION,
            format!("Bearer {}", self.cfg.api_key)
                .parse()
                .map_err(|_| AiError::Config("invalid openai.api_key".into()))?,
        );

        let url = format!("{}/chat/completions", self.cfg.base_url.trim_end_matches('/'));
        stream_completions(
            &self.client,
            &url,
            headers,
            &self.cfg.model,
            system,
            messages,
            on_delta,
        )
        .await
    }

    async fn is_available(&self) -> bool {
        !self.cfg.api_key.is_empty()
    }
}
