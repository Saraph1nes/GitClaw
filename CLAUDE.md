# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Commands

```bash
# Build
cargo build

# Run (defaults to current directory as repo)
cargo run
cargo run -- /path/to/repo

# Run tests
cargo test

# Run a single test
cargo test test_parse_status_untracked

# Check for errors without building
cargo check

# Lint
cargo clippy
```

## Configuration

Settings are stored at `~/.config/gitclaw/config.toml` (auto-created on first save). API keys can also be provided via environment variables: `ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, `MINIMAX_API_KEY`, `MINIMAX_CN_API_KEY`.

## Architecture

GitClaw is a Tokio async TUI application built with ratatui + crossterm.

### Data flow

```
main.rs  →  App::run() (event loop)
              ├── EventHandler (background thread: key events + ticks)
              ├── git::* (synchronous subprocess calls via run_git())
              ├── ai::* (async HTTP via tokio::spawn → AppEvent channel)
              └── ui::render() (pure read of App state → ratatui widgets)
```

The app never blocks the draw loop: AI calls run in `tokio::spawn` and post results back as `AppEvent::AiResponse` / `AppEvent::AiError` through the same `mpsc::Sender<AppEvent>` that the event thread uses.

### Key types

- **`App`** (`src/app.rs`) — owns all mutable state: `files: Vec<FileEntry>`, `file_tree: FileTree`, `diff_lines`, `modal`, `ai_suggestion`, `settings`. The entire key-handling logic lives here in `handle_key` / `handle_modal_key`.
- **`FileTree`** (`src/ui/file_tree.rs`) — wraps `Vec<FileEntry>` in a collapsible directory tree. Maintains `expanded: HashSet<String>` of directory paths that survive `rebuild()` calls. `visible: Vec<VisibleRow>` is the pre-computed flat list that the UI renders and `selected_file` indexes into.
- **`Modal`** (`src/app.rs`) — enum that drives all overlay dialogs (commit input, error, confirm, model select, API key setup/input, branch list, stash menu, help). When `app.modal.is_some()`, key events go to `handle_modal_key` instead of the main handler.
- **`AppEvent`** (`src/event.rs`) — the single channel type: `Key`, `Tick`, `AiResponse(String)`, `AiError(String)`.

### Module layout

| Module | Responsibility |
|--------|---------------|
| `git/status.rs` | Parses `git status --porcelain=v1` into `Vec<FileEntry>` |
| `git/diff.rs` | Fetches file diffs (staged/unstaged/untracked) and parses into `Vec<DiffLine>` |
| `git/commit.rs` | `stage_file`, `unstage_file`, `commit` |
| `git/branch.rs` | `current_branch`, `list_branches` |
| `git/stash.rs` | `stash_push`, `stash_pop`, `stash_list` |
| `ai/types.rs` | `ModelKind`, `CommitRequest`, `SYSTEM_PROMPT`, `clean_response()` |
| `ai/claude.rs` | Claude Messages API (`claude-sonnet-4-20250514`) |
| `ai/openai.rs` | OpenAI Chat Completions (`gpt-4o-mini`) |
| `ai/minimax.rs` | MiniMax chat API — global (`api.minimax.io`) and CN (`api.minimaxi.com`) |
| `ui/mod.rs` | Top-level layout (40/60 split), modal rendering, help text |
| `ui/file_list.rs` | Renders `FileTree::visible` as a ratatui `List` with status icons |
| `ui/diff_panel.rs` | Renders `app.diff_lines` with colour-coded `DiffLineKind` |
| `ui/ai_panel.rs` | Renders AI loading / suggestion / idle state |
| `config/settings.rs` | TOML-backed `Settings` struct; `AiSettings` resolves keys from config or env |

### AI response pipeline

`app.rs: request_ai_suggestion()` → `git::diff::staged_diff_raw()` → `tokio::spawn(ai::generate_commit_message_with())` → `AppEvent::AiResponse` → `app.ai_suggestion = Some(msg)`. The `clean_response()` helper in `ai/types.rs` strips `<think>...</think>` reasoning blocks that some models emit. Diffs are truncated to 8 000 chars before sending.

### UI layout

```
┌─────────────────────┬──────────────────────────────┐
│  Files [branch] (n) │         Diff                 │
│   40%               │         60%                  │
├─────────────────────┴──────────────────────────────┤
│  AI Suggestions  (optional, 8 lines)               │
├────────────────────────────────────────────────────┤
│  help bar (1 line, context-sensitive)              │
└────────────────────────────────────────────────────┘
```

Focus cycles FileList → DiffPanel → AiPanel via `Tab`. Active panel gets a cyan border.

### Each HTTP client is a `OnceLock<reqwest::Client>`

`claude.rs`, `openai.rs`, and `minimax.rs` each hold their own static `OnceLock<reqwest::Client>` so the connection pool is reused across calls within a session.
