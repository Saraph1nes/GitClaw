use std::path::Path;

use super::{FileEntry, FileStatus, GitError, run_git};

/// Parse `git status --porcelain=v1` into a list of file entries.
pub fn get_status(repo: &Path) -> Result<Vec<FileEntry>, GitError> {
    // --untracked-files=all: list every untracked file individually instead of
    // collapsing an entire untracked directory to a single "?? dir/" entry.
    // Without this flag the tree cannot expand untracked directories.
    let output = run_git(repo, &["status", "--porcelain=v1", "--untracked-files=all"])?;
    let mut files = Vec::new();

    for line in output.lines() {
        if line.len() < 4 {
            continue;
        }

        // Porcelain v1 guarantees the first two bytes are ASCII status chars.
        let index_char = line.as_bytes()[0] as char;
        let worktree_char = line.as_bytes()[1] as char;
        let path = line[3..].to_string();

        // Renamed files encode as "R  old -> new"; extract the destination path.
        let path = if index_char == 'R' || worktree_char == 'R' {
            if let Some(idx) = path.find(" -> ") {
                path[idx + 4..].to_string()
            } else {
                path
            }
        } else {
            path
        };

        let (status, staged, unstaged) = parse_status_chars(index_char, worktree_char);

        files.push(FileEntry {
            path,
            status,
            staged,
            unstaged,
        });
    }

    Ok(files)
}

fn parse_status_chars(index: char, worktree: char) -> (FileStatus, bool, bool) {
    match (index, worktree) {
        ('?', '?') => (FileStatus::Untracked, false, true),
        ('U', _) | (_, 'U') | ('A', 'A') | ('D', 'D') => (FileStatus::Unmerged, false, true),
        _ => {
            let staged = !matches!(index, ' ' | '?');
            let unstaged = !matches!(worktree, ' ' | '?');

            // Determine primary status (prefer index status if staged)
            let status = if staged {
                char_to_status(index)
            } else {
                char_to_status(worktree)
            };

            (status, staged, unstaged)
        }
    }
}

fn char_to_status(c: char) -> FileStatus {
    match c {
        'M' => FileStatus::Modified,
        'A' => FileStatus::Added,
        'D' => FileStatus::Deleted,
        'R' => FileStatus::Renamed,
        'C' => FileStatus::Copied,
        '?' => FileStatus::Untracked,
        _ => FileStatus::Modified,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_status_modified_unstaged() {
        let (status, staged, unstaged) = parse_status_chars(' ', 'M');
        assert_eq!(status, FileStatus::Modified);
        assert!(!staged);
        assert!(unstaged);
    }

    #[test]
    fn test_parse_status_modified_staged() {
        let (status, staged, unstaged) = parse_status_chars('M', ' ');
        assert_eq!(status, FileStatus::Modified);
        assert!(staged);
        assert!(!unstaged);
    }

    #[test]
    fn test_parse_status_added_staged() {
        let (status, staged, unstaged) = parse_status_chars('A', ' ');
        assert_eq!(status, FileStatus::Added);
        assert!(staged);
        assert!(!unstaged);
    }

    #[test]
    fn test_parse_status_untracked() {
        let (status, staged, unstaged) = parse_status_chars('?', '?');
        assert_eq!(status, FileStatus::Untracked);
        assert!(!staged);
        assert!(unstaged);
    }

    #[test]
    fn test_parse_status_deleted_unstaged() {
        let (status, staged, unstaged) = parse_status_chars(' ', 'D');
        assert_eq!(status, FileStatus::Deleted);
        assert!(!staged);
        assert!(unstaged);
    }

    #[test]
    fn test_parse_status_both_modified() {
        let (status, staged, unstaged) = parse_status_chars('M', 'M');
        assert_eq!(status, FileStatus::Modified);
        assert!(staged);
        assert!(unstaged);
    }
}
