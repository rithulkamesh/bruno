//! LM Studio streaming client (local OpenAI-compatible server).
//!
//! LM Studio exposes the OpenAI Chat Completions API at `http://localhost:1234/v1`,
//! so this reuses [`super::openai::stream_completions`]. Auth is optional.

use async_trait::async_trait;
use reqwest::header::{HeaderMap, AUTHORIZATION, CONTENT_TYPE};
use reqwest::Client;

use super::{openai, Provider};
use crate::config::LMStudioConfig;
use crate::error::AiError;
use crate::message::Message;

const CHAT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);
const CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);

pub struct LMStudioProvider {
    client: Client,
    cfg: LMStudioConfig,
}

impl LMStudioProvider {
    pub fn new(cfg: LMStudioConfig) -> Self {
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

#[async_trait]
impl Provider for LMStudioProvider {
    async fn chat_stream(
        &self,
        system: &str,
        messages: &[Message],
        on_delta: &mut (dyn FnMut(String) + Send),
    ) -> Result<String, AiError> {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, "application/json".parse().unwrap());
        if !self.cfg.api_key.is_empty() {
            headers.insert(
                AUTHORIZATION,
                format!("Bearer {}", self.cfg.api_key)
                    .parse()
                    .map_err(|_| AiError::Config("invalid lmstudio.api_key".into()))?,
            );
        }

        // An empty model lets LM Studio use its currently loaded model.
        let url = format!("{}/chat/completions", self.cfg.base_url.trim_end_matches('/'));
        openai::stream_completions(
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
        let url = format!("{}/models", self.cfg.base_url.trim_end_matches('/'));
        match self.client.get(&url).send().await {
            Ok(r) => r.status().is_success(),
            Err(_) => false,
        }
    }
}
