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
pub const SYSTEM_PROMPT: &str = "\
You are a git commit message generator. Your job is to read a diff and produce \
one Conventional Commit message — nothing else.\n\
\n\
STRICT OUTPUT RULES:\n\
- Output ONLY the commit message inside <commit>...</commit> tags.\n\
- Do NOT write any analysis, reasoning, explanations, lists, or preamble.\n\
- Do NOT start with phrases like \"Let me\", \"I will\", \"Here is\", etc.\n\
- No markdown, no bullet points, no code blocks outside the tags.\n\
\n\
FORMAT (inside <commit>):\n\
  type(scope): short description   ← imperative mood, max 72 chars\n\
\n\
  optional body                    ← plain prose only, max 3 sentences,\n\
                                     omit entirely if not needed\n\
\n\
Valid types: feat, fix, refactor, docs, style, test, chore, perf, ci, build\n\
Scope is optional.\n\
\n\
EXAMPLES:\n\
<commit>\n\
feat(auth): add JWT refresh token support\n\
</commit>\n\
\n\
<commit>\n\
fix: prevent panic when staged diff is empty\n\
</commit>\n\
\n\
<commit>\n\
refactor(ui): simplify file tree rendering\n\
\n\
Extract directory expansion logic into a dedicated method to reduce\n\
coupling between FileTree and the render layer.\n\
</commit>";

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
            // This also handles bare </think> closers (model reasoning leaked in).
            let cleaned = strip_think_blocks(trimmed);
            let result = cleaned.trim().to_string();
            if !result.is_empty() {
                // Only return if there's a valid CC header.  If the block contains
                // only prose analysis (no CC line), fall through so the caller gets
                // an empty string rather than a wall of reasoning text.
                if let Some(msg) = extract_conventional_commit(&result) {
                    return msg;
                }
                // No CC header found — the block is pure prose, fall through.
            }
            // commit block was entirely reasoning (stripped to empty) — fall through.
        }
    }

    // ── 2. Strip <think> blocks first, then scan for a CC header ─────────────
    // We must strip before extracting so that reasoning prose that happens to
    // start with a valid CC prefix (e.g. "feat: or refactor: – …") is not
    // mistaken for the real commit line.
    // Also remove any stray <commit>/<commit> tag fragments that remain when
    // the model returns an empty or malformed commit block — these must never
    // be shown to the user.
    let stripped = strip_think_blocks(raw);
    let stripped = stripped
        .replace("<commit>", "")
        .replace("</commit>", "");
    let stripped = stripped.trim();

    if let Some(msg) = extract_conventional_commit(stripped) {
        return msg;
    }

    // ── 3. Last resort: inner content of the last think block ─────────────────
    // Only return if a CC header is found inside; never return raw prose.
    last_think_inner(raw)
        .and_then(|s| {
            let inner = s.trim();
            extract_conventional_commit(inner)
        })
        .unwrap_or_default()
}

/// Remove all `<think>...</think>` blocks (including unclosed ones) from `text`
/// and return the remainder.
///
/// Also handles the case where the model emits a bare `</think>` with no
/// matching opener — this happens when assistant-prefill places `<commit>\n`
/// *after* an implicit `<think>` block that the model started before our
/// prefill.  Everything up to and including a bare `</think>` is reasoning
/// that leaked through, so we discard it.
fn strip_think_blocks(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    loop {
        // Find the next tag — either opener or closer.
        let think_open  = rest.find("<think>");
        let think_close = rest.find("</think>");

        match (think_open, think_close) {
            // Paired block: <think>...</think>
            (Some(open), Some(close)) if open < close => {
                out.push_str(&rest[..open]);
                let after_close = close + "</think>".len();
                rest = &rest[after_close..];
            }
            // Bare </think> with no preceding <think>: everything before it
            // is orphaned reasoning — discard it.
            (None, Some(close)) | (Some(_), Some(close)) => {
                rest = &rest[close + "</think>".len()..];
            }
            // Unclosed <think> — discard everything from here.
            (Some(_open), None) => {
                break;
            }
            // No more tags.
            (None, None) => {
                out.push_str(rest);
                break;
            }
        }
    }
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

    #[test]
    fn test_bare_close_think_tag_discarded() {
        // Claude uses assistant-prefill "<commit>\n", then starts reasoning
        // inside the commit block with an implicit <think> that has no opener
        // visible to us — only the </think> closer leaks through.
        // Everything before the bare </think> is reasoning prose and must be
        // stripped; only the actual commit line after it should survive.
        let raw = "<commit>\n\
feat(core): improve commit extraction from think blocks\n\
\n\
Wait, this doesn't add a new feature – it's a refactor. Let me reconsider.\n\
\n\
refactor(commit): streamline commit message extraction logic\n\
\n\
This better captures it. Let me check the max length — it's under 72 chars.\n\
</think>\n\
\n\
refactor(commit): streamline commit message extraction logic\n\
</commit>";
        assert_eq!(
            clean_response(raw),
            "refactor(commit): streamline commit message extraction logic"
        );
    }

    #[test]
    fn test_bare_close_think_no_opener() {
        // Simpler variant: text starts with reasoning, bare </think> in the middle,
        // then the real commit line.
        let raw = "some reasoning prose\n</think>\nfeat: add feature";
        assert_eq!(clean_response(raw), "feat: add feature");
    }

    #[test]
    fn test_pure_prose_no_cc_header_returns_empty() {
        // Model returns only analysis prose with no CC header at all.
        // clean_response must return "" rather than dumping the whole wall of text.
        let raw = "<commit>\nLet me analyze the diff to understand what changes were made:\n\
\n\
1. The `clean_response` function was refactored to handle `<think>` blocks better\n\
2. The function now:\n\
   - First looks for a `<commit>...</commit>` block\n\
   - If found, it strips `<think>` blocks from within the commit content\n\
   - Then tries to extract a conventional commit from the cleaned content\n\
   - Falls back to stripping all `<think>` blocks from the raw text\n\
   - Finally, as a last resort, uses the inner content of the last `<think>` block\n\
\n\
3. A new helper function `strip_think_blocks` was extracted to remove all `<think>...";
        let result = clean_response(raw);
        assert_eq!(
            result, "",
            "pure analysis prose must not be returned as a commit message, got: {result:?}"
        );
    }

    #[test]
    fn test_single_commit_tag_not_leaked() {
        // When the model returns only "<commit>" with no closing tag and no content,
        // the raw passed to clean_response is "<commit>\n".
        // Result must be empty — the tag itself must NOT be shown to the user.
        let raw = "<commit>\n";
        assert_eq!(clean_response(raw), "");
    }

    #[test]
    fn test_single_commit_tag_with_content_after() {
        // Model returns content but the commit tag was never closed.
        let raw = "<commit>\nfeat: add login\n";
        assert_eq!(clean_response(raw), "feat: add login");
    }

    #[test]
    fn test_reasoning_with_fake_cc_line_then_bare_think_close() {
        // Model emits reasoning that contains a line *starting* with a valid
        // CC prefix ("feat: or refactor: …") followed by a bare </think>, then
        // the actual commit message.  The fake CC line must NOT be returned;
        // only the real line after </think> should survive.
        let raw = "<commit>\nfeat: or refactor: – since this is improving code structure \
without changing external behavior, refactor is more accurate.\n\n\
Looking at the rules: feat for new features, fix for bug\n\
</think>\n\nrefactor: streamline commit extraction logic\n</commit>";
        assert_eq!(
            clean_response(raw),
            "refactor: streamline commit extraction logic"
        );
    }

    #[test]
    fn test_think_reasoning_with_fake_cc_no_commit_tag() {
        // Without <commit> tags: reasoning block has a fake CC header, real commit
        // appears after </think>.
        let raw = "<think>feat: or refactor: – improving structure\n\
Looking at the rules: feat for new features, fix for bug\n\
</think>\n\nrefactor: streamline commit extraction logic";
        assert_eq!(
            clean_response(raw),
            "refactor: streamline commit extraction logic"
        );
    }

    #[test]
    fn test_screenshot_bug_reasoning_leaks_into_commit_modal() {
        // Reproduces the exact screenshot scenario:
        // The AI client prefixes the raw response with "<commit>\n".
        // Model did NOT wrap output in <commit> tags itself — it just output
        // reasoning prose that happens to start with a fake CC header, followed
        // by a bare </think>, but NO real commit line after it.
        // Expected: fall back gracefully, NOT show raw reasoning in the modal.
        let raw = "<commit>\n\
feat: or refactor: \u{2013} since this is improving code structure without changing external behavior, \
refactor is more accurate.\n\
\n\
Looking at the rules: feat for new features, fix for bug\n\
</think>";
        // The result must NOT contain "</think>" or raw reasoning prose.
        let result = clean_response(raw);
        assert!(
            !result.contains("</think>"),
            "clean_response should strip </think> but got: {result:?}"
        );
        assert!(
            !result.contains("Looking at the rules"),
            "clean_response should strip reasoning prose but got: {result:?}"
        );
    }
}
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
