//! Bounded, deterministic geometry. No repository, parser or semantic operations.
use super::*;
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct NodeBox {
    pub ordinal: usize,
    pub rect: Rect,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Edge {
    pub from: usize,
    pub to: usize,
    pub points: Vec<(u16, u16)>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Drawing {
    pub nodes: Vec<NodeBox>,
    pub edges: Vec<Edge>,
    pub shown: usize,
    pub total: usize,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Key {
    pub width: u16,
    pub height: u16,
    pub focus: String,
    pub page: usize,
    pub mode: Mode,
    pub total: usize,
    pub self_edge: bool,
}
pub(super) fn capacity(width: u16, height: u16) -> usize {
    let cols = (width.saturating_sub(4) / 16).clamp(1, 3) as usize;
    let rows = (height.saturating_sub(3) / 6) as usize;
    (cols * rows).clamp(1, 12)
}
pub(super) fn layout(
    width: u16,
    height: u16,
    total: usize,
    page: usize,
    mode: Mode,
    self_edge: bool,
) -> Option<Drawing> {
    if width < 28 || height < 9 {
        return None;
    }
    let cols = ((width - 4) / 16).clamp(1, 3) as usize;
    let capacity = capacity(width, height);
    let start = page.min(total.saturating_sub(1) / capacity) * capacity;
    let count = total.saturating_sub(start).min(capacity);
    let cw = 22.min(width - 6);
    let center = Rect::new((width - cw) / 2, 0, cw, 3);
    let cx = center.x + cw / 2;
    let mut d = Drawing {
        nodes: vec![NodeBox {
            ordinal: 0,
            rect: center,
        }],
        edges: vec![],
        shown: count,
        total,
    };
    for i in 0..count {
        let row = i / cols;
        let col = i % cols;
        let row_count = cols.min(count - row * cols);
        let w = ((width - 4 - 2 * (row_count as u16 - 1)) / row_count as u16).min(24);
        let used = w * row_count as u16 + 2 * (row_count as u16 - 1);
        let x = (width - used) / 2 + col as u16 * (w + 2);
        let y = 6 + row as u16 * 6;
        let rect = Rect::new(x, y, w, 3);
        let nx = x + w / 2;
        let mut points = if count <= cols {
            vec![(cx, 3), (cx, 4), (nx, 4), (nx, y - 1)]
        } else {
            vec![(cx, 3), (0, 3), (0, y - 2), (nx, y - 2), (nx, y - 1)]
        };
        let ordinal = start + i + 1;
        let (from, to) = if mode == Mode::UsedBy {
            points.reverse();
            (ordinal, 0)
        } else {
            (0, ordinal)
        };
        d.edges.push(Edge { from, to, points });
        d.nodes.push(NodeBox { ordinal, rect });
    }
    if self_edge {
        d.edges.push(Edge {
            from: 0,
            to: 0,
            points: vec![
                (center.right(), 1),
                (width - 1, 1),
                (width - 1, 4),
                (cx, 4),
                (cx, 3),
            ],
        });
    }
    Some(d)
}
pub(super) fn render(frame: &mut Frame<'_>, area: Rect, s: &State) -> bool {
    let choices = s.choices();
    if choices.is_empty() {
        return false;
    }
    let entry = &s.index.nodes[&s.focus];
    let links = match s.mode {
        Mode::Imports => &entry.outgoing,
        Mode::UsedBy => &entry.incoming,
        Mode::Hierarchy => &entry.outgoing,
    };
    let self_edge = s.mode != Mode::Hierarchy && links.iter().any(|e| e.id == s.focus);
    let key = Key {
        width: area.width,
        height: area.height,
        focus: s.focus.clone(),
        page: s.page.get(),
        mode: s.mode,
        total: choices.len() - 1,
        self_edge,
    };
    let mut cache = s.diagram_cache.borrow_mut();
    if cache.as_ref().is_none_or(|(k, _)| *k != key) {
        *cache = Some((
            key.clone(),
            layout(
                area.width,
                area.height,
                key.total,
                key.page,
                s.mode,
                self_edge,
            ),
        ));
    }
    let Some(d) = &cache.as_ref().unwrap().1 else {
        return false;
    };
    let base = if s.mode == Mode::Hierarchy {
        theme::MUTED
    } else {
        theme::NUMBER
    };
    let mut strokes = std::collections::BTreeMap::<(u16, u16), (u8, ratatui::style::Color)>::new();
    let mut arrows = vec![];
    for e in &d.edges {
        let ordinal = if e.from == 0 { e.to } else { e.from };
        let conditional = if s.mode == Mode::Hierarchy {
            !choices[ordinal].conditions.is_empty()
        } else {
            links
                .iter()
                .find(|l| l.id == choices[ordinal].id)
                .is_some_and(|l| l.conditional > 0)
        };
        let color = if conditional { theme::WARNING } else { base };
        for pair in e.points.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            let distance = a.0.abs_diff(b.0) + a.1.abs_diff(b.1);
            let mut previous = a;
            for step in 1..=distance {
                let next = if a.0 == b.0 {
                    (
                        a.0,
                        a.1.min(b.1) + if a.1 < b.1 { step } else { distance - step },
                    )
                } else {
                    (
                        a.0.min(b.0) + if a.0 < b.0 { step } else { distance - step },
                        a.1,
                    )
                };
                let (forward, back) = if next.0 > previous.0 {
                    (2, 1)
                } else if next.0 < previous.0 {
                    (1, 2)
                } else if next.1 > previous.1 {
                    (8, 4)
                } else {
                    (4, 8)
                };
                for (point, mask) in [(previous, forward), (next, back)] {
                    let stroke = strokes.entry(point).or_insert((0, color));
                    stroke.0 |= mask;
                    if conditional {
                        stroke.1 = theme::WARNING;
                    }
                }
                previous = next;
            }
        }
        for (point, ordinal) in [(e.points[0], e.from), (*e.points.last().unwrap(), e.to)] {
            let rect = d.nodes.iter().find(|n| n.ordinal == ordinal).unwrap().rect;
            let mask = if point.0 < rect.x {
                2
            } else if point.0 >= rect.right() {
                1
            } else if point.1 < rect.y {
                8
            } else {
                4
            };
            strokes.entry(point).or_insert((0, color)).0 |= mask;
        }
        let last = e.points[e.points.len() - 1];
        let prev = e.points[e.points.len() - 2];
        arrows.push((last, if last.1 > prev.1 { "▼" } else { "▲" }, color));
    }
    for ((x, y), (mask, color)) in strokes {
        let symbol = match mask {
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
        };
        put(frame, area, x, y, symbol, color);
    }
    for ((x, y), symbol, color) in arrows {
        put(frame, area, x, y, symbol, color);
    }
    for b in &d.nodes {
        let n = choices[b.ordinal];
        let is_selected = n.id == s.selected;
        let losses = s.index.nodes.get(&n.id).is_some_and(|e| e.losses > 0);
        let color = if is_selected {
            theme::ACTIVE
        } else if losses {
            theme::ERROR
        } else {
            theme::ACCENT
        };
        let badge = if b.ordinal == 0 {
            if let Some(l) = links.iter().find(|l| self_edge && l.id == s.focus) {
                format!("0 focus ↺×{}", l.records)
            } else {
                "0 · focus".into()
            }
        } else if s.mode == Mode::Hierarchy {
            format!(
                "{} · mod{}",
                b.ordinal,
                if n.conditions.is_empty() {
                    ""
                } else {
                    " [cfg]"
                }
            )
        } else {
            let l = links.iter().find(|l| l.id == n.id).unwrap();
            format!(
                "{} · ×{}{}",
                b.ordinal,
                l.records,
                if l.conditional > 0 {
                    format!(" [cfg]{}", l.conditional)
                } else {
                    String::new()
                }
            )
        };
        let title = format!("{}{}", if is_selected { ">" } else { "" }, badge);
        let block = if b.ordinal == 0 {
            ratatui::widgets::Block::bordered().title_top(title)
        } else {
            ratatui::widgets::Block::bordered().title_bottom(title)
        };
        let rect = Rect::new(
            area.x + b.rect.x,
            area.y + b.rect.y,
            b.rect.width,
            b.rect.height,
        );
        frame.render_widget(
            Paragraph::new(components::compact(
                if b.ordinal == 0 {
                    &n.qualified_path
                } else {
                    &n.name
                },
                b.rect.width.saturating_sub(2) as usize,
            ))
            .style(Style::default().fg(color))
            .block(block.border_style(Style::default().fg(color))),
            rect,
        );
        if !d.edges.is_empty() {
            put(
                frame,
                area,
                b.rect.x + b.rect.width / 2,
                if b.ordinal == 0 {
                    b.rect.bottom() - 1
                } else {
                    b.rect.y
                },
                if b.ordinal == 0 { "┬" } else { "┴" },
                color,
            );
        }
    }
    if d.total == 0 {
        let text = if self_edge {
            "Verified self-import ↺\nNo other verified neighbors.\nv switches views · o source"
        } else {
            match s.mode{Mode::Hierarchy=>"No known child modules. o opens source.",Mode::Imports=>"No verified outgoing imports.\nSome imports may be unclassified.\nv switches views · o source",Mode::UsedBy=>"No verified incoming imports.\nNot semantic usage analysis.\nv switches views · o source"}
        };
        let y = if self_edge { 5 } else { 4 };
        frame.render_widget(
            Paragraph::new(text)
                .style(Style::default().fg(theme::MUTED))
                .wrap(ratatui::widgets::Wrap { trim: false }),
            Rect::new(
                area.x + 2,
                area.y + y,
                area.width.saturating_sub(4),
                area.height.saturating_sub(y),
            ),
        );
    }
    true
}
fn put(frame: &mut Frame<'_>, area: Rect, x: u16, y: u16, ch: &str, color: ratatui::style::Color) {
    if x < area.width && y < area.height {
        frame.buffer_mut()[(area.x + x, area.y + y)]
            .set_symbol(ch)
            .set_fg(color);
    }
}
