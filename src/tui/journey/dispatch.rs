use super::super::{
    event::{open_exact_file, send_command},
    worker::AnalyzerCommand,
};
use super::actions;
use super::*;
use crossterm::event::{KeyCode as K, KeyEvent};
use tokio::sync::mpsc::Sender;
pub(in crate::tui) async fn flush(app: &mut App, tx: &Sender<AnalyzerCommand>) {
    advance(app);
    if let Some(command) = app.journey.dispatch.take() {
        send_command(app, tx, command).await;
    }
}
pub(in crate::tui) async fn handle_key(
    app: &mut App,
    key: KeyEvent,
    tx: &Sender<AnalyzerCommand>,
) -> Option<bool> {
    if app.journey.picker.is_some() {
        match key.code {
            K::Char('q') => return Some(true),
            K::Esc => app.journey.picker = None,
            K::Char('j') | K::Down => {
                let p = app.journey.picker.as_mut().unwrap();
                p.selected = (p.selected + 1).min(p.actions.len().saturating_sub(1));
            }
            K::Char('k') | K::Up => {
                let p = app.journey.picker.as_mut().unwrap();
                p.selected = p.selected.saturating_sub(1);
            }
            K::Enter | K::Char('c') => {
                let p = app.journey.picker.as_ref().unwrap();
                let action = p.actions.get(p.selected).cloned();
                let calls = p.calls || key.code == K::Char('c');
                if let Some(action) = action {
                    if calls && matches!(action, Action::File(..)) {
                        app.notice=Some("File has no exact callable anchor. Enter opens source; select a function for Calls.".into());
                        return Some(false);
                    }
                    app.journey.picker = None;
                    activate(
                        app,
                        tx,
                        action,
                        if calls {
                            Destination::Calls
                        } else {
                            Destination::Inspector
                        },
                    )
                    .await;
                } else {
                    app.notice = Some(
                        "No known candidates in this bounded report; try source or another module"
                            .into(),
                    );
                }
            }
            _ => {}
        }
        return Some(false);
    }
    if app.overlay != Overlay::None {
        return None;
    }
    if key.code == K::Char('p') && app.view == View::Map && app.module_map.active {
        let title = app
            .module_map
            .graph
            .as_ref()
            .and_then(|g| g.node(&app.module_map.selected))
            .map_or("Module", |n| n.qualified_path.as_str())
            .to_owned();
        app.journey.picker = Some(Picker {
            title: format!("REVIEW PRIORITIES · {title}"),
            actions: actions::priorities(app),
            selected: 0,
            calls: false,
        });
        return Some(false);
    }
    let start = (app.view == View::Symbols
        && app.symbols_query.is_empty()
        && app.symbols_results.is_empty()
        && app.pane == Pane::Symbols)
        || (app.view == View::Calls && app.calls.center.is_none());
    if start {
        let calls = app.view == View::Calls;
        let routes = actions::routes(calls);
        match key.code {
            K::Down | K::Char('j') => {
                app.journey.start_selection =
                    (app.journey.start_selection + 1).min(routes.len() - 1);
                return Some(false);
            }
            K::Up | K::Char('k') => {
                app.journey.start_selection = app.journey.start_selection.saturating_sub(1);
                return Some(false);
            }
            code if code == K::Enter || (calls && code == K::Char('c')) => {
                let (label, items) = actions::available(
                    app,
                    routes[app.journey.start_selection.min(routes.len() - 1)],
                    calls,
                );
                if items.is_empty() {
                    app.notice = Some(format!("{label}: unavailable; data may still be loading"));
                } else {
                    app.journey.picker = Some(Picker {
                        title: label,
                        actions: items,
                        selected: 0,
                        calls,
                    });
                }
                return Some(false);
            }
            _ => {}
        }
    }
    None
}
async fn activate(
    app: &mut App,
    tx: &Sender<AnalyzerCommand>,
    action: Action,
    destination: Destination,
) {
    cancel(app);
    match action {
        Action::File(path, _) => open_exact_file(app, tx, path, None).await,
        Action::Function(anchor) => {
            let before = app.explorer.file.as_ref().map(|f| f.request_id);
            open_exact_file(app, tx, anchor.path.clone(), Some(anchor.clone())).await;
            if let Some(f) = app
                .explorer
                .file
                .as_ref()
                .filter(|f| Some(f.request_id) != before && f.path == anchor.path)
            {
                app.journey.pending = Some(Pending {
                    id: f.request_id,
                    anchor,
                    destination,
                });
                advance(app);
            }
        }
        Action::Symbol(symbol) => {
            if destination == Destination::Calls {
                let origin = point(app, View::Calls);
                if crate::model::is_callable_kind(&symbol.kind) {
                    app.switch_view(View::Calls);
                    app.calls.history.clear();
                    let (id, target, scope) =
                        app.load_calls(crate::model::CallTarget::from_symbol(&symbol), false);
                    remember(app, origin);
                    send_command(app, tx, AnalyzerCommand::LoadCalls { id, target, scope }).await;
                }
            }
        }
    }
}
