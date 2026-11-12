//! Route guidance uses already available facts, never decorative semantic requests.
use super::super::{
    components, theme,
    ui::{heading, truncate},
};
use super::{
    actions::{self, Start},
    *,
};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::Style,
    text::Line,
    widgets::{List, ListItem, ListState, Paragraph, Wrap},
    Frame,
};

pub(super) fn index(app: &App, calls: bool) -> usize {
    let selected = if calls {
        app.journey.start_selection
    } else {
        app.journey.symbols_start_selection
    };
    selected.min(actions::routes(calls).len() - 1)
}
pub(super) fn enabled(app: &App, route: Start, calls: bool) -> bool {
    if route == Start::Search {
        return !matches!(app.analyzer_state, AnalyzerState::Error(_));
    }
    !actions::available(app, route, calls).1.is_empty()
}
fn label(route: Start, calls: bool) -> &'static str {
    match route {
        Start::Search if calls => "Search for a callable symbol",
        Start::Search => "Search by name",
        Start::Entry if calls => "Start from an executable",
        Start::Entry => "Browse known source roots",
        Start::Longest if calls => "Explore a known function",
        Start::Longest => "Inspect long functions",
        Start::Deepest => "Inspect deep functions",
        Start::Hotspots => "Review hotspot files",
        Start::Previous => "Previously inspected function",
    }
}
fn purpose(route: Start, calls: bool) -> (&'static str, &'static str) {
    match route {
        Start::Search => ("Find Rust symbols by name with rust-analyzer. This is not natural-language search.", "A symbol list, then Enter opens its exact Inspector."),
        Start::Entry if calls => ("Choose a Cargo binary; RA verifies its exact top-level main.", "Static callers/callees, not a runtime trace; g opens Graph."),
        Start::Entry => ("Choose a binary or library root already known to Cargo.", "Its exact file and available Rust symbols."),
        Start::Longest => ("Choose a measured function from bounded length rankings, not an importance ranking.", if calls { "RA verifies its exact anchor before opening static Calls." } else { "Its source/Inspector; c opens Calls for a verified callable." }),
        Start::Deepest => ("Choose a function with measured nested control structures. Depth is not cyclomatic complexity.", "Exact source and an Inspector after RA anchor verification."),
        Start::Hotspots => ("Inspect current files with recorded changes and structural evidence, not predicted defects.", "The exact file; choose an RA callable there for Calls."),
        Start::Previous => ("Reuse the exact previously inspected callable, if one exists.", "Its static callers and callees; no name-only lookup."),
    }
}
pub(in crate::tui) fn render_start(frame: &mut Frame<'_>, area: Rect, app: &App, calls: bool) {
    let block = components::styled_panel(
        if calls {
            " CALL EXPLORATION "
        } else {
            " SYMBOL EXPLORATION "
        },
        true,
    );
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let routes = actions::routes(calls);
    let selected = index(app, calls);
    let panels = if area.width >= 100 {
        Layout::horizontal([Constraint::Percentage(43), Constraint::Min(1)]).split(inner)
    } else {
        Layout::vertical([
            Constraint::Length(routes.len() as u16 + 1),
            Constraint::Min(1),
        ])
        .split(inner)
    };
    let rows = routes
        .iter()
        .map(|r| {
            let available = enabled(app, *r, calls);
            ListItem::new(Line::styled(
                format!(
                    "{}{}",
                    label(*r, calls),
                    if available { "" } else { " · Unavailable" }
                ),
                Style::default().fg(if available { theme::TEXT } else { theme::MUTED }),
            ))
        })
        .collect::<Vec<_>>();
    let mut selection = ListState::default().with_selected(Some(selected));
    frame.render_stateful_widget(
        List::new(rows)
            .highlight_symbol("> ")
            .highlight_style(components::selectable_row()),
        panels[0],
        &mut selection,
    );
    let route = routes[selected];
    let (what, result) = purpose(route, calls);
    let available = enabled(app, route, calls);
    let status = if route == Start::Search || !available {
        actions::unavailable_reason(app, route, calls)
    } else if !matches!(app.analyzer_state, AnalyzerState::Ready) && route != Start::Hotspots {
        if matches!(app.analyzer_state, AnalyzerState::Error(_)) {
            "Source available; semantic inspection/Calls unavailable without RA".into()
        } else {
            "Source available; exact semantic navigation waits for RA Ready".into()
        }
    } else {
        format!(
            "{} known candidate(s) · Enter chooses",
            actions::available(app, route, calls).1.len()
        )
    };
    let mut details = vec![
        heading(label(route, calls)),
        Line::raw(what),
        Line::raw(format!("You will see: {result}")),
        Line::styled(
            status,
            Style::default().fg(if available {
                theme::MUTED
            } else {
                theme::WARNING
            }),
        ),
        heading(if available {
            if route == Start::Search {
                "Next: Enter / / search"
            } else {
                "Next: Enter choose · Esc back"
            }
        } else {
            "Next: choose another route · 1 Overview · 4 Explorer"
        }),
    ];
    if area.width >= 100 && available && route != Start::Search {
        details.push(heading("Known candidates · Enter chooses"));
        for action in actions::available(app, route, calls).1.iter().take(3) {
            details.push(Line::raw(truncate(
                &action.label(),
                panels[1].width.saturating_sub(2) as usize,
            )));
            details.push(components::path_label(
                std::path::Path::new(&action.location()),
                panels[1].width.saturating_sub(2) as usize,
            ));
        }
    } else if route == Start::Search && area.width >= 100 {
        details.extend([
            heading("Search tips"),
            Line::raw("Use a module/type qualifier when names are ambiguous."),
            Line::raw("Browse source roots if you do not know a symbol name yet."),
        ]);
    }
    if !calls {
        if let Some(i) = &app.inspector {
            details.extend([
                heading("LAST INSPECTED · retained, not current"),
                Line::raw(truncate(
                    &super::super::app::qualified_symbol(&i.symbol),
                    panels[1].width as usize,
                )),
            ]);
        }
    }
    frame.render_widget(
        Paragraph::new(details).wrap(Wrap { trim: false }),
        panels[1],
    );
}
