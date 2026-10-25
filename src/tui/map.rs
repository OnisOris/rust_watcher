//! Interactive architecture state and presentation; no metadata, Git or AST work here.
mod graph;
mod inspector;
#[cfg(test)]
mod tests;
use super::{
    app::{App, Pane},
    components, theme,
};
use crate::architecture::{Architecture, DependencyKind};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Clear, List, ListItem, ListState, Paragraph},
    Frame,
};

pub(super) struct State {
    pub selected: String,
    pub focus: Option<String>,
    pub history: Vec<(Option<String>, String)>,
    pub inspector_scroll: usize,
    pub scroll_limit: std::cell::Cell<Option<usize>>,
    pub target_picker: Option<usize>,
}
impl State {
    pub fn new(model: &Architecture) -> Self {
        Self {
            selected: model
                .nodes
                .first()
                .map_or_else(String::new, |n| n.id.clone()),
            focus: None,
            history: vec![],
            inspector_scroll: 0,
            scroll_limit: Default::default(),
            target_picker: None,
        }
    }
    pub fn choices(&self, model: &Architecture) -> Vec<String> {
        let ids = self.focus.as_ref().map(|id| model.neighborhood(id));
        model
            .nodes
            .iter()
            .filter(|n| ids.as_ref().is_none_or(|ids| ids.contains(&n.id)))
            .map(|n| n.id.clone())
            .collect()
    }
    pub fn move_by(&mut self, model: &Architecture, delta: isize) {
        let ids = self.choices(model);
        if ids.is_empty() {
            return;
        }
        let index = ids.iter().position(|id| id == &self.selected).unwrap_or(0);
        self.selected = ids[index.saturating_add_signed(delta).min(ids.len() - 1)].clone();
        self.inspector_scroll = 0;
        self.target_picker = None;
    }
    pub fn focus_selected(&mut self) {
        if self.focus.as_ref() == Some(&self.selected) {
            return;
        }
        self.history
            .push((self.focus.clone(), self.selected.clone()));
        if self.history.len() > 50 {
            self.history.remove(0);
        }
        self.focus = Some(self.selected.clone());
    }
    pub fn back(&mut self) {
        if let Some((focus, selected)) = self.history.pop() {
            self.focus = focus;
            self.selected = selected;
            self.inspector_scroll = 0;
        } else {
            self.focus = None;
        }
        self.target_picker = None;
    }
}
pub(super) fn edge_color(kind: DependencyKind) -> ratatui::style::Color {
    match kind {
        DependencyKind::Normal => theme::ACCENT,
        DependencyKind::Dev => theme::NUMBER,
        DependencyKind::Build => theme::WARNING,
    }
}
pub(super) fn edge_mark(kind: DependencyKind) -> &'static str {
    match kind {
        DependencyKind::Normal => "─n→",
        DependencyKind::Dev => "┄d→",
        DependencyKind::Build => "═b⇒",
    }
}
pub(super) fn render(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let panes = Layout::horizontal([
        Constraint::Percentage(if area.width < 100 { 52 } else { 60 }),
        Constraint::Min(24),
    ])
    .split(area);
    graph::render(frame, panes[0], app);
    let text = inspector::details(app);
    frame.render_widget(
        components::scrollable(
            text,
            components::styled_panel(
                " CRATE INSPECTOR · j/k scroll ",
                app.pane == Pane::MapInspector,
            ),
            panes[1],
            app.map.inspector_scroll,
            &app.map.scroll_limit,
        ),
        panes[1],
    );
    if let Some(index) = app.map.target_picker {
        render_targets(frame, area, app, index);
    }
}
fn render_targets(frame: &mut Frame<'_>, area: Rect, app: &App, index: usize) {
    let Some(node) = app.project.architecture.node(&app.map.selected) else {
        return;
    };
    let width = area.width.saturating_sub(8);
    let height = (node.targets.len() as u16 * 2 + 4).min(area.height.saturating_sub(2));
    let popup = Rect::new(
        area.x + 4,
        area.y + (area.height - height) / 2,
        width,
        height,
    );
    frame.render_widget(Clear, popup);
    let items: Vec<_> = node
        .targets
        .iter()
        .map(|t| {
            ListItem::new(vec![
                Line::styled(
                    format!("{} · {}", t.name, t.kinds.join(",")),
                    Style::default().fg(theme::TEXT),
                ),
                components::path_label(&t.path, width.saturating_sub(6) as usize),
            ])
        })
        .collect();
    let mut selected = ListState::default().with_selected(Some(index));
    frame.render_stateful_widget(
        List::new(items)
            .highlight_symbol("> ")
            .highlight_style(components::selectable_row())
            .block(components::styled_panel(
                " CARGO TARGETS · j/k choose · Enter open · Esc cancel ",
                true,
            )),
        popup,
        &mut selected,
    );
}
