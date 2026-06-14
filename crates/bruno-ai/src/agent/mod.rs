//! Bruno's tool-using agent: an OpenAI/Azure function-calling loop with
//! Veclite-backed RAG memory and keyless web access.
//!
//! Supported on the OpenAI-compatible providers (Azure, OpenAI). For Ollama and
//! Claude, [`Agent::from_config`] returns `None` and the app falls back to plain
//! chat.

mod embed;
mod memory;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use reqwest::header::{HeaderMap, AUTHORIZATION, CONTENT_TYPE};
use reqwest::Client;
use serde_json::{json, Value};

use crate::config::{Config, ProviderKind};
use crate::error::AiError;
use memory::Memory;

const MAX_ITERS: usize = 6;
const RECALL_K: usize = 5;
const PAGE_CHARS: usize = 6_000;

/// Web access for the agent, implemented by the host app (e.g. a headless
/// WKWebView on macOS). Injected via [`Agent::with_browser`].
#[async_trait]
pub trait Browser: Send + Sync {
    /// Load a page and return its rendered, readable text.
    async fn fetch(&self, url: &str) -> Result<String, AiError>;
    /// Run a web search and return a formatted result list (titles, urls, snippets).
    async fn search(&self, query: &str) -> Result<String, AiError>;
}

const AGENT_SYSTEM: &str = "You are Bruno, a calm, capable research and work companion for a \
developer/researcher/musician. Your replies are spoken aloud, so the FINAL answer must be one or \
two short, natural sentences — no markdown, lists, JSON, or quotes. Use your tools to actually do \
things: search the web and fetch pages for current or unknown info, remember useful facts the \
user shares or that you find, and recall from memory before saying you don't know. Prefer acting \
over asking. When you've gathered what you need, give a brief spoken summary, not a wall of text.";

/// Resolved OpenAI-compatible endpoint (chat + embeddings).
pub(crate) struct Endpoint {
    client: Client,
    headers: HeaderMap,
    chat_url: String,
    embed_url: String,
    /// Body `model` for OpenAI-style; `None` for Azure (deployment is in the URL).
    chat_model: Option<String>,
    embed_model: Option<String>,
}

pub struct Agent {
    ep: Endpoint,
    memory: Mutex<Memory>,
    browser: Option<Arc<dyn Browser>>,
}

impl Agent {
    /// Build an agent for the configured provider, or `None` if the provider
    /// doesn't support the agent path or memory can't be opened.
    pub fn from_config(cfg: &Config) -> Option<Self> {
        let ep = Endpoint::from_config(cfg)?;
        match Memory::open() {
            Ok(memory) => Some(Self {
                ep,
                memory: Mutex::new(memory),
                browser: None,
            }),
            Err(e) => {
                tracing::warn!("agent memory unavailable: {e}");
                None
            }
        }
    }

    /// Attach a web browser implementation (enables `web_search` / `fetch_url`).
    pub fn with_browser(mut self, browser: Arc<dyn Browser>) -> Self {
        self.browser = Some(browser);
        self
    }

    /// Run the tool loop for one user message; returns the final spoken answer.
    pub async fn run(&self, user_message: &str) -> Result<String, AiError> {
        let mut messages: Vec<Value> = vec![
            json!({ "role": "system", "content": AGENT_SYSTEM }),
            json!({ "role": "user", "content": user_message }),
        ];

        for _ in 0..MAX_ITERS {
            let mut body = json!({
                "messages": messages,
                "tools": tools_json(),
                "tool_choice": "auto",
                "stream": false,
            });
            if let Some(model) = &self.ep.chat_model {
                body["model"] = json!(model);
            }

            let resp = self
                .ep
                .client
                .post(&self.ep.chat_url)
                .headers(self.ep.headers.clone())
                .json(&body)
                .send()
                .await
                .map_err(|e| AiError::Request(e.to_string()))?;
            if !resp.status().is_success() {
                return Err(AiError::Status(resp.status().as_u16()));
            }
            let v: Value = resp
                .json()
                .await
                .map_err(|e| AiError::Decode(e.to_string()))?;

            let message = v["choices"][0]["message"].clone();
            let tool_calls = message["tool_calls"].as_array().cloned().unwrap_or_default();
            tracing::debug!(
                tool_calls = tool_calls.len(),
                content_len = message["content"].as_str().unwrap_or("").len(),
                "agent iter"
            );

            if tool_calls.is_empty() {
                let text = message["content"].as_str().unwrap_or("").trim().to_string();
                return Ok(text);
            }

            // Echo the assistant turn (with its tool_calls) then answer each call.
            messages.push(message);
            for call in tool_calls {
                let id = call["id"].as_str().unwrap_or("").to_string();
                let name = call["function"]["name"].as_str().unwrap_or("").to_string();
                let args: Value = call["function"]["arguments"]
                    .as_str()
                    .and_then(|s| serde_json::from_str(s).ok())
                    .unwrap_or_else(|| json!({}));
                let result = self.dispatch(&name, &args).await;
                tracing::info!(tool = %name, "agent tool call");
                messages.push(json!({
                    "role": "tool",
                    "tool_call_id": id,
                    "content": result,
                }));
            }
        }

        Ok("I dug around but couldn't wrap that up cleanly.".to_string())
    }

    async fn dispatch(&self, name: &str, args: &Value) -> String {
        match name {
            "remember" => {
                let text = args["text"].as_str().unwrap_or("").trim();
                if text.is_empty() {
                    return "Nothing to remember.".into();
                }
                let source = args["source"].as_str().unwrap_or("user");
                match self.store(text, source).await {
                    Ok(_) => "Saved to memory.".into(),
                    Err(e) => format!("Couldn't save: {e}"),
                }
            }
            "recall" => {
                let query = args["query"].as_str().unwrap_or("").trim();
                if query.is_empty() {
                    return "No query.".into();
                }
                match self.recall(query).await {
                    Ok(hits) if !hits.is_empty() => hits,
                    Ok(_) => "Nothing relevant in memory.".into(),
                    Err(e) => format!("Recall failed: {e}"),
                }
            }
            "web_search" => {
                let query = args["query"].as_str().unwrap_or("").trim();
                if query.is_empty() {
                    return "No query.".into();
                }
                let Some(browser) = &self.browser else {
                    return "Web browsing isn't available.".into();
                };
                match browser.search(query).await {
                    Ok(s) if !s.trim().is_empty() => s,
                    Ok(_) => "No results.".into(),
                    Err(e) => format!("Search failed: {e}"),
                }
            }
            "fetch_url" => {
                let url = args["url"].as_str().unwrap_or("").trim();
                if url.is_empty() {
                    return "No url.".into();
                }
                let Some(browser) = &self.browser else {
                    return "Web browsing isn't available.".into();
                };
                match browser.fetch(url).await {
                    Ok(text) if !text.trim().is_empty() => {
                        // Best-effort: store the page in memory for later recall.
                        let _ = self.store(&text, url).await;
                        text.chars().take(PAGE_CHARS).collect()
                    }
                    Ok(_) => "Page had no readable text.".into(),
                    Err(e) => format!("Fetch failed: {e}"),
                }
            }
            other => format!("Unknown tool: {other}"),
        }
    }

    async fn store(&self, text: &str, source: &str) -> Result<String, AiError> {
        let vector = embed::embed(&self.ep, text).await?;
        let mut mem = self.memory.lock().map_err(|_| AiError::Config("memory lock poisoned".into()))?;
        mem.remember(vector, text, source)
    }

    async fn recall(&self, query: &str) -> Result<String, AiError> {
        let vector = embed::embed(&self.ep, query).await?;
        let hits = {
            let mem = self.memory.lock().map_err(|_| AiError::Config("memory lock poisoned".into()))?;
            mem.recall(&vector, RECALL_K)?
        };
        Ok(hits
            .iter()
            .enumerate()
            .map(|(i, h)| {
                let src = if h.source.is_empty() {
                    String::new()
                } else {
                    format!(" [{}]", h.source)
                };
                format!("{}. {}{}", i + 1, h.text, src)
            })
            .collect::<Vec<_>>()
            .join("\n"))
    }
}

impl Endpoint {
    fn from_config(cfg: &Config) -> Option<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(60))
            .build()
            .ok()?;

        match cfg.ai.provider {
            ProviderKind::Azure => {
                let a = &cfg.ai.azure;
                if a.endpoint.is_empty() || a.api_key.is_empty() || a.deployment.is_empty() {
                    return None;
                }
                let mut headers = HeaderMap::new();
                headers.insert(CONTENT_TYPE, "application/json".parse().ok()?);
                headers.insert("api-key", a.api_key.parse().ok()?);
                let base = a.endpoint.trim_end_matches('/');
                Some(Endpoint {
                    client,
                    headers,
                    chat_url: format!(
                        "{base}/openai/deployments/{}/chat/completions?api-version={}",
                        a.deployment, a.api_version
                    ),
                    embed_url: format!(
                        "{base}/openai/deployments/{}/embeddings?api-version={}",
                        a.embedding_deployment, a.api_version
                    ),
                    chat_model: None,
                    embed_model: None,
                })
            }
            ProviderKind::OpenAi => {
                let o = &cfg.ai.openai;
                if o.api_key.is_empty() {
                    return None;
                }
                let mut headers = HeaderMap::new();
                headers.insert(CONTENT_TYPE, "application/json".parse().ok()?);
                headers.insert(AUTHORIZATION, format!("Bearer {}", o.api_key).parse().ok()?);
                let base = o.base_url.trim_end_matches('/');
                Some(Endpoint {
                    client,
                    headers,
                    chat_url: format!("{base}/chat/completions"),
                    embed_url: format!("{base}/embeddings"),
                    chat_model: Some(o.model.clone()),
                    embed_model: Some("text-embedding-3-small".to_string()),
                })
            }
            // Ollama / Claude / LM Studio: no agent path (fall back to plain chat).
            _ => None,
        }
    }
}

fn tools_json() -> Value {
    json!([
        {
            "type": "function",
            "function": {
                "name": "web_search",
                "description": "Search the web for current or unknown information. Returns titles, URLs, and snippets.",
                "parameters": {
                    "type": "object",
                    "properties": { "query": { "type": "string", "description": "the search query" } },
                    "required": ["query"]
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "fetch_url",
                "description": "Fetch a web page and return its readable text. The page is also stored in memory.",
                "parameters": {
                    "type": "object",
                    "properties": { "url": { "type": "string", "description": "the page URL" } },
                    "required": ["url"]
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "remember",
                "description": "Save a fact, note, or snippet to long-term memory for later recall.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "text": { "type": "string", "description": "the content to remember" },
                        "source": { "type": "string", "description": "optional origin (url, 'user', etc.)" }
                    },
                    "required": ["text"]
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "recall",
                "description": "Semantic search over long-term memory. Use before saying you don't know.",
                "parameters": {
                    "type": "object",
                    "properties": { "query": { "type": "string", "description": "what to look for" } },
                    "required": ["query"]
                }
            }
        }
    ])
}
