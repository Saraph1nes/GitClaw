use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::event::{KeyCode, KeyModifiers, MouseEventKind};
use ratatui::layout::Rect;
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

use crate::ai;
use crate::config::Settings;
use crate::event::{AppEvent, EventHandler};
use crate::git;
use crate::git::{DiffLine, DiffLineKind, FileEntry};
use crate::ui;
use crate::ui::MODEL_NAMES;
use crate::ui::file_tree::{FileTree, RowKind};


/// How to render the diff panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffViewMode {
    Unified,
    SideBySide,
}

/// Which panel currently has focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    FileList,
    DiffPanel,
    AiPanel,
}

impl Focus {
    pub fn next(self) -> Self {
        match self {
            Focus::FileList => Focus::DiffPanel,
            Focus::DiffPanel => Focus::AiPanel,
            Focus::AiPanel => Focus::FileList,
        }
    }
}

/// Modal dialog state.
#[derive(Debug, Clone)]
pub enum Modal {
    /// Commit message input.
    CommitInput(String),
    /// Error message display.
    Error(String),
    /// Confirmation dialog.
    Confirm {
        message: String,
        action: ConfirmAction,
    },
    /// Model selection list.
    ModelSelect(usize),
    /// API key setup: choose between browser-auth or manual key entry.
    /// `model` is the provider slug ("claude" / "openai" / "minimax").
    ApiKeySetup {
        model: String,
        selected: usize, // 0 = browser, 1 = manual entry
    },
    /// Manual API key input for a given provider.
    ApiKeyInput {
        model: String,
        input: String,
    },
    /// Branch list.
    BranchList {
        branches: Vec<String>,
        selected: usize,
    },
    /// Stash operations.
    StashMenu,
    /// Help screen.
    Help,
}

#[derive(Debug, Clone)]
pub enum ConfirmAction {
    StashPush,
    StashPop,
}

/// Main application state.
pub struct App {
    pub repo_path: PathBuf,
    pub running: bool,
    pub focus: Focus,
    pub files: Vec<FileEntry>,
    pub selected_file: usize,
    pub diff_lines: Vec<DiffLine>,
    pub diff_scroll: usize,
    /// Horizontal scroll offset (in columns) for the diff panel.
    pub diff_hscroll: usize,
    /// Unified or side-by-side view mode.
    pub diff_view_mode: DiffViewMode,
    /// Collapsed mode: only show changed hunks + minimal context.
    pub diff_collapsed: bool,
    /// Positions (raw diff_lines indices) of each HunkHeader in the current diff.
    pub hunk_positions: Vec<usize>,
    pub ai_scroll: usize,
    pub branch_name: String,
    pub modal: Option<Modal>,
    pub ai_suggestion: Option<String>,
    pub ai_loading: bool,
    pub settings: Settings,
    pub event_tx: Option<mpsc::Sender<AppEvent>>,
    /// Collapsible tree view over `files`.
    pub file_tree: FileTree,
    /// Panel areas tracked during render for mouse hit-detection.
    pub file_list_area: Rect,
    pub diff_panel_area: Rect,
    pub ai_panel_area: Rect,
    /// Virtual scroll offset for the file list (first visible row index).
    pub file_list_scroll: usize,
    /// Maximum valid diff_scroll value — written by the renderer each frame so
    /// event handlers can clamp without duplicating the display_total logic.
    pub diff_scroll_max: usize,
    /// Maximum valid ai_scroll value — written by the renderer each frame.
    pub ai_scroll_max: usize,
    /// Whether a diff reload is pending (debounced navigation).
    pending_diff_load: bool,
    /// Timestamp of the last file-list navigation event.
    last_nav_time: Option<Instant>,
}

impl App {
    pub fn new(repo_path: PathBuf) -> Self {
        let settings = Settings::load().unwrap_or_default();
        Self {
            repo_path,
            running: true,
            focus: Focus::FileList,
            files: Vec::new(),
            selected_file: 0,
            diff_lines: Vec::new(),
            diff_scroll: 0,
            diff_hscroll: 0,
            diff_view_mode: DiffViewMode::Unified,
            diff_collapsed: false,
            hunk_positions: Vec::new(),
            ai_scroll: 0,
            branch_name: String::new(),
            modal: None,
            ai_suggestion: None,
            ai_loading: false,
            settings,
            event_tx: None,
            file_tree: FileTree::new(&[]),
            file_list_area: Rect::default(),
            diff_panel_area: Rect::default(),
            ai_panel_area: Rect::default(),
            file_list_scroll: 0,
            diff_scroll_max: 0,
            ai_scroll_max: 0,
            pending_diff_load: false,
            last_nav_time: None,
        }
    }

    /// Main event loop.
    pub async fn run(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    ) -> Result<()> {
        let tick_rate = Duration::from_millis(self.settings.ui.tick_rate_ms);
        let events = EventHandler::new(tick_rate);
        self.event_tx = Some(events.sender());

        // Initial data load
        self.refresh_status();
        self.refresh_branch();

        while self.running {
            // Draw UI
            terminal.draw(|frame| {
                ui::render(frame, self);
            })?;

            // Handle events
            match events.next()? {
                AppEvent::Key(key) => self.handle_key(key.code, key.modifiers),
                AppEvent::Mouse(mouse) => self.handle_mouse(mouse.kind, mouse.column, mouse.row),
                AppEvent::Tick => {
                    self.maybe_load_pending_diff();
                }
                AppEvent::AiResponse(msg) => {
                    self.ai_loading = false;
                    self.ai_scroll = 0;
                    self.ai_suggestion = Some(msg.clone());
                    // Auto-open the commit dialog so the user can immediately
                    // review / edit and commit without extra key presses.
                    if self.modal.is_none() {
                        self.modal = Some(Modal::CommitInput(msg));
                    }
                }
                AppEvent::AiError(err) => {
                    self.ai_loading = false;
                    self.modal = Some(Modal::Error(format!("AI Error: {}", err)));
                }
            }
        }

        Ok(())
    }

    fn handle_mouse(&mut self, kind: MouseEventKind, col: u16, row: u16) {
        // Ignore mouse events when a modal is open.
        if self.modal.is_some() {
            return;
        }

        let (scroll_up, scroll_down) = match kind {
            MouseEventKind::ScrollUp => (true, false),
            MouseEventKind::ScrollDown => (false, true),
            _ => return,
        };

        // Determine which panel was scrolled by hit-testing the stored areas.
        let in_file_list = contains(self.file_list_area, col, row);
        let in_diff_panel = contains(self.diff_panel_area, col, row);
        let in_ai_panel = self.settings.ui.show_ai_panel && contains(self.ai_panel_area, col, row);

        if in_file_list {
            if scroll_up && self.selected_file > 0 {
                self.selected_file -= 1;
                self.clamp_file_list_scroll();
                self.mark_diff_pending();
            } else if scroll_down && self.selected_file + 1 < self.file_tree.visible.len() {
                self.selected_file += 1;
                self.clamp_file_list_scroll();
                self.mark_diff_pending();
            }
        } else if in_diff_panel {
            if scroll_up && self.diff_scroll > 0 {
                self.diff_scroll -= 1;
            } else if scroll_down && self.diff_scroll < self.diff_scroll_max {
                self.diff_scroll += 1;
            }
        } else if in_ai_panel {
            if scroll_up && self.ai_scroll > 0 {
                self.ai_scroll -= 1;
            } else if scroll_down && self.ai_scroll < self.ai_scroll_max {
                self.ai_scroll += 1;
            }
        }
    }

    fn handle_key(&mut self, code: KeyCode, modifiers: KeyModifiers) {
        // If a modal is open, handle modal keys first
        if self.modal.is_some() {
            self.handle_modal_key(code, modifiers);
            return;
        }

        match code {
            KeyCode::Char('q') => self.running = false,
            KeyCode::Char('c') if modifiers.contains(KeyModifiers::CONTROL) => {
                self.running = false;
            }
            KeyCode::Tab => {
                self.focus = self.focus.next();
                // Skip AiPanel if it's hidden
                if self.focus == Focus::AiPanel && !self.settings.ui.show_ai_panel {
                    self.focus = self.focus.next();
                }
            }
            // ── File list navigation ──────────────────────────────────────────
            KeyCode::Up | KeyCode::Char('k') if self.focus == Focus::FileList => {
                if self.selected_file > 0 {
                    self.selected_file -= 1;
                    self.clamp_file_list_scroll();
                    self.mark_diff_pending();
                }
            }
            KeyCode::Down | KeyCode::Char('j') if self.focus == Focus::FileList => {
                if self.selected_file + 1 < self.file_tree.visible.len() {
                    self.selected_file += 1;
                    self.clamp_file_list_scroll();
                    self.mark_diff_pending();
                }
            }
            // → / l  →  expand directory (no-op on files)
            KeyCode::Right | KeyCode::Char('l') if self.focus == Focus::FileList => {
                if let Some(row) = self.file_tree.visible.get(self.selected_file) {
                    if let RowKind::Dir { path, expanded, .. } = row.kind.clone() {
                        if !expanded {
                            self.file_tree.expand(&path);
                        }
                    }
                }
            }
            // ← / h  →  collapse directory; on a file: collapse & jump to parent
            KeyCode::Left | KeyCode::Char('h') if self.focus == Focus::FileList => {
                if let Some(row) = self.file_tree.visible.get(self.selected_file) {
                    match row.kind.clone() {
                        RowKind::Dir { path, expanded, .. } => {
                            if expanded {
                                self.file_tree.collapse(&path);
                                // Clamp selection — row count may shrink.
                                let len = self.file_tree.visible.len();
                                if len == 0 {
                                    self.selected_file = 0;
                                } else if self.selected_file >= len {
                                    self.selected_file = len - 1;
                                }
                            }
                        }
                        RowKind::File { .. } => {
                            // Collapse parent dir and jump to it.
                            let parent = self.file_tree.parent_dir_of_visible(self.selected_file);
                            if let Some(parent_path) = parent {
                                self.file_tree.collapse(&parent_path);
                                // Find the (now-collapsed) parent row and jump to it.
                                if let Some(idx) = self
                                    .file_tree
                                    .visible
                                    .iter()
                                    .position(|r| matches!(&r.kind, RowKind::Dir { path, .. } if path == &parent_path))
                                {
                                    self.selected_file = idx;
                                }
                            }
                        }
                    }
                }
            }
            // Enter: Dir → toggle; File → load diff
            KeyCode::Enter if self.focus == Focus::FileList => {
                if let Some(row) = self.file_tree.visible.get(self.selected_file) {
                    match row.kind.clone() {
                        RowKind::Dir { path, .. } => {
                            self.file_tree.toggle(&path);
                            // Clamp selection after potential shrink.
                            let len = self.file_tree.visible.len();
                            if len == 0 {
                                self.selected_file = 0;
                            } else if self.selected_file >= len {
                                self.selected_file = len - 1;
                            }
                        }
                        RowKind::File { .. } => {
                            self.load_selected_diff();
                        }
                    }
                }
            }
            // Diff scrolling
            KeyCode::Up | KeyCode::Char('k') if self.focus == Focus::DiffPanel => {
                if self.diff_scroll > 0 {
                    self.diff_scroll -= 1;
                }
            }
            KeyCode::Down | KeyCode::Char('j') if self.focus == Focus::DiffPanel => {
                if self.diff_scroll < self.diff_scroll_max {
                    self.diff_scroll += 1;
                }
            }
            // Diff horizontal scrolling
            KeyCode::Right | KeyCode::Char('l') if self.focus == Focus::DiffPanel => {
                self.diff_hscroll = self.diff_hscroll.saturating_add(4);
            }
            KeyCode::Left | KeyCode::Char('h') if self.focus == Focus::DiffPanel => {
                self.diff_hscroll = self.diff_hscroll.saturating_sub(4);
            }
            // Toggle unified / side-by-side diff view
            KeyCode::Char('v') if self.focus == Focus::DiffPanel => {
                self.diff_view_mode = match self.diff_view_mode {
                    DiffViewMode::Unified => DiffViewMode::SideBySide,
                    DiffViewMode::SideBySide => DiffViewMode::Unified,
                };
                self.diff_scroll = 0;
            }
            // Jump to next hunk
            KeyCode::Char('n') if self.focus == Focus::DiffPanel => {
                if let Some(&pos) = self
                    .hunk_positions
                    .iter()
                    .find(|&&p| p > self.diff_scroll)
                {
                    self.diff_scroll = pos;
                }
            }
            // Jump to previous hunk
            KeyCode::Char('N') if self.focus == Focus::DiffPanel => {
                if let Some(&pos) = self
                    .hunk_positions
                    .iter()
                    .rev()
                    .find(|&&p| p < self.diff_scroll)
                {
                    self.diff_scroll = pos;
                }
            }
            // Toggle collapsed mode (only show changed hunks)
            KeyCode::Char('z') if self.focus == Focus::DiffPanel => {
                self.diff_collapsed = !self.diff_collapsed;
                self.diff_scroll = 0;
            }
            // Git operations
            KeyCode::Char('a') => self.stage_selected(),
            KeyCode::Char('u') => self.unstage_selected(),
            KeyCode::Char('c') => {
                self.modal = Some(Modal::CommitInput(
                    self.ai_suggestion.clone().unwrap_or_default(),
                ));
            }
            // AI operations
            KeyCode::Char('i') => self.request_ai_suggestion(),
            KeyCode::Char('m') => {
                // Derive current index from stored model string via ModelKind.
                use crate::ai::types::ModelKind;
                let current = match ModelKind::from_str(&self.settings.ai.default_model) {
                    ModelKind::Claude    => 0,
                    ModelKind::OpenAI    => 1,
                    ModelKind::MiniMax   => 2,
                    ModelKind::MiniMaxCN => 3,
                };
                self.modal = Some(Modal::ModelSelect(current));
            }
            // AI panel: accept suggestion
            KeyCode::Enter if self.focus == Focus::AiPanel => {
                if let Some(ref suggestion) = self.ai_suggestion {
                    self.modal = Some(Modal::CommitInput(suggestion.clone()));
                }
            }
            // Branch & Stash
            KeyCode::Char('b') => self.show_branch_list(),
            KeyCode::Char('s') => {
                self.modal = Some(Modal::StashMenu);
            }
            // Help
            KeyCode::Char('?') => {
                self.modal = Some(Modal::Help);
            }
            _ => {}
        }
    }

    fn handle_modal_key(&mut self, code: KeyCode, modifiers: KeyModifiers) {
        // Mutate in-place where possible to avoid cloning Vec<String> on every keypress.
        match &mut self.modal {
            Some(Modal::CommitInput(msg)) => match code {
                KeyCode::Esc => self.modal = None,
                KeyCode::Enter => {
                    let message = msg.clone();
                    if !message.trim().is_empty() {
                        match git::commit::commit(&self.repo_path, &message) {
                            Ok(_) => {
                                self.modal = None;
                                self.ai_suggestion = None;
                                self.settings.ui.show_ai_panel = false;
                                self.refresh_status();
                                self.diff_lines.clear();
                            }
                            Err(e) => {
                                self.modal = Some(Modal::Error(format!("Commit failed: {}", e)));
                            }
                        }
                    }
                }
                KeyCode::Backspace => {
                    msg.pop();
                }
                KeyCode::Char(c) => {
                    if c == 'c' && modifiers.contains(KeyModifiers::CONTROL) {
                        self.modal = None;
                    } else {
                        msg.push(c);
                    }
                }
                _ => {}
            },
            Some(Modal::Error(_)) => {
                if matches!(code, KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q')) {
                    self.modal = None;
                }
            }
            Some(Modal::Confirm { action, .. }) => match code {
                KeyCode::Char('y') | KeyCode::Enter => {
                    let action = action.clone();
                    self.modal = None;
                    self.execute_confirm(action);
                }
                KeyCode::Char('n') | KeyCode::Esc => {
                    self.modal = None;
                }
                _ => {}
            },
            Some(Modal::ModelSelect(selected)) => match code {
                KeyCode::Up | KeyCode::Char('k') => {
                    if *selected > 0 {
                        *selected -= 1;
                    }
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    if *selected < MODEL_NAMES.len() - 1 {
                        *selected += 1;
                    }
                }
                KeyCode::Enter => {
                    let chosen = match self.modal {
                        Some(Modal::ModelSelect(0)) => "claude",
                        Some(Modal::ModelSelect(1)) => "openai",
                        Some(Modal::ModelSelect(2)) => "minimax",
                        _                           => "minimax-cn",
                    };
                    self.settings.ai.default_model = chosen.to_string();
                    // If no key is configured for this provider, guide the user.
                    let has_key = match chosen {
                        "claude"     => self.settings.ai.claude_api_key().is_ok(),
                        "openai"     => self.settings.ai.openai_api_key().is_ok(),
                        "minimax"    => self.settings.ai.minimax_api_key().is_ok(),
                        "minimax-cn" => self.settings.ai.minimax_cn_api_key().is_ok(),
                        _            => true,
                    };
                    if has_key {
                        self.modal = None;
                    } else {
                        self.modal = Some(Modal::ApiKeySetup {
                            model: chosen.to_string(),
                            selected: 0,
                        });
                    }
                }
                KeyCode::Esc => self.modal = None,
                _ => {}
            },
            Some(Modal::ApiKeySetup { selected, model }) => match code {
                KeyCode::Up | KeyCode::Char('k') => {
                    if *selected > 0 {
                        *selected -= 1;
                    }
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    if *selected < 1 {
                        *selected += 1;
                    }
                }
                KeyCode::Enter => {
                    let model_name = model.clone();
                    let choice = *selected;
                    if choice == 0 {
                        // Open browser to the provider's API key page.
                        if let Some(url) = crate::config::AiSettings::oauth_url(&model_name) {
                            if let Err(e) = open::that(url) {
                                self.modal = Some(Modal::Error(format!("Cannot open browser: {}", e)));
                                return;
                            }
                        }
                        // After opening browser, drop into key-input so user can paste the key.
                        self.modal = Some(Modal::ApiKeyInput {
                            model: model_name,
                            input: String::new(),
                        });
                    } else {
                        // Go straight to manual key input.
                        self.modal = Some(Modal::ApiKeyInput {
                            model: model_name,
                            input: String::new(),
                        });
                    }
                }
                KeyCode::Esc => self.modal = None,
                _ => {}
            },
            Some(Modal::ApiKeyInput { model, input }) => match code {
                KeyCode::Esc => self.modal = None,
                KeyCode::Enter => {
                    let key = input.trim().to_string();
                    if key.is_empty() {
                        return;
                    }
                    let model_name = model.clone();
                    self.settings.ai.set_api_key(&model_name, key);
                    match self.settings.save() {
                        Ok(_) => {
                            self.modal = None;
                        }
                        Err(e) => {
                            self.modal = Some(Modal::Error(format!("Failed to save config: {}", e)));
                        }
                    }
                }
                KeyCode::Backspace => {
                    input.pop();
                }
                KeyCode::Char(c) => {
                    if c == 'c' && modifiers.contains(KeyModifiers::CONTROL) {
                        self.modal = None;
                    } else {
                        input.push(c);
                    }
                }
                _ => {}
            },
            Some(Modal::BranchList { selected, branches }) => match code {
                KeyCode::Up | KeyCode::Char('k') => {
                    if *selected > 0 {
                        *selected -= 1;
                    }
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    if *selected + 1 < branches.len() {
                        *selected += 1;
                    }
                }
                KeyCode::Esc => self.modal = None,
                _ => {}
            },
            Some(Modal::StashMenu) => match code {
                KeyCode::Char('p') => {
                    self.modal = Some(Modal::Confirm {
                        message: "Push current changes to stash?".to_string(),
                        action: ConfirmAction::StashPush,
                    });
                }
                KeyCode::Char('o') => {
                    self.modal = Some(Modal::Confirm {
                        message: "Pop latest stash?".to_string(),
                        action: ConfirmAction::StashPop,
                    });
                }
                KeyCode::Esc => self.modal = None,
                _ => {}
            },
            Some(Modal::Help) => {
                if matches!(code, KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('?')) {
                    self.modal = None;
                }
            }
            None => {}
        }
    }

    fn execute_confirm(&mut self, action: ConfirmAction) {
        match action {
            ConfirmAction::StashPush => match git::stash::stash_push(&self.repo_path) {
                Ok(_) => self.refresh_status(),
                Err(e) => self.modal = Some(Modal::Error(format!("Stash push failed: {}", e))),
            },
            ConfirmAction::StashPop => match git::stash::stash_pop(&self.repo_path) {
                Ok(_) => self.refresh_status(),
                Err(e) => self.modal = Some(Modal::Error(format!("Stash pop failed: {}", e))),
            },
        }
    }

    fn refresh_status(&mut self) {
        match git::status::get_status(&self.repo_path) {
            Ok(files) => {
                self.files = files;
                self.file_tree.rebuild(&self.files);
                let len = self.file_tree.visible.len();
                if len == 0 {
                    self.selected_file = 0;
                } else if self.selected_file >= len {
                    self.selected_file = len - 1;
                }
            }
            Err(e) => {
                self.modal = Some(Modal::Error(format!("Git status failed: {}", e)));
            }
        }
    }

    fn refresh_branch(&mut self) {
        self.branch_name = git::branch::current_branch(&self.repo_path).unwrap_or_default();
    }

    fn load_selected_diff(&mut self) {
        // Only load diff for File rows; Dir rows do nothing.
        let entry_index = match self.file_tree.visible.get(self.selected_file) {
            Some(row) => match &row.kind {
                RowKind::File { entry_index } => *entry_index,
                RowKind::Dir { .. } => return,
            },
            None => return,
        };

        if let Some(file) = self.files.get(entry_index) {
            let path = file.path.clone();
            let is_untracked = file.is_untracked();
            let is_staged = file.staged;

            let result = if is_untracked {
                git::diff::untracked_file_diff(&self.repo_path, &path)
            } else if is_staged {
                git::diff::file_diff_staged(&self.repo_path, &path)
            } else {
                git::diff::file_diff(&self.repo_path, &path)
            };

            match result {
                Ok(lines) => {
                    self.diff_lines = lines;
                    self.diff_scroll = 0;
                    self.diff_hscroll = 0;
                    self.hunk_positions = self
                        .diff_lines
                        .iter()
                        .enumerate()
                        .filter(|(_, l)| l.kind == DiffLineKind::HunkHeader)
                        .map(|(i, _)| i)
                        .collect();
                }
                Err(e) => {
                    self.diff_lines = vec![DiffLine::context(format!("Error: {}", e))];
                }
            }
        }
    }

    // ── Debounce & virtual-scroll helpers ─────────────────────────────────────

    /// Mark that a diff reload is needed; start (or reset) the debounce timer.
    fn mark_diff_pending(&mut self) {
        self.pending_diff_load = true;
        self.last_nav_time = Some(Instant::now());
    }

    /// Called every Tick. Loads the diff if navigation has settled for ≥50 ms.
    fn maybe_load_pending_diff(&mut self) {
        if !self.pending_diff_load {
            return;
        }
        const DEBOUNCE_MS: u128 = 50;
        if let Some(t) = self.last_nav_time {
            if t.elapsed().as_millis() >= DEBOUNCE_MS {
                self.pending_diff_load = false;
                self.last_nav_time = None;
                self.load_selected_diff();
            }
        }
    }

    /// Keep `file_list_scroll` so that `selected_file` stays in the viewport.
    /// Must be called after every `selected_file` change.
    fn clamp_file_list_scroll(&mut self) {
        let viewport = self.file_list_area.height.saturating_sub(2) as usize;
        if viewport == 0 {
            return;
        }
        if self.selected_file < self.file_list_scroll {
            self.file_list_scroll = self.selected_file;
        } else if self.selected_file >= self.file_list_scroll + viewport {
            self.file_list_scroll = self.selected_file + 1 - viewport;
        }
    }

    /// Shared helper: run a git file operation on the selected file, then refresh.
    fn apply_file_op(
        &mut self,
        op: impl FnOnce(&std::path::Path, &str) -> anyhow::Result<()>,
        err_prefix: &str,
    ) {
        // Resolve the file index from the visible tree row.
        let entry_index = match self.file_tree.visible.get(self.selected_file) {
            Some(row) => match &row.kind {
                RowKind::File { entry_index } => *entry_index,
                RowKind::Dir { .. } => return, // handled separately
            },
            None => return,
        };

        if let Some(file) = self.files.get(entry_index) {
            let path = file.path.clone();
            match op(&self.repo_path, &path) {
                Ok(_) => {
                    self.refresh_status();
                    self.load_selected_diff();
                }
                Err(e) => {
                    self.modal = Some(Modal::Error(format!("{}: {}", err_prefix, e)));
                }
            }
        }
    }

    fn stage_selected(&mut self) {
        // Check if the selected row is a directory.
        if let Some(row) = self.file_tree.visible.get(self.selected_file) {
            if let RowKind::Dir { path, .. } = row.kind.clone() {
                let indices = self.file_tree.collect_file_indices(&path);
                let paths: Vec<String> = indices
                    .iter()
                    .filter_map(|&i| self.files.get(i).map(|f| f.path.clone()))
                    .collect();
                let mut err: Option<String> = None;
                for p in &paths {
                    if let Err(e) = git::commit::stage_file(&self.repo_path, p) {
                        err = Some(format!("Stage failed: {}", e));
                        break;
                    }
                }
                if let Some(msg) = err {
                    self.modal = Some(Modal::Error(msg));
                } else {
                    self.refresh_status();
                }
                return;
            }
        }
        self.apply_file_op(
            |repo, path| git::commit::stage_file(repo, path).map_err(Into::into),
            "Stage failed",
        );
    }

    fn unstage_selected(&mut self) {
        // Check if the selected row is a directory.
        if let Some(row) = self.file_tree.visible.get(self.selected_file) {
            if let RowKind::Dir { path, .. } = row.kind.clone() {
                let indices = self.file_tree.collect_file_indices(&path);
                let paths: Vec<String> = indices
                    .iter()
                    .filter_map(|&i| self.files.get(i).map(|f| f.path.clone()))
                    .collect();
                let mut err: Option<String> = None;
                for p in &paths {
                    if let Err(e) = git::commit::unstage_file(&self.repo_path, p) {
                        err = Some(format!("Unstage failed: {}", e));
                        break;
                    }
                }
                if let Some(msg) = err {
                    self.modal = Some(Modal::Error(msg));
                } else {
                    self.refresh_status();
                }
                return;
            }
        }
        self.apply_file_op(
            |repo, path| git::commit::unstage_file(repo, path).map_err(Into::into),
            "Unstage failed",
        );
    }

    fn request_ai_suggestion(&mut self) {
        if self.ai_loading {
            return;
        }

        // Use the raw diff text directly — no need to parse into DiffLines just to re-join.
        let diff_text = match git::diff::staged_diff_raw(&self.repo_path) {
            Ok(text) => text,
            Err(e) => {
                self.modal = Some(Modal::Error(format!("Cannot get staged diff: {}", e)));
                return;
            }
        };

        if diff_text.trim().is_empty() {
            self.modal = Some(Modal::Error("No staged changes for AI to analyze.".into()));
            return;
        }

        self.ai_loading = true;
        self.ai_suggestion = None;
        self.settings.ui.show_ai_panel = true;

        let tx = self.event_tx.clone().unwrap();
        // Only clone the AI settings — UiSettings is not needed in the task.
        let ai_settings = self.settings.ai.clone();

        let handle = tokio::spawn(async move {
            match ai::generate_commit_message_with(&ai_settings, &diff_text).await {
                Ok(msg) if msg.is_empty() => {
                    let _ = tx.send(AppEvent::AiError("AI returned an empty response.".into()));
                }
                Ok(msg) => {
                    let _ = tx.send(AppEvent::AiResponse(msg));
                }
                Err(e) => {
                    let _ = tx.send(AppEvent::AiError(e.to_string()));
                }
            }
        });
        // Spawn a watcher task so that if the AI task panics, ai_loading is
        // reset via an AiError event instead of staying stuck forever.
        let tx_panic = self.event_tx.clone().unwrap();
        tokio::spawn(async move {
            if let Err(e) = handle.await {
                let panic_box = e.into_panic();
                let msg = panic_box
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| panic_box.downcast_ref::<&str>().map(|s| s.to_string()))
                    .unwrap_or_else(|| "AI task panicked unexpectedly.".into());
                let _ = tx_panic.send(AppEvent::AiError(msg));
            }
        });
    }

    fn show_branch_list(&mut self) {
        match git::branch::list_branches(&self.repo_path) {
            Ok(branches) => {
                self.modal = Some(Modal::BranchList {
                    branches,
                    selected: 0,
                });
            }
            Err(e) => {
                self.modal = Some(Modal::Error(format!("Branch list failed: {}", e)));
            }
        }
    }
}

/// Returns true if `(col, row)` falls inside `area`.
fn contains(area: Rect, col: u16, row: u16) -> bool {
    col >= area.x && col < area.x + area.width && row >= area.y && row < area.y + area.height
}
