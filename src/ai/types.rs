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
pub const SYSTEM_PROMPT: &str = "You are an expert at writing git commit messages.\n\
\n\
Your response must be a single Conventional Commit message wrapped in <commit>...</commit> tags.\n\
\n\
Format inside <commit>:\n\
  type(scope): short description   ← max 72 chars, required\n\
  \n\
  optional body                    ← plain prose only, omit if not needed\n\
\n\
Valid types: feat, fix, refactor, docs, style, test, chore, perf, ci, build\n\
Scope is optional. No markdown, no bullet points, no explanations outside the tags.";

/// Extract the final commit message from the model's raw output.
///
/// Strategy (in order):
/// 1. `<commit>...</commit>` tag  — explicit wrapper the prompt requests.
/// 2. A line matching the Conventional Commits pattern (`type(scope): desc`)
///    found anywhere in the text, plus any following non-preamble body lines.
///    This handles models that ignore formatting instructions and mix the commit
///    line into a wall of reasoning prose.
/// 3. Strip `<think>...</think>` blocks and return what remains.
/// 4. Fall back to the last `<think>` block's content.
pub fn clean_response(raw: &str) -> String {
    // ── 1. Explicit <commit> tag ──────────────────────────────────────────────
    if let Some(start) = raw.find("<commit>") {
        let after_open = &raw[start + "<commit>".len()..];
        let content = if let Some(end) = after_open.find("</commit>") {
            &after_open[..end]
        } else {
            after_open
        };
        let trimmed = content.trim();
        if !trimmed.is_empty() {
            // The commit block itself may contain <think> reasoning — strip it.
            let cleaned = strip_think_blocks(trimmed);
            let result = cleaned.trim().to_string();
            if !result.is_empty() {
                // Also run the conventional-commit extractor in case prose
                // reasoning leaked into the commit block.
                return extract_conventional_commit(&result)
                    .unwrap_or(result);
            }
        }
    }

    // ── 2. Scan for a Conventional Commits header line ────────────────────────
    // Pattern: type[(scope)]: description
    // Collect that line plus any immediately following body (blank line then prose).
    if let Some(msg) = extract_conventional_commit(raw) {
        return msg;
    }

    // ── 3. Strip <think> blocks, return remainder ─────────────────────────────
    let out = strip_think_blocks(raw);
    let result = out.trim().to_string();
    if !result.is_empty() {
        return result;
    }

    // ── 4. Last resort: inner content of the last think block ─────────────────
    last_think_inner(raw)
        .map(|s| {
            // Recursively clean the think content — it may itself contain a
            // conventional commit line buried in reasoning.
            let inner = s.trim();
            extract_conventional_commit(inner).unwrap_or_else(|| inner.to_string())
        })
        .unwrap_or_default()
}

/// Remove all `<think>...</think>` blocks (including unclosed ones) from `text`
/// and return the remainder.
fn strip_think_blocks(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("<think>") {
        out.push_str(&rest[..start]);
        let inner_start = start + "<think>".len();
        match rest[inner_start..].find("</think>") {
            Some(end_offset) => {
                rest = &rest[inner_start + end_offset + "</think>".len()..];
            }
            None => {
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

/// Return the inner content of the **last** `<think>` block found in `text`,
/// or `None` if there is no `<think>` tag.
fn last_think_inner(text: &str) -> Option<String> {
    let mut last: Option<String> = None;
    let mut rest = text;
    while let Some(start) = rest.find("<think>") {
        let inner_start = start + "<think>".len();
        match rest[inner_start..].find("</think>") {
            Some(end_offset) => {
                last = Some(rest[inner_start..inner_start + end_offset].to_string());
                rest = &rest[inner_start + end_offset + "</think>".len()..];
            }
            None => {
                last = Some(rest[inner_start..].to_string());
                rest = "";
            }
        }
    }
    last
}

/// Scan `text` for a line that looks like a Conventional Commit header and
/// return that line plus any optional body that follows it.
/// Returns `None` if no such line is found.
fn extract_conventional_commit(text: &str) -> Option<String> {
    let lines: Vec<&str> = text.lines().collect();
    let header_idx = lines.iter().position(|line| is_commit_header(line))?;

    // Collect the header and any body after it.
    // Stop if we hit another non-empty line that looks like prose analysis
    // (e.g. "The diff shows:" or a numbered list item).
    let mut result_lines: Vec<&str> = vec![lines[header_idx]];
    let mut i = header_idx + 1;
    // Allow one optional blank line followed by body prose.
    while i < lines.len() {
        let line = lines[i];
        // Stop at lines that look like preamble / analysis, not commit body.
        if looks_like_analysis(line) {
            break;
        }
        result_lines.push(line);
        i += 1;
    }

    // Trim trailing blank lines.
    while result_lines.last().map(|l| l.trim().is_empty()).unwrap_or(false) {
        result_lines.pop();
    }

    Some(result_lines.join("\n"))
}

/// Returns true if `line` matches the Conventional Commits header pattern:
///   type[(scope)]: description
fn is_commit_header(line: &str) -> bool {
    let line = line.trim();
    // Must contain ": " after the type/scope part.
    let Some(colon_pos) = line.find(": ") else {
        return false;
    };
    let prefix = &line[..colon_pos];
    // prefix is either "type" or "type(scope)" — no spaces allowed.
    if prefix.contains(' ') {
        return false;
    }
    // type is the part before the optional "(scope)".
    let type_part = prefix.split('(').next().unwrap_or("");
    const VALID_TYPES: &[&str] = &[
        "feat", "fix", "refactor", "docs", "style",
        "test", "chore", "perf", "ci", "build", "revert",
    ];
    VALID_TYPES.contains(&type_part)
}

/// Returns true if `line` looks like analysis prose rather than a commit body.
fn looks_like_analysis(line: &str) -> bool {
    let t = line.trim();
    if t.is_empty() {
        return false; // blank lines are fine in a commit body
    }
    // Numbered list items: "1.", "2.", …
    if t.len() > 2 && t.as_bytes()[0].is_ascii_digit() && t.as_bytes()[1] == b'.' {
        return true;
    }
    // Markdown bold/bullet preamble
    if t.starts_with("**") || t.starts_with("- ") || t.starts_with("* ") {
        return true;
    }
    // Lines ending in ":" are typically section headers in analysis
    if t.ends_with(':') {
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::clean_response;

    #[test]
    fn test_normal_response_unchanged() {
        assert_eq!(clean_response("feat: add login"), "feat: add login");
    }

    #[test]
    fn test_commit_tag_extracted() {
        let raw = "<think>some reasoning</think>\n<commit>\nfeat: add login\n</commit>";
        assert_eq!(clean_response(raw), "feat: add login");
    }

    #[test]
    fn test_commit_tag_no_think() {
        let raw = "<commit>fix(auth): handle expired tokens</commit>";
        assert_eq!(clean_response(raw), "fix(auth): handle expired tokens");
    }

    #[test]
    fn test_commit_tag_unclosed() {
        let raw = "<commit>\nfeat: add login\n";
        assert_eq!(clean_response(raw), "feat: add login");
    }

    #[test]
    fn test_conventional_commit_extracted_from_prose() {
        // Model outputs reasoning prose with the commit line buried inside
        let raw = "The user wants me to generate a commit message. Let me analyze:\n\
                   1. New workflow file added\n\
                   2. npm publish scripts added\n\
                   \n\
                   ci: add release workflow and npm publish scripts";
        assert_eq!(
            clean_response(raw),
            "ci: add release workflow and npm publish scripts"
        );
    }

    #[test]
    fn test_conventional_commit_with_body() {
        let raw = "Here is the commit:\n\
                   feat(auth): add JWT refresh token support\n\
                   \n\
                   Tokens now auto-renew 60 s before expiry.";
        assert_eq!(
            clean_response(raw),
            "feat(auth): add JWT refresh token support\n\nTokens now auto-renew 60 s before expiry."
        );
    }

    #[test]
    fn test_think_inside_commit_tag_stripped() {
        // Model puts <think> reasoning inside the <commit> block — must be stripped.
        let raw = "<commit>\n<think>let me think...</think>\nfeat: add login\n</commit>";
        assert_eq!(clean_response(raw), "feat: add login");
    }

    #[test]
    fn test_think_block_stripped_fallback() {
        let raw = "<think>reasoning here</think>\nfeat: add login";
        assert_eq!(clean_response(raw), "feat: add login");
    }

    #[test]
    fn test_answer_entirely_inside_think_block() {
        let raw = "<think>feat: add login</think>";
        assert_eq!(clean_response(raw), "feat: add login");
    }

    #[test]
    fn test_unclosed_think_tag_falls_back_to_inner_content() {
        let raw = "<think>feat: add login";
        assert_eq!(clean_response(raw), "feat: add login");
    }

    #[test]
    fn test_empty_input() {
        assert_eq!(clean_response(""), "");
    }
}

/// Request to generate a commit message.
#[derive(Debug, Clone)]
pub struct CommitRequest {
    pub diff: String,
}

impl CommitRequest {
    pub fn new(diff: &str) -> Self {
        // Truncate very large diffs to avoid token limits.
        // Use a char-boundary-safe split to avoid panicking on multi-byte characters.
        let truncated = if diff.len() > 8000 {
            // Scan backwards from the byte limit to find a valid UTF-8 boundary.
            // This is O(char_len) ≤ O(4) instead of an O(n) forward scan.
            let mut boundary = 8000;
            while !diff.is_char_boundary(boundary) {
                boundary -= 1;
            }
            format!("{}...\n[diff truncated]", &diff[..boundary])
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
