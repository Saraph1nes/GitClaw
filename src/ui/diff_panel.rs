use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;

use crate::app::{App, Focus};
use crate::git::DiffLineKind;

pub fn render(frame: &mut Frame, app: &App, area: Rect) {
    let focused = app.focus == Focus::DiffPanel;
    let border_color = if focused { Color::Cyan } else { Color::DarkGray };

    let block = Block::default()
        .title(" Diff ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color));

    if app.diff_lines.is_empty() {
        let placeholder = Paragraph::new("Select a file to view diff")
            .block(block)
            .style(Style::default().fg(Color::DarkGray));
        frame.render_widget(placeholder, area);
        return;
    }

    // Calculate visible area (block borders take 2 lines)
    let inner_height = area.height.saturating_sub(2) as usize;
    let max_scroll = app.diff_lines.len().saturating_sub(inner_height);
    let scroll = app.diff_scroll.min(max_scroll);

    let lines: Vec<Line> = app
        .diff_lines
        .iter()
        .skip(scroll)
        .take(inner_height)
        .map(|line| {
            let style = match line.kind {
                DiffLineKind::Added => Style::default().fg(Color::Green),
                DiffLineKind::Removed => Style::default().fg(Color::Red),
                DiffLineKind::Header => Style::default().fg(Color::Cyan),
                DiffLineKind::Context => Style::default().fg(Color::White),
            };
            Line::from(Span::styled(&line.content, style))
        })
        .collect();

    let diff = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false });

    frame.render_widget(diff, area);
}
