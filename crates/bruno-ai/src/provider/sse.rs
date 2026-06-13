//! Minimal Server-Sent Events reader shared by the OpenAI/Azure/Claude providers.

use futures_util::StreamExt;

use crate::error::AiError;

/// Consume an SSE response, invoking `on_data` with the JSON payload of each
/// `data:` line. Stops on the `[DONE]` sentinel. Non-`data:` lines (comments,
/// `event:` lines, blank separators) are ignored.
pub(crate) async fn read<F>(response: reqwest::Response, mut on_data: F) -> Result<(), AiError>
where
    F: FnMut(&str),
{
    let mut stream = response.bytes_stream();
    let mut buffer = String::new();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| AiError::Request(e.to_string()))?;
        buffer.push_str(&String::from_utf8_lossy(&chunk));

        while let Some(pos) = buffer.find('\n') {
            let line = buffer[..pos].trim().to_string();
            buffer.drain(..=pos);

            let Some(payload) = line.strip_prefix("data:") else {
                continue;
            };
            let payload = payload.trim();
            if payload == "[DONE]" {
                return Ok(());
            }
            if !payload.is_empty() {
                on_data(payload);
            }
        }
    }

    Ok(())
}
