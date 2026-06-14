//! Text embeddings (OpenAI-compatible / Azure) for RAG.

use serde::Deserialize;
use serde_json::json;

use super::Endpoint;
use crate::error::AiError;

#[derive(Deserialize)]
struct EmbedResponse {
    data: Vec<EmbedData>,
}

#[derive(Deserialize)]
struct EmbedData {
    embedding: Vec<f32>,
}

/// Embed a single string into a vector.
pub(super) async fn embed(ep: &Endpoint, text: &str) -> Result<Vec<f32>, AiError> {
    let mut body = json!({ "input": text });
    if let Some(model) = &ep.embed_model {
        body["model"] = json!(model);
    }

    let resp = ep
        .client
        .post(&ep.embed_url)
        .headers(ep.headers.clone())
        .json(&body)
        .send()
        .await
        .map_err(|e| AiError::Request(e.to_string()))?;

    if !resp.status().is_success() {
        return Err(AiError::Status(resp.status().as_u16()));
    }

    let parsed: EmbedResponse = resp
        .json()
        .await
        .map_err(|e| AiError::Decode(e.to_string()))?;
    parsed
        .data
        .into_iter()
        .next()
        .map(|d| d.embedding)
        .ok_or_else(|| AiError::Decode("empty embedding response".into()))
}
