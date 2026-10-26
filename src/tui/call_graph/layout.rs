//! Deterministic five-column layout; bounded grid routing never crosses node boxes.
use super::model::{Identity, State};
use ratatui::layout::Rect;
use std::collections::{BTreeMap, VecDeque};
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Key {
    pub width: u16,
    pub height: u16,
    pub revision: u64,
    pub page: usize,
}
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(super) struct Drawing {
    pub boxes: BTreeMap<Identity, Rect>,
    pub routes: Vec<(Vec<(u16, u16)>, bool)>,
    pub stubs: usize,
    pub cycles: usize,
}
pub(super) fn layout(s: &State, w: u16, h: u16) -> Drawing {
    let mut d = Drawing::default();
    if w < 80 || h < 12 {
        let visible = s.visible();
        for (a, b) in &s.edges {
            if a == b && visible.contains(a) {
                d.cycles += 1;
            } else if !visible.contains(a) || !visible.contains(b) {
                d.stubs += 1;
            }
        }
        return d;
    }
    let cw = ((w - 12) / 5).min(24);
    let gap = (w - 5 * cw) / 4;
    for id in s.visible() {
        let n = &s.nodes[&id];
        let col = (n.layer + 2) as u16;
        let row = if n.layer == 0 {
            h.saturating_sub(3) / 2
        } else {
            let ids: Vec<_> = s
                .visible()
                .into_iter()
                .filter(|k| s.nodes[k].layer == n.layer)
                .collect();
            let at = ids.iter().position(|k| k == &id).unwrap() as u16;
            (h - 3) * (at + 1) / (ids.len() as u16 + 1)
        };
        d.boxes.insert(id, Rect::new(col * (cw + gap), row, cw, 3));
    }
    for (a, b) in &s.edges {
        if a == b {
            if d.boxes.contains_key(a) {
                d.cycles += 1;
            }
            continue;
        }
        let (Some(from), Some(to)) = (d.boxes.get(a), d.boxes.get(b)) else {
            d.stubs += 1;
            continue;
        };
        let forward = from.x < to.x;
        let start = (
            if forward {
                from.right()
            } else {
                from.x.saturating_sub(1)
            },
            from.y + 1,
        );
        let end = (
            if forward {
                to.x.saturating_sub(1)
            } else {
                to.right()
            },
            to.y + 1,
        );
        if let Some(path) = route(w, h, start, end, &d.boxes) {
            d.routes.push((path, forward));
        } else {
            d.stubs += 1;
        }
    }
    d
}
fn route(
    w: u16,
    h: u16,
    start: (u16, u16),
    end: (u16, u16),
    boxes: &BTreeMap<Identity, Rect>,
) -> Option<Vec<(u16, u16)>> {
    let allowed =
        |p: (u16, u16)| p.0 < w && p.1 < h && !boxes.values().any(|r| r.contains(p.into()));
    if !allowed(start) || !allowed(end) {
        return None;
    }
    let index = |p: (u16, u16)| p.1 as usize * w as usize + p.0 as usize;
    let mut previous = vec![None; (w as usize) * (h as usize)];
    let mut queue = VecDeque::from([start]);
    previous[index(start)] = Some(start);
    while let Some(p) = queue.pop_front() {
        if p == end {
            let mut path = vec![p];
            let mut at = p;
            while at != start {
                at = previous[index(at)]?;
                path.push(at);
            }
            path.reverse();
            return Some(path);
        }
        for next in [
            (p.0.checked_add(1), Some(p.1)),
            (Some(p.0), p.1.checked_add(1)),
            (p.0.checked_sub(1), Some(p.1)),
            (Some(p.0), p.1.checked_sub(1)),
        ] {
            if let (Some(x), Some(y)) = next {
                let q = (x, y);
                if allowed(q) && previous[index(q)].is_none() {
                    previous[index(q)] = Some(p);
                    queue.push_back(q);
                }
            }
        }
    }
    None
}
