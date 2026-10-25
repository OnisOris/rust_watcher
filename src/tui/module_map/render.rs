use super::*;
use crate::module_graph::ModuleStatus;
pub(super) fn render(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let state = &app.module_map;
    let name = app
        .project
        .architecture
        .node(&state.crate_id)
        .map_or("?", |n| n.name.as_str());
    let rows = Layout::vertical([Constraint::Length(2), Constraint::Min(5)]).split(area);
    frame.render_widget(
        Paragraph::new(vec![
            components::section_header(&format!("Workspace > {name} > Modules")),
            Line::styled(
                format!(
                    "Scope: {} · mod ownership ≠ imports ≠ calls",
                    state
                        .graph
                        .as_ref()
                        .and_then(|g| g.node(&state.focus))
                        .map_or("loading...", |n| n.qualified_path.as_str())
                ),
                Style::default().fg(theme::MUTED),
            ),
        ]),
        rows[0],
    );
    let panes =
        Layout::horizontal([Constraint::Percentage(56), Constraint::Percentage(44)]).split(rows[1]);
    let Some(g) = &state.graph else {
        frame.render_widget(components::empty_state("MODULE MAP",state.error.as_deref().unwrap_or("Loading module declarations...\nRepository scan / bounded syntax analysis in progress.\nRA is independent; q remains available.")).block(components::styled_panel(" MODULE MAP ",true)),rows[1]);
        return;
    };
    let choices = state.choices();
    let mut items: Vec<_> = choices
        .iter()
        .map(|n| {
            let parent = n.id == state.focus;
            let marker = if n.status == ModuleStatus::Unresolved {
                " [?]"
            } else if !n.conditions.is_empty() {
                " [cfg]"
            } else {
                ""
            };
            ListItem::new(Line::from(vec![
                Span::styled(
                    if parent { "[ " } else { "  ├─mod▶ " },
                    Style::default().fg(theme::ACCENT),
                ),
                Span::raw(format!(
                    "{}{}{}",
                    components::compact(
                        if parent { &n.qualified_path } else { &n.name },
                        panes[0].width.saturating_sub(22) as usize
                    ),
                    if parent { " ]" } else { "" },
                    marker
                )),
            ]))
        })
        .collect();
    let selected = g.node(&state.selected);
    if let Some(n) = selected {
        if n.status == ModuleStatus::Unresolved {
            items.push(ListItem::new(Line::styled(
                "[?] Module contents unknown · see details",
                Style::default().fg(theme::WARNING),
            )));
        } else if g.children(&n.id).next().is_none() {
            items.push(ListItem::new(Line::styled(
                if g.unresolved_by_module
                    .get(&n.id)
                    .is_some_and(|c| c.other > 0)
                {
                    "No known children · o source · [?] details"
                } else {
                    "Leaf module · o opens exact source"
                },
                Style::default().fg(theme::MUTED),
            )));
        }
        items.push(ListItem::new(components::section_header(
            "SELECTED IMPORTS · syntax, not calls",
        )));
        let imports: std::collections::BTreeSet<_> = g
            .verified_import_edges
            .iter()
            .filter(|e| e.from == n.id)
            .map(|e| e.to.as_str())
            .collect();
        for to in imports.iter().take(8) {
            if let Some(dest) = g.node(to) {
                items.push(ListItem::new(Line::styled(
                    format!(
                        "  ┄use▶ {}{}",
                        dest.qualified_path,
                        if choices.iter().any(|n| n.id == *to) {
                            ""
                        } else {
                            " [outside scope]"
                        }
                    ),
                    Style::default().fg(theme::NUMBER),
                )));
            }
        }
        if imports.len() > 8 {
            items.push(ListItem::new(format!(
                "  +{} more · details scroll",
                imports.len() - 8
            )));
        }
        let unresolved = g
            .unresolved_by_module
            .get(&n.id)
            .map_or(0, |c| c.imports + c.other);
        items.push(ListItem::new(Line::styled(
            format!("[?] {unresolved} unresolved · details →"),
            Style::default().fg(theme::WARNING),
        )));
    }
    let index = choices.iter().position(|n| n.id == state.selected);
    let mut selection = ListState::default().with_selected(index);
    let target = g
        .targets
        .iter()
        .find(|t| t.id == state.target_id)
        .map_or("?", |t| t.name.as_str());
    let title = format!(
        " MODULE MAP · {target}{} · t targets ",
        if g.partial { " · PARTIAL" } else { "" }
    );
    let panel = components::styled_panel(&title, app.pane == Pane::MapGraph);
    let inner = panel.inner(panes[0]);
    frame.render_widget(panel, panes[0]);
    let left = Layout::vertical([Constraint::Min(1), Constraint::Length(2)]).split(inner);
    frame.render_widget(
        Paragraph::new(vec![
            Line::styled(
                "─mod▶ contains · ┄use▶ import",
                Style::default().fg(theme::MUTED),
            ),
            Line::styled(
                "[?] unresolved · [cfg] conditional",
                Style::default().fg(theme::WARNING),
            ),
        ]),
        left[1],
    );
    frame.render_stateful_widget(
        List::new(items)
            .highlight_symbol("> ")
            .highlight_style(components::selectable_row()),
        left[0],
        &mut selection,
    );
    frame.render_widget(
        components::scrollable(
            details(app),
            components::styled_panel(" MODULE INSPECTOR ", app.pane == Pane::MapInspector),
            panes[1],
            app.map.inspector_scroll,
            &app.map.scroll_limit,
        ),
        panes[1],
    );
}
fn value(n: Option<impl std::fmt::Display>) -> String {
    n.map_or_else(|| "unknown".into(), |n| n.to_string())
}
fn details(app: &App) -> Vec<Line<'static>> {
    let Some(g) = &app.module_map.graph else {
        return vec![];
    };
    let Some(n) = g.node(&app.module_map.selected) else {
        return vec![Line::raw("No module in this target")];
    };
    let files = g.subtree_files(&n.id);
    let loc = app
        .metrics
        .as_ref()
        .filter(|_| !files.is_empty())
        .and_then(|r| {
            files.iter().try_fold(0u64, |sum, p| {
                r.source_files
                    .iter()
                    .find(|f| f.path == *p)
                    .map(|f| sum + f.nonblank_lines)
            })
        });
    let ast = app
        .maintainability
        .as_ref()
        .filter(|_| !files.is_empty())
        .and_then(|r| {
            files
                .iter()
                .map(|p| {
                    r.files
                        .iter()
                        .find(|f| f.path == *p)
                        .and_then(|f| f.structure.as_ref())
                })
                .collect::<Option<Vec<_>>>()
        });
    let outgoing: std::collections::BTreeSet<_> = g
        .verified_import_edges
        .iter()
        .filter(|e| e.from == n.id)
        .map(|e| e.to.as_str())
        .collect();
    let incoming: std::collections::BTreeSet<_> = g
        .verified_import_edges
        .iter()
        .filter(|e| e.to == n.id)
        .map(|e| e.from.as_str())
        .collect();
    let unresolved: Vec<_> = g
        .unresolved
        .iter()
        .filter(|u| u.module_id == n.id)
        .collect();
    let mut l = vec![
        components::section_header(&n.qualified_path),
        Line::styled(
            format!(
                "{} · {}",
                if n.inline {
                    "inline declaration"
                } else {
                    "file-backed module"
                },
                match n.status {
                    ModuleStatus::Resolved => "resolved",
                    ModuleStatus::DeclaredConditional => "declared conditional",
                    ModuleStatus::Unresolved => "unresolved",
                }
            ),
            Style::default().fg(theme::MUTED),
        ),
        components::path_label(
            n.source_file.as_deref().unwrap_or(&n.declaration_file),
            usize::MAX,
        ),
        Line::raw(format!(
            "Declaration: {}:{}",
            n.declaration_file.display(),
            n.declaration_line
        )),
        components::metric_value(
            "Declared children",
            value((n.status != ModuleStatus::Unresolved).then(|| g.children(&n.id).count())),
        ),
        components::metric_value(
            "Verified imports",
            g.verified_import_edges
                .iter()
                .filter(|e| e.from == n.id)
                .count(),
        ),
        components::metric_value("Imported by modules", incoming.len()),
        components::metric_value(
            "Unresolved imports",
            g.unresolved_by_module.get(&n.id).map_or(0, |c| c.imports),
        ),
        components::metric_value(
            "Other unresolved",
            g.unresolved_by_module.get(&n.id).map_or(0, |c| c.other),
        ),
        Line::raw(""),
        components::section_header("KNOWN PHYSICAL SUBTREE FILES"),
        Line::styled(
            "Deduplicated files, NOT exclusive module contents.",
            Style::default().fg(theme::MUTED),
        ),
        components::metric_value("Physical files", files.len()),
        components::metric_value("File nonblank LOC", value(loc)),
        components::metric_value(
            "File functions",
            value(
                ast.as_ref()
                    .map(|s| s.iter().map(|s| s.functions).sum::<usize>()),
            ),
        ),
        components::metric_value(
            "Longest in files",
            value(ast.as_ref().map(|s| {
                s.iter()
                    .map(|s| s.longest_function_lines)
                    .max()
                    .unwrap_or(0)
            })),
        ),
        components::metric_value(
            "Max depth in files",
            value(
                ast.as_ref()
                    .map(|s| s.iter().map(|s| s.max_control_depth).max().unwrap_or(0)),
            ),
        ),
        components::metric_value(
            "Hotspot files",
            value(app.hotspots.as_ref().filter(|r| r.available).map(|r| {
                r.candidate_paths
                    .iter()
                    .filter(|p| files.contains(*p))
                    .count()
            })),
        ),
    ];
    if n.inline {
        l.push(Line::styled(
            "Inline modules share file LOC/AST; own metrics unknown.",
            Style::default().fg(theme::WARNING),
        ));
    }
    for cfg in &n.conditions {
        l.push(Line::styled(
            format!("[cfg] {cfg} · not evaluated"),
            Style::default().fg(theme::WARNING),
        ));
    }
    l.push(components::section_header("MODULE FILES · o exact source"));
    for file in &files {
        l.push(components::path_label(file, usize::MAX));
    }
    l.push(components::section_header(
        "VERIFIED MODULE IMPORTS · ┄use▶",
    ));
    for id in outgoing {
        if let Some(dest) = g.node(id) {
            l.push(Line::styled(
                dest.qualified_path.clone(),
                Style::default().fg(theme::NUMBER),
            ));
        }
    }
    l.push(components::section_header("IMPORTED BY"));
    for id in incoming {
        if let Some(src) = g.node(id) {
            l.push(Line::raw(src.qualified_path.clone()));
        }
    }
    l.push(components::section_header("[?] UNRESOLVED · NOT INFERRED"));
    let count = g
        .unresolved_by_module
        .get(&n.id)
        .map_or(0, |c| c.imports + c.other);
    if count > unresolved.len() {
        l.push(Line::styled(
            format!(
                "{} of {count} reasons retained (global detail budget)",
                unresolved.len()
            ),
            Style::default().fg(theme::WARNING),
        ));
    }
    for u in unresolved {
        l.push(Line::styled(
            format!("{}:{} {}", u.source_file.display(), u.line, u.reason),
            Style::default().fg(theme::WARNING),
        ));
    }
    l.push(Line::raw(format!(
        "Graph: {} nodes · {} mod · {} use · {} unresolved · {} conditional",
        g.nodes.len(),
        g.containment_edges.len(),
        g.verified_import_edges.len(),
        g.unresolved_count,
        g.conditional_relations
    )));
    l.push(Line::raw(
        "Enter focuses children · o source · t next Cargo target · Esc back",
    ));
    l
}
