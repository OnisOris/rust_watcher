//! Bounded cross-view intent. Semantic identity is accepted only from exact RA anchors.
mod actions;
mod anchors;
mod dispatch;
#[cfg(test)]
mod entrypoint_tests;
#[cfg(test)]
mod performance_tests;
mod render;
#[cfg(test)]
mod tests;
use super::{
    app::{AnalyzerState, App, Overlay, Pane, View},
    explorer::ExplorerRightMode,
    worker::AnalyzerCommand,
};
use crate::maintainability::FunctionMetrics;
pub(super) use actions::{Action, BinaryEntrypoint, Picker};
pub(super) use anchors::prepare as prepare_file_symbols;
use anchors::Selector;
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
    pub selector: Selector,
    pub destination: Destination,
    pub deadline: std::time::Instant,
}
pub(super) const NAVIGATION_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);
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
        usize,
    )>,
}
#[derive(Default)]
pub(super) struct State {
    pub picker: Option<Picker>,
    pub pending: Option<Pending>,
    pub dispatch: Option<AnalyzerCommand>,
    pub start_selection: usize,
    pub binary_context: Option<(BinaryEntrypoint, crate::model::Symbol)>,
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
                s.page.get(),
            )
        }),
    }
}
pub(super) fn remember(app: &mut App, mut p: ReturnPoint) {
    // A semantic file request is a bridge, not an extra stop between Map and Calls.
    if p.from == View::Explorer
        && p.to == View::Calls
        && app.journey.history.last().is_some_and(|old| {
            old.to == View::Explorer && matches!(old.from, View::Map | View::Calls)
        })
    {
        let old = app.journey.history.pop().unwrap();
        p = ReturnPoint {
            to: View::Calls,
            ..old
        };
    }
    if p.from == p.to {
        return;
    }
    app.journey.history.push(p);
    if app.journey.history.len() > 50 {
        app.journey.history.remove(0);
    }
}
pub(super) fn source_returns_to_calls(app: &App) -> bool {
    app.journey
        .history
        .last()
        .is_some_and(|p| p.from == View::Calls && p.to == View::Explorer)
}
pub(super) fn back(app: &mut App) -> bool {
    if !app.journey.history.last().is_some_and(|p| p.to == app.view) {
        return false;
    }
    let p = app.journey.history.pop().unwrap();
    app.switch_view(p.from);
    app.pane = p.pane;
    app.explorer.right_mode = p.explorer_mode;
    if let Some((c, t, f, s, m, d, page)) = p.module {
        let map = &mut app.module_map;
        map.crate_id = c;
        map.target_id = t;
        map.focus = f;
        map.selected = s;
        map.mode = m;
        map.diagram = d;
        map.page.set(page);
    }
    // InspectFile/Inspect can supersede the worker's remaining Calls phase.
    // Resume an incomplete target on return, with a fresh ID rejecting late replies.
    if app.view == View::Calls && (app.calls.callers_loading || app.calls.callees_loading) {
        if let Some(target) = app.calls.center.clone() {
            let (id, target, scope) = app.load_calls(target, false);
            app.journey.dispatch = Some(AnalyzerCommand::LoadCalls { id, target, scope });
        }
    }
    true
}
pub(super) fn cancel(app: &mut App) {
    if app.journey.pending.take().is_some() {
        if let Some(f) = app.explorer.file.as_mut() {
            f.retry_when_ready = false;
        }
    }
    app.journey.dispatch = None;
}
pub(super) fn rebind(app: &mut App, id: u64, file: &std::path::Path) {
    if let Some(p) = &mut app.journey.pending {
        if p.selector.path() == file {
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
    if std::time::Instant::now() >= p.deadline {
        let reason="Semantic navigation timed out (120s). Source retained; retry selection after RA Ready.";
        if let Some(f) = app
            .explorer
            .file
            .as_mut()
            .filter(|f| f.request_id == p.id && f.path == p.selector.path())
        {
            f.loading = false;
            f.error = None;
            f.retry_when_ready = false;
            f.anchor_matched = false;
            f.anchor_notice = Some(reason.into());
        }
        app.notice = Some(reason.into());
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
    if f.request_id != p.id || f.path != p.selector.path() {
        cancel(app);
        return;
    }
    if let AnalyzerState::Error(reason) = &app.analyzer_state {
        let f = app.explorer.file.as_mut().unwrap();
        f.loading = false;
        f.error = Some(reason.clone());
        f.anchor_notice = Some(
            "Source available. Call hierarchy unavailable: rust-analyzer not available.".into(),
        );
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
    // Binary discovery is provisional during indexing, even when one main is returned.
    // Ready triggers the existing single bounded file retry, not a workspace search.
    if matches!(p.selector, Selector::Binary(_)) && app.analyzer_state != AnalyzerState::Ready {
        let f = app.explorer.file.as_mut().unwrap();
        f.retry_when_ready = true;
        f.anchor_notice =
            Some("Entrypoint: indexing; waiting for RA Ready and one exact-file retry".into());
        return;
    }
    let symbol = app
        .explorer
        .selected_symbol()
        .filter(|s| f.anchor_matched && p.selector.matches(s));
    let Some(symbol) = symbol else {
        if app.analyzer_state != AnalyzerState::Ready {
            let f = app.explorer.file.as_mut().unwrap();
            f.retry_when_ready = true;
            f.anchor_notice =
                Some("Exact anchor not available while indexing; retry on Ready".into());
            return;
        }
        app.notice = if matches!(p.selector, Selector::Binary(_)) {
            f.error.clone().or_else(|| f.anchor_notice.clone())
        } else {
            Some(
            "No unique callable RA symbol at exact anchor; file/line retained. Select manually."
                .into(),
        )
        };
        cancel(app);
        return;
    };
    let p = app.journey.pending.take().unwrap();
    if let Selector::Binary(t) = p.selector {
        app.journey.binary_context = Some((t, symbol.clone()));
    }
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
