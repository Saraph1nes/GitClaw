/// Which AI provider to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelKind {
    Claude,
    OpenAI,
    MiniMax,
    MiniMaxCN,
}

impl ModelKind {
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "openai" | "gpt"             => ModelKind::OpenAI,
            "minimax"  | "mini-max"      => ModelKind::MiniMax,
            "minimax-cn" | "mini-max-cn" => ModelKind::MiniMaxCN,
            _ => ModelKind::Claude,
        }
    }
}

/// Shared message struct used by both Claude and OpenAI request bodies.
#[derive(serde::Serialize, Clone)]
pub struct ApiMessage {
    pub role: String,
    pub content: String,
}

/// Shared error body shape returned by both APIs.
#[derive(serde::Deserialize)]
pub struct ApiErrorBody {
    pub error: ApiErrorDetail,
}

#[derive(serde::Deserialize)]
pub struct ApiErrorDetail {
    pub message: String,
}

/// System prompt for commit message generation.
pub const SYSTEM_PROMPT: &str = "You are an expert at writing git commit messages in Conventional Commits format.\n\
\n\
Output format:\n\
  <type>(<scope>): <short description>   ← required, max 72 chars\n\
  <blank line>\n\
  <optional body>                        ← only if extra context genuinely helps\n\
\n\
Valid types: feat, fix, refactor, docs, style, test, chore, perf, ci, build\n\
\n\
STRICT rules — violating any disqualifies the response:\n\
1. Output ONLY the commit message. Zero preamble, zero explanation.\n\
2. NO markdown: no bullet points, no backticks, no bold, no headers.\n\
3. NO chain-of-thought: do not reason out loud, do not use <think> tags.\n\
4. The first line MUST match: type(scope): description\n\
5. Scope is optional but recommended when it clarifies what area changed.\n\
6. Body lines must be plain prose, not bullet lists.\n\
\n\
Example of a CORRECT response:\n\
feat(auth): add JWT refresh token support\n\
\n\
Tokens now auto-renew 60 s before expiry to prevent silent session drops.\n\
\n\
Example of an INCORRECT response (do not do this):\n\
Here is a commit message:\n\
- Added JWT refresh tokens\n\
- Updated expiry logic";

/// Strip `<think>...</think>` reasoning blocks that some models emit,
/// then trim surrounding whitespace.  Works with nested tags.
pub fn clean_response(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(start) = rest.find("<think>") {
        out.push_str(&rest[..start]);
        // Skip everything up to and including </think>
        rest = match rest[start..].find("</think>") {
            Some(end_offset) => &rest[start + end_offset + "</think>".len()..],
            // Malformed: no closing tag — drop the rest to avoid showing junk
            None => "",
        };
    }
    out.push_str(rest);
    out.trim().to_string()
}

/// Request to generate a commit message.
#[derive(Debug, Clone)]
pub struct CommitRequest {
    pub diff: String,
}

impl CommitRequest {
    pub fn new(diff: &str) -> Self {
        // Truncate very large diffs to avoid token limits
        let truncated = if diff.len() > 8000 {
            format!("{}...\n[diff truncated]", &diff[..8000])
        } else {
            diff.to_string()
        };
        Self { diff: truncated }
    }

    pub fn user_message(&self) -> String {
        format!(
            "Generate a commit message for the following staged changes:\n\n```diff\n{}\n```",
            self.diff
        )
    }
}
