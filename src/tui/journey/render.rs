use super::super::{components, theme};
use super::*;
use ratatui::{
    layout::Rect,
    style::Style,
    text::Line,
    widgets::{Clear, List, ListItem, ListState, Paragraph, Wrap},
    Frame,
};
pub(in crate::tui) fn render_picker(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let Some(p) = &app.journey.picker else {
        return;
    };
    let width = area.width.saturating_sub(6).min(110);
    let height = (p.actions.len() as u16 * 2 + 5)
        .min(area.height.saturating_sub(4))
        .max(5);
    let rect = Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    );
    frame.render_widget(Clear, rect);
    let title = format!(" {} ", p.title);
    let block = components::styled_panel(&title, true);
    let inside = block.inner(rect);
    frame.render_widget(block, rect);
    let rows = ratatui::layout::Layout::vertical([
        ratatui::layout::Constraint::Min(1),
        ratatui::layout::Constraint::Length(2),
    ])
    .split(inside);
    if p.actions.is_empty() {
        frame.render_widget(Paragraph::new("No known candidates in this subtree.\nReports are bounded or still loading; o opens source.").wrap(Wrap{trim:false}),rows[0]);
    } else {
        let items: Vec<_> = p
            .actions
            .iter()
            .map(|a| {
                ListItem::new(vec![
                    Line::styled(
                        components::compact(&a.label(), width.saturating_sub(6) as usize),
                        Style::default().fg(theme::TEXT),
                    ),
                    Line::styled(
                        components::compact(&a.location(), width.saturating_sub(6) as usize),
                        Style::default().fg(theme::MUTED),
                    ),
                ])
            })
            .collect();
        let mut selection = ListState::default().with_selected(Some(p.selected));
        frame.render_stateful_widget(
            List::new(items)
                .highlight_symbol("> ")
                .highlight_style(components::selectable_row()),
            rows[0],
            &mut selection,
        );
    }
    frame.render_widget(
        Paragraph::new(vec![
            Line::raw(if p.calls {
                if matches!(app.analyzer_state,AnalyzerState::Error(_)) {"Enter exact source · Calls unavailable (RA) · Esc back"}else{"Enter resolve exact Calls · Esc back"}
            } else {"Enter source / Inspector · c exact Calls · Esc back"}),
            Line::styled(
                app.notice
                    .as_deref()
                    .unwrap_or(if p.actions.iter().any(|a|matches!(a,Action::BinaryEntrypoint(_))) {"Cargo binary targets → exact RA file symbols. Without RA, Enter opens source."} else {"Known ranked functions/files only; no name-only lookup."})
                    .to_owned(),
                Style::default().fg(theme::MUTED),
            ),
        ]),
        rows[1],
    );
}
pub(in crate::tui) fn render_start(frame: &mut Frame<'_>, area: Rect, app: &App, calls: bool) {
    let title = if calls {
        " CALL EXPLORATION "
    } else {
        " START EXPLORING SYMBOLS "
    };
    let block = components::styled_panel(title, calls || app.pane == Pane::Symbols);
    let inside = block.inner(area);
    frame.render_widget(block, area);
    let rows = ratatui::layout::Layout::vertical([
        ratatui::layout::Constraint::Length(2),
        ratatui::layout::Constraint::Min(1),
        ratatui::layout::Constraint::Length(3),
    ])
    .split(inside);
    frame.render_widget(
        Paragraph::new(components::section_header(if calls {
            "No callable symbol selected."
        } else {
            "Explore known locations"
        })),
        rows[0],
    );
    let mut items = vec![];
    for r in super::actions::routes(calls) {
        let (label, actions) = super::actions::available(app, r, calls);
        let enabled = !actions.is_empty();
        let location = if let Some(a) = actions.first() {
            if calls
                && matches!(app.analyzer_state, AnalyzerState::Error(_))
                && matches!(a, Action::BinaryEntrypoint(_))
            {
                "Source available · Calls unavailable (RA)".into()
            } else {
                format!("  {} location(s) · {}", actions.len(), a.location())
            }
        } else {
            super::actions::unavailable_reason(app, r, calls)
        };
        let width = inside.width.saturating_sub(2) as usize;
        items.push(ListItem::new(vec![
            Line::styled(
                components::compact(&label, width),
                if enabled {
                    Style::default().fg(theme::TEXT)
                } else {
                    Style::default().fg(theme::MUTED)
                },
            ),
            Line::styled(
                components::compact(&location, width),
                Style::default().fg(theme::MUTED),
            ),
        ]));
    }
    let mut selection = ListState::default().with_selected(Some(app.journey.start_selection));
    frame.render_stateful_widget(
        List::new(items)
            .highlight_symbol("> ")
            .highlight_style(components::selectable_row()),
        rows[1],
        &mut selection,
    );
    let mut lines = vec![
        Line::raw("j/k choose · Enter candidates"),
        Line::raw("/ Search by name"),
    ];
    if matches!(app.analyzer_state, AnalyzerState::Error(_)) {
        lines.push(Line::styled(
            "Semantic inspection unavailable. Metrics/source remain accessible.",
            Style::default().fg(theme::WARNING),
        ));
    }
    if let Some(notice) = &app.notice {
        lines.push(Line::styled(
            notice.clone(),
            Style::default().fg(theme::WARNING),
        ));
    }
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), rows[2]);
}
