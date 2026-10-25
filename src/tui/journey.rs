//! Bounded cross-view intent. Semantic identity is accepted only from exact RA anchors.
mod actions;
mod dispatch;
mod render;
#[cfg(test)]
mod tests;
use super::{
    app::{AnalyzerState, App, Overlay, Pane, View},
    explorer::ExplorerRightMode,
    worker::AnalyzerCommand,
};
use crate::maintainability::FunctionMetrics;
pub(super) use actions::{Action, Picker};
pub(super) use dispatch::{flush, handle_key};
pub(super) use render::{render_picker, render_start};
use std::path::PathBuf;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Destination {
    Inspector,
    Calls,
}
pub(super) struct Pending {
    pub id: u64,
    pub anchor: FunctionMetrics,
    pub destination: Destination,
}
#[derive(Clone)]
pub(super) struct ReturnPoint {
    from: View,
    to: View,
    pane: Pane,
    explorer_mode: ExplorerRightMode,
    module: Option<(
        String,
        String,
        String,
        String,
        super::module_map::Mode,
        bool,
    )>,
}
#[derive(Default)]
pub(super) struct State {
    pub picker: Option<Picker>,
    pub pending: Option<Pending>,
    pub dispatch: Option<AnalyzerCommand>,
    pub start_selection: usize,
    history: Vec<ReturnPoint>,
}
pub(super) fn point(app: &App, to: View) -> ReturnPoint {
    let s = &app.module_map;
    ReturnPoint {
        from: app.view,
        to,
        pane: app.pane,
        explorer_mode: app.explorer.right_mode,
        module: (app.view == View::Map && s.active).then(|| {
            (
                s.crate_id.clone(),
                s.target_id.clone(),
                s.focus.clone(),
                s.selected.clone(),
                s.mode,
                s.diagram,
            )
        }),
    }
}
pub(super) fn remember(app: &mut App, p: ReturnPoint) {
    if p.from == p.to {
        return;
    }
    app.journey.history.push(p);
    if app.journey.history.len() > 50 {
        app.journey.history.remove(0);
    }
}
pub(super) fn back(app: &mut App) -> bool {
    if !app.journey.history.last().is_some_and(|p| p.to == app.view) {
        return false;
    }
    let p = app.journey.history.pop().unwrap();
    app.switch_view(p.from);
    app.pane = p.pane;
    app.explorer.right_mode = p.explorer_mode;
    if let Some((c, t, f, s, m, d)) = p.module {
        let map = &mut app.module_map;
        map.crate_id = c;
        map.target_id = t;
        map.focus = f;
        map.selected = s;
        map.mode = m;
        map.diagram = d;
    }
    true
}
pub(super) fn cancel(app: &mut App) {
    app.journey.pending = None;
    app.journey.dispatch = None;
}
pub(super) fn rebind(app: &mut App, id: u64, file: &std::path::Path) {
    if let Some(p) = &mut app.journey.pending {
        if p.anchor.path == file {
            p.id = id;
        }
    }
}
pub(super) fn advance(app: &mut App) {
    let Some(p) = &app.journey.pending else {
        return;
    };
    if app.view != View::Explorer {
        cancel(app);
        return;
    }
    if app.overlay != Overlay::None {
        return;
    }
    let Some(f) = &app.explorer.file else {
        cancel(app);
        return;
    };
    if f.request_id != p.id || f.path != p.anchor.path {
        cancel(app);
        return;
    }
    if matches!(app.analyzer_state, AnalyzerState::Error(_)) {
        app.notice = Some(
            "Semantic navigation unavailable: rust-analyzer error. Exact file/anchor retained."
                .into(),
        );
        cancel(app);
        return;
    }
    if f.loading {
        return;
    }
    let symbol = app
        .explorer
        .selected_symbol()
        .filter(|s| f.anchor_matched && super::explorer::anchor_matches(&p.anchor, s));
    let Some(symbol) = symbol else {
        if app.analyzer_state != AnalyzerState::Ready {
            let f = app.explorer.file.as_mut().unwrap();
            f.error = Some("Exact anchor not available while indexing; retry on Ready".into());
            return;
        }
        app.notice = Some(
            "No unique callable RA symbol at exact anchor; file/line retained. Select manually."
                .into(),
        );
        cancel(app);
        return;
    };
    let p = app.journey.pending.take().unwrap();
    match p.destination {
        Destination::Inspector => {
            if let Some((id, symbol)) = app.begin_explorer_symbol_inspect() {
                app.journey.dispatch = Some(AnalyzerCommand::Inspect { id, symbol });
            }
        }
        Destination::Calls => {
            if !crate::model::is_callable_kind(&symbol.kind) {
                app.notice = Some("Call hierarchy requires a function or method".into());
                return;
            }
            let origin = point(app, View::Calls);
            if let Some((id, target, scope)) = app.open_calls_for_inspector() {
                remember(app, origin);
                app.journey.dispatch = Some(AnalyzerCommand::LoadCalls { id, target, scope });
            }
        }
    }
}
