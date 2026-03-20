pub mod types;
pub mod claude;
pub mod openai;
pub mod minimax;

use anyhow::Result;

use crate::config::{AiSettings, Settings};
use types::ModelKind;

/// Generate a commit message using the model configured in `Settings`.
#[allow(dead_code)]
pub async fn generate_commit_message(settings: &Settings, diff: &str) -> Result<String> {
    generate_commit_message_with(&settings.ai, diff).await
}

/// Generate a commit message using just the `AiSettings` sub-struct.
/// Called from the async task in `app.rs` to avoid cloning the full `Settings`.
pub async fn generate_commit_message_with(ai: &AiSettings, diff: &str) -> Result<String> {
    let model = ModelKind::from_str(&ai.default_model);
    let request = types::CommitRequest::new(diff);

    match model {
        ModelKind::Claude => {
            let api_key = ai.claude_api_key()?;
            claude::generate(&api_key, &request).await
        }
        ModelKind::OpenAI => {
            let api_key = ai.openai_api_key()?;
            openai::generate(&api_key, &request).await
        }
        ModelKind::MiniMax => {
            let api_key = ai.minimax_api_key()?;
            minimax::generate(&api_key, &request).await
        }
        ModelKind::MiniMaxCN => {
            let api_key = ai.minimax_cn_api_key()?;
            minimax::generate_cn(&api_key, &request).await
        }
    }
}
