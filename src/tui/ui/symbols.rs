use super::*;
pub(super) fn symbol_row(symbol: &crate::model::Symbol, width: usize) -> ListItem<'static> {
    let mut path = components::path_label(&symbol.file, width.saturating_sub(8));
    path.spans.insert(0, Span::raw("  "));
    path.spans.push(Span::styled(
        format!(":{}", symbol.selection_range.start.line + 1),
        Style::default().fg(theme::NUMBER),
    ));
    ListItem::new(vec![
        Line::from(vec![
            Span::styled(
                format!("{:<9}", truncate(&symbol.kind, 8)),
                Style::default().fg(theme::MUTED),
            ),
            Span::styled(
                {
                    let available = width.saturating_sub(9);
                    let qualified = qualified_symbol(symbol);
                    if qualified.chars().count() <= available {
                        qualified
                    } else {
                        truncate(&symbol.name, available)
                    }
                },
                Style::default().fg(theme::TEXT),
            ),
        ]),
        path,
    ])
}
pub(super) fn render_symbols_view(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let panes = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(area);
    render_symbols(frame, panes[0], app);
    render_inspector(frame, panes[1], app);
}

pub(super) fn render_symbols(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let active = app.pane == Pane::Symbols;
    if app.symbols_query.is_empty() && app.symbols_results.is_empty() {
        super::super::journey::render_start(frame, area, app, false);
        return;
    }

    if app.symbols_results.is_empty() {
        let message = if app.symbols_loading {
            "Searching..."
        } else {
            "No symbols found"
        };
        frame.render_widget(
            Paragraph::new(vec![
                Line::raw(format!("query: {}", app.symbols_query)),
                Line::raw(""),
                Line::raw(message),
            ])
            .block(pane_block(" SYMBOLS ", active)),
            area,
        );
        return;
    }

    let inner_width = area.width.saturating_sub(4) as usize;
    let items: Vec<_> = app
        .symbols_results
        .iter()
        .map(|s| symbol_row(s, inner_width))
        .collect();
    let title = if app.symbols_loading {
        format!(" SYMBOLS · {} · searching… ", app.symbols_query)
    } else if app.symbols_truncated {
        format!(" SYMBOLS · {} · first 50 ", app.symbols_query)
    } else {
        format!(" SYMBOLS · {} ", app.symbols_query)
    };
    let mut state = ListState::default().with_selected(Some(app.symbols_selection));
    frame.render_stateful_widget(
        List::new(items)
            .highlight_symbol("> ")
            .highlight_style(
                Style::default()
                    .fg(theme::ACTIVE)
                    .add_modifier(Modifier::BOLD),
            )
            .block(pane_block(&title, active)),
        area,
        &mut state,
    );
}

pub(super) fn render_inspector(frame: &mut Frame<'_>, area: Rect, app: &App) {
    render_inspector_block(frame, area, app, app.pane == Pane::Inspector);
}

pub(super) fn render_inspector_block(frame: &mut Frame<'_>, area: Rect, app: &App, active: bool) {
    let empty_search =
        app.view == View::Symbols && app.symbols_query.is_empty() && app.symbols_results.is_empty();
    if empty_search || app.inspector.is_none() {
        let mut text = vec![
            heading("No symbol selected."),
            Line::raw(""),
            Line::raw("Search for a function, method, struct or trait."),
            Line::raw("Press / to search."),
        ];
        if let Some(previous) = &app.inspector {
            text.extend([
                Line::raw(""),
                heading("LAST INSPECTED"),
                Line::raw(qualified_symbol(&previous.symbol)),
                components::path_label(
                    &previous.symbol.file,
                    area.width.saturating_sub(4) as usize,
                ),
            ]);
        }
        frame.render_widget(
            Paragraph::new(text)
                .wrap(Wrap { trim: false })
                .block(pane_block(" INSPECTOR ", active)),
            area,
        );
        return;
    }
    let inspector = app.inspector.as_ref().unwrap();
    let current = if app.view == View::Symbols {
        !app.symbols_loading
            && app.symbols_dirty_at.is_none()
            && app
                .symbols_results
                .get(app.symbols_selection)
                .is_some_and(|s| s.id == inspector.symbol.id)
    } else {
        app.explorer
            .file
            .as_ref()
            .is_some_and(|f| f.path == inspector.symbol.file)
    };
    let mut text = vec![];
    if !current {
        text.extend([
            heading("LAST INSPECTED · not the current selection"),
            Line::raw("Select a symbol and press Enter to update this Inspector."),
            Line::raw(""),
        ]);
    }
    if app.view == View::Explorer {
        if let Some(file) = &app.explorer.file {
            text.push(Line::styled(
                format!(
                    "Current file: {} · Esc returns to file symbols",
                    file.path.display()
                ),
                Style::default().fg(theme::MUTED),
            ));
        }
    }
    text.extend(inspector_text(inspector));
    frame.render_widget(
        components::scrollable(
            text,
            pane_block(
                if current {
                    " INSPECTOR "
                } else {
                    " LAST INSPECTED "
                },
                active,
            ),
            area,
            inspector.scroll,
            &inspector.scroll_limit,
        ),
        area,
    );
}

pub(super) fn inspector_text(inspector: &Inspector) -> Vec<Line<'static>> {
    let definition = match &inspector.definition {
        None => "loading...".into(),
        Some(None) => "not found".into(),
        Some(Some(l)) => format!("{}:{}", l.file.display(), l.range.start.line + 1),
    };
    let callable = crate::model::is_callable_kind(&inspector.symbol.kind);
    let count = |section, value| {
        if section == "calls" && !callable {
            "not applicable".into()
        } else {
            section_count(inspector, section, value)
        }
    };
    let mut lines = vec![
        heading("SYMBOL"),
        Line::styled(
            qualified_symbol(&inspector.symbol),
            components::selectable_row(),
        ),
        Line::styled(
            format!(
                "{} · {}:{}",
                inspector.symbol.kind,
                inspector.symbol.file.display(),
                inspector.symbol.selection_range.start.line + 1
            ),
            Style::default().fg(theme::MUTED),
        ),
        Line::raw(""),
        heading("USAGE"),
        components::metric_value("References", count("references", inspector.references)),
        components::metric_value("Callers", count("calls", inspector.callers)),
        components::metric_value("Callees", count("calls", inspector.callees)),
        components::metric_value("Problems", count("diagnostics", inspector.diagnostics)),
        Line::raw(""),
        heading("DOCUMENTATION · Hover"),
    ];
    lines.extend(
        inspector
            .hover
            .as_ref()
            .map(|h| h.as_deref().unwrap_or("No documentation."))
            .unwrap_or("loading...")
            .lines()
            .map(|s| Line::raw(s.to_owned())),
    );
    lines.extend([Line::raw(""), heading("Source")]);
    if let Some(source) = &inspector.source {
        lines.extend(super::super::highlight::preview(source));
    } else {
        lines.push(Line::raw("loading..."));
    }
    lines.extend([
        Line::raw(""),
        heading("LOCATION"),
        Line::raw(format!("Definition   {definition}")),
    ]);
    if !inspector.errors.is_empty() {
        lines.push(heading("Errors"));
        lines.extend(
            inspector
                .errors
                .iter()
                .map(|(s, e)| Line::styled(format!("{s}: {e}"), Style::default().fg(theme::ERROR))),
        );
    }
    lines
}

pub(super) fn section_count(inspector: &Inspector, section: &str, value: Option<usize>) -> String {
    if inspector.errors.iter().any(|(name, _)| name == section) {
        return "error".into();
    }
    value.map_or_else(|| "loading...".into(), |value| value.to_string())
}
