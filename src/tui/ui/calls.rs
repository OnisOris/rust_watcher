use super::*;
pub(super) fn render_calls_view(frame: &mut Frame<'_>, area: Rect, app: &App) {
    if let Some(reason) = &app.calls.unavailable {
        frame.render_widget(
            components::empty_state("Call hierarchy is not applicable", reason)
                .block(pane_block(" CALLS · not applicable ", true)),
            area,
        );
        return;
    }
    if let (Some(incoming), Some(outgoing)) = (&app.calls.callers_error, &app.calls.callees_error) {
        if incoming == outgoing {
            frame.render_widget(
                components::empty_state("Call hierarchy unavailable", incoming)
                    .block(pane_block(" CALLS · RA error ", true)),
                area,
            );
            return;
        }
    }
    if app.calls.center.is_none() {
        frame.render_widget(
            components::empty_state("No symbol selected", "Open Symbols with 2, search for a function or method,\nthen press Enter or c to explore its execution flow.")
                .block(pane_block(" CALLS ", true)),
            area,
        );
        return;
    }
    let context = app.inspector.as_ref().filter(|i| {
        app.calls.center.as_ref().is_some_and(|c| {
            c.file == i.symbol.file && c.selection_range.start == i.symbol.selection_range.start
        })
    });
    let sections = Layout::vertical([
        Constraint::Length(if area.height >= 28 && context.is_some() {
            14
        } else {
            area.height
        }),
        Constraint::Min(0),
    ])
    .split(area);
    let panes = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(32),
            Constraint::Percentage(36),
            Constraint::Percentage(32),
        ])
        .split(sections[0]);
    render_call_list(frame, panes[0], app, true);
    render_call_current(frame, panes[1], app);
    render_call_list(frame, panes[2], app, false);
    if sections[1].height > 0 {
        let inspector = context.unwrap();
        let mut lines = vec![Line::styled(
            format!(
                "{} · {}:{}",
                qualified_symbol(&inspector.symbol),
                inspector.symbol.file.display(),
                inspector.symbol.selection_range.start.line + 1
            ),
            Style::default().fg(theme::MUTED),
        )];
        if let Some(source) = &inspector.source {
            lines.extend(super::super::highlight::preview(source));
        } else {
            lines.push(Line::raw("Source loading..."));
        }
        frame.render_widget(
            components::scrollable(
                lines,
                pane_block(
                    " CURRENT SOURCE · exact inspected location · j/k scroll ",
                    app.pane == Pane::CallCurrent,
                ),
                sections[1],
                app.calls.context_scroll,
                &app.calls.context_scroll_limit,
            ),
            sections[1],
        );
    }
}

pub(super) fn render_call_list(frame: &mut Frame<'_>, area: Rect, app: &App, incoming: bool) {
    let (items, selection, loading, truncated, error, pane, label) = if incoming {
        (
            &app.calls.callers,
            app.calls.callers_selection,
            app.calls.callers_loading,
            app.calls.callers_truncated,
            app.calls.callers_error.as_deref(),
            Pane::Callers,
            "CALLERS →",
        )
    } else {
        (
            &app.calls.callees,
            app.calls.callees_selection,
            app.calls.callees_loading,
            app.calls.callees_truncated,
            app.calls.callees_error.as_deref(),
            Pane::Callees,
            "→ CALLEES",
        )
    };
    let title = if truncated {
        format!(" {label} (first 20) ")
    } else {
        format!(" {label} ")
    };
    if let Some(error) = error {
        frame.render_widget(
            Paragraph::new(format!("error: {error}"))
                .wrap(Wrap { trim: false })
                .block(pane_block(&title, app.pane == pane)),
            area,
        );
        return;
    }
    if loading && items.is_empty() {
        frame.render_widget(
            Paragraph::new(format!("Loading {}...", label.to_lowercase()))
                .block(pane_block(&title, app.pane == pane)),
            area,
        );
        return;
    }
    if items.is_empty() {
        frame.render_widget(
            Paragraph::new(if incoming {
                "No callers in selected scope"
            } else {
                "No callees in selected scope"
            })
            .block(pane_block(&title, app.pane == pane)),
            area,
        );
        return;
    }
    let rows: Vec<_> = items
        .iter()
        .map(|target| {
            call_target_row(target, &app.project.workspace_root).style(Style::default().fg(
                if incoming {
                    theme::ACCENT
                } else {
                    theme::ACTIVE
                },
            ))
        })
        .collect();
    let mut state = ListState::default().with_selected(Some(selection));
    frame.render_stateful_widget(
        List::new(rows)
            .highlight_symbol("> ")
            .highlight_style(
                Style::default()
                    .fg(theme::ACTIVE)
                    .add_modifier(Modifier::BOLD),
            )
            .block(pane_block(&title, app.pane == pane)),
        area,
        &mut state,
    );
}

pub(super) fn call_target_row(
    target: &CallTarget,
    workspace_root: &std::path::Path,
) -> ListItem<'static> {
    let external = !target.is_workspace_local(workspace_root);
    let name = if external {
        format!("[ext] {}", call_target_name(target))
    } else {
        call_target_name(target)
    };
    ListItem::new(vec![
        Line::raw(name),
        Line::styled(
            format!(
                "  {}:{}",
                compact_call_path(&target.file, workspace_root),
                target.selection_range.start.line + 1
            ),
            Style::default().fg(theme::MUTED),
        ),
    ])
}

pub(super) fn render_call_current(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let target = app.calls.center.as_ref().expect("checked by calls view");
    let mut text = vec![heading(&call_target_name(target))];
    if target
        .detail
        .as_deref()
        .is_some_and(|detail| !detail.is_empty() && !detail_is_container(detail))
    {
        text.push(Line::styled(
            target.detail.clone().unwrap_or_default(),
            Style::default().fg(theme::MUTED),
        ));
    }
    text.extend([
        Line::raw(""),
        Line::raw(format!("kind      {}", target.kind)),
        Line::raw(format!(
            "location  {}:{}",
            compact_call_path(&target.file, &app.project.workspace_root),
            target.selection_range.start.line + 1
        )),
        Line::raw(""),
        Line::raw(format!(
            "callers   {}",
            call_count(
                app.calls.callers_loading,
                app.calls.callers_error.as_ref(),
                app.calls.callers.len()
            )
        )),
        Line::raw(format!(
            "callees   {}",
            call_count(
                app.calls.callees_loading,
                app.calls.callees_error.as_ref(),
                app.calls.callees.len()
            )
        )),
        Line::raw(""),
        Line::raw(format!("history   {}", app.calls.history.len())),
    ]);
    frame.render_widget(
        Paragraph::new(text)
            .wrap(Wrap { trim: false })
            .block(pane_block(
                &format!(" CURRENT · {} ", app.calls.scope.label()),
                app.pane == Pane::CallCurrent,
            )),
        area,
    );
}

pub(super) fn compact_call_path(
    path: &std::path::Path,
    workspace_root: &std::path::Path,
) -> String {
    if let Ok(relative) = path.strip_prefix(workspace_root) {
        return relative.display().to_string();
    }
    if !path.is_absolute() {
        return path.display().to_string();
    }
    let components: Vec<_> = path
        .components()
        .filter_map(|component| component.as_os_str().to_str())
        .collect();
    if let Some(index) = components
        .iter()
        .position(|component| *component == "library")
    {
        return components[index + 1..].join("/");
    }
    if let Some(registry) = components
        .iter()
        .position(|component| *component == "registry")
    {
        if let Some(source) = components[registry + 1..]
            .iter()
            .position(|component| *component == "src")
        {
            let start = registry + source + 3;
            if start < components.len() {
                return components[start..].join("/");
            }
        }
    }
    components[components.len().saturating_sub(4)..].join("/")
}

pub(super) fn call_target_name(target: &CallTarget) -> String {
    target
        .detail
        .as_deref()
        .filter(|detail| detail_is_container(detail))
        .map_or_else(
            || target.name.clone(),
            |detail| format!("{detail}::{}", target.name),
        )
}

pub(super) fn detail_is_container(detail: &str) -> bool {
    detail
        .chars()
        .all(|character| character.is_alphanumeric() || matches!(character, '_' | ':'))
}

pub(super) fn call_count(loading: bool, error: Option<&String>, count: usize) -> String {
    if error.is_some() {
        "error".into()
    } else if loading {
        "loading...".into()
    } else {
        count.to_string()
    }
}
