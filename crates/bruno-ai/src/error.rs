//! Error type for the AI layer.

use std::fmt;

#[derive(Debug)]
pub enum AiError {
    /// The HTTP request itself failed (network, DNS, timeout).
    Request(String),
    /// The provider returned a non-success status.
    Status(u16),
    /// The response body could not be parsed.
    Decode(String),
    /// The selected provider is missing required configuration (e.g. an API key).
    Config(String),
}

impl fmt::Display for AiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AiError::Request(e) => write!(f, "request failed: {e}"),
            AiError::Status(s) => write!(f, "provider returned status {s}"),
            AiError::Decode(e) => write!(f, "failed to decode response: {e}"),
            AiError::Config(e) => write!(f, "configuration error: {e}"),
        }
    }
}

impl std::error::Error for AiError {}
