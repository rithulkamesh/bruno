//! Ollama streaming chat client (local inference via NDJSON).

use async_trait::async_trait;
use futures_util::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tracing::warn;

use super::Provider;
use crate::config::OllamaConfig;
use crate::error::AiError;
use crate::message::Message;

const CHAT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);
const CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);

pub struct OllamaProvider {
    client: Client,
    cfg: OllamaConfig,
}

impl OllamaProvider {
    pub fn new(cfg: OllamaConfig) -> Self {
        Self {
            client: Client::builder()
                .timeout(CHAT_TIMEOUT)
                .connect_timeout(CONNECT_TIMEOUT)
                .build()
                .unwrap_or_else(|_| Client::new()),
            cfg,
        }
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
    content: &'a str,
}

#[derive(Deserialize)]
struct StreamChunk {
    message: Option<StreamMessage>,
}

#[derive(Deserialize)]
struct StreamMessage {
    content: String,
}

#[async_trait]
impl Provider for OllamaProvider {
    async fn chat_stream(
        &self,
        system: &str,
        messages: &[Message],
        on_delta: &mut (dyn FnMut(String) + Send),
    ) -> Result<String, AiError> {
        let wire: Vec<WireMessage> = std::iter::once(WireMessage {
            role: "system",
            content: system,
        })
        .chain(messages.iter().map(|m| WireMessage {
            role: m.role.as_str(),
            content: &m.content,
        }))
        .collect();

        let request = ChatRequest {
            model: &self.cfg.model,
            messages: wire,
            stream: true,
        };

        let url = format!("{}/api/chat", self.cfg.url.trim_end_matches('/'));
        let response = self
            .client
            .post(&url)
            .json(&request)
            .send()
            .await
            .map_err(|e| AiError::Request(e.to_string()))?;

        if !response.status().is_success() {
            return Err(AiError::Status(response.status().as_u16()));
        }

        let mut full = String::new();
        let mut stream = response.bytes_stream();
        let mut buffer = String::new();

        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| AiError::Request(e.to_string()))?;
            buffer.push_str(&String::from_utf8_lossy(&chunk));

            while let Some(pos) = buffer.find('\n') {
                let line = buffer[..pos].trim().to_string();
                buffer.drain(..=pos);
                if line.is_empty() {
                    continue;
                }
                if let Ok(parsed) = serde_json::from_str::<StreamChunk>(&line) {
                    if let Some(msg) = parsed.message {
                        if !msg.content.is_empty() {
                            full.push_str(&msg.content);
                            on_delta(msg.content);
                        }
                    }
                }
            }
        }

        Ok(full)
    }

    async fn is_available(&self) -> bool {
        let url = self.cfg.url.trim_end_matches('/');
        match self.client.get(url).send().await {
            Ok(r) => r.status().is_success(),
            Err(e) => {
                warn!("ollama health check failed: {e}");
                false
            }
        }
    }
}
