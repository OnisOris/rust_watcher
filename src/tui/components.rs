//! Small reusable visual primitives; no I/O or application behavior.
use super::theme;
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph, Wrap},
};
use std::path::Path;

/// Cache only a layout bound, not semantic selection. Use Ratatui's own wrapping
/// so narrow terminals can reach the end of long errors without overscrolling.
pub fn scrollable<'a>(
    lines: Vec<Line<'a>>,
    panel: Block<'a>,
    area: ratatui::layout::Rect,
    requested: usize,
    limit: &std::cell::Cell<Option<usize>>,
) -> Paragraph<'a> {
    let inner = panel.inner(area);
    let text = Paragraph::new(lines).wrap(Wrap { trim: false });
    let max = text
        .line_count(inner.width)
        .saturating_sub(inner.height as usize)
        .min(u16::MAX as usize);
    limit.set(Some(max));
    text.scroll((requested.min(max) as u16, 0)).block(panel)
}

pub fn styled_panel(title: &str, active: bool) -> Block<'_> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(Line::styled(
            title,
            Style::default()
                .fg(if active { theme::ACTIVE } else { theme::MUTED })
                .add_modifier(Modifier::BOLD),
        ))
        .border_style(Style::default().fg(if active { theme::ACTIVE } else { theme::BORDER }))
}
pub fn section_header(text: &str) -> Line<'static> {
    Line::styled(
        text.to_owned(),
        Style::default()
            .fg(theme::ACCENT)
            .add_modifier(Modifier::BOLD),
    )
}
pub fn metric_value(label: &str, value: impl std::fmt::Display) -> Line<'static> {
    let value = value.to_string();
    let color = match value.as_str() {
        "unknown" | "not applicable" => theme::MUTED,
        "loading..." => theme::ACCENT,
        "error" | "unavailable" => theme::ERROR,
        _ => theme::NUMBER,
    };
    Line::from(vec![
        Span::styled(format!("{label:<21} "), Style::default().fg(theme::MUTED)),
        Span::styled(
            value,
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ),
    ])
}
pub fn status_label(label: impl Into<String>, color: Color) -> Span<'static> {
    Span::styled(
        label.into(),
        Style::default().fg(color).add_modifier(Modifier::BOLD),
    )
}
pub fn selectable_row() -> Style {
    Style::default()
        .fg(theme::ACTIVE)
        .add_modifier(Modifier::BOLD)
}
pub fn path_label(path: &Path, width: usize) -> Line<'static> {
    let text = compact(&path.to_string_lossy(), width);
    let (parent, name) = text
        .rsplit_once('/')
        .map_or(("", text.as_str()), |(p, n)| (p, n));
    Line::from(vec![
        Span::styled(
            if parent.is_empty() {
                String::new()
            } else {
                format!("{parent}/")
            },
            Style::default().fg(theme::MUTED),
        ),
        Span::styled(name.to_owned(), Style::default().fg(theme::TEXT)),
    ])
}
pub fn compact(text: &str, width: usize) -> String {
    let mut escaped = String::new();
    for c in text.chars() {
        if c.is_control() {
            escaped.extend(c.escape_default());
        } else {
            escaped.push(c);
        }
    }
    if Span::raw(&escaped).width() <= width {
        return escaped;
    }
    let available = width.saturating_sub(2);
    let mut used = 0;
    let mut tail = Vec::new();
    for c in escaped.chars().rev() {
        let w = Span::raw(c.to_string()).width();
        if used + w > available {
            break;
        }
        used += w;
        tail.push(c);
    }
    let tail: String = tail.into_iter().rev().collect();
    if width >= 2 {
        format!("…/{tail}")
    } else {
        "…".chars().take(width).collect()
    }
}
pub fn empty_state(title: &str, explanation: &str) -> Paragraph<'static> {
    let mut lines = vec![section_header(title), Line::raw("")];
    lines.extend(
        explanation
            .lines()
            .map(|line| Line::styled(line.to_owned(), Style::default().fg(theme::MUTED))),
    );
    Paragraph::new(lines).wrap(Wrap { trim: false })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{buffer::Buffer, layout::Rect, widgets::Widget};
    #[test]
    fn wrapped_errors_remain_reachable_without_blank_overscroll() {
        let area = Rect::new(0, 0, 30, 8);
        let limit = std::cell::Cell::new(None);
        let mut buffer = Buffer::empty(area);
        let mut lines = vec![Line::raw("long diagnostic ".repeat(80))];
        lines.push(Line::raw("END OF ERROR"));
        scrollable(
            lines,
            styled_panel(" ERROR ", true),
            area,
            usize::MAX,
            &limit,
        )
        .render(area, &mut buffer);
        assert!(limit.get().unwrap() > 20);
        assert!(buffer
            .content
            .iter()
            .map(|c| c.symbol())
            .collect::<String>()
            .contains("END OF ERROR"));
    }
    #[test]
    fn multiline_empty_state_keeps_sentence_boundaries() {
        let area = Rect::new(0, 0, 80, 10);
        let mut buffer = Buffer::empty(area);
        empty_state("Not applicable", "This is a field.\n\nChoose a function.")
            .render(area, &mut buffer);
        assert_eq!(buffer[(0, 2)].symbol(), "T");
        assert_eq!(buffer[(0, 4)].symbol(), "C");
    }
}
