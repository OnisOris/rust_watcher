use super::*;
use crate::{
    model::{CallScope, CallTarget, CallTargets, Position, Range},
    tui::{
        app::{AnalyzerState, App, View},
        event,
        worker::{AnalyzerCommand, AnalyzerEvent},
    },
};
use crossterm::event::{KeyCode as K, KeyEvent, KeyModifiers};
use ratatui::{backend::TestBackend, Terminal};
mod integration;
mod performance;
fn target(name: &str, line: u32) -> CallTarget {
    let p = Position { line, character: 3 };
    let r = Range {
        start: p,
        end: Position {
            line,
            character: 3 + name.len() as u32,
        },
    };
    CallTarget {
        name: name.into(),
        kind: "function".into(),
        file: "src/main.rs".into(),
        range: r,
        selection_range: r,
        detail: None,
    }
}
fn data(items: Vec<CallTarget>) -> Result<Option<CallTargets>, String> {
    Ok(Some(CallTargets {
        items,
        truncated: false,
    }))
}
fn graph() -> State {
    let root = target("root", 1);
    let mut s = State::new(&root, true);
    let id = Identity::from(&root);
    s.ingest(&id, true, data(vec![target("caller", 0)]));
    s.ingest(&id, false, data(vec![target("run", 2), target("run", 3)]));
    s
}
fn fixture() -> (tempfile::TempDir, App) {
    let t = tempfile::tempdir().unwrap();
    std::fs::create_dir(t.path().join("src")).unwrap();
    std::fs::write(
        t.path().join("Cargo.toml"),
        "[package]\nname='graph-fixture'\nversion='0.1.0'\nedition='2021'\n",
    )
    .unwrap();
    std::fs::write(t.path().join("src/main.rs"),"fn main(){start();}\nfn start(){process();log();}\nfn process(){finish();}\nfn log(){}\nfn finish(){}\nfn recurse(){recurse();}\n").unwrap();
    let p = crate::project::Project::discover(t.path()).unwrap();
    let tree = crate::repository::scan(t.path()).unwrap();
    let mut app = App::new(p);
    app.explorer.apply_tree(tree);
    app.switch_view(View::Calls);
    (t, app)
}
async fn key(app: &mut App, k: K, tx: &tokio::sync::mpsc::Sender<AnalyzerCommand>) {
    event::handle_key(app, KeyEvent::new(k, KeyModifiers::NONE), tx)
        .await
        .unwrap();
    crate::tui::journey::flush(app, tx).await;
}
#[test]
fn identities_edges_depth_limits_and_states_are_exact() {
    let only = State::new(&target("single", 0), true);
    assert_eq!(only.nodes.len(), 1);
    assert!(only.edges.is_empty());
    let mut s = graph();
    let root = s.focus.clone().unwrap();
    assert_eq!(s.nodes.len(), 4);
    assert_eq!(s.edges.len(), 3);
    assert!(s
        .edges
        .contains(&(Identity::from(&target("caller", 0)), root.clone())));
    assert!(s
        .edges
        .contains(&(root.clone(), Identity::from(&target("run", 2)))));
    let run = Identity::from(&target("run", 2));
    s.selected = Some(run.clone());
    let request = s.begin(1, 1, CallScope::Workspace).unwrap();
    assert!(matches!(s.nodes[&run].sides[1], Expansion::Loading));
    assert!(s.begin(2, 1, CallScope::Workspace).is_err());
    s.pending = None;
    s.ingest(
        &run,
        false,
        data(vec![
            target("finish", 4),
            target("finish", 4),
            target("root", 1),
        ]),
    );
    assert_eq!(s.nodes.len(), 5);
    assert_eq!(s.nodes[&Identity::from(&target("finish", 4))].layer, 2);
    assert!(s.edges.contains(&(run.clone(), root.clone())));
    assert!(s.begin(3, 1, request.scope).is_err());
    s.selected = Some(Identity::from(&target("finish", 4)));
    assert!(s.begin(4, 1, request.scope).is_err());
    s.ingest(&root, false, data(vec![target("root", 1)]));
    assert!(s.edges.contains(&(root.clone(), root.clone())));
    s.ingest(&run, true, Ok(None));
    assert!(matches!(s.nodes[&run].sides[0], Expansion::Unsupported));
    s.ingest(&run, true, Err("failure".into()));
    assert!(s.nodes[&run].sides[0].label().contains("RPC error"));
    for i in 10..80 {
        s.ingest(&root, false, data(vec![target("other", i)]));
    }
    assert_eq!(s.nodes.len(), MAX_NODES);
    assert!(s.limited);
    assert!(s.edges.len() <= MAX_EDGES);
    let mut t = target("run", 2);
    t.file = "another.rs".into();
    assert_ne!(Identity::from(&t), run);
    t.file = "/external/lib.rs".into();
    assert!(!t.is_workspace_local(std::path::Path::new("/workspace")));
    s.ingest(
        &root,
        true,
        Ok(Some(CallTargets {
            items: vec![],
            truncated: true,
        })),
    );
    assert!(s.nodes[&root].sides[0].label().contains("truncated"));
    let owners = s.nodes.keys().cloned().collect::<Vec<_>>();
    let targets = s
        .nodes
        .values()
        .map(|n| n.target.clone())
        .collect::<Vec<_>>();
    for owner in owners {
        s.ingest(&owner, false, data(targets.clone()));
    }
    assert_eq!(s.edges.len(), MAX_EDGES);
}
#[tokio::test]
async fn cancelled_and_superseded_requests_cannot_add_edges_or_reopen_view() {
    let (_t, mut app) = fixture();
    app.analyzer_state = AnalyzerState::Ready;
    app.calls.center = Some(target("root", 1));
    app.calls.graph = graph();
    app.calls.graph.selected = Some(Identity::from(&target("run", 2)));
    let old = app
        .calls
        .graph
        .begin(1, app.calls.request_id, app.calls.scope)
        .unwrap();
    app.calls.graph.cancel();
    assert!(matches!(
        app.calls.graph.nodes[&old.identity].sides[1],
        Expansion::Cancelled
    ));
    let fresh = app
        .calls
        .graph
        .begin(2, app.calls.request_id, app.calls.scope)
        .unwrap();
    app.apply_event(AnalyzerEvent::CallGraphExpanded {
        request: Box::new(old),
        result: data(vec![target("stale", 20)]),
    });
    assert_eq!(app.calls.graph.pending.as_ref(), Some(&fresh));
    let (tx, _rx) = tokio::sync::mpsc::channel(8);
    key(&mut app, K::Esc, &tx).await;
    let view = app.view;
    app.apply_event(AnalyzerEvent::CallGraphExpanded {
        request: Box::new(fresh),
        result: data(vec![target("stale", 20)]),
    });
    assert_eq!(app.view, view);
    assert_ne!(app.view, View::Calls);
    assert!(!app
        .calls
        .graph
        .nodes
        .contains_key(&Identity::from(&target("stale", 20))));
}
#[test]
fn layout_pages_geometry_and_selection_are_stable() {
    let mut s = graph();
    let root = s.focus.clone().unwrap();
    for i in 10..30 {
        s.ingest(&root, false, data(vec![target("callee", i)]));
    }
    for (w, h) in [(80, 20), (100, 30), (120, 35), (160, 45)] {
        s.page.set(0);
        s.viewport(w, h);
        s.page.set(0);
        let first = layout::layout(&s, w, h);
        let focus = s.focus.clone();
        for _ in 0..10 {
            s.move_by(1);
            assert_eq!(s.focus, focus);
            assert_eq!(s.page.get(), 0);
            assert_eq!(layout::layout(&s, w, h), first);
        }
        for (id, r) in &first.boxes {
            assert!(r.right() <= w && r.bottom() <= h);
            for (other, b) in &first.boxes {
                if id != other {
                    assert!(r.intersection(*b).is_empty());
                }
            }
        }
        for (route, forward) in &first.routes {
            for p in route {
                assert!(p.0 < w && p.1 < h);
                assert!(first.boxes.values().all(|b| !b.contains((*p).into())));
            }
            let end = *route.last().unwrap();
            assert!(first.boxes.values().any(|b| b.y < end.1
                && end.1 < b.bottom()
                && if *forward {
                    end.0 + 1 == b.x
                } else {
                    end.0 == b.right()
                }));
        }
        let mut known = std::collections::BTreeSet::new();
        loop {
            known.extend(s.visible());
            let old = s.page.get();
            s.change_page(1);
            if old == s.page.get() {
                break;
            }
        }
        assert_eq!(known.len(), s.nodes.len());
        while s.page.get() > 0 {
            s.change_page(-1);
        }
        assert_eq!(layout::layout(&s, w, h), first);
    }
    assert!(layout::layout(&s, 40, 8).boxes.is_empty());
}
#[test]
fn shared_targets_have_order_independent_canonical_placement() {
    let mut a = graph();
    let mut b = graph();
    let first = Identity::from(&target("run", 2));
    let second = Identity::from(&target("run", 3));
    let shared = target("shared", 8);
    for owner in [&first, &second] {
        a.ingest(owner, false, data(vec![shared.clone()]));
    }
    for owner in [&second, &first] {
        b.ingest(owner, false, data(vec![shared.clone()]));
    }
    assert_eq!(a.edges, b.edges);
    assert_eq!(a.nodes.len(), 5);
    assert_eq!(a.nodes[&Identity::from(&shared)].via, Some(first));
    a.viewport(120, 35);
    b.viewport(120, 35);
    assert_eq!(layout::layout(&a, 120, 35), layout::layout(&b, 120, 35));
}
#[test]
fn expansion_page_reflow_preserves_the_selected_branch() {
    let root = target("root", 0);
    let id = Identity::from(&root);
    let mut s = State::new(&root, true);
    s.ingest(
        &id,
        true,
        data((1..=10).map(|i| target("caller", i)).collect()),
    );
    s.ingest(
        &id,
        false,
        data((20..30).map(|i| target("callee", i)).collect()),
    );
    s.ingest(
        &Identity::from(&target("callee", 20)),
        false,
        data((100..104).map(|i| target("shared", i)).collect()),
    );
    let selected = Identity::from(&target("caller", 10));
    s.selected = Some(selected.clone());
    s.page.set(
        s.plan()
            .iter()
            .position(|ids| ids.contains(&selected))
            .unwrap(),
    );
    assert_eq!(s.page.get(), 4);
    s.ingest(&selected, true, data(vec![target("shared", 100)]));
    assert_eq!(s.page.get(), 3);
    assert_eq!(s.pages(), 4);
    assert!(s.visible().contains(&selected));
    assert_eq!(s.focus, Some(id));
}
#[tokio::test]
async fn incoming_expansion_session_budget_and_external_source_are_explicit() {
    let (_tmp, mut app) = fixture();
    app.analyzer_state = AnalyzerState::Ready;
    app.calls.center = Some(target("root", 1));
    app.calls.graph = graph();
    let caller = Identity::from(&target("caller", 0));
    app.calls.graph.selected = Some(caller.clone());
    let request = app
        .calls
        .graph
        .begin(1, app.calls.request_id, app.calls.scope)
        .unwrap();
    assert!(request.incoming);
    app.apply_event(AnalyzerEvent::CallGraphExpanded {
        request: Box::new(request),
        result: data(vec![target("caller2", 9)]),
    });
    assert!(app
        .calls
        .graph
        .edges
        .contains(&(Identity::from(&target("caller2", 9)), caller)));
    assert_eq!(
        app.calls.graph.nodes[&Identity::from(&target("caller2", 9))].layer,
        -2
    );
    let (tx, mut rx) = tokio::sync::mpsc::channel(8);
    app.graph_expansions = MAX_EXPANSIONS;
    key(&mut app, K::Char('x'), &tx).await;
    assert!(app.notice.as_ref().unwrap().contains("budget"));
    assert!(rx.try_recv().is_err());
    let mut external = target("run", 2);
    external.file = "/external/src/lib.rs".into();
    let root = app.calls.graph.focus.clone().unwrap();
    app.calls
        .graph
        .ingest(&root, false, data(vec![external.clone()]));
    app.calls.graph.selected = Some(Identity::from(&external));
    key(&mut app, K::Char('o'), &tx).await;
    assert_eq!(app.view, View::Calls);
    assert!(app
        .notice
        .as_ref()
        .unwrap()
        .contains("External source unavailable"));
    assert!(rx.try_recv().is_err());
}
#[tokio::test]
async fn handlers_preserve_focus_graph_cache_history_and_reject_stale_expansions() {
    let (_t, mut app) = fixture();
    app.analyzer_state = AnalyzerState::Ready;
    let (tx, mut rx) = tokio::sync::mpsc::channel(16);
    let (id, _, _) = app.load_calls(target("root", 1), false);
    app.apply_event(AnalyzerEvent::CallersLoaded {
        id,
        result: Some(CallTargets {
            items: vec![],
            truncated: false,
        }),
    });
    app.apply_event(AnalyzerEvent::CalleesLoaded {
        id,
        result: Some(CallTargets {
            items: vec![target("run", 2)],
            truncated: false,
        }),
    });
    key(&mut app, K::Char('g'), &tx).await;
    let mut terminal = Terminal::new(TestBackend::new(160, 45)).unwrap();
    terminal.draw(|f| crate::tui::ui::render(f, &app)).unwrap();
    let before = app.calls.graph.cache.borrow().clone();
    key(&mut app, K::Char('j'), &tx).await;
    terminal.draw(|f| crate::tui::ui::render(f, &app)).unwrap();
    assert_eq!(*app.calls.graph.cache.borrow(), before);
    key(&mut app, K::Char('x'), &tx).await;
    let request = match rx.recv().await.unwrap() {
        AnalyzerCommand::ExpandCallGraph { request } => request,
        _ => panic!(),
    };
    key(&mut app, K::Char('g'), &tx).await;
    key(&mut app, K::Char('g'), &tx).await;
    assert_eq!(app.calls.graph.pending.as_ref(), Some(&request));
    assert!(rx.try_recv().is_err());
    app.apply_event(AnalyzerEvent::CallGraphExpanded {
        request: Box::new(request.clone()),
        result: data(vec![target("finish", 4)]),
    });
    app.calls.graph.selected = Some(Identity::from(&target("finish", 4)));
    key(&mut app, K::Char('g'), &tx).await;
    assert!(app
        .notice
        .as_ref()
        .unwrap()
        .contains("Second-level selection retained"));
    key(&mut app, K::Char('g'), &tx).await;
    assert_eq!(
        app.calls.graph.selected,
        Some(Identity::from(&target("finish", 4)))
    );
    app.calls.graph.selected = Some(Identity::from(&target("run", 2)));
    assert!(app
        .calls
        .graph
        .nodes
        .contains_key(&Identity::from(&target("finish", 4))));
    let old = app.calls.graph.clone();
    key(&mut app, K::Enter, &tx).await;
    let load = rx.recv().await.unwrap();
    let id = match load {
        AnalyzerCommand::LoadCalls { id, target, .. } => {
            assert_eq!(target.name, "run");
            id
        }
        _ => panic!(),
    };
    app.apply_event(AnalyzerEvent::CallGraphExpanded {
        request: Box::new(request.clone()),
        result: data(vec![target("stale", 20)]),
    });
    assert!(!app
        .calls
        .graph
        .nodes
        .contains_key(&Identity::from(&target("stale", 20))));
    for incoming in [true, false] {
        app.apply_event(if incoming {
            AnalyzerEvent::CallersLoaded {
                id,
                result: Some(CallTargets {
                    items: vec![],
                    truncated: false,
                }),
            }
        } else {
            AnalyzerEvent::CalleesLoaded {
                id,
                result: Some(CallTargets {
                    items: vec![],
                    truncated: false,
                }),
            }
        });
    }
    key(&mut app, K::Esc, &tx).await;
    assert_eq!(app.calls.center.as_ref().unwrap().name, "root");
    assert_eq!(app.calls.graph.nodes.len(), old.nodes.len());
    assert_eq!(app.calls.graph.selected, old.selected);
    key(&mut app, K::Char('e'), &tx).await;
    app.apply_event(AnalyzerEvent::CallGraphExpanded {
        request: Box::new(request),
        result: data(vec![target("wrong_scope", 30)]),
    });
    assert!(!app
        .calls
        .graph
        .nodes
        .contains_key(&Identity::from(&target("wrong_scope", 30))));
    for (w, h) in [(80, 20), (100, 30), (120, 35), (160, 45)] {
        let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
        t.draw(|f| crate::tui::ui::render(f, &app)).unwrap();
        let text = t
            .backend()
            .buffer()
            .content
            .iter()
            .map(|c| c.symbol())
            .collect::<String>();
        assert!(text.contains("CALL GRAPH"));
        assert!(text.contains("Selected:"));
    }
    app.calls.graph = graph();
    let mut narrow = Terminal::new(TestBackend::new(80, 20)).unwrap();
    narrow.draw(|f| crate::tui::ui::render(f, &app)).unwrap();
    app.pane = crate::tui::app::Pane::Callees; // Details focused before resize.
    key(&mut app, K::Tab, &tx).await;
    key(&mut app, K::Char('j'), &tx).await;
    assert_eq!(app.pane, crate::tui::app::Pane::CallCurrent);
    assert_ne!(app.calls.graph.selected, app.calls.graph.focus);
    app.apply_event(AnalyzerEvent::Error("RA unavailable".into()));
    assert!(!app.calls.callers_loading && !app.calls.callees_loading);
    assert!(app.calls.graph.pending.is_none());
    app.color_mode = crate::tui::theme::ColorMode::None;
    terminal.draw(|f| crate::tui::ui::render(f, &app)).unwrap();
    assert!(terminal
        .backend()
        .buffer()
        .content
        .iter()
        .all(|c| c.fg == ratatui::style::Color::Reset && c.bg == ratatui::style::Color::Reset));
    let text = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect::<String>();
    assert!(text.contains("RA unavailable"));
}
