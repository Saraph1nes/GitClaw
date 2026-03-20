pub mod status;
pub mod diff;
pub mod commit;
pub mod branch;
pub mod stash;

use std::path::Path;
use std::process::Command;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum GitError {
    #[error("Git command failed: {0}")]
    CommandFailed(String),
    #[error("Not a git repository: {0}")]
    #[allow(dead_code)]
    NotARepo(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Parse error: {0}")]
    #[allow(dead_code)]
    Parse(String),
}

/// Status of a file in git.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileStatus {
    Modified,
    Added,
    Deleted,
    Renamed,
    Copied,
    Untracked,
    Unmerged,
}

/// A file entry from git status.
#[derive(Debug, Clone)]
pub struct FileEntry {
    pub path: String,
    pub status: FileStatus,
    /// True if the file has staged changes (index column).
    pub staged: bool,
    /// True if the file has unstaged changes (worktree column).
    #[allow(dead_code)]
    pub unstaged: bool,
}

impl FileEntry {
    pub fn is_staged(&self) -> bool {
        self.staged
    }

    pub fn is_untracked(&self) -> bool {
        self.status == FileStatus::Untracked
    }
}

/// Kind of a diff line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiffLineKind {
    Added,
    Removed,
    Header,
    Context,
}

/// A single line in a diff output.
#[derive(Debug, Clone)]
pub struct DiffLine {
    pub content: String,
    pub kind: DiffLineKind,
}

impl DiffLine {
    pub fn context(content: String) -> Self {
        Self {
            content,
            kind: DiffLineKind::Context,
        }
    }
}

/// Run a git command in the given repo directory and return stdout.
pub fn run_git(repo: &Path, args: &[&str]) -> Result<String, GitError> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        Err(GitError::CommandFailed(stderr))
    }
}
