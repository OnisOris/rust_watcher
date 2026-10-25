use super::*;
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn render(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let model = &app.project.architecture;
    let panel = components::styled_panel(
        if model.partial {
            " ARCHITECTURE · PARTIAL "
        } else if area.width < 60 {
            " ARCHITECTURE · A → B "
        } else {
            " ARCHITECTURE MAP · workspace crates "
        },
        app.pane == Pane::MapGraph,
    );
    let inner = panel.inner(area);
    frame.render_widget(panel, area);
    if model.nodes.is_empty() {
        frame.render_widget(
            components::empty_state(
                "No normalized workspace crates",
                "Cargo metadata did not provide a usable workspace node.",
            ),
            inner,
        );
        return;
    }
    let rows = Layout::vertical([
        Constraint::Length(2),
        Constraint::Min(3),
        Constraint::Length(2),
    ])
    .split(inner);
    let focus = app.map.focus.as_ref().unwrap_or(&app.map.selected);
    let global = app.map.focus.is_none() && model.nodes.len() <= 10;
    let ids = if global {
        model.nodes.iter().map(|n| n.id.clone()).collect()
    } else {
        model.neighborhood(focus)
    };
    frame.render_widget(
        Paragraph::new(vec![
            Line::styled(
                format!(
                    "Showing {} of {} workspace crates{}",
                    ids.len(),
                    model.nodes.len(),
                    if global {
                        " · workspace"
                    } else {
                        " · one hop"
                    }
                ),
                Style::default().fg(theme::MUTED),
            ),
            Line::raw(format!(
                "Selected: {}",
                model
                    .node(&app.map.selected)
                    .map_or("unknown", |n| n.name.as_str())
            )),
        ]),
        rows[0],
    );
    if !global || !layered(frame, rows[1], app) {
        adjacency(
            frame,
            rows[1],
            app,
            &ids,
            if global { None } else { Some(focus) },
        );
    }
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(vec![
                Span::styled(
                    "─n→ normal  ",
                    Style::default().fg(edge_color(DependencyKind::Normal)),
                ),
                Span::styled(
                    "┄d→ dev  ",
                    Style::default().fg(edge_color(DependencyKind::Dev)),
                ),
                Span::styled(
                    "═b⇒ build",
                    Style::default().fg(edge_color(DependencyKind::Build)),
                ),
            ]),
            Line::styled(
                "A → B: A depends on B · declared dependencies",
                Style::default().fg(theme::MUTED),
            ),
        ]),
        rows[2],
    );
}

fn caption(model: &Architecture, id: &str, width: usize) -> String {
    model.nodes.iter().position(|n| n.id == id).map_or_else(
        || "[unknown]".into(),
        |i| {
            format!(
                "#{:02} {}",
                i + 1,
                components::compact(&model.nodes[i].name, width.saturating_sub(4))
            )
        },
    )
}
fn adjacency(
    frame: &mut Frame<'_>,
    area: Rect,
    app: &App,
    ids: &BTreeSet<String>,
    focus: Option<&String>,
) {
    let model = &app.project.architecture;
    let width = area.width.saturating_sub(11) as usize / 2;
    let mut edges: Vec<_> = model
        .edges
        .iter()
        .filter(|e| {
            ids.contains(&e.from)
                && ids.contains(&e.to)
                && focus.is_none_or(|id| &e.from == id || &e.to == id)
        })
        .collect();
    // Keep outgoing branches directly beneath their parent. Interleaving incoming
    // edges here would visually attach a dependency to the preceding dependent.
    edges.sort_by_key(|e| focus.is_some_and(|id| &e.from != id));
    let outgoing = edges
        .iter()
        .take_while(|e| focus.is_some_and(|id| &e.from == id))
        .count();
    let center_selected = focus.is_some_and(|id| id == &app.map.selected);
    let mut selection = center_selected.then_some(0);
    let mut items = Vec::new();
    if let Some(id) = focus {
        items.push(ListItem::new(Line::styled(
            format!(
                "[ {} ]",
                caption(model, id, area.width.saturating_sub(6) as usize)
            ),
            components::selectable_row(),
        )));
    }
    for (index, edge) in edges.into_iter().enumerate() {
        if !center_selected && (app.map.selected == edge.from || app.map.selected == edge.to) {
            selection.get_or_insert(items.len());
        }
        let mut lines = Vec::new();
        if focus.is_some_and(|id| id == &edge.from) {
            lines.push(Line::from(vec![
                Span::styled(
                    format!(
                        "  {}{}",
                        if index + 1 == outgoing { "└" } else { "├" },
                        edge_mark(edge.kind)
                    ),
                    Style::default().fg(edge_color(edge.kind)),
                ),
                Span::styled(
                    format!(
                        " [{}]",
                        caption(model, &edge.to, area.width.saturating_sub(12) as usize)
                    ),
                    Style::default().fg(if app.map.selected == edge.to {
                        theme::ACTIVE
                    } else {
                        theme::TEXT
                    }),
                ),
            ]));
        } else {
            let from = caption(model, &edge.from, width);
            let to = if focus.is_some() {
                String::from("focus")
            } else {
                caption(model, &edge.to, width)
            };
            lines.push(Line::from(vec![
                Span::styled(
                    format!("[{from}] "),
                    Style::default().fg(if app.map.selected == edge.from {
                        theme::ACTIVE
                    } else {
                        theme::TEXT
                    }),
                ),
                Span::styled(
                    edge_mark(edge.kind),
                    Style::default().fg(edge_color(edge.kind)),
                ),
                Span::styled(
                    format!(" [{to}]"),
                    Style::default().fg(if app.map.selected == edge.to {
                        theme::ACTIVE
                    } else {
                        theme::TEXT
                    }),
                ),
            ]));
        }
        let target_name = model.node(&edge.to).map_or("", |n| n.name.as_str());
        if edge.alias != target_name || edge.target.is_some() || edge.optional {
            lines.push(Line::styled(
                format!(
                    "    as {}{}{}",
                    edge.alias,
                    edge.target
                        .as_ref()
                        .map_or_else(String::new, |t| format!(" · {t}")),
                    if edge.optional { " · optional" } else { "" }
                ),
                Style::default().fg(theme::MUTED),
            ));
        }
        items.push(ListItem::new(lines));
    }
    for node in model.nodes.iter().filter(|n| {
        ids.contains(&n.id) && model.edges.iter().all(|e| e.from != n.id && e.to != n.id)
    }) {
        if app.map.selected == node.id {
            selection = Some(items.len());
        }
        items.push(ListItem::new(vec![
            Line::raw(format!(
                "[{}]",
                caption(model, &node.id, area.width.saturating_sub(4) as usize)
            )),
            Line::styled(
                "  no internal dependencies or dependents",
                Style::default().fg(theme::MUTED),
            ),
        ]));
    }
    let mut state = ListState::default().with_selected(selection);
    frame.render_stateful_widget(
        List::new(items)
            .highlight_symbol("> ")
            .highlight_style(components::selectable_row()),
        area,
        &mut state,
    );
}

/// Bounded deterministic Kahn layers. Cycles/dense/parallel edges use adjacency.
fn layered(frame: &mut Frame<'_>, area: Rect, app: &App) -> bool {
    let m = &app.project.architecture;
    let mut indegree: BTreeMap<_, usize> = m.nodes.iter().map(|n| (n.id.clone(), 0)).collect();
    let mut pairs = BTreeSet::new();
    for e in &m.edges {
        if !pairs.insert((&e.from, &e.to)) {
            return false;
        }
        let Some(n) = indegree.get_mut(&e.to) else {
            return false;
        };
        *n += 1;
    }
    let mut pending: BTreeSet<_> = indegree
        .iter()
        .filter(|(_, n)| **n == 0)
        .map(|(id, _)| id.clone())
        .collect();
    let mut level: BTreeMap<_, usize> = m.nodes.iter().map(|n| (n.id.clone(), 0)).collect();
    let mut visited = 0;
    while let Some(id) = pending.pop_first() {
        visited += 1;
        for e in m.outgoing(&id) {
            let next = level[&id] + 1;
            level
                .entry(e.to.clone())
                .and_modify(|n| *n = (*n).max(next));
            let Some(count) = indegree.get_mut(&e.to) else {
                return false;
            };
            *count -= 1;
            if *count == 0 {
                pending.insert(e.to.clone());
            }
        }
    }
    if visited != m.nodes.len() {
        return false;
    }
    let columns = level.values().max().copied().unwrap_or(0) + 1;
    let cell = area.width as usize / columns;
    let box_width = cell.saturating_sub(4).min(25);
    if box_width < 12 {
        return false;
    }
    let mut groups = BTreeMap::<usize, Vec<&str>>::new();
    for n in &m.nodes {
        groups.entry(level[&n.id]).or_default().push(&n.id);
    }
    if groups.values().any(|g| g.len() * 4 > area.height as usize) {
        return false;
    }
    let mut positions = BTreeMap::new();
    for (col, nodes) in groups {
        for (row, id) in nodes.iter().enumerate() {
            positions.insert(
                *id,
                Rect::new(
                    area.x + (col * cell + (cell - box_width) / 2) as u16,
                    area.y + ((area.height as usize - (nodes.len() * 4 - 1)) / 2 + row * 4) as u16,
                    box_width as u16,
                    3,
                ),
            );
        }
    }
    for e in &m.edges {
        let (Some(a), Some(b)) = (positions.get(e.from.as_str()), positions.get(e.to.as_str()))
        else {
            return false;
        };
        let x = a.right();
        let end = b.x.saturating_sub(1);
        if end <= x {
            return false;
        }
        let ay = a.y + 1;
        let by = b.y + 1;
        let turn = x + 1;
        let horizontal = match e.kind {
            DependencyKind::Normal => "─",
            DependencyKind::Dev => "┄",
            DependencyKind::Build => "═",
        };
        for col in x..end {
            put(frame, area, col, ay, horizontal, edge_color(e.kind));
        }
        for row in ay.min(by)..=ay.max(by) {
            put(frame, area, turn, row, "│", edge_color(e.kind));
        }
        for col in turn..end {
            put(frame, area, col, by, horizontal, edge_color(e.kind));
        }
        if ay != by {
            put(
                frame,
                area,
                turn,
                ay,
                if by > ay { "┐" } else { "┘" },
                edge_color(e.kind),
            );
            put(
                frame,
                area,
                turn,
                by,
                if by > ay { "└" } else { "┌" },
                edge_color(e.kind),
            );
        } else {
            put(frame, area, turn, ay, horizontal, edge_color(e.kind));
        }
        put(frame, area, end, by, "▶", edge_color(e.kind));
        put(
            frame,
            area,
            end.saturating_sub(1),
            by,
            match e.kind {
                DependencyKind::Normal => "n",
                DependencyKind::Dev => "d",
                DependencyKind::Build => "b",
            },
            edge_color(e.kind),
        );
    }
    for n in &m.nodes {
        let rect = positions[&n.id.as_str()];
        let active = n.id == app.map.selected;
        frame.render_widget(Clear, rect);
        frame.render_widget(
            Paragraph::new(Line::styled(
                format!(
                    "{}{}",
                    if active { ">" } else { " " },
                    caption(m, &n.id, box_width.saturating_sub(3))
                ),
                if active {
                    components::selectable_row()
                } else {
                    Style::default().fg(theme::TEXT)
                },
            ))
            .block(components::styled_panel("", active)),
            rect,
        );
    }
    true
}
fn put(
    frame: &mut Frame<'_>,
    area: Rect,
    x: u16,
    y: u16,
    symbol: &str,
    color: ratatui::style::Color,
) {
    if x < area.right() && y < area.bottom() {
        frame.buffer_mut()[(x, y)].set_symbol(symbol).set_fg(color);
    }
}
