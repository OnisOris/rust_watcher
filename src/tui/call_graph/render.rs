use super::layout;
use crate::tui::{
    app::{AnalyzerState, App, Pane},
    components, theme,
};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::Style,
    text::Line,
    widgets::{Block, Paragraph, Wrap},
    Frame,
};
pub(in crate::tui) fn render(f: &mut Frame<'_>, area: Rect, app: &App) {
    let s = &app.calls.graph;
    let Some(focus) = s.focus.as_ref().and_then(|id| s.nodes.get(id)) else {
        return;
    };
    let rows = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(1),
        Constraint::Length(3),
    ])
    .split(area);
    let selected = s.selected_target().unwrap_or(&focus.target);
    f.render_widget(
        Paragraph::new(vec![
            components::section_header(&format!(
                "CALL GRAPH · {} · depth {}/2",
                app.calls.scope.label(),
                s.nodes.values().map(|n| n.layer.abs()).max().unwrap_or(0)
            )),
            Line::raw(format!(
                "Focus: {} · {}:{}:{}",
                focus.target.name,
                focus.target.file.display(),
                focus.target.selection_range.start.line + 1,
                focus.target.selection_range.start.character + 1
            )),
            Line::styled(
                format!(
                    "Selected: {} · {}:{}:{}",
                    selected.name,
                    selected.file.display(),
                    selected.selection_range.start.line + 1,
                    selected.selection_range.start.character + 1
                ),
                Style::default().fg(theme::ACTIVE),
            ),
        ]),
        rows[0],
    );
    let panes = if area.width >= 120 {
        Layout::horizontal([Constraint::Percentage(78), Constraint::Percentage(22)]).split(rows[1])
    } else {
        Layout::horizontal([Constraint::Percentage(100), Constraint::Length(0)]).split(rows[1])
    };
    s.details_visible.set(panes[1].width > 0);
    let panel = components::styled_panel(
        " CALLERS → FOCUS → CALLEES · RA verified ",
        app.pane == Pane::CallCurrent || !s.details_visible.get(),
    );
    let inner = panel.inner(panes[0]);
    f.render_widget(panel, panes[0]);
    s.viewport(inner.width, inner.height);
    let key = layout::Key {
        width: inner.width,
        height: inner.height,
        revision: s.revision,
        page: s.page.get(),
    };
    let mut cache = s.cache.borrow_mut();
    if cache.as_ref().is_none_or(|(old, _)| *old != key) {
        *cache = Some((key, layout::layout(s, inner.width, inner.height)));
    }
    let drawing = &cache.as_ref().unwrap().1;
    if drawing.boxes.is_empty() {
        let mut lines = vec![Line::styled(
            "Graph fallback: compact directed List",
            Style::default().fg(theme::WARNING),
        )];
        for id in s.visible() {
            let node = &s.nodes[&id];
            let relationship = if let Some(parent) = &node.via {
                let (a, b) = if node.layer < 0 {
                    (&id, parent)
                } else {
                    (parent, &id)
                };
                if s.edges.contains(&(a.clone(), b.clone())) {
                    let label = |id: &super::Identity| {
                        let t = &s.nodes[id].target;
                        if t.is_workspace_local(&app.project.workspace_root) {
                            t.name.clone()
                        } else {
                            format!("[ext] {}", t.name)
                        }
                    };
                    format!("{} → {}", label(a), label(b))
                } else {
                    format!("{} · edge omitted (budget)", node.target.name)
                }
            } else {
                format!("◎ focus {}", node.target.name)
            };
            lines.push(Line::raw(format!(
                "{}{}",
                if Some(&id) == s.selected.as_ref() {
                    ">"
                } else {
                    " "
                },
                relationship
            )));
        }
        f.render_widget(Paragraph::new(lines), inner);
    } else {
        let mut strokes = std::collections::BTreeMap::<(u16, u16), u8>::new();
        for (path, _) in &drawing.routes {
            for p in path.windows(2) {
                let (a, b) = (p[0], p[1]);
                let (forward, back) = if b.0 > a.0 {
                    (2, 1)
                } else if b.0 < a.0 {
                    (1, 2)
                } else if b.1 > a.1 {
                    (8, 4)
                } else {
                    (4, 8)
                };
                *strokes.entry(a).or_default() |= forward;
                *strokes.entry(b).or_default() |= back;
            }
        }
        for ((x, y), mask) in strokes {
            put(
                f,
                inner,
                x,
                y,
                match mask {
                    3 => "─",
                    12 => "│",
                    10 => "┌",
                    9 => "┐",
                    6 => "└",
                    5 => "┘",
                    11 => "┬",
                    7 => "┴",
                    14 => "├",
                    13 => "┤",
                    15 => "┼",
                    1 | 2 => "─",
                    _ => "│",
                },
                theme::NUMBER,
            );
        }
        for (id, r) in &drawing.boxes {
            let n = &s.nodes[id];
            let chosen = Some(id) == s.selected.as_ref();
            let center = n.layer == 0;
            let color = if chosen { theme::ACTIVE } else { theme::ACCENT };
            let label = format!(
                "{}{}",
                if chosen { ">" } else { "" },
                if center {
                    "◎ focus"
                } else if n.layer.abs() == 2 {
                    "L2"
                } else {
                    "L1"
                }
            );
            let rect = Rect::new(inner.x + r.x, inner.y + r.y, r.width, r.height);
            let label_name = if n.target.is_workspace_local(&app.project.workspace_root) {
                n.target.name.clone()
            } else {
                format!("[ext] {}", n.target.name)
            };
            f.render_widget(
                Paragraph::new(components::compact(
                    &label_name,
                    r.width.saturating_sub(2) as usize,
                ))
                .style(Style::default().fg(color))
                .block(
                    Block::bordered()
                        .title(label)
                        .title_bottom(components::compact(
                            &format!(
                                "{}:{}",
                                n.target
                                    .file
                                    .file_name()
                                    .unwrap_or_default()
                                    .to_string_lossy(),
                                n.target.selection_range.start.line + 1
                            ),
                            r.width.saturating_sub(2) as usize,
                        ))
                        .border_style(Style::default().fg(color)),
                ),
                rect,
            );
        }
        for (path, forward) in &drawing.routes {
            if let Some(end) = path.last() {
                put(
                    f,
                    inner,
                    end.0,
                    end.1,
                    if *forward { "▶" } else { "◀" },
                    theme::NUMBER,
                );
            }
        }
    }
    let node = s
        .selected
        .as_ref()
        .and_then(|id| s.nodes.get(id))
        .unwrap_or(focus);
    if panes[1].width > 0 {
        f.render_widget(
            components::scrollable(
                vec![
                    components::section_header("SELECTED SYMBOL"),
                    Line::raw(selected.name.clone()),
                    Line::raw(selected.detail.clone().unwrap_or_default()),
                    components::path_label(&selected.file, usize::MAX),
                    Line::raw(format!(
                        "Line {} · {}",
                        selected.selection_range.start.line + 1,
                        selected.kind
                    )),
                    Line::raw(""),
                    Line::raw(format!("Incoming: {}", node.sides[0].label())),
                    Line::raw(format!("Outgoing: {}", node.sides[1].label())),
                    Line::raw(""),
                    Line::raw("x expand direct neighbor"),
                    Line::raw("Enter focus · o source"),
                    Line::raw("g List · e scope"),
                    Line::raw(format!("{} confirmed edges", s.edges.len())),
                    Line::raw(format!("{} ↺ recursive edges", drawing.cycles)),
                    Line::raw(format!("{} edges outside page / ↻ repeated", drawing.stubs)),
                ],
                components::styled_panel(" DETAILS · Tab / j/k scroll ", app.pane == Pane::Callees),
                panes[1],
                app.calls.context_scroll,
                &app.calls.context_scroll_limit,
            ),
            panes[1],
        );
    }
    f.render_widget(
        Paragraph::new(vec![
            Line::raw(format!(
                "Page {}/{} · Showing {} of {} retained nodes · PgUp/PgDn",
                s.page.get() + 1,
                s.pages(),
                s.visible().len(),
                s.nodes.len()
            )),
            Line::styled(
                if s.limited {
                    "Graph budget reached: additional known results omitted".into()
                } else {
                    format!(
                        "Callers: {} · Callees: {}",
                        focus.sides[0].label(),
                        focus.sides[1].label()
                    )
                },
                Style::default().fg(if s.limited {
                    theme::WARNING
                } else {
                    theme::MUTED
                }),
            ),
            Line::raw(app.notice.clone().unwrap_or_else(|| {
                if let AnalyzerState::Error(reason) = &app.analyzer_state {
                    return format!("RA unavailable: {reason} · cached graph retained");
                }
                if app.analyzer_state != AnalyzerState::Ready {
                    return "RA indexing · results provisional · retry expansion after Ready"
                        .into();
                }
                format!(
                    "{} · {} ↺ recursive · {} off-page/repeated edges",
                    if s.pending.is_some() {
                        "Expansion: loading"
                    } else {
                        "RA call hierarchy · no inferred cfg"
                    },
                    drawing.cycles,
                    drawing.stubs
                )
            })),
        ])
        .wrap(Wrap { trim: false }),
        rows[2],
    );
}
fn put(f: &mut Frame<'_>, area: Rect, x: u16, y: u16, text: &str, color: ratatui::style::Color) {
    if x < area.width && y < area.height {
        f.buffer_mut()[(area.x + x, area.y + y)]
            .set_symbol(text)
            .set_fg(color);
    }
}
