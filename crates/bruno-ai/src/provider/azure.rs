//! Azure AI Foundry streaming client.
//!
//! Foundry serves the OpenAI Chat Completions wire format, so this reuses
//! [`super::openai::stream_completions`]; only the URL shape and the `api-key`
//! auth header differ.

use async_trait::async_trait;
use reqwest::header::{HeaderMap, CONTENT_TYPE};
use reqwest::Client;

use super::{openai, Provider};
use crate::config::AzureConfig;
use crate::error::AiError;
use crate::message::Message;

const CHAT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

pub struct AzureProvider {
    client: Client,
    cfg: AzureConfig,
}

impl AzureProvider {
    pub fn new(cfg: AzureConfig) -> Self {
        Self {
            client: Client::builder()
                .timeout(CHAT_TIMEOUT)
                .build()
                .unwrap_or_else(|_| Client::new()),
            cfg,
        }
    }

    fn configured(&self) -> bool {
        !self.cfg.endpoint.is_empty()
            && !self.cfg.api_key.is_empty()
            && !self.cfg.deployment.is_empty()
    }
}

#[async_trait]
impl Provider for AzureProvider {
    async fn chat_stream(
        &self,
        system: &str,
        messages: &[Message],
        on_delta: &mut (dyn FnMut(String) + Send),
    ) -> Result<String, AiError> {
        if !self.configured() {
            return Err(AiError::Config(
                "azure requires endpoint, api_key and deployment".into(),
            ));
        }

        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, "application/json".parse().unwrap());
        headers.insert(
            "api-key",
            self.cfg
                .api_key
                .parse()
                .map_err(|_| AiError::Config("invalid azure.api_key".into()))?,
        );

        let url = format!(
            "{}/openai/deployments/{}/chat/completions?api-version={}",
            self.cfg.endpoint.trim_end_matches('/'),
            self.cfg.deployment,
            self.cfg.api_version,
        );

        // Azure routes by deployment in the URL; the body `model` is ignored but required.
        openai::stream_completions(
            &self.client,
            &url,
            headers,
            &self.cfg.deployment,
            system,
            messages,
            on_delta,
        )
        .await
    }

    async fn is_available(&self) -> bool {
        self.configured()
    }
}
