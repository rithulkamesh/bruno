//! Bruno's AI/agent layer.
//!
//! Multi-provider LLM inference behind a single [`AiClient`]. The provider
//! (Ollama, OpenAI, Claude, Azure AI Foundry) and its credentials are selected
//! from `~/.config/bruno/config.toml` — see [`Config`].

pub mod agent;
pub mod config;
pub mod error;
pub mod message;
pub mod provider;

mod client;

pub use agent::{Agent, Browser};
pub use client::AiClient;
pub use config::{Config, NeuroConfig, NeuroProfile, NudgeTone, ProviderKind};
pub use error::AiError;
pub use message::{Message, Role};
pub use provider::Provider;
