//! Explicit branch pages. Expanded nodes retain their direct parent as an anchor.
use super::model::{Identity, State};
impl State {
    // An explicit expansion can insert/remove overflow pages before the current
    // branch. Keep the selected identity visible, rather than visiting another branch.
    pub(super) fn reconcile_page(&self) {
        let pages = self.plan();
        let current = self.page.get().min(pages.len() - 1);
        let visible = self
            .selected
            .as_ref()
            .is_none_or(|id| pages[current].contains(id));
        self.page.set(if visible {
            current
        } else {
            pages
                .iter()
                .position(|ids| self.selected.as_ref().is_some_and(|id| ids.contains(id)))
                .unwrap_or(current)
        });
    }
    pub(super) fn plan(&self) -> Vec<Vec<Identity>> {
        let rows = self.rows.get().max(1);
        let incoming = self.neighbors(-1);
        let outgoing = self.neighbors(1);
        let bases = incoming.len().max(outgoing.len()).div_ceil(rows).max(1);
        let mut pages = vec![];
        for base in 0..bases {
            let parents: Vec<_> = incoming
                .iter()
                .skip(base * rows)
                .take(rows)
                .chain(outgoing.iter().skip(base * rows).take(rows))
                .cloned()
                .collect();
            let children = |layer| {
                self.nodes
                    .iter()
                    .filter(|(_, n)| {
                        n.layer == layer && n.via.as_ref().is_some_and(|p| parents.contains(p))
                    })
                    .map(|(id, _)| id.clone())
                    .collect::<Vec<_>>()
            };
            let left = children(-2);
            let right = children(2);
            for sub in 0..left.len().max(right.len()).div_ceil(rows).max(1) {
                pages.push(
                    self.focus
                        .iter()
                        .cloned()
                        .chain(parents.iter().cloned())
                        .chain(left.iter().skip(sub * rows).take(rows).cloned())
                        .chain(right.iter().skip(sub * rows).take(rows).cloned())
                        .collect(),
                );
            }
        }
        pages
    }
    pub fn pages(&self) -> usize {
        self.plan().len()
    }
    pub fn visible(&self) -> Vec<Identity> {
        let pages = self.plan();
        pages
            .get(self.page.get().min(pages.len() - 1))
            .cloned()
            .unwrap_or_default()
    }
}
