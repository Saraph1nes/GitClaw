use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem};
use ratatui::Frame;

use crate::app::{App, Focus};
use crate::git::FileStatus;
use crate::ui::file_tree::RowKind;

pub fn render(frame: &mut Frame, app: &App, area: Rect) {
    let focused = app.focus == Focus::FileList;
    let border_color = if focused { Color::Cyan } else { Color::DarkGray };

    let title = format!(
        " Files [{}] ({}) ",
        app.branch_name,
        app.files.len()
    );

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color));

    let items: Vec<ListItem> = app
        .file_tree
        .visible
        .iter()
        .enumerate()
        .map(|(i, row)| {
            let selected = i == app.selected_file;
            let indent = "  ".repeat(row.depth);

            match &row.kind {
                // ── Directory row ───────────────────────────────────────────────
                RowKind::Dir {
                    name,
                    expanded,
                    file_count,
                    has_staged,
                    ..
                } => {
                    let arrow = if *expanded { "▾" } else { "▸" };
                    let staged_dot = if *has_staged {
                        Span::styled("● ", Style::default().fg(Color::Green))
                    } else {
                        Span::raw("  ")
                    };
                    let cursor = if selected { "▸ " } else { "  " };

                    let dir_style = if selected {
                        Style::default()
                            .fg(Color::Blue)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::Blue)
                    };

                    ListItem::new(Line::from(vec![
                        Span::raw(format!("{}{}", indent, cursor)),
                        Span::styled(format!("{} ", arrow), dir_style),
                        staged_dot,
                        Span::styled(
                            format!("{}/ ({})", name, file_count),
                            dir_style,
                        ),
                    ]))
                }

                // ── File row ────────────────────────────────────────────────────
                RowKind::File { entry_index } => {
                    let file = match app.files.get(*entry_index) {
                        Some(f) => f,
                        None => {
                            return ListItem::new(Line::from(Span::raw("  <invalid>")));
                        }
                    };

                    let staged_marker = if file.is_staged() {
                        Span::styled("● ", Style::default().fg(Color::Green))
                    } else {
                        Span::raw("  ")
                    };

                    let status_icon = match &file.status {
                        FileStatus::Modified  => Span::styled("M ", Style::default().fg(Color::Yellow)),
                        FileStatus::Added     => Span::styled("A ", Style::default().fg(Color::Green)),
                        FileStatus::Deleted   => Span::styled("D ", Style::default().fg(Color::Red)),
                        FileStatus::Renamed   => Span::styled("R ", Style::default().fg(Color::Cyan)),
                        FileStatus::Copied    => Span::styled("C ", Style::default().fg(Color::Cyan)),
                        FileStatus::Untracked => Span::styled("? ", Style::default().fg(Color::Gray)),
                        FileStatus::Unmerged  => Span::styled("U ", Style::default().fg(Color::Magenta)),
                    };

                    let file_style = if selected {
                        Style::default()
                            .fg(Color::White)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::White)
                    };

                    let cursor = if selected { "▸ " } else { "  " };

                    // Show only the basename for files inside directories, full
                    // path for root-level files (depth == 0 and no parent dir).
                    let display_name = if row.depth == 0 {
                        file.path.as_str()
                    } else {
                        // basename: last path component
                        file.path
                            .rsplit('/')
                            .next()
                            .unwrap_or(file.path.as_str())
                    };

                    ListItem::new(Line::from(vec![
                        Span::raw(format!("{}{}", indent, cursor)),
                        staged_marker,
                        status_icon,
                        Span::styled(display_name, file_style),
                    ]))
                }
            }
        })
        .collect();

    let list = List::new(items).block(block);
    frame.render_widget(list, area);
}
