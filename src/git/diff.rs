use std::path::Path;

use super::{DiffLine, DiffLineKind, GitError, run_git};

/// Get diff for a specific unstaged file.
pub fn file_diff(repo: &Path, file_path: &str) -> Result<Vec<DiffLine>, GitError> {
    let output = run_git(repo, &["diff", "--", file_path])?;
    Ok(parse_diff(&output))
}

/// Get diff for a specific staged file.
pub fn file_diff_staged(repo: &Path, file_path: &str) -> Result<Vec<DiffLine>, GitError> {
    let output = run_git(repo, &["diff", "--cached", "--", file_path])?;
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

    let mut lines = vec![
        DiffLine {
            content: format!("=== Untracked file: {} ===", file_path),
            kind: DiffLineKind::Header,
        },
    ];

    for line in content.lines() {
        lines.push(DiffLine {
            content: format!("+{}", line),
            kind: DiffLineKind::Added,
        });
    }

    Ok(lines)
}

/// Parse raw git diff output into typed DiffLines.
fn parse_diff(raw: &str) -> Vec<DiffLine> {
    raw.lines()
        .map(|line| {
            let kind = if line.starts_with('+') && !line.starts_with("+++") {
                DiffLineKind::Added
            } else if line.starts_with('-') && !line.starts_with("---") {
                DiffLineKind::Removed
            } else if line.starts_with("@@")
                || line.starts_with("diff ")
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
            }
        })
        .collect()
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
        assert_eq!(lines[4].kind, DiffLineKind::Header); // @@
        assert_eq!(lines[5].kind, DiffLineKind::Context);
        assert_eq!(lines[6].kind, DiffLineKind::Added);
        assert_eq!(lines[7].kind, DiffLineKind::Removed);
        assert_eq!(lines[8].kind, DiffLineKind::Context);
    }
}
