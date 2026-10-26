//! Local, on-demand RA call neighborhoods. No structural/semantic fallback.
mod layout;
mod model;
mod pagination;
mod render;
#[cfg(test)]
mod tests;
pub(super) use model::*;
pub(super) use render::render;

use super::{
    app::{AnalyzerState, App, Overlay, Pane, View},
    event::send_command,
    worker::{AnalyzerCommand, AnalyzerEvent},
};
use crossterm::event::{KeyCode as K, KeyEvent};

pub(super) async fn handle_key(
    app: &mut App,
    key: KeyEvent,
    tx: &tokio::sync::mpsc::Sender<AnalyzerCommand>,
) -> bool {
    if app.view != View::Calls
        || app.overlay != Overlay::None
        || app.journey.picker.is_some()
        || app.calls.center.is_none()
        || app.calls.unavailable.is_some()
    {
        return false;
    }
    if key.code == K::Char('g') {
        app.notice = None;
        app.calls.graph.enabled = !app.calls.graph.enabled;
        if app.calls.graph.enabled {
            app.pane = Pane::CallCurrent;
        } else if let Some(selected) = app.calls.graph.selected.as_ref() {
            if Some(selected) == app.calls.graph.focus.as_ref() {
                app.pane = Pane::CallCurrent;
            } else if let Some(i) = app
                .calls
                .callers
                .iter()
                .position(|t| Identity::from(t) == *selected)
            {
                app.calls.callers_selection = i;
                app.pane = Pane::Callers;
            } else if let Some(i) = app
                .calls
                .callees
                .iter()
                .position(|t| Identity::from(t) == *selected)
            {
                app.calls.callees_selection = i;
                app.pane = Pane::Callees;
            } else {
                app.pane = Pane::CallCurrent;
                if Some(selected) != app.calls.graph.focus.as_ref() {
                    app.notice=Some("Second-level selection retained in Graph; List shows the first hop only. g returns to it.".into());
                }
            }
        }
        return true;
    }
    if !app.calls.graph.enabled {
        return false;
    }
    if !app.calls.graph.details_visible.get() {
        app.pane = Pane::CallCurrent;
    }
    match key.code {
        K::Tab | K::BackTab => {
            app.pane = if app.calls.graph.details_visible.get() && app.pane == Pane::CallCurrent {
                Pane::Callees
            } else {
                Pane::CallCurrent
            }
        }
        K::Char('h')
            if key
                .modifiers
                .contains(crossterm::event::KeyModifiers::CONTROL) =>
        {
            app.pane = Pane::CallCurrent
        }
        K::Char('l')
            if key
                .modifiers
                .contains(crossterm::event::KeyModifiers::CONTROL) =>
        {
            app.pane = if app.calls.graph.details_visible.get() {
                Pane::Callees
            } else {
                Pane::CallCurrent
            }
        }
        K::Char('j') | K::Down if key.modifiers.is_empty() => {
            if app.pane == Pane::CallCurrent {
                app.calls.graph.move_by(1);
                app.calls.context_scroll = 0;
                app.notice = None;
            } else {
                app.calls.context_scroll = (app.calls.context_scroll + 1)
                    .min(app.calls.context_scroll_limit.get().unwrap_or(0));
            }
        }
        K::Char('k') | K::Up if key.modifiers.is_empty() => {
            if app.pane == Pane::CallCurrent {
                app.calls.graph.move_by(-1);
                app.calls.context_scroll = 0;
                app.notice = None;
            } else {
                app.calls.context_scroll = app.calls.context_scroll.saturating_sub(1);
            }
        }
        K::PageDown | K::Char(']') => {
            app.calls.graph.change_page(1);
            app.calls.context_scroll = 0;
            app.notice = None;
        }
        K::PageUp | K::Char('[') => {
            app.calls.graph.change_page(-1);
            app.calls.context_scroll = 0;
            app.notice = None;
        }
        K::Char('x') => {
            if app.calls.callers_loading || app.calls.callees_loading {
                app.notice = Some("Expansion waits for both initial Calls sides to finish".into());
            } else if app.analyzer_state != AnalyzerState::Ready {
                app.notice = Some(
                    "Expansion unavailable: rust-analyzer is not Ready; retry after indexing"
                        .into(),
                );
            } else if app.graph_expansions >= MAX_EXPANSIONS {
                app.notice = Some("Graph expansion session budget reached (12 requests)".into());
            } else {
                app.graph_request += 1;
                match app.calls.graph.begin(
                    app.graph_request,
                    app.calls.request_id,
                    app.calls.scope,
                ) {
                    Ok(request) => {
                        app.notice = None;
                        app.graph_expansions += 1;
                        send_command(app, tx, AnalyzerCommand::ExpandCallGraph { request }).await;
                    }
                    Err(reason) => app.notice = Some(reason.into()),
                }
            }
        }
        K::Enter => {
            if let Some(target) = app
                .calls
                .graph
                .selected_target()
                .cloned()
                .filter(|t| Some(Identity::from(t)) != app.calls.graph.focus)
            {
                if !crate::model::is_callable_kind(&target.kind) {
                    app.notice =
                        Some("Call hierarchy is only available for functions and methods".into());
                    return true;
                }
                let (id, target, scope) = app.load_calls(target, true);
                send_command(app, tx, AnalyzerCommand::LoadCalls { id, target, scope }).await;
            }
        }
        _ => return false,
    }
    true
}

pub(super) fn apply(
    app: &mut App,
    request: Request,
    result: Result<Option<crate::model::CallTargets>, String>,
) {
    if app.view == View::Calls
        && request.generation == app.calls.request_id
        && request.scope == app.calls.scope
        && app.calls.graph.pending.as_ref() == Some(&request)
    {
        app.calls.graph.pending = None;
        app.calls
            .graph
            .ingest(&request.identity, request.incoming, result);
    }
}
pub(super) fn failed_event(request: Request, message: String) -> AnalyzerEvent {
    AnalyzerEvent::CallGraphExpanded {
        request: Box::new(request),
        result: Err(message),
    }
}
