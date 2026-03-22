use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;

use crate::app::{App, Focus};

pub fn render(frame: &mut Frame, app: &mut App, area: Rect) {
    let focused = app.focus == Focus::AiPanel;
    let border_color = if focused { Color::Cyan } else { Color::DarkGray };

    let block = Block::default()
        .title(" AI Suggestions ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color));

    let content: Vec<Line> = if app.ai_loading {
        vec![Line::from(Span::styled(
            "⟳ Thinking...",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ))]
    } else if let Some(ref suggestion) = app.ai_suggestion {
        let mut lines = vec![Line::from(Span::styled(
            "AI Suggestion (Enter to open commit dialog, c to commit):",
            Style::default().fg(Color::Green),
        ))];
        for line in suggestion.lines() {
            lines.push(Line::from(Span::styled(
                format!("  {}", line),
                Style::default().fg(Color::White),
            )));
        }
        lines
    } else {
        vec![
            Line::from(Span::styled(
                "Press 'i' to generate AI commit message from staged changes",
                Style::default().fg(Color::DarkGray),
            )),
            Line::from(Span::styled(
                format!("Model: {} | Press 'm' to change", app.settings.ai.default_model),
                Style::default().fg(Color::DarkGray),
            )),
        ]
    };

    // Apply scroll offset: inner area = total height minus 2 border lines.
    let inner_height = area.height.saturating_sub(2) as usize;
    let max_scroll = content.len().saturating_sub(inner_height);
    // Write back so event handlers use the exact same bound.
    app.ai_scroll_max = max_scroll;
    let scroll = app.ai_scroll.min(max_scroll);
    let visible: Vec<Line> = content.into_iter().skip(scroll).collect();

    let paragraph = Paragraph::new(visible)
        .block(block)
        .wrap(Wrap { trim: false });

    frame.render_widget(paragraph, area);
}
