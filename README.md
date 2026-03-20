# GitClaw

An AI-powered terminal Git TUI. Visualize changes, stage files, generate commit messages with AI, and manage branches & stash — all without leaving your terminal.

## Features

- **Collapsible file tree** — browse changed files by directory, bulk stage/unstage entire folders
- **Inline diff viewer** — syntax-colored added/removed/context lines with scrolling
- **AI commit messages** — generate Conventional Commits-formatted messages from your staged diff
- **Multi-provider AI** — switch between Claude, OpenAI and MiniMax (Global / CN) at runtime
- **Branch viewer** — list all local branches in a modal
- **Stash operations** — push and pop stash without leaving the TUI

## Installation

**Prerequisites:** Rust toolchain (stable), `git` in `PATH`.

```bash
git clone https://github.com/Saraph1nes/gitclaw
cd gitclaw
cargo install --path .
```

## Usage

```bash
# Open GitClaw in the current directory
gitclaw

# Open a specific repository
gitclaw /path/to/repo
```

## Key Bindings

| Key | Action |
|-----|--------|
| `↑`/`↓` or `j`/`k` | Navigate file list / scroll diff |
| `→`/`l` | Expand directory |
| `←`/`h` | Collapse directory (or jump to parent) |
| `Enter` | Directory: toggle expand/collapse — File: load diff |
| `Tab` | Cycle focus: Files → Diff → AI |
| `a` | Stage file or all files in selected directory |
| `u` | Unstage file or all files in selected directory |
| `c` | Open commit dialog |
| `i` | Generate AI commit message from staged changes |
| `m` | Select AI model |
| `b` | Show branch list |
| `s` | Stash operations menu |
| `?` | Toggle help |
| `q` / `Ctrl+C` | Quit |

## Configuration

GitClaw stores settings at `~/.config/gitclaw/config.toml`. The file is created automatically when you save an API key from within the TUI.

```toml
[ai]
default_model = "claude"       # claude | openai | minimax | minimax-cn
claude_api_key = ""
openai_api_key = ""
minimax_api_key = ""
minimax_cn_api_key = ""

[ui]
tick_rate_ms = 250
show_ai_panel = true
```

API keys can also be set via environment variables (take precedence over the config file):

| Provider | Environment Variable |
|----------|---------------------|
| Claude (Anthropic) | `ANTHROPIC_API_KEY` |
| OpenAI | `OPENAI_API_KEY` |
| MiniMax Global | `MINIMAX_API_KEY` |
| MiniMax CN | `MINIMAX_CN_API_KEY` |

## UI Layout

```
┌─────────────────────┬──────────────────────────────┐
│  Files [branch] (n) │  Diff                        │
│                     │  + added line                │
│  ▸ ● src/ (3)       │  - removed line              │
│      ▸ M app.rs     │  @@ context header           │
│      ▸ A new.rs     │                              │
├─────────────────────┴──────────────────────────────┤
│  AI Suggestions                                    │
│  feat(app): add collapsible file tree              │
├────────────────────────────────────────────────────┤
│  ↑↓:navigate  a:stage  c:commit  i:AI  q:quit     │
└────────────────────────────────────────────────────┘
```

- **Green dot `●`** — file or directory has staged changes
- **`M`/`A`/`D`/`R`/`?`/`U`** — Modified / Added / Deleted / Renamed / Untracked / Unmerged
- Active panel has a **cyan border**

## AI Workflow

1. Stage the files you want to commit (`a`)
2. Press `i` to generate a commit message from the staged diff
3. The AI panel shows the suggestion once ready
4. Press `Enter` in the AI panel (or `c`) to open the commit dialog with the suggestion pre-filled
5. Edit if needed, then `Enter` to commit

To change the AI provider, press `m`. If no API key is configured for the selected provider, GitClaw will guide you through setting one up.

## License

MIT
