use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

use super::types::{clean_response, ApiErrorBody, ApiMessage, CommitRequest, SYSTEM_PROMPT};

const OPENAI_API_URL: &str = "https://api.openai.com/v1/chat/completions";
const OPENAI_MODEL: &str = "gpt-4o-mini";

/// Shared HTTP client — holds connection pool and TLS session cache.
static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
fn client() -> &'static reqwest::Client {
    CLIENT.get_or_init(reqwest::Client::new)
}

#[derive(Serialize)]
struct OpenAIRequest {
    model: &'static str,
    messages: Vec<ApiMessage>,
    max_tokens: u32,
    temperature: f32,
}

#[derive(Deserialize)]
struct OpenAIResponse {
    choices: Vec<OpenAIChoice>,
}

#[derive(Deserialize)]
struct OpenAIChoice {
    message: OpenAIResponseMessage,
}

#[derive(Deserialize)]
struct OpenAIResponseMessage {
    content: String,
}

pub async fn generate(api_key: &str, request: &CommitRequest) -> Result<String> {
    let body = OpenAIRequest {
        model: OPENAI_MODEL,
        messages: vec![
            ApiMessage {
                role: "system".to_string(),
                content: SYSTEM_PROMPT.to_string(),
            },
            ApiMessage {
                role: "user".to_string(),
                content: request.user_message(),
            },
            // Prefill: force the model to continue from inside <commit>.
            ApiMessage {
                role: "assistant".to_string(),
                content: "<commit>\n".to_string(),
            },
        ],
        max_tokens: 256,
        temperature: 0.3,
    };

    let response = client()
        .post(OPENAI_API_URL)
        .header("Authorization", format!("Bearer {}", api_key))
        .json(&body)
        .send()
        .await?;

    let status = response.status();
    let text = response.text().await?;

    if !status.is_success() {
        let msg = serde_json::from_str::<ApiErrorBody>(&text)
            .map(|e| e.error.message)
            .unwrap_or_else(|_| format!("{} — {}", status, text));
        return Err(anyhow!("OpenAI API error: {}", msg));
    }

    let parsed: OpenAIResponse = serde_json::from_str(&text)
        .map_err(|e| anyhow!("Failed to parse OpenAI response: {}", e))?;

    parsed
        .choices
        .first()
        .map(|c| clean_response(&format!("<commit>\n{}", c.message.content)))
        .ok_or_else(|| anyhow!("Empty response from OpenAI"))
}
