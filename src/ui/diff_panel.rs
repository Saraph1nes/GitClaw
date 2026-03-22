use std::collections::{HashMap, HashSet};

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState};
use ratatui::Frame;
use similar::{ChangeTag, TextDiff};

use crate::app::{App, DiffViewMode, Focus};
use crate::git::{DiffLine, DiffLineKind};

// ─── Colour constants ──────────────────────────────────────────────────────────

const BG_ADDED: Color = Color::Rgb(0, 45, 0);
const BG_REMOVED: Color = Color::Rgb(50, 0, 0);
const BG_ADDED_EMPH: Color = Color::Rgb(0, 100, 0);
const BG_REMOVED_EMPH: Color = Color::Rgb(110, 0, 0);
const BG_HUNK: Color = Color::Rgb(30, 30, 50);
const FG_GUTTER: Color = Color::Rgb(90, 90, 90);

// ─── Gutter helpers ────────────────────────────────────────────────────────────

/// Width of the gutter: "1234 5678 │" = 10 chars
const GUTTER_WIDTH: usize = 10;

fn gutter_text(old: Option<u32>, new: Option<u32>) -> String {
    match (old, new) {
        (Some(o), Some(n)) => format!("{:>4} {:>4} │", o, n),
        (Some(o), None) => format!("{:>4}      │", o),
        (None, Some(n)) => format!("     {:>4} │", n),
        (None, None) => "          │".to_string(),
    }
}

fn gutter_span(old: Option<u32>, new: Option<u32>) -> Span<'static> {
    Span::styled(gutter_text(old, new), Style::default().fg(FG_GUTTER))
}

// ─── Collapsed view ───────────────────────────────────────────────────────────

/// Number of context lines to show around each hunk in collapsed mode
const COLLAPSE_CONTEXT: usize = 3;

/// A row in the collapsed view — either a real line (by index into diff_lines)
/// or a separator showing how many lines were hidden.
#[derive(Debug)]
enum CollapsedRow {
    Real(usize),
    Separator(usize), // usize = number of hidden lines
}

/// Build the collapsed view: show COLLAPSE_CONTEXT context lines around each
/// hunk, skip the rest, insert separator rows for hidden stretches.
fn build_collapsed_view(diff_lines: &[DiffLine]) -> Vec<CollapsedRow> {
    if diff_lines.is_empty() {
        return Vec::new();
    }

    // Mark which lines must be visible (within COLLAPSE_CONTEXT of a change)
    let n = diff_lines.len();
    let mut visible = vec![false; n];

    for (i, dl) in diff_lines.iter().enumerate() {
        let is_change = matches!(
            dl.kind,
            DiffLineKind::Added
                | DiffLineKind::Removed
                | DiffLineKind::HunkHeader
                | DiffLineKind::Binary
                | DiffLineKind::Header
        );
        if is_change {
            let start = i.saturating_sub(COLLAPSE_CONTEXT);
            let end = (i + COLLAPSE_CONTEXT + 1).min(n);
            visible[start..end].fill(true);
        }
    }

    let mut rows: Vec<CollapsedRow> = Vec::new();
    let mut i = 0;
    while i < n {
        if visible[i] {
            rows.push(CollapsedRow::Real(i));
            i += 1;
        } else {
            // Count contiguous hidden lines
            let start = i;
            while i < n && !visible[i] {
                i += 1;
            }
            let hidden = i - start;
            rows.push(CollapsedRow::Separator(hidden));
        }
    }

    rows
}

// ─── Intra-line highlighting ───────────────────────────────────────────────────

/// Compute word-level diff spans for a removed/added pair.
/// Returns (removed_spans, added_spans) with changed words highlighted brighter.
fn intra_line_spans(old_line: &str, new_line: &str) -> (Vec<Span<'static>>, Vec<Span<'static>>) {
    let base_old = Style::default().fg(Color::Red).bg(BG_REMOVED);
    let emph_old = Style::default()
        .fg(Color::White)
        .bg(BG_REMOVED_EMPH)
        .add_modifier(Modifier::BOLD);
    let base_new = Style::default().fg(Color::Green).bg(BG_ADDED);
    let emph_new = Style::default()
        .fg(Color::White)
        .bg(BG_ADDED_EMPH)
        .add_modifier(Modifier::BOLD);

    // Use word-level diff to avoid per-character span fragmentation
    let diff = TextDiff::from_words(old_line, new_line);
    let mut old_spans: Vec<Span> = Vec::new();
    let mut new_spans: Vec<Span> = Vec::new();

    for change in diff.iter_all_changes() {
        let s = change.to_string();
        match change.tag() {
            ChangeTag::Equal => {
                old_spans.push(Span::styled(s.clone(), base_old));
                new_spans.push(Span::styled(s, base_new));
            }
            ChangeTag::Delete => {
                old_spans.push(Span::styled(s, emph_old));
            }
            ChangeTag::Insert => {
                new_spans.push(Span::styled(s, emph_new));
            }
        }
    }

    (old_spans, new_spans)
}

// ─── Content truncation (horizontal scroll) ───────────────────────────────────

/// Truncate `text` to `max_cols` characters starting from `hscroll` offset.
fn hscroll_str(text: &str, hscroll: usize, max_cols: usize) -> String {
    text.chars().skip(hscroll).take(max_cols).collect()
}

// ─── Public entry point ────────────────────────────────────────────────────────

pub fn render(frame: &mut Frame, app: &mut App, area: Rect) {
    match app.diff_view_mode {
        DiffViewMode::Unified => render_unified(frame, app, area),
        DiffViewMode::SideBySide => render_side_by_side(frame, app, area),
    }
}

// ─── Unified view ─────────────────────────────────────────────────────────────

fn render_unified(frame: &mut Frame, app: &mut App, area: Rect) {
    let focused = app.focus == Focus::DiffPanel;
    let border_color = if focused { Color::Cyan } else { Color::DarkGray };

    let inner_height = area.height.saturating_sub(2) as usize;
    let inner_width = area.width.saturating_sub(2) as usize;

    let plain_total = app.diff_lines.len();
    // Build collapsed view once and reuse for both title and rendering to avoid
    // scanning + allocating the full diff twice per frame.
    let collapsed_view = if app.diff_collapsed {
        Some(build_collapsed_view(&app.diff_lines))
    } else {
        None
    };
    let display_total = collapsed_view.as_ref().map_or(plain_total, |v| v.len());

    let title = if display_total == 0 {
        " Diff ".to_string()
    } else {
        let max_scroll = display_total.saturating_sub(inner_height);
        let pos = app.diff_scroll.min(max_scroll) + 1;
        let mode = if app.diff_collapsed { " [collapsed]" } else { "" };
        if app.diff_hscroll > 0 {
            format!(" Diff{} [{}/{}] ◄col {} ", mode, pos, display_total, app.diff_hscroll)
        } else {
            format!(" Diff{} [{}/{}] ", mode, pos, display_total)
        }
    };

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color));

    if app.diff_lines.is_empty() {
        let placeholder = Paragraph::new("Select a file to view diff")
            .block(block)
            .style(Style::default().fg(Color::DarkGray));
        frame.render_widget(placeholder, area);
        return;
    }

    let max_scroll = display_total.saturating_sub(inner_height);
    // Write back so event handlers use the exact same bound.
    app.diff_scroll_max = max_scroll;
    let scroll = app.diff_scroll.min(max_scroll);
    let hscroll = app.diff_hscroll;
    let content_width = inner_width.saturating_sub(GUTTER_WIDTH);

    // Get visible slice of rows
    let lines_out = if let Some(ref visible_rows) = collapsed_view {
        let slice: Vec<&CollapsedRow> = visible_rows.iter().skip(scroll).take(inner_height).collect();
        render_rows_unified(&slice, &app.diff_lines, hscroll, content_width)
    } else {
        let visible: Vec<&DiffLine> = app.diff_lines.iter().skip(scroll).take(inner_height).collect();
        let intra_map = build_intra_map(&visible);
        let consumed_set: HashSet<usize> = intra_map.values().copied().collect();
        render_diff_lines_unified(&visible, &intra_map, &consumed_set, hscroll, content_width)
    };

    let diff_para = Paragraph::new(lines_out).block(block);
    frame.render_widget(diff_para, area);

    // Vertical scrollbar
    if display_total > inner_height {
        let mut scrollbar_state = ScrollbarState::new(display_total).position(scroll);
        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(None)
            .end_symbol(None);
        frame.render_stateful_widget(scrollbar, area, &mut scrollbar_state);
    }
}

fn render_rows_unified(
    slice: &[&CollapsedRow],
    diff_lines: &[DiffLine],
    hscroll: usize,
    content_width: usize,
) -> Vec<Line<'static>> {
    let mut out: Vec<Line<'static>> = Vec::new();

    // Collect the real DiffLine references in order
    let real_lines: Vec<&DiffLine> = slice
        .iter()
        .filter_map(|r| match r {
            CollapsedRow::Real(i) => diff_lines.get(*i),
            _ => None,
        })
        .collect();

    let intra_map = build_intra_map(&real_lines);
    let consumed_set: HashSet<usize> = intra_map.values().copied().collect();

    // Walk the slice, maintaining a pointer into real_lines for non-separator rows
    let mut real_idx = 0usize;

    for row in slice {
        match row {
            CollapsedRow::Separator(hidden) => {
                let text = format!("── {} lines hidden ──", hidden);
                out.push(Line::from(vec![Span::styled(
                    text,
                    Style::default()
                        .fg(Color::DarkGray)
                        .add_modifier(Modifier::ITALIC),
                )]));
            }
            CollapsedRow::Real(_) => {
                if consumed_set.contains(&real_idx) {
                    real_idx += 1;
                    continue;
                }
                let rendered = render_one_line(
                    real_lines[real_idx],
                    &intra_map,
                    real_idx,
                    &real_lines,
                    hscroll,
                    content_width,
                );
                out.extend(rendered);
                real_idx += 1;
            }
        }
    }

    out
}

// ─── Core unified line renderer ───────────────────────────────────────────────

/// Render a slice of DiffLines for unified view.
fn render_diff_lines_unified(
    visible: &[&DiffLine],
    intra_map: &HashMap<usize, usize>,
    consumed_set: &HashSet<usize>,
    hscroll: usize,
    content_width: usize,
) -> Vec<Line<'static>> {
    let mut out: Vec<Line<'static>> = Vec::new();
    let mut i = 0usize;
    while i < visible.len() {
        if consumed_set.contains(&i) {
            i += 1;
            continue;
        }
        let rendered = render_one_line(visible[i], intra_map, i, visible, hscroll, content_width);
        out.extend(rendered);
        i += 1;
    }
    out
}

/// Render a single DiffLine (or a Removed+Added pair) into one or two `Line`s.
fn render_one_line(
    dl: &DiffLine,
    intra_map: &HashMap<usize, usize>,
    idx: usize,
    visible: &[&DiffLine],
    hscroll: usize,
    content_width: usize,
) -> Vec<Line<'static>> {
    match dl.kind {
        DiffLineKind::HunkHeader => {
            let hint = dl.content.split("@@").nth(2).map(|s| s.trim()).unwrap_or("");
            let display = if hint.is_empty() {
                dl.content.clone()
            } else {
                let base = dl
                    .content
                    .rfind("@@")
                    .map(|p| &dl.content[..p + 2])
                    .unwrap_or(&dl.content);
                format!("{} {}", base, hint)
            };
            let truncated = hscroll_str(&display, hscroll, content_width);
            let gutter = Span::styled(gutter_text(None, None), Style::default().fg(FG_GUTTER));
            let content_span = Span::styled(
                truncated,
                Style::default()
                    .fg(Color::Cyan)
                    .bg(BG_HUNK)
                    .add_modifier(Modifier::BOLD),
            );
            vec![Line::from(vec![gutter, content_span])]
        }
        DiffLineKind::Binary => {
            let g = gutter_span(None, None);
            let s = Span::styled(
                dl.content.clone(),
                Style::default().fg(Color::Yellow).add_modifier(Modifier::ITALIC),
            );
            vec![Line::from(vec![g, s])]
        }
        DiffLineKind::Header => {
            let g = gutter_span(None, None);
            let truncated = hscroll_str(&dl.content, hscroll, content_width);
            let s = Span::styled(truncated, Style::default().fg(Color::Cyan));
            vec![Line::from(vec![g, s])]
        }
        DiffLineKind::Context => {
            let g = gutter_span(dl.old_lineno, dl.new_lineno);
            // Strip the leading space that git diff adds to context lines
            let content = strip_leading_space(&dl.content);
            let truncated = hscroll_str(content, hscroll, content_width);
            let s = Span::styled(truncated, Style::default().fg(Color::White));
            vec![Line::from(vec![g, s])]
        }
        DiffLineKind::Removed => {
            if let Some(&added_i) = intra_map.get(&idx) {
                if let Some(added_dl) = visible.get(added_i) {
                    let old_content = strip_prefix(&dl.content, '-');
                    let new_content = strip_prefix(&added_dl.content, '+');
                    let (old_spans, new_spans) = intra_line_spans(old_content, new_content);

                    // Removed row
                    let g_old = gutter_span(dl.old_lineno, None);
                    let prefix_old = Span::styled("-", Style::default().fg(Color::Red).bg(BG_REMOVED));
                    let mut row_old = vec![g_old, prefix_old];
                    row_old.extend(apply_hscroll_spans(old_spans, hscroll, content_width.saturating_sub(1)));

                    // Added row
                    let g_new = gutter_span(None, added_dl.new_lineno);
                    let prefix_new = Span::styled("+", Style::default().fg(Color::Green).bg(BG_ADDED));
                    let mut row_new = vec![g_new, prefix_new];
                    row_new.extend(apply_hscroll_spans(new_spans, hscroll, content_width.saturating_sub(1)));

                    return vec![Line::from(row_old), Line::from(row_new)];
                }
            }
            // Plain removed
            let g = gutter_span(dl.old_lineno, None);
            let truncated = hscroll_str(&dl.content, hscroll, content_width);
            let s = Span::styled(truncated, Style::default().fg(Color::Red).bg(BG_REMOVED));
            vec![Line::from(vec![g, s])]
        }
        DiffLineKind::Added => {
            let g = gutter_span(None, dl.new_lineno);
            let truncated = hscroll_str(&dl.content, hscroll, content_width);
            let s = Span::styled(truncated, Style::default().fg(Color::Green).bg(BG_ADDED));
            vec![Line::from(vec![g, s])]
        }
    }
}

// ─── Side-by-side view ────────────────────────────────────────────────────────

fn render_side_by_side(frame: &mut Frame, app: &mut App, area: Rect) {
    let focused = app.focus == Focus::DiffPanel;
    let border_color = if focused { Color::Cyan } else { Color::DarkGray };
    let inner_height = area.height.saturating_sub(2) as usize;

    // Build full logical row list first to get the true total
    let (all_logical, logical_total) = build_side_by_side_all(&app.diff_lines);

    let max_scroll = logical_total.saturating_sub(inner_height);
    // Write back so event handlers use the exact same bound.
    app.diff_scroll_max = max_scroll;
    let scroll = app.diff_scroll.min(max_scroll);

    let mode = if app.diff_collapsed { " [collapsed+split]" } else { " [split]" };
    let title = if logical_total == 0 {
        format!(" Diff{} ", mode)
    } else {
        format!(" Diff{} [{}/{}] ", mode, scroll + 1, logical_total)
    };

    let outer_block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color));

    if app.diff_lines.is_empty() {
        let placeholder = Paragraph::new("Select a file to view diff")
            .block(outer_block)
            .style(Style::default().fg(Color::DarkGray));
        frame.render_widget(placeholder, area);
        return;
    }

    frame.render_widget(outer_block, area);

    let inner = Rect {
        x: area.x + 1,
        y: area.y + 1,
        width: area.width.saturating_sub(2),
        height: area.height.saturating_sub(2),
    };

    let col_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(inner);

    let hscroll = app.diff_hscroll;
    let left_width = col_chunks[0].width.saturating_sub(1) as usize;
    let right_width = col_chunks[1].width as usize;
    let content_left = left_width.saturating_sub(GUTTER_WIDTH);
    let content_right = right_width.saturating_sub(GUTTER_WIDTH);

    let pairs: Vec<&SbsRow> = all_logical.iter().skip(scroll).take(inner_height).collect();

    let mut left_lines: Vec<Line> = Vec::with_capacity(inner_height);
    let mut right_lines: Vec<Line> = Vec::with_capacity(inner_height);

    for pair in &pairs {
        match pair {
            SbsRow::Both { left, right } => {
                let gl = gutter_span(left.old_lineno, None);
                let gr = gutter_span(None, right.new_lineno);
                // Strip the leading space git diff adds to context lines
                let tl = hscroll_str(strip_leading_space(&left.content), hscroll, content_left);
                let tr = hscroll_str(strip_leading_space(&right.content), hscroll, content_right);
                left_lines.push(Line::from(vec![gl, Span::styled(tl, Style::default().fg(Color::White))]));
                right_lines.push(Line::from(vec![gr, Span::styled(tr, Style::default().fg(Color::White))]));
            }
            SbsRow::Left(dl) => {
                let g = gutter_span(dl.old_lineno, None);
                let t = hscroll_str(&dl.content, hscroll, content_left);
                left_lines.push(Line::from(vec![g, Span::styled(t, Style::default().fg(Color::Red).bg(BG_REMOVED))]));
                right_lines.push(Line::from(vec![Span::raw("")]));
            }
            SbsRow::Right(dl) => {
                let g = gutter_span(None, dl.new_lineno);
                let t = hscroll_str(&dl.content, hscroll, content_right);
                right_lines.push(Line::from(vec![g, Span::styled(t, Style::default().fg(Color::Green).bg(BG_ADDED))]));
                left_lines.push(Line::from(vec![Span::raw("")]));
            }
            SbsRow::Paired { removed, added } => {
                let old_content = strip_prefix(&removed.content, '-');
                let new_content = strip_prefix(&added.content, '+');
                let (old_spans, new_spans) = intra_line_spans(old_content, new_content);

                let gl = gutter_span(removed.old_lineno, None);
                let gr = gutter_span(None, added.new_lineno);
                let prefix_old = Span::styled("-", Style::default().fg(Color::Red).bg(BG_REMOVED));
                let prefix_new = Span::styled("+", Style::default().fg(Color::Green).bg(BG_ADDED));

                let mut lrow = vec![gl, prefix_old];
                lrow.extend(apply_hscroll_spans(old_spans, hscroll, content_left.saturating_sub(1)));
                left_lines.push(Line::from(lrow));

                let mut rrow = vec![gr, prefix_new];
                rrow.extend(apply_hscroll_spans(new_spans, hscroll, content_right.saturating_sub(1)));
                right_lines.push(Line::from(rrow));
            }
            SbsRow::FullWidth(dl) => {
                let style = match dl.kind {
                    DiffLineKind::HunkHeader => Style::default().fg(Color::Cyan).bg(BG_HUNK).add_modifier(Modifier::BOLD),
                    DiffLineKind::Binary => Style::default().fg(Color::Yellow).add_modifier(Modifier::ITALIC),
                    _ => Style::default().fg(Color::Cyan),
                };
                let t = hscroll_str(&dl.content, hscroll, content_left);
                let g = gutter_span(None, None);
                left_lines.push(Line::from(vec![g, Span::styled(t, style)]));
                right_lines.push(Line::from(vec![Span::raw("")]));
            }
        }
    }

    let left_block = Block::default()
        .borders(Borders::RIGHT)
        .border_style(Style::default().fg(Color::DarkGray));
    frame.render_widget(Paragraph::new(left_lines).block(left_block), col_chunks[0]);
    frame.render_widget(Paragraph::new(right_lines), col_chunks[1]);

    if logical_total > inner_height {
        let mut scrollbar_state = ScrollbarState::new(logical_total).position(scroll);
        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(None)
            .end_symbol(None);
        frame.render_stateful_widget(scrollbar, area, &mut scrollbar_state);
    }
}

// ─── Side-by-side row types ───────────────────────────────────────────────────

enum SbsRow<'a> {
    Both { left: &'a DiffLine, right: &'a DiffLine },
    Left(&'a DiffLine),
    Right(&'a DiffLine),
    Paired { removed: &'a DiffLine, added: &'a DiffLine },
    FullWidth(&'a DiffLine),
}

/// Build the full logical side-by-side row list from all diff_lines.
/// Returns (rows, total_row_count) so callers can compute scroll range correctly.
fn build_side_by_side_all(diff_lines: &[DiffLine]) -> (Vec<SbsRow<'_>>, usize) {
    let mut logical: Vec<SbsRow> = Vec::new();
    let mut i = 0;

    while i < diff_lines.len() {
        let dl = &diff_lines[i];
        match dl.kind {
            DiffLineKind::Header | DiffLineKind::HunkHeader | DiffLineKind::Binary => {
                logical.push(SbsRow::FullWidth(dl));
                i += 1;
            }
            DiffLineKind::Context => {
                logical.push(SbsRow::Both { left: dl, right: dl });
                i += 1;
            }
            DiffLineKind::Removed => {
                let mut removed_run: Vec<usize> = vec![i];
                let mut j = i + 1;
                while j < diff_lines.len() && diff_lines[j].kind == DiffLineKind::Removed {
                    removed_run.push(j);
                    j += 1;
                }
                let mut added_run: Vec<usize> = Vec::new();
                while j < diff_lines.len() && diff_lines[j].kind == DiffLineKind::Added {
                    added_run.push(j);
                    j += 1;
                }
                let pair_count = removed_run.len().min(added_run.len());
                for k in 0..pair_count {
                    logical.push(SbsRow::Paired {
                        removed: &diff_lines[removed_run[k]],
                        added: &diff_lines[added_run[k]],
                    });
                }
                for &ri in &removed_run[pair_count..] {
                    logical.push(SbsRow::Left(&diff_lines[ri]));
                }
                for &ai in &added_run[pair_count..] {
                    logical.push(SbsRow::Right(&diff_lines[ai]));
                }
                i = j;
            }
            DiffLineKind::Added => {
                logical.push(SbsRow::Right(dl));
                i += 1;
            }
        }
    }

    let total = logical.len();
    (logical, total)
}

// ─── Intra-line map helpers ────────────────────────────────────────────────────

/// Build a map from visible-slice index of Removed lines to their matching Added index.
fn build_intra_map(visible: &[&DiffLine]) -> HashMap<usize, usize> {
    let mut map = HashMap::new();
    let mut i = 0;
    while i < visible.len() {
        if visible[i].kind == DiffLineKind::Removed {
            let mut removed_run: Vec<usize> = vec![i];
            let mut j = i + 1;
            while j < visible.len() && visible[j].kind == DiffLineKind::Removed {
                removed_run.push(j);
                j += 1;
            }
            let mut added_run: Vec<usize> = Vec::new();
            while j < visible.len() && visible[j].kind == DiffLineKind::Added {
                added_run.push(j);
                j += 1;
            }
            let pair_count = removed_run.len().min(added_run.len());
            for k in 0..pair_count {
                map.insert(removed_run[k], added_run[k]);
            }
            i = j;
        } else {
            i += 1;
        }
    }
    map
}

// ─── Utility helpers ──────────────────────────────────────────────────────────

/// Strip the leading diff prefix character (+/-) from a changed line.
fn strip_prefix(line: &str, prefix: char) -> &str {
    if line.starts_with(prefix) { &line[prefix.len_utf8()..] } else { line }
}

/// Strip the leading space that git diff adds to context lines.
fn strip_leading_space(line: &str) -> &str {
    line.strip_prefix(' ').unwrap_or(line)
}

/// Apply hscroll to pre-built Spans, coalescing adjacent same-style spans.
fn apply_hscroll_spans(spans: Vec<Span<'static>>, hscroll: usize, max_cols: usize) -> Vec<Span<'static>> {
    if hscroll == 0 && max_cols >= usize::MAX / 2 {
        return spans;
    }

    let mut result: Vec<Span<'static>> = Vec::new();
    let mut col = 0usize;
    let end = hscroll + max_cols;

    for span in spans {
        let style = span.style;
        for ch in span.content.chars() {
            if col >= end {
                break;
            }
            if col >= hscroll {
                if let Some(last) = result.last_mut() {
                    if last.style == style {
                        let mut s = last.content.to_string();
                        s.push(ch);
                        last.content = s.into();
                        col += 1;
                        continue;
                    }
                }
                result.push(Span::styled(ch.to_string(), style));
            }
            col += 1;
        }
        if col >= end {
            break;
        }
    }

    result
}
