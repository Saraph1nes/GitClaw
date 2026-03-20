//! Collapsible file tree built on top of `Vec<FileEntry>`.
//!
//! This module is a pure view layer — it does **not** issue any git commands.
//! All it does is:
//!  1. Parse `FileEntry.path` fields into a hierarchical `TreeNode` structure.
//!  2. Flatten the visible portion of that hierarchy into `Vec<VisibleRow>`.
//!  3. Expose helpers to toggle directories and collect file indices for
//!     bulk staging operations.

use std::collections::{BTreeMap, HashSet};

use crate::git::FileEntry;

// ─────────────────────────────────────────────
// Public surface
// ─────────────────────────────────────────────

/// A single row in the flattened, visible representation of the tree.
#[derive(Debug, Clone)]
pub struct VisibleRow {
    pub depth: usize,
    pub kind: RowKind,
}

/// What kind of node a visible row represents.
#[derive(Debug, Clone)]
pub enum RowKind {
    /// A directory node.
    Dir {
        /// Full path from repo root, e.g. `"src/ui"`.
        path: String,
        /// Basename only, e.g. `"ui"`.
        name: String,
        /// Whether this directory is currently expanded.
        expanded: bool,
        /// Total number of files under this directory (recursively).
        file_count: usize,
        /// `true` if *any* file under this directory is staged.
        has_staged: bool,
    },
    /// A file node; `entry_index` is the index into the original `Vec<FileEntry>`.
    File { entry_index: usize },
}

/// The main collapsible tree.
pub struct FileTree {
    /// Internal tree nodes (root level).
    root: Vec<TreeNode>,
    /// Set of directory paths that are currently expanded.
    expanded: HashSet<String>,
    /// Flattened, visible rows — rebuilt whenever the tree changes.
    pub visible: Vec<VisibleRow>,
}

// ─────────────────────────────────────────────
// Internal tree representation
// ─────────────────────────────────────────────

/// Internal tree node — not exposed to callers.
#[derive(Debug)]
enum TreeNode {
    Dir {
        name: String,
        /// Full path from repo root.
        path: String,
        children: Vec<TreeNode>,
        file_count: usize,
        has_staged: bool,
    },
    File {
        entry_index: usize,
    },
}

// ─────────────────────────────────────────────
// FileTree implementation
// ─────────────────────────────────────────────

impl FileTree {
    /// Create an empty tree (useful before the first `rebuild` call).
    pub fn new(files: &[FileEntry]) -> Self {
        let mut tree = Self {
            root: Vec::new(),
            expanded: HashSet::new(),
            visible: Vec::new(),
        };
        tree.rebuild(files);
        tree
    }

    // ── Public methods ────────────────────────

    /// Rebuild the internal tree from a fresh `Vec<FileEntry>`.
    ///
    /// The current `expanded` set is preserved across rebuilds so that
    /// toggling a directory, then doing a git operation, keeps the directory
    /// open.  Paths that no longer exist are pruned from `expanded`.
    pub fn rebuild(&mut self, files: &[FileEntry]) {
        self.root = build_tree(files);
        // Prune stale expanded paths.
        let live_dirs: HashSet<String> = collect_dir_paths(&self.root);
        self.expanded.retain(|p| live_dirs.contains(p));
        self.recompute_visible();
    }

    /// Toggle the expanded/collapsed state of a directory.
    pub fn toggle(&mut self, dir_path: &str) {
        if self.expanded.contains(dir_path) {
            self.expanded.remove(dir_path);
        } else {
            self.expanded.insert(dir_path.to_string());
        }
        self.recompute_visible();
    }

    /// Expand a directory (no-op if already expanded).
    pub fn expand(&mut self, dir_path: &str) {
        self.expanded.insert(dir_path.to_string());
        self.recompute_visible();
    }

    /// Collapse a directory (no-op if already collapsed).
    pub fn collapse(&mut self, dir_path: &str) {
        self.expanded.remove(dir_path);
        self.recompute_visible();
    }

    /// Collect all `entry_index` values for files under `dir_path`
    /// (recursively).  Used for bulk stage/unstage operations.
    pub fn collect_file_indices(&self, dir_path: &str) -> Vec<usize> {
        let mut out = Vec::new();
        collect_indices_under(&self.root, dir_path, &mut out);
        out
    }

    /// Find the parent directory path of a file visible row.
    /// Returns `None` if the row is at the root level or is not a file.
    pub fn parent_dir_of_visible(&self, row_idx: usize) -> Option<String> {
        if let Some(row) = self.visible.get(row_idx) {
            match &row.kind {
                RowKind::File { entry_index: _ } => {
                    // Walk backwards to find the nearest Dir row with depth < current depth.
                    for i in (0..row_idx).rev() {
                        if let RowKind::Dir { path, .. } = &self.visible[i].kind {
                            if self.visible[i].depth < row.depth {
                                return Some(path.clone());
                            }
                        }
                    }
                    None
                }
                RowKind::Dir { path, .. } => Some(path.clone()),
            }
        } else {
            None
        }
    }

    // ── Private helpers ───────────────────────

    /// DFS-walk `root`, emitting only expanded branches into `visible`.
    fn recompute_visible(&mut self) {
        self.visible.clear();
        dfs_flatten(&self.root, 0, &self.expanded, &mut self.visible);
    }
}

// ─────────────────────────────────────────────
// Build algorithm
// ─────────────────────────────────────────────

/// Convert a flat `Vec<FileEntry>` into a tree of `TreeNode`s.
///
/// Strategy:
///  1. Use a `BTreeMap<Vec<String>, _>` keyed on path components to build an
///     intermediate representation.
///  2. Convert that map into sorted `Vec<TreeNode>`, directories before files.
fn build_tree(files: &[FileEntry]) -> Vec<TreeNode> {
    // Intermediate: map from directory-component-path → list of (file_name, entry_index, staged)
    // We build a recursive map manually.
    let mut dir_map: BTreeMap<Vec<String>, Vec<(String, usize, bool)>> = BTreeMap::new();

    for (idx, entry) in files.iter().enumerate() {
        let parts: Vec<&str> = entry.path.split('/').collect();
        if parts.len() == 1 {
            // Root-level file — key = empty vec.
            dir_map
                .entry(vec![])
                .or_default()
                .push((parts[0].to_string(), idx, entry.staged));
        } else {
            // File inside a directory.
            let dir_parts: Vec<String> = parts[..parts.len() - 1]
                .iter()
                .map(|s| s.to_string())
                .collect();
            dir_map
                .entry(dir_parts)
                .or_default()
                .push((parts[parts.len() - 1].to_string(), idx, entry.staged));
        }
    }

    // Build the tree recursively from the root.
    build_nodes(&dir_map, &[], files)
}

/// Recursive helper: build `Vec<TreeNode>` for nodes that live directly under
/// `parent_components`.
fn build_nodes(
    dir_map: &BTreeMap<Vec<String>, Vec<(String, usize, bool)>>,
    parent: &[String],
    files: &[FileEntry],
) -> Vec<TreeNode> {
    // Collect immediate subdirectory names under `parent`.
    let mut sub_dirs: BTreeMap<String, ()> = BTreeMap::new();
    for key in dir_map.keys() {
        if key.len() > parent.len() && key.starts_with(parent) {
            sub_dirs.insert(key[parent.len()].clone(), ());
        }
    }

    let mut nodes: Vec<TreeNode> = Vec::new();

    // Directories first (sorted by BTreeMap iteration order).
    for dir_name in sub_dirs.keys() {
        let mut child_path_parts = parent.to_vec();
        child_path_parts.push(dir_name.clone());
        let full_path = child_path_parts.join("/");

        let children = build_nodes(dir_map, &child_path_parts, files);
        let file_count = count_files(&children);
        let has_staged = any_staged(&children, files);

        nodes.push(TreeNode::Dir {
            name: dir_name.clone(),
            path: full_path,
            children,
            file_count,
            has_staged,
        });
    }

    // Files under this exact directory (sorted by name via BTreeMap).
    if let Some(file_list) = dir_map.get(parent) {
        // Sort by name.
        let mut sorted = file_list.clone();
        sorted.sort_by(|a, b| a.0.cmp(&b.0));
        for (_name, idx, _staged) in sorted {
            nodes.push(TreeNode::File {
                entry_index: idx,
            });
        }
    }

    nodes
}

/// Count all file nodes recursively under a slice of nodes.
fn count_files(nodes: &[TreeNode]) -> usize {
    nodes.iter().map(|n| match n {
        TreeNode::File { .. } => 1,
        TreeNode::Dir { children, .. } => count_files(children),
    }).sum()
}

/// Return `true` if any file recursively under `nodes` is staged.
fn any_staged(nodes: &[TreeNode], files: &[FileEntry]) -> bool {
    nodes.iter().any(|n| match n {
        TreeNode::File { entry_index, .. } => {
            files.get(*entry_index).map_or(false, |f| f.staged)
        }
        TreeNode::Dir { children, .. } => any_staged(children, files),
    })
}

// ─────────────────────────────────────────────
// Flatten (DFS)
// ─────────────────────────────────────────────

/// DFS-walk the tree and push visible rows into `out`.
fn dfs_flatten(
    nodes: &[TreeNode],
    depth: usize,
    expanded: &HashSet<String>,
    out: &mut Vec<VisibleRow>,
) {
    for node in nodes {
        match node {
            TreeNode::Dir {
                name,
                path,
                children,
                file_count,
                has_staged,
            } => {
                let is_expanded = expanded.contains(path.as_str());
                out.push(VisibleRow {
                    depth,
                    kind: RowKind::Dir {
                        path: path.clone(),
                        name: name.clone(),
                        expanded: is_expanded,
                        file_count: *file_count,
                        has_staged: *has_staged,
                    },
                });
                if is_expanded {
                    dfs_flatten(children, depth + 1, expanded, out);
                }
            }
            TreeNode::File { entry_index, .. } => {
                out.push(VisibleRow {
                    depth,
                    kind: RowKind::File {
                        entry_index: *entry_index,
                    },
                });
            }
        }
    }
}

// ─────────────────────────────────────────────
// Utility helpers
// ─────────────────────────────────────────────

/// Collect all directory `path` strings from the tree (for pruning `expanded`).
fn collect_dir_paths(nodes: &[TreeNode]) -> HashSet<String> {
    let mut set = HashSet::new();
    for node in nodes {
        if let TreeNode::Dir { path, children, .. } = node {
            set.insert(path.clone());
            set.extend(collect_dir_paths(children));
        }
    }
    set
}

/// Recursively collect file `entry_index` values under `dir_path`.
fn collect_indices_under(nodes: &[TreeNode], dir_path: &str, out: &mut Vec<usize>) {
    for node in nodes {
        match node {
            TreeNode::Dir { path, children, .. } => {
                // If this dir *is* the target, collect all files below it.
                // If this dir is a prefix of the target, recurse deeper.
                if path == dir_path || dir_path.starts_with(&format!("{}/", path)) {
                    if path == dir_path {
                        collect_all_file_indices(children, out);
                    } else {
                        collect_indices_under(children, dir_path, out);
                    }
                }
            }
            TreeNode::File { .. } => {} // Root-level files; not inside any dir.
        }
    }
}

/// Collect every file index under a node slice (no filtering).
fn collect_all_file_indices(nodes: &[TreeNode], out: &mut Vec<usize>) {
    for node in nodes {
        match node {
            TreeNode::File { entry_index, .. } => out.push(*entry_index),
            TreeNode::Dir { children, .. } => collect_all_file_indices(children, out),
        }
    }
}

// ─────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::{FileEntry, FileStatus};

    fn entry(path: &str, staged: bool) -> FileEntry {
        FileEntry {
            path: path.to_string(),
            status: FileStatus::Modified,
            staged,
            unstaged: !staged,
        }
    }

    #[test]
    fn test_empty() {
        let tree = FileTree::new(&[]);
        assert!(tree.visible.is_empty());
    }

    #[test]
    fn test_root_file() {
        let files = vec![entry("README.md", false)];
        let tree = FileTree::new(&files);
        // One root-level file row.
        assert_eq!(tree.visible.len(), 1);
        assert!(matches!(tree.visible[0].kind, RowKind::File { entry_index: 0 }));
        assert_eq!(tree.visible[0].depth, 0);
    }

    #[test]
    fn test_single_dir_collapsed() {
        let files = vec![entry("src/main.rs", false)];
        let tree = FileTree::new(&files);
        // Only the dir row is visible (collapsed by default).
        assert_eq!(tree.visible.len(), 1);
        assert!(matches!(tree.visible[0].kind, RowKind::Dir { .. }));
    }

    #[test]
    fn test_toggle_expand() {
        let files = vec![entry("src/main.rs", false)];
        let mut tree = FileTree::new(&files);
        tree.toggle("src");
        // Dir + file.
        assert_eq!(tree.visible.len(), 2);
        assert!(matches!(tree.visible[1].kind, RowKind::File { entry_index: 0 }));
    }

    #[test]
    fn test_collect_indices() {
        let files = vec![
            entry("src/a.rs", false),
            entry("src/b.rs", true),
            entry("README.md", false),
        ];
        let tree = FileTree::new(&files);
        let indices = tree.collect_file_indices("src");
        // Should contain the indices for a.rs and b.rs.
        assert_eq!(indices.len(), 2);
        assert!(indices.contains(&0));
        assert!(indices.contains(&1));
    }

    #[test]
    fn test_has_staged_aggregation() {
        let files = vec![
            entry("src/a.rs", false),
            entry("src/b.rs", true),
        ];
        let tree = FileTree::new(&files);
        if let RowKind::Dir { has_staged, .. } = &tree.visible[0].kind {
            assert!(*has_staged);
        } else {
            panic!("Expected Dir row");
        }
    }

    #[test]
    fn test_nested_dirs() {
        let files = vec![entry("a/b/c/file.rs", false)];
        let tree = FileTree::new(&files);
        // Only 'a' dir visible at root.
        assert_eq!(tree.visible.len(), 1);
        assert!(matches!(&tree.visible[0].kind, RowKind::Dir { name, .. } if name == "a"));
    }
}
