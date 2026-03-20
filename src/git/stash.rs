use std::path::Path;

use super::{GitError, run_git};

/// Push current changes to stash.
pub fn stash_push(repo: &Path) -> Result<(), GitError> {
    run_git(repo, &["stash", "push"])?;
    Ok(())
}

/// Pop the latest stash entry.
pub fn stash_pop(repo: &Path) -> Result<(), GitError> {
    run_git(repo, &["stash", "pop"])?;
    Ok(())
}

/// List all stash entries.
#[allow(dead_code)]
pub fn stash_list(repo: &Path) -> Result<Vec<String>, GitError> {
    let output = run_git(repo, &["stash", "list"])?;
    Ok(output
        .lines()
        .map(|l| l.to_string())
        .filter(|l| !l.is_empty())
        .collect())
}
