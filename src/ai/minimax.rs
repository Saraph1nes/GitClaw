use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

use super::types::{clean_response, ApiErrorBody, ApiMessage, CommitRequest, SYSTEM_PROMPT};

const MODEL: &str = "MiniMax-M2.7";

/// 国际版端点（api.minimax.io）
const URL_GLOBAL: &str = "https://api.minimax.io/v1/chat/completions";
/// 国内版端点（api.minimaxi.com）
const URL_CN: &str = "https://api.minimaxi.com/v1/chat/completions";

/// Shared HTTP client — holds connection pool and TLS session cache.
static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
fn client() -> &'static reqwest::Client {
    CLIENT.get_or_init(reqwest::Client::new)
}

#[derive(Serialize)]
struct MiniMaxRequest {
    model: &'static str,
    messages: Vec<ApiMessage>,
    max_tokens: u32,
    temperature: f32,
}

#[derive(Deserialize)]
struct MiniMaxResponse {
    choices: Vec<MiniMaxChoice>,
}

#[derive(Deserialize)]
struct MiniMaxChoice {
    message: MiniMaxMessage,
}

#[derive(Deserialize)]
struct MiniMaxMessage {
    content: String,
}

/// 国际版
pub async fn generate(api_key: &str, request: &CommitRequest) -> Result<String> {
    call(api_key, request, URL_GLOBAL).await
}

/// 国内版（api.minimaxi.com）
pub async fn generate_cn(api_key: &str, request: &CommitRequest) -> Result<String> {
    call(api_key, request, URL_CN).await
}

async fn call(api_key: &str, request: &CommitRequest, url: &str) -> Result<String> {
    let body = MiniMaxRequest {
        model: MODEL,
        messages: vec![
            ApiMessage {
                role: "system".to_string(),
                content: SYSTEM_PROMPT.to_string(),
            },
            ApiMessage {
                role: "user".to_string(),
                content: request.user_message(),
            },
        ],
        max_tokens: 256,
        temperature: 0.3,
    };

    let response = client()
        .post(url)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await?;

    let status = response.status();
    let text = response.text().await?;

    if !status.is_success() {
        let msg = serde_json::from_str::<ApiErrorBody>(&text)
            .map(|e| e.error.message)
            .unwrap_or_else(|_| format!("{} — {}", status, text));
        return Err(anyhow!("MiniMax API error: {}", msg));
    }

    let parsed: MiniMaxResponse = serde_json::from_str(&text)
        .map_err(|e| anyhow!("Failed to parse MiniMax response: {}", e))?;

    parsed
        .choices
        .first()
        .map(|c| clean_response(&c.message.content))
        .ok_or_else(|| anyhow!("Empty response from MiniMax"))
}
