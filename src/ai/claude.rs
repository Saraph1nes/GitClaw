use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

use super::types::{clean_response, ApiErrorBody, ApiMessage, CommitRequest, SYSTEM_PROMPT};

const CLAUDE_API_URL: &str = "https://api.anthropic.com/v1/messages";
const CLAUDE_MODEL: &str = "claude-sonnet-4-20250514";

/// Shared HTTP client — holds connection pool and TLS session cache.
static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
fn client() -> &'static reqwest::Client {
    CLIENT.get_or_init(reqwest::Client::new)
}

#[derive(Serialize)]
struct ClaudeRequest {
    model: &'static str,
    max_tokens: u32,
    system: &'static str,
    messages: Vec<ApiMessage>,
}

#[derive(Deserialize)]
struct ClaudeResponse {
    content: Vec<ClaudeContent>,
}

#[derive(Deserialize)]
struct ClaudeContent {
    text: String,
}

pub async fn generate(api_key: &str, request: &CommitRequest) -> Result<String> {
    let body = ClaudeRequest {
        model: CLAUDE_MODEL,
        max_tokens: 256,
        system: SYSTEM_PROMPT,
        messages: vec![
            ApiMessage {
                role: "user".to_string(),
                content: request.user_message(),
            },
            // Prefill the assistant turn so the model is forced to continue
            // from here — it cannot output any preamble or reasoning prose.
            ApiMessage {
                role: "assistant".to_string(),
                content: "<commit>\n".to_string(),
            },
        ],
    };

    let response = client()
        .post(CLAUDE_API_URL)
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .json(&body)
        .send()
        .await?;

    let status = response.status();
    let text = response.text().await?;

    if !status.is_success() {
        let msg = serde_json::from_str::<ApiErrorBody>(&text)
            .map(|e| e.error.message)
            .unwrap_or_else(|_| format!("{} — {}", status, text));
        return Err(anyhow!("Claude API error: {}", msg));
    }

    let parsed: ClaudeResponse = serde_json::from_str(&text)
        .map_err(|e| anyhow!("Failed to parse Claude response: {}", e))?;

    parsed
        .content
        .first()
        .map(|c| {
            // The assistant prefill started with "<commit>\n", so the model
            // continues from there. Prepend it back so clean_response can
            // extract the content between the <commit> tags correctly.
            clean_response(&format!("<commit>\n{}", c.text))
        })
        .ok_or_else(|| anyhow!("Empty response from Claude"))
}
