//! Thin, cached ModuleGraph adapter for Map. No source reads or AST work here.
mod diagram;
#[cfg(test)]
mod diagram_tests;
mod index;
mod inspector;
mod list;
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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum Mode {
    #[default]
    Hierarchy,
    Imports,
    UsedBy,
}
impl Mode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Hierarchy => "Hierarchy",
            Self::Imports => "Imports",
            Self::UsedBy => "Used by",
        }
    }
}
pub(super) struct State {
    pub active: bool,
    pub graph: Option<ModuleGraph>,
    pub error: Option<String>,
    pub crate_id: String,
    pub target_id: String,
    pub focus: String,
    pub selected: String,
    pub history: Vec<(String, String, String, Mode)>,
    pub mode: Mode,
    pub show_files: bool,
    pub show_limits: bool,
    pub diagram: bool,
    diagram_cache: std::cell::RefCell<Option<(diagram::Key, Option<diagram::Drawing>)>>,
    pub(in crate::tui) index: index::Index,
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
            mode: Mode::Hierarchy,
            show_files: false,
            show_limits: false,
            diagram: true,
            diagram_cache: Default::default(),
            index: Default::default(),
        }
    }
    pub fn apply(&mut self, g: ModuleGraph) {
        self.diagram_cache.borrow_mut().take();
        self.index = index::Index::build(&g);
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
            self.mode = Mode::Hierarchy;
        }
        self.active = true;
        self.initialize();
    }
    pub fn choices(&self) -> Vec<&ModuleNode> {
        let Some(g) = &self.graph else { return vec![] };
        let mut nodes = vec![];
        if let Some(n) = self.node(g, &self.focus) {
            nodes.push(n);
        }
        if let Some(e) = self.index.nodes.get(&self.focus) {
            let ids: Vec<_> = match self.mode {
                Mode::Hierarchy => e.children.iter().map(String::as_str).collect(),
                Mode::Imports => e.outgoing.iter().map(|l| l.id.as_str()).collect(),
                Mode::UsedBy => e.incoming.iter().map(|l| l.id.as_str()).collect(),
            };
            nodes.extend(
                ids.into_iter()
                    .filter(|id| *id != self.focus)
                    .filter_map(|id| self.node(g, id)),
            );
        }
        nodes
    }
    fn node<'a>(&self, g: &'a ModuleGraph, id: &str) -> Option<&'a ModuleNode> {
        self.index
            .nodes
            .get(id)
            .and_then(|e| g.nodes.get(e.position))
    }
    pub fn cycle_mode(&mut self) {
        self.remember();
        self.focus = self.selected.clone();
        self.mode = match self.mode {
            Mode::Hierarchy => Mode::Imports,
            Mode::Imports => Mode::UsedBy,
            Mode::UsedBy => Mode::Hierarchy,
        };
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
        if self.focus == self.selected && self.mode != Mode::Hierarchy {
            self.remember();
            self.mode = Mode::Hierarchy;
            return;
        }
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
            self.mode,
        ));
        if self.history.len() > 50 {
            self.history.remove(0);
        }
    }
    pub fn back(&mut self) {
        if let Some((target, focus, selected, mode)) = self.history.pop() {
            self.target_id = target;
            self.focus = focus;
            self.selected = selected;
            self.mode = mode;
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
