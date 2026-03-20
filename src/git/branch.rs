use std::path::Path;

use super::{GitError, run_git};

/// Get the current branch name.
pub fn current_branch(repo: &Path) -> Result<String, GitError> {
    let output = run_git(repo, &["rev-parse", "--abbrev-ref", "HEAD"])?;
    Ok(output.trim().to_string())
}

/// List all local branches.
pub fn list_branches(repo: &Path) -> Result<Vec<String>, GitError> {
    let output = run_git(repo, &["branch", "--format=%(refname:short)"])?;
    Ok(output
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect())
}
