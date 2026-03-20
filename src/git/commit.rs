use std::path::Path;

use super::{GitError, run_git};

/// Stage a file (`git add`).
pub fn stage_file(repo: &Path, file_path: &str) -> Result<(), GitError> {
    run_git(repo, &["add", "--", file_path])?;
    Ok(())
}

/// Unstage a file (`git reset HEAD`).
pub fn unstage_file(repo: &Path, file_path: &str) -> Result<(), GitError> {
    run_git(repo, &["reset", "HEAD", "--", file_path])?;
    Ok(())
}

/// Create a commit with the given message.
pub fn commit(repo: &Path, message: &str) -> Result<(), GitError> {
    run_git(repo, &["commit", "-m", message])?;
    Ok(())
}
