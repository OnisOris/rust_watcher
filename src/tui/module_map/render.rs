use super::*;
use crate::module_graph::ModuleStatus;
pub(super) fn render(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let s = &app.module_map;
    let name = app
        .project
        .architecture
        .node(&s.crate_id)
        .map_or("?", |n| n.name.as_str());
    let rows = Layout::vertical([Constraint::Length(3), Constraint::Min(3)]).split(area);
    let scope = s
        .graph
        .as_ref()
        .and_then(|g| g.node(&s.focus))
        .map_or("loading...", |n| n.qualified_path.as_str());
    let tabs = [Mode::Hierarchy, Mode::Imports, Mode::UsedBy]
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
        .collect::<Vec<_>>();
    let status = s.graph.as_ref().map_or_else(
        || {
            Line::styled(
                if s.error.is_some() {
                    "Coverage: Error"
                } else {
                    "Coverage: loading"
                },
                Style::default().fg(if s.error.is_some() {
                    theme::ERROR
                } else {
                    theme::MUTED
                }),
            )
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
            components::section_header(&format!("Workspace > {name} > Modules")),
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
    let left = Layout::vertical([
        Constraint::Length(2),
        Constraint::Min(1),
        Constraint::Length(2),
    ])
    .split(inner);
    frame.render_widget(
        Paragraph::new(vec![
            Line::styled(
                format!(
                    "{}[0] {}",
                    if s.selected == s.focus { "> " } else { "  " },
                    components::compact(scope, inner.width.saturating_sub(5) as usize)
                ),
                Style::default().fg(theme::ACCENT),
            ),
            Line::styled(
                match s.mode {
                    Mode::Hierarchy => "   │ declares child modules",
                    Mode::Imports => "   │ imports → neighbors (syntax)",
                    Mode::UsedBy => "   ▲ imported by neighbors (syntax)",
                },
                Style::default().fg(theme::MUTED),
            ),
        ]),
        left[0],
    );
    let choices = s.choices();
    let items: Vec<_> = choices
        .iter()
        .enumerate()
        .map(|(i, n)| {
            let label = if i == 0 {
                format!("[0] {} (focus)", n.qualified_path)
            } else if s.mode == Mode::Hierarchy {
                format!("├─mod▶ {}", n.name)
            } else {
                let e = &s.index.nodes[&s.focus];
                let links = if s.mode == Mode::Imports {
                    &e.outgoing
                } else {
                    &e.incoming
                };
                let link = links
                    .iter()
                    .find(|l| l.id == n.id)
                    .expect("indexed neighbor");
                let name =
                    components::compact(&n.qualified_path, inner.width.saturating_sub(17) as usize);
                let relation = if s.mode == Mode::Imports {
                    format!("├─use▶ {name}")
                } else {
                    format!("{name} ─use▶ [0]")
                };
                relation
                    + &format!(
                        "\n   {} record{}{}",
                        link.records,
                        if link.records == 1 { "" } else { "s" },
                        if link.conditional > 0 {
                            format!(" · [cfg] {}", link.conditional)
                        } else {
                            String::new()
                        }
                    )
            };
            let marker = if n.status == ModuleStatus::Unresolved {
                " [?]"
            } else if !n.conditions.is_empty() && (i == 0 || s.mode == Mode::Hierarchy) {
                " [cfg]"
            } else {
                ""
            };
            ListItem::new(
                format!("{label}{marker}")
                    .lines()
                    .map(|l| {
                        Line::styled(
                            l.to_owned(),
                            Style::default().fg(if s.mode == Mode::Hierarchy {
                                theme::ACCENT
                            } else {
                                theme::NUMBER
                            }),
                        )
                    })
                    .collect::<Vec<_>>(),
            )
        })
        .collect();
    let selected = choices.iter().position(|n| n.id == s.selected);
    let total = choices.len().saturating_sub(1);
    let visible =
        left[1].height.saturating_sub(1) as usize / if s.mode == Mode::Hierarchy { 1 } else { 2 };
    frame.render_widget(
        Paragraph::new(vec![
            Line::styled(
                format!(
                    "Showing window ≤{} of {total} related · j/k",
                    visible.min(total)
                ),
                Style::default().fg(theme::MUTED),
            ),
            Line::styled(
                match s.mode {
                    Mode::Hierarchy => "─mod▶ ownership · v views · o source",
                    Mode::Imports => "[0] ─use▶ neighbor · not calls",
                    Mode::UsedBy => "neighbor ─use▶ [0] · not calls",
                },
                Style::default().fg(theme::MUTED),
            ),
        ]),
        left[2],
    );
    if total == 0 {
        let self_link = s.index.nodes.get(&s.focus).and_then(|e| match s.mode {
            Mode::Imports => e.outgoing.iter().find(|l| l.id == s.focus),
            Mode::UsedBy => e.incoming.iter().find(|l| l.id == s.focus),
            Mode::Hierarchy => None,
        });
        if let Some(link) = self_link {
            frame.render_widget(
                Paragraph::new(format!(
                    "↺ [0] ─use▶ [0]\n{} records · {} conditional\nNo other verified neighbors.",
                    link.records, link.conditional
                ))
                .style(Style::default().fg(theme::NUMBER)),
                left[1],
            );
        } else {
            let reason = match s.mode {
                Mode::Hierarchy => "No known child modules.\no opens exact source.",
                Mode::Imports => {
                    "No verified outgoing imports.\nUnclassified does not mean no coupling."
                }
                Mode::UsedBy => {
                    "No verified incoming imports.\nThis is not semantic usage analysis."
                }
            };
            frame.render_widget(
                Paragraph::new(reason).style(Style::default().fg(theme::MUTED)),
                left[1],
            );
        }
    } else {
        let mut selection = ListState::default().with_selected(selected);
        frame.render_stateful_widget(
            List::new(items)
                .highlight_symbol("> ")
                .highlight_style(components::selectable_row()),
            left[1],
            &mut selection,
        );
    }
    // Sparse, truthful neighborhoods should explain the next action, not invent
    // edges to fill the terminal. Never overlap a scrolling relationship list.
    let used = if total == 0 {
        3
    } else if s.mode == Mode::Hierarchy {
        choices.len()
    } else {
        1 + total * 2
    };
    if left[1].height as usize > used + 9 {
        let hint = Rect::new(
            left[1].x,
            left[1].y + used as u16 + 1,
            left[1].width,
            left[1].height - used as u16 - 1,
        );
        frame.render_widget(
            Paragraph::new(vec![
                components::section_header("CONTINUE EXPLORING"),
                Line::raw("j/k select · Enter focuses that module"),
                Line::raw("v changes view around the selection"),
                Line::raw("o opens its exact source file"),
                Line::raw("Tab / Ctrl+l → scroll Inspector"),
                Line::raw("f files · a limitations · t targets"),
                Line::raw(""),
                Line::styled(
                    "Only direct syntax relations, not calls.",
                    Style::default().fg(theme::MUTED),
                ),
                Line::styled(
                    "Subtree totals are separate in Inspector.",
                    Style::default().fg(theme::MUTED),
                ),
            ]),
            hint,
        );
    }
    frame.render_widget(
        components::scrollable(
            super::inspector::details(app),
            components::styled_panel(" MODULE INSPECTOR ", app.pane == Pane::MapInspector),
            panes[1],
            app.map.inspector_scroll,
            &app.map.scroll_limit,
        ),
        panes[1],
    );
}
