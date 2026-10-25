use super::*;
pub(in crate::tui) fn render_summary(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let number = |n: Option<usize>| {
        n.map_or_else(
            || "…".into(),
            |n| {
                if n >= 10000 {
                    format!("{:.1}k", n as f64 / 1000.)
                } else {
                    n.to_string()
                }
            },
        )
    };
    let mut spans = vec![];
    for (value, label) in [
        (
            number(app.metrics.as_ref().map(|m| m.code_nonblank_lines as usize)),
            " source LOC",
        ),
        (
            number(app.maintainability.as_ref().map(|m| m.functions)),
            " functions",
        ),
        (
            number(app.hotspots.as_ref().and_then(|h| h.candidate_files)),
            " review files",
        ),
    ] {
        spans.push(components::status_label(value, theme::NUMBER));
        spans.push(Span::styled(
            format!("{label}   "),
            Style::default().fg(theme::MUTED),
        ));
    }
    frame.render_widget(
        Paragraph::new(Line::from(spans)).block(pane_block(" REPOSITORY OVERVIEW ", false)),
        area,
    );
}
pub(in crate::tui) fn render_project(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let width = area.width.saturating_sub(5) as usize;
    let items = targets(app)
        .into_iter()
        .map(|target| {
            let lines = match target {
                Target::Guide(g) => vec![Line::styled(
                    format!("  {} →", g.label()),
                    Style::default().fg(theme::ACTIVE),
                )],
                Target::Workspace => vec![heading("Workspace summary")],
                Target::Group(g) => vec![Line::styled(
                    format!(
                        "{} {}",
                        if app.overview.expanded.contains(&g) {
                            "▾"
                        } else {
                            "▸"
                        },
                        g.label()
                    ),
                    Style::default().fg(if g == Group::Hotspots {
                        theme::GIT
                    } else {
                        theme::ACCENT
                    }),
                )],
                Target::Hotspot(p) | Target::Historical(p) => {
                    let f = app
                        .hotspots
                        .as_ref()
                        .unwrap()
                        .rows()
                        .iter()
                        .chain(app.hotspots.as_ref().unwrap().historical_files.iter())
                        .find(|f| f.path == p)
                        .unwrap();
                    file_row(
                        &p,
                        &format!(
                            "{} changes{}",
                            f.change_commits,
                            if f.review_candidate {
                                ""
                            } else {
                                " · history"
                            }
                        ),
                        width,
                        theme::GIT,
                    )
                }
                Target::File(p, n) => file_row(&p, &format!("{n} lines"), width, theme::NUMBER),
                Target::Unmeasured(f) => file_row(&f.path, "LOC unknown", width, theme::WARNING),
                Target::Entrypoint(p) => file_row(&p, "entry", width, theme::ACCENT),
                Target::Package(n) => vec![Line::raw(format!("  {n}"))],
                Target::Longest(f) => function_row(&f, false, width),
                Target::Deepest(f) => function_row(&f, true, width),
            };
            ListItem::new(lines)
        })
        .collect::<Vec<_>>();
    let mut state = ListState::default().with_selected(index(app));
    frame.render_stateful_widget(
        List::new(items)
            .highlight_symbol("> ")
            .highlight_style(components::selectable_row())
            .block(pane_block(" EXPLORATION ", app.pane == Pane::Project)),
        area,
        &mut state,
    );
}
fn function_row(f: &FunctionMetrics, deep: bool, width: usize) -> Vec<Line<'static>> {
    vec![
        Line::from(vec![
            Span::styled(
                format!(
                    "  {} {}{} · ",
                    if deep {
                        f.max_control_depth
                    } else {
                        f.physical_lines
                    },
                    if deep { "depth" } else { "lines" },
                    if f.test_classified { " [test]" } else { "" }
                ),
                Style::default().fg(theme::NUMBER),
            ),
            Span::raw(components::compact(
                &f.qualified_name,
                width.saturating_sub(12),
            )),
        ]),
        components::path_label(&f.path, width),
    ]
}
fn file_row(
    path: &std::path::Path,
    value: &str,
    width: usize,
    color: ratatui::style::Color,
) -> Vec<Line<'static>> {
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let available = width.saturating_sub(value.chars().count() + 3);
    vec![
        Line::from(vec![
            Span::raw(format!(
                "  {:<available$} ",
                components::compact(&name, available)
            )),
            Span::styled(value.to_owned(), Style::default().fg(color)),
        ]),
        components::path_label(path, width),
    ]
}
pub(in crate::tui) fn render_details(frame: &mut Frame<'_>, area: Rect, app: &App) {
    frame.render_widget(
        components::scrollable(
            details(app),
            pane_block(
                if super::super::hotspots::selected(app).is_some() {
                    " HOTSPOT DETAILS "
                } else {
                    " SELECTED ITEM · j/k scroll "
                },
                app.pane == Pane::Details,
            ),
            area,
            app.metrics_scroll,
            &app.metrics_scroll_limit,
        ),
        area,
    );
}
