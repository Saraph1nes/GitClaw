use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;

use crate::app::{App, Focus};

pub fn render(frame: &mut Frame, app: &App, area: Rect) {
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
            "AI Suggestion (Enter to accept, c to edit):",
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

    let paragraph = Paragraph::new(content)
        .block(block)
        .wrap(Wrap { trim: false });

    frame.render_widget(paragraph, area);
}
