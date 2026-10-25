use super::*;
pub(super) fn render(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let s = &app.module_map;
    let name = app
        .project
        .architecture
        .node(&s.crate_id)
        .map_or("?", |n| n.name.as_str());
    let rows = Layout::vertical([Constraint::Length(3), Constraint::Min(3)]).split(area);
    let mut tabs: Vec<_> = [Mode::Hierarchy, Mode::Imports, Mode::UsedBy]
        .into_iter()
        .map(|m| {
            Span::styled(
                format!(" {}{} ", if m == s.mode { ">" } else { " " }, m.label()),
                if m == s.mode {
                    components::selectable_row()
                } else {
                    Style::default().fg(theme::MUTED)
                },
            )
        })
        .collect();
    tabs.push(Span::styled(
        if s.diagram {
            " · g Diagram"
        } else {
            " · g List"
        },
        Style::default().fg(theme::MUTED),
    ));
    let status = s.graph.as_ref().map_or_else(
        || {
            Line::raw(if s.error.is_some() {
                "Coverage: Error"
            } else {
                "Coverage: loading"
            })
        },
        |g| {
            Line::from(vec![
                Span::styled(
                    format!("Coverage: {:?}", g.coverage_status),
                    Style::default().fg(match g.coverage_status {
                        crate::module_graph::CoverageStatus::Complete => theme::SUCCESS,
                        crate::module_graph::CoverageStatus::Partial => theme::WARNING,
                        crate::module_graph::CoverageStatus::Error => theme::ERROR,
                    }),
                ),
                Span::styled(
                    format!(
                        " · Resolution: {:?} · cfg {}",
                        g.resolution_status, g.conditional_relations
                    ),
                    Style::default().fg(theme::MUTED),
                ),
            ])
        },
    );
    frame.render_widget(
        Paragraph::new(vec![
            components::section_header(&format!(
                "Workspace > {name} > Modules · {}",
                s.graph
                    .as_ref()
                    .and_then(|g| g.node(&s.focus))
                    .map_or("loading", |n| n.qualified_path.as_str())
            )),
            Line::from(tabs),
            status,
        ]),
        rows[0],
    );
    let Some(g) = &s.graph else {
        frame.render_widget(components::empty_state("MODULE MAP",s.error.as_deref().unwrap_or("Loading module declarations...\nRepository scan / bounded syntax analysis in progress.\nRA is independent; q remains available.")).block(components::styled_panel(" MODULE MAP ",true)),rows[1]);
        return;
    };
    let panes =
        Layout::horizontal([Constraint::Percentage(55), Constraint::Percentage(45)]).split(rows[1]);
    let target = g
        .targets
        .iter()
        .find(|t| t.id == s.target_id)
        .map_or("?", |t| t.name.as_str());
    let title = format!(" MODULE MAP · {target} · {} ", s.mode.label());
    let panel = components::styled_panel(&title, app.pane == Pane::MapGraph);
    let inner = panel.inner(panes[0]);
    frame.render_widget(panel, panes[0]);
    let regions = Layout::vertical([Constraint::Min(1), Constraint::Length(2)]).split(inner);
    let drawn = s.diagram && super::diagram::render(frame, regions[0], s);
    if !drawn {
        super::list::render(frame, regions[0], s);
    }
    let total = s.choices().len().saturating_sub(1);
    let caption = if drawn {
        let cache = s.diagram_cache.borrow();
        let d = cache.as_ref().and_then(|(_, d)| d.as_ref()).unwrap();
        format!("Diagram · Showing {} / {} related", d.shown, d.total)
    } else if s.diagram {
        "List fallback: viewport <28×9".into()
    } else {
        format!("List · {total} related · j/k scroll")
    };
    frame.render_widget(
        Paragraph::new(vec![
            Line::styled(caption, Style::default().fg(theme::MUTED)),
            Line::styled(
                match s.mode {
                    Mode::Hierarchy => "mod containment · p priorities · o source",
                    Mode::Imports => "use: focus → neighbors · [cfg] conditional",
                    Mode::UsedBy => "use: neighbors → focus · not calls",
                },
                Style::default().fg(theme::MUTED),
            ),
        ]),
        regions[1],
    );
    frame.render_widget(
        components::scrollable(
            super::inspector::details(app),
            components::styled_panel(
                " MODULE INSPECTOR · p priorities ",
                app.pane == Pane::MapInspector,
            ),
            panes[1],
            app.map.inspector_scroll,
            &app.map.scroll_limit,
        ),
        panes[1],
    );
}
