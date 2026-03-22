mod file_list;
mod diff_panel;
mod ai_panel;
pub mod file_tree;

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap};
use ratatui::Frame;

use crate::app::{App, Focus, Modal};

/// Static help text — computed once, never reallocated on render ticks.
static HELP_TEXT: &str = "\
GitClaw — AI-Powered Git TUI

Navigation:
  ↑/↓ or j/k    Navigate file list / scroll diff
  h/l            Diff: horizontal scroll (when diff focused)
  n / N          Diff: jump to next / previous hunk
  v              Diff: toggle unified / side-by-side view
  z              Diff: toggle collapsed mode (hide unchanged lines)
  →/l            File list: expand directory
  ←/h            File list: collapse directory (or jump to parent)
  Enter          Dir: toggle expand/collapse  File: load diff
  Tab            Cycle focus: Files → Diff → AI

Git Operations:
  a              Stage file / Stage all files in directory
  u              Unstage file / Unstage all files in directory
  c              Open commit dialog
  b              Show branch list
  s              Stash operations menu

AI:
  i              Generate AI commit message
  m              Select AI model

General:
  ?              Toggle help
  q / Ctrl+C     Quit";

/// Model names — single source of truth for `ModelSelect` rendering and navigation.
pub const MODEL_NAMES: &[&str] = &[
    "Claude (Anthropic)",
    "OpenAI (GPT)",
    "MiniMax (Global)",
    "MiniMax CN (国内)",
];

/// Main render function — draws all panels and modals.
pub fn render(frame: &mut Frame, app: &mut crate::app::App) {
    let size = frame.area();

    // Build vertical layout: top panels + optional AI panel + help bar.
    let mut constraints = vec![Constraint::Min(10)];
    if app.settings.ui.show_ai_panel {
        constraints.push(Constraint::Length(8));
    }
    constraints.push(Constraint::Length(1));

    let main_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(size);

    // Top: file list (fixed 40 cols) | diff panel (remaining) — diff panel
    // is only shown when a file is actively selected (diff_lines non-empty).
    let show_diff = !app.diff_lines.is_empty();
    let top_constraints: Vec<Constraint> = if show_diff {
        vec![Constraint::Length(40), Constraint::Min(0)]
    } else {
        vec![Constraint::Min(0)]
    };

    let top_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(top_constraints)
        .split(main_chunks[0]);

    // Render panels
    file_list::render(frame, app, top_chunks[0]);
    if show_diff {
        diff_panel::render(frame, app, top_chunks[1]);
    }

    // Store panel areas for mouse hit-detection.
    app.file_list_area = top_chunks[0];
    app.diff_panel_area = if show_diff { top_chunks[1] } else { Rect::default() };

    let help_bar_idx = if app.settings.ui.show_ai_panel {
        app.ai_panel_area = main_chunks[1];
        ai_panel::render(frame, app, main_chunks[1]);
        2
    } else {
        1
    };
    render_help_bar(frame, app, main_chunks[help_bar_idx]);

    // Render modal on top if present
    if let Some(ref modal) = app.modal {
        render_modal(frame, modal, size);
    }
}

fn render_help_bar(frame: &mut Frame, app: &App, area: Rect) {
    let hints = match app.focus {
        Focus::FileList => "↑↓:navigate  →/l:expand  ←/h:collapse  Enter:toggle/diff  a:stage  u:unstage  Tab:switch  c:commit  i:AI  m:model  b:branches  s:stash  ?:help  q:quit",
        Focus::DiffPanel => "j/k:scroll  h/l:hscroll  n/N:next/prev hunk  z:collapse  v:split  Tab:switch  a:stage  u:unstage  c:commit  i:AI  q:quit",
        Focus::AiPanel => "Enter:accept suggestion  Tab:switch  c:commit  i:AI  q:quit",
    };

    let bar = Paragraph::new(Line::from(vec![
        Span::styled(" ", Style::default().bg(Color::DarkGray)),
        Span::styled(hints, Style::default().fg(Color::White).bg(Color::DarkGray)),
    ]))
    .style(Style::default().bg(Color::DarkGray));

    frame.render_widget(bar, area);
}

/// Build a selectable list with a "▸ " cursor and highlight colour.
fn build_list_items<'a>(items: &'a [&str], selected: usize, highlight: Color) -> Vec<ListItem<'a>> {
    items
        .iter()
        .enumerate()
        .map(|(i, &label)| {
            let (style, prefix) = if i == selected {
                (
                    Style::default()
                        .fg(highlight)
                        .add_modifier(Modifier::BOLD),
                    "▸ ",
                )
            } else {
                (Style::default(), "  ")
            };
            ListItem::new(format!("{}{}", prefix, label)).style(style)
        })
        .collect()
}

fn render_modal(frame: &mut Frame, modal: &Modal, area: Rect) {
    let modal_area = centered_rect(60, 40, area);

    match modal {
        Modal::CommitInput(msg) => {
            frame.render_widget(Clear, modal_area);
            let block = Block::default()
                .title(" Commit Message (Enter=commit, Esc=cancel) ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Yellow));
            let text = Paragraph::new(format!("{}_", msg))
                .block(block)
                .wrap(Wrap { trim: false });
            frame.render_widget(text, modal_area);
        }
        Modal::Error(msg) => {
            frame.render_widget(Clear, modal_area);
            let block = Block::default()
                .title(" Error (Esc to close) ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Red));
            let text = Paragraph::new(msg.as_str())
                .block(block)
                .wrap(Wrap { trim: false })
                .style(Style::default().fg(Color::Red));
            frame.render_widget(text, modal_area);
        }
        Modal::Confirm { message, .. } => {
            frame.render_widget(Clear, modal_area);
            let block = Block::default()
                .title(" Confirm (y/n) ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Yellow));
            let text = Paragraph::new(message.as_str())
                .block(block)
                .wrap(Wrap { trim: false });
            frame.render_widget(text, modal_area);
        }
        Modal::ModelSelect(selected) => {
            let small = centered_rect(40, 20, area);
            frame.render_widget(Clear, small);
            let items = build_list_items(MODEL_NAMES, *selected, Color::Yellow);
            let list = List::new(items).block(
                Block::default()
                    .title(" Select AI Model (Enter=select, Esc=cancel) ")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Cyan)),
            );
            frame.render_widget(list, small);
        }
        Modal::ApiKeySetup { model, selected } => {
            let small = centered_rect(52, 30, area);
            frame.render_widget(Clear, small);
            let provider = model_display_name(model);
            let title = format!(" {} — No API Key Found ", provider);
            let auth_options = &[
                "  Open browser → get API key from dashboard",
                "  Enter API key manually",
            ];
            let items = build_list_items(auth_options, *selected, Color::Cyan);
            let list = List::new(items).block(
                Block::default()
                    .title(title)
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Yellow)),
            );
            frame.render_widget(list, small);
        }
        Modal::ApiKeyInput { model, input } => {
            let small = centered_rect(60, 25, area);
            frame.render_widget(Clear, small);
            let provider = model_display_name(model);
            let title = format!(" {} API Key (Enter=save, Esc=cancel) ", provider);
            // Mask the key: show last 4 chars, rest as '●'
            let display = mask_key(input);
            let block = Block::default()
                .title(title)
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan));
            let text = Paragraph::new(format!("{}_", display))
                .block(block)
                .style(Style::default().fg(Color::White))
                .wrap(Wrap { trim: false });
            frame.render_widget(text, small);
        }
        Modal::BranchList { branches, selected } => {
            frame.render_widget(Clear, modal_area);
            let labels: Vec<&str> = branches.iter().map(String::as_str).collect();
            let items = build_list_items(&labels, *selected, Color::Green);
            let list = List::new(items).block(
                Block::default()
                    .title(" Branches (Esc to close) ")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Green)),
            );
            frame.render_widget(list, modal_area);
        }
        Modal::StashMenu => {
            let small = centered_rect(40, 20, area);
            frame.render_widget(Clear, small);
            let text = Paragraph::new("p: Push to stash\no: Pop from stash\nEsc: Cancel")
                .block(
                    Block::default()
                        .title(" Stash Operations ")
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::Magenta)),
                );
            frame.render_widget(text, small);
        }
        Modal::Help => {
            let big = centered_rect(70, 70, area);
            frame.render_widget(Clear, big);
            let text = Paragraph::new(HELP_TEXT)
                .block(
                    Block::default()
                        .title(" Help (Esc/q/? to close) ")
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::White)),
                )
                .wrap(Wrap { trim: false });
            frame.render_widget(text, big);
        }
    }
}

/// Map provider slug → human-readable display name.
fn model_display_name(model: &str) -> &str {
    match model {
        "claude"     => "Claude (Anthropic)",
        "openai"     => "OpenAI (GPT)",
        "minimax"    => "MiniMax (Global)",
        "minimax-cn" => "MiniMax CN (国内)",
        other        => other,
    }
}

/// Mask an API key string, showing only the last 4 characters.
/// Empty or very short strings are shown as-is.
fn mask_key(key: &str) -> String {
    let chars: Vec<char> = key.chars().collect();
    if chars.len() <= 4 {
        return key.to_string();
    }
    let visible = chars.len().saturating_sub(4);
    let masked: String = std::iter::repeat('●').take(visible).collect();
    let tail: String = chars[visible..].iter().collect();
    format!("{}{}", masked, tail)
}

/// Helper to create a centered rect of given percentage width/height.
fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}
