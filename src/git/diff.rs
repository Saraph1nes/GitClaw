use std::path::Path;

use super::{DiffLine, DiffLineKind, GitError, run_git};

/// Get diff for a specific unstaged file.
pub fn file_diff(repo: &Path, file_path: &str) -> Result<Vec<DiffLine>, GitError> {
    let output = run_git(repo, &["diff", "-U99999", "--", file_path])?;
    Ok(parse_diff(&output))
}

/// Get diff for a specific staged file.
pub fn file_diff_staged(repo: &Path, file_path: &str) -> Result<Vec<DiffLine>, GitError> {
    let output = run_git(repo, &["diff", "--cached", "-U99999", "--", file_path])?;
    Ok(parse_diff(&output))
}

/// Get the full staged diff (all staged changes).
#[allow(dead_code)]
pub fn staged_diff(repo: &Path) -> Result<Vec<DiffLine>, GitError> {
    let output = run_git(repo, &["diff", "--cached"])?;
    Ok(parse_diff(&output))
}

/// Get the raw staged diff text — for AI consumption (avoids parse round-trip).
pub fn staged_diff_raw(repo: &Path) -> Result<String, GitError> {
    run_git(repo, &["diff", "--cached"])
}

/// Show content of an untracked file as an "added" diff.
pub fn untracked_file_diff(repo: &Path, file_path: &str) -> Result<Vec<DiffLine>, GitError> {
    let full_path = repo.join(file_path);
    let content = std::fs::read_to_string(&full_path)?;

    let mut lines = vec![DiffLine {
        content: format!("=== Untracked file: {} ===", file_path),
        kind: DiffLineKind::Header,
        old_lineno: None,
        new_lineno: None,
    }];

    for (i, line) in content.lines().enumerate() {
        lines.push(DiffLine {
            content: format!("+{}", line),
            kind: DiffLineKind::Added,
            old_lineno: None,
            new_lineno: Some((i + 1) as u32),
        });
    }

    Ok(lines)
}

/// Parse `@@ -old_start[,len] +new_start[,len] @@` and return `(old_start, new_start)`.
fn parse_hunk_header(line: &str) -> Option<(u32, u32)> {
    // line looks like: "@@ -10,5 +12,7 @@ fn foo()"
    let inner = line.strip_prefix("@@ -")?;
    let (old_part, rest) = inner.split_once(' ')?;
    let old_start: u32 = old_part.split(',').next()?.parse().ok()?;
    let new_part = rest.strip_prefix('+')?;
    let new_start: u32 = new_part
        .split([',', ' '])
        .next()?
        .parse()
        .ok()?;
    Some((old_start, new_start))
}

/// Parse raw git diff output into typed DiffLines with line numbers.
fn parse_diff(raw: &str) -> Vec<DiffLine> {
    // Step 1: classify each line
    let mut lines: Vec<DiffLine> = raw
        .lines()
        .map(|line| {
            let kind = if line.starts_with("Binary files") && line.contains("differ") {
                DiffLineKind::Binary
            } else if line.starts_with("@@") {
                DiffLineKind::HunkHeader
            } else if line.starts_with('+') && !line.starts_with("+++") {
                DiffLineKind::Added
            } else if line.starts_with('-') && !line.starts_with("---") {
                DiffLineKind::Removed
            } else if line.starts_with('\\') {
                // "\ No newline at end of file" — treat as header so it doesn't affect line numbers
                DiffLineKind::Header
            } else if line.starts_with("diff ")
                || line.starts_with("index ")
                || line.starts_with("---")
                || line.starts_with("+++")
            {
                DiffLineKind::Header
            } else {
                DiffLineKind::Context
            };

            DiffLine {
                content: line.to_string(),
                kind,
                old_lineno: None,
                new_lineno: None,
            }
        })
        .collect();

    // Step 2: assign line numbers by walking hunk headers
    let mut old_cur: u32 = 0;
    let mut new_cur: u32 = 0;

    for line in &mut lines {
        match line.kind {
            DiffLineKind::HunkHeader => {
                if let Some((old_start, new_start)) = parse_hunk_header(&line.content) {
                    old_cur = old_start;
                    new_cur = new_start;
                }
            }
            DiffLineKind::Context => {
                line.old_lineno = Some(old_cur);
                line.new_lineno = Some(new_cur);
                old_cur += 1;
                new_cur += 1;
            }
            DiffLineKind::Added => {
                line.new_lineno = Some(new_cur);
                new_cur += 1;
            }
            DiffLineKind::Removed => {
                line.old_lineno = Some(old_cur);
                old_cur += 1;
            }
            // Header, HunkHeader, Binary — no line numbers
            _ => {}
        }
    }

    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_diff_lines() {
        let raw = "diff --git a/foo.rs b/foo.rs\nindex abc..def 100644\n--- a/foo.rs\n+++ b/foo.rs\n@@ -1,3 +1,4 @@\n context line\n+added line\n-removed line\n context line\n";
        let lines = parse_diff(raw);

        assert_eq!(lines[0].kind, DiffLineKind::Header); // diff --git
        assert_eq!(lines[1].kind, DiffLineKind::Header); // index
        assert_eq!(lines[2].kind, DiffLineKind::Header); // ---
        assert_eq!(lines[3].kind, DiffLineKind::Header); // +++
        assert_eq!(lines[4].kind, DiffLineKind::HunkHeader); // @@
        assert_eq!(lines[5].kind, DiffLineKind::Context);
        assert_eq!(lines[6].kind, DiffLineKind::Added);
        assert_eq!(lines[7].kind, DiffLineKind::Removed);
        assert_eq!(lines[8].kind, DiffLineKind::Context);
    }

    #[test]
    fn test_parse_hunk_header() {
        assert_eq!(parse_hunk_header("@@ -10,5 +12,7 @@ fn foo()"), Some((10, 12)));
        assert_eq!(parse_hunk_header("@@ -1 +1 @@"), Some((1, 1)));
        assert_eq!(parse_hunk_header("@@ -0,0 +1,3 @@"), Some((0, 1)));
    }

    #[test]
    fn test_line_numbers() {
        let raw = "diff --git a/foo.rs b/foo.rs\n--- a/foo.rs\n+++ b/foo.rs\n@@ -5,3 +5,4 @@\n ctx\n+added\n-removed\n ctx2\n";
        let lines = parse_diff(raw);
        // index 0: Header "diff --git..."
        // index 1: Header "---"
        // index 2: Header "+++"
        // index 3: HunkHeader "@@"
        // index 4: Context " ctx"
        // index 5: Added "+added"
        // index 6: Removed "-removed"
        // index 7: Context " ctx2"

        // HunkHeader at index 3 — no lineno
        assert_eq!(lines[3].kind, DiffLineKind::HunkHeader);
        assert_eq!(lines[3].old_lineno, None);

        // Context at index 4: old=5, new=5
        assert_eq!(lines[4].old_lineno, Some(5));
        assert_eq!(lines[4].new_lineno, Some(5));

        // Added at index 5: no old, new=6
        assert_eq!(lines[5].old_lineno, None);
        assert_eq!(lines[5].new_lineno, Some(6));

        // Removed at index 6: old=6, no new
        assert_eq!(lines[6].old_lineno, Some(6));
        assert_eq!(lines[6].new_lineno, None);

        // Context at index 7: old=7, new=7
        assert_eq!(lines[7].old_lineno, Some(7));
        assert_eq!(lines[7].new_lineno, Some(7));
    }

    #[test]
    fn test_binary_detection() {
        let raw = "diff --git a/img.png b/img.png\nindex abc..def 100644\nBinary files a/img.png and b/img.png differ\n";
        let lines = parse_diff(raw);
        assert_eq!(lines[2].kind, DiffLineKind::Binary);
    }
}
