//! Thin, cached ModuleGraph adapter for Map. No source reads or AST work here.
mod render;
#[cfg(test)]
mod tests;
use super::{
    app::{App, Pane},
    components, theme,
};
use crate::{
    architecture::Architecture,
    module_graph::{ModuleGraph, ModuleNode},
};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{List, ListItem, ListState, Paragraph},
    Frame,
};

pub(super) struct State {
    pub active: bool,
    pub graph: Option<ModuleGraph>,
    pub error: Option<String>,
    pub crate_id: String,
    pub target_id: String,
    pub focus: String,
    pub selected: String,
    pub history: Vec<(String, String, String)>,
}
impl State {
    pub fn new(a: &Architecture) -> Self {
        Self {
            active: a.nodes.len() == 1,
            graph: None,
            error: None,
            crate_id: a.nodes.first().map_or_else(String::new, |n| n.id.clone()),
            target_id: String::new(),
            focus: String::new(),
            selected: String::new(),
            history: vec![],
        }
    }
    pub fn apply(&mut self, g: ModuleGraph) {
        self.graph = Some(g);
        self.initialize();
    }
    fn initialize(&mut self) {
        let Some(g) = &self.graph else { return };
        if g.node(&self.selected)
            .is_some_and(|n| n.crate_id == self.crate_id)
        {
            return;
        }
        let target = g
            .targets
            .iter()
            .find(|t| t.crate_id == self.crate_id)
            .map(|t| t.id.as_str());
        if let Some(n) = g
            .nodes
            .iter()
            .find(|n| Some(n.target_id.as_str()) == target && n.parent_id.is_none())
        {
            self.target_id = n.target_id.clone();
            self.focus = n.id.clone();
            self.selected = n.id.clone();
        }
    }
    pub fn enter(&mut self, id: &str) {
        if self.crate_id != id {
            self.crate_id = id.into();
            self.selected.clear();
            self.focus.clear();
            self.target_id.clear();
            self.history.clear();
        }
        self.active = true;
        self.initialize();
    }
    pub fn choices(&self) -> Vec<&ModuleNode> {
        let Some(g) = &self.graph else { return vec![] };
        let mut nodes = vec![];
        if let Some(n) = g.node(&self.focus) {
            nodes.push(n);
        }
        nodes.extend(g.children(&self.focus));
        nodes
    }
    pub fn move_by(&mut self, d: isize) {
        let nodes = self.choices();
        let i = nodes
            .iter()
            .position(|n| n.id == self.selected)
            .unwrap_or(0);
        if !nodes.is_empty() {
            self.selected = nodes[i.saturating_add_signed(d).min(nodes.len() - 1)]
                .id
                .clone();
        }
    }
    pub fn focus_selected(&mut self) {
        if self.focus == self.selected || self.selected.is_empty() {
            return;
        }
        self.remember();
        self.focus = self.selected.clone();
    }
    fn remember(&mut self) {
        self.history.push((
            self.target_id.clone(),
            self.focus.clone(),
            self.selected.clone(),
        ));
        if self.history.len() > 50 {
            self.history.remove(0);
        }
    }
    pub fn back(&mut self) {
        if let Some((target, focus, selected)) = self.history.pop() {
            self.target_id = target;
            self.focus = focus;
            self.selected = selected;
        } else {
            self.active = false;
        }
    }
    pub fn next_target(&mut self) {
        let Some(g) = &self.graph else { return };
        let targets: Vec<_> = g
            .targets
            .iter()
            .filter(|t| t.crate_id == self.crate_id)
            .collect();
        if targets.len() < 2 {
            return;
        }
        let i = targets
            .iter()
            .position(|t| t.id == self.target_id)
            .unwrap_or(0);
        let next = &targets[(i + 1) % targets.len()];
        let Some(n) = g
            .nodes
            .iter()
            .find(|n| n.target_id == next.id && n.parent_id.is_none())
        else {
            return;
        };
        let id = n.id.clone();
        let target = next.id.clone();
        self.remember();
        self.target_id = target;
        self.focus = id.clone();
        self.selected = id;
    }
}
pub(super) fn render(frame: &mut Frame<'_>, area: Rect, app: &App) {
    render::render(frame, area, app)
}
