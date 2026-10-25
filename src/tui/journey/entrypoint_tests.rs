use super::super::{event::handle_key, worker::AnalyzerEvent};
use super::*;
use crate::{
    model::{Position, Range, Symbol},
    project::Project,
};
use crossterm::event::{KeyCode as K, KeyEvent, KeyModifiers};
use tokio::sync::mpsc;

fn fixture(binary: bool) -> (tempfile::TempDir, App) {
    let t = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(t.path().join("src")).unwrap();
    let bins = if binary {
        "\n[[bin]]\nname='entry'\npath='src/main.rs'\n[[bin]]\nname='alias'\npath='src/main.rs'\n[[bin]]\nname='other'\npath='src/other.rs'\n"
    } else {
        ""
    };
    std::fs::write(
        t.path().join("Cargo.toml"),
        format!("[package]\nname='entry'\nversion='0.1.0'\nedition='2021'\n{bins}"),
    )
    .unwrap();
    std::fs::write(t.path().join("src/lib.rs"), "pub fn library(){}\n").unwrap();
    if binary {
        let mut src = "fn main(){ leaf(); }\nfn leaf() {}\n".to_string();
        // More than both ranking limits, each strictly longer and deeper than main.
        for i in 0..25 {
            src.push_str(&format!("fn ranked_{i}() {{\n if true {{\n  if true {{\n   if true {{\n    if true {{}}\n   }}\n  }}\n }}\n}}\n"));
        }
        std::fs::write(t.path().join("src/main.rs"), src).unwrap();
        std::fs::write(
            t.path().join("src/other.rs"),
            "#[tokio::main]\nasync fn main(){ leaf(); }\nfn leaf() {}\n",
        )
        .unwrap();
        std::fs::create_dir(t.path().join("examples")).unwrap();
        std::fs::write(t.path().join("examples/demo.rs"), "fn main(){}\n").unwrap();
    }
    let p = Project::discover(t.path()).unwrap();
    let tree = crate::repository::scan(t.path()).unwrap();
    let metrics = crate::maintainability::analyze(t.path(), &tree, &p);
    let mut app = App::new(p);
    app.explorer.apply_tree(tree);
    app.maintainability = Some(metrics);
    app.analyzer_state = AnalyzerState::Ready;
    app.switch_view(View::Calls);
    (t, app)
}
async fn key(app: &mut App, k: K, tx: &mpsc::Sender<AnalyzerCommand>) {
    handle_key(app, KeyEvent::new(k, KeyModifiers::NONE), tx)
        .await
        .unwrap();
}
pub(super) async fn choose(
    app: &mut App,
    name: &str,
    tx: &mpsc::Sender<AnalyzerCommand>,
    rx: &mut mpsc::Receiver<AnalyzerCommand>,
) -> (u64, PathBuf) {
    app.switch_view(View::Calls);
    app.calls.center = None;
    app.journey.start_selection = 0;
    key(app, K::Enter, tx).await;
    let p = app.journey.picker.as_mut().expect("Cargo target picker");
    p.selected = p
        .actions
        .iter()
        .position(|a| matches!(a,Action::BinaryEntrypoint(t)if t.name==name))
        .unwrap();
    key(app, K::Enter, tx).await;
    let AnalyzerCommand::InspectFile { id, file } = rx.recv().await.unwrap() else {
        panic!("entrypoint must use documentSymbol")
    };
    (id, file)
}
fn main_symbol(file: &std::path::Path, line: u32) -> Symbol {
    Symbol {
        id: format!("{}:{line}:3", file.display()),
        name: "main".into(),
        kind: "function".into(),
        file: file.into(),
        range: Range {
            start: Position { line, character: 0 },
            end: Position {
                line: line + 1,
                character: 0,
            },
        },
        selection_range: Range {
            start: Position { line, character: 3 },
            end: Position { line, character: 7 },
        },
        container: None,
    }
}
fn reply(app: &mut App, id: u64, file: PathBuf, symbols: Vec<Symbol>) {
    app.apply_event(AnalyzerEvent::FileSymbols { id, file, symbols });
}

#[tokio::test]
async fn cargo_binary_identity_and_short_main_do_not_depend_on_rankings() {
    let (_t, mut app) = fixture(true);
    let r = app.maintainability.as_ref().unwrap();
    assert!(!r
        .longest_functions
        .iter()
        .chain(&r.deepest_functions)
        .any(|f| f.path == std::path::Path::new("src/main.rs") && f.name == "main"));
    let (_, a) = actions::available(&app, actions::Start::Entry, true);
    let entries: Vec<_> = a
        .iter()
        .filter_map(|a| {
            if let Action::BinaryEntrypoint(t) = a {
                Some(t)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(entries.len(), 3); // library and example are deliberately excluded
    assert_eq!(
        entries
            .iter()
            .filter(|t| t.path == std::path::Path::new("src/main.rs"))
            .count(),
        2
    );
    assert_eq!(
        entries
            .iter()
            .map(|t| t.id())
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        3
    );
    app.maintainability = None;
    let (tx, mut rx) = mpsc::channel(16);
    let (id, file) = choose(&mut app, "entry", &tx, &mut rx).await;
    let expected = main_symbol(&file, 0);
    reply(&mut app, id, file.clone(), vec![expected.clone()]);
    flush(&mut app, &tx).await;
    let AnalyzerCommand::LoadCalls { target, .. } = rx.recv().await.unwrap() else {
        panic!()
    };
    assert_eq!(target.file, file);
    assert_eq!(target.selection_range, expected.selection_range);
    assert_eq!(app.journey.binary_context.as_ref().unwrap().0.name, "entry");
    assert!(app.journey.history.is_empty()); // Calls → file → Calls is a bridge, not a cycle
    reply(&mut app, id, file, vec![expected]);
    flush(&mut app, &tx).await;
    assert!(app.journey.history.is_empty());
    assert!(rx.try_recv().is_err()); // repeated RA events cannot duplicate navigation
}

#[tokio::test]
async fn calls_empty_unsupported_and_rpc_error_are_not_conflated() {
    let (_t, mut app) = fixture(true);
    let (tx, mut rx) = mpsc::channel(16);
    let (id, file) = choose(&mut app, "entry", &tx, &mut rx).await;
    reply(&mut app, id, file.clone(), vec![main_symbol(&file, 0)]);
    flush(&mut app, &tx).await;
    let AnalyzerCommand::LoadCalls { id, .. } = rx.recv().await.unwrap() else {
        panic!()
    };
    app.apply_event(AnalyzerEvent::CallersLoaded {
        id,
        result: Some(crate::model::CallTargets {
            items: vec![],
            truncated: false,
        }),
    });
    app.apply_event(AnalyzerEvent::CalleesLoaded { id, result: None });
    let mut t = ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 35)).unwrap();
    t.draw(|f| super::super::ui::render(f, &app)).unwrap();
    let text = t
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect::<String>();
    assert!(text.contains("No callers in selected scope"));
    assert!(text.contains("not supported"));
    app.apply_event(AnalyzerEvent::CallsFailed {
        id,
        incoming: false,
        message: "RPC failed".into(),
    });
    t.draw(|f| super::super::ui::render(f, &app)).unwrap();
    let text = t
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect::<String>();
    assert!(text.contains("RA error: RPC failed"));
    assert!(!text.contains("not supported"));
}

#[tokio::test]
async fn binary_requests_reject_stale_cancelled_map_and_ambiguous_results() {
    let (_t, mut app) = fixture(true);
    let (tx, mut rx) = mpsc::channel(16);
    let (old, old_file) = choose(&mut app, "entry", &tx, &mut rx).await;
    let (new, file) = choose(&mut app, "other", &tx, &mut rx).await;
    reply(
        &mut app,
        old,
        old_file.clone(),
        vec![main_symbol(&old_file, 0)],
    );
    assert_eq!(app.journey.pending.as_ref().unwrap().id, new);
    assert_eq!(app.view, View::Explorer);
    reply(&mut app, new, file.clone(), vec![main_symbol(&file, 1)]);
    assert_eq!(app.view, View::Calls);
    assert_eq!(app.calls.center.as_ref().unwrap().file, file);
    cancel(&mut app); // discard test outbox before the next user action
    for leave in [K::Esc, K::Char('5')] {
        let (id, file) = choose(&mut app, "entry", &tx, &mut rx).await;
        key(&mut app, leave, &tx).await;
        let view = app.view;
        reply(&mut app, id, file.clone(), vec![main_symbol(&file, 0)]);
        flush(&mut app, &tx).await;
        assert_eq!(app.view, view);
        assert!(rx.try_recv().is_err());
    }
    let (id, file) = choose(&mut app, "entry", &tx, &mut rx).await;
    reply(
        &mut app,
        id,
        file.clone(),
        vec![main_symbol(&file, 0), main_symbol(&file, 10)],
    );
    assert!(app.notice.as_ref().unwrap().contains("Ambiguous"));
    assert!(app.journey.pending.is_none());
    assert_eq!(app.view, View::Explorer);
    assert!(app.explorer.selected_symbol().is_none());
}

#[tokio::test]
async fn missing_nested_wrong_kind_truncation_and_long_main_remain_honest() {
    let (_t, mut app) = fixture(true);
    let (tx, mut rx) = mpsc::channel(16);
    for truncated in [false, true] {
        let (id, file) = choose(&mut app, "entry", &tx, &mut rx).await;
        let mut nested = main_symbol(&file, 0);
        nested.container = Some("nested".into());
        let mut wrong = main_symbol(&file, 1);
        wrong.kind = "method".into();
        let mut foreign = main_symbol(std::path::Path::new("src/other.rs"), 0);
        foreign.container = None;
        let mut symbols = vec![nested, wrong, foreign];
        if truncated {
            for i in 0..210 {
                let mut s = main_symbol(&file, i + 5);
                s.name = format!("item{i}");
                symbols.push(s);
            }
        }
        reply(&mut app, id, file, symbols);
        assert_eq!(app.view, View::Explorer);
        assert!(app.journey.pending.is_none());
        assert!(app.notice.as_ref().unwrap().contains(if truncated {
            "truncated"
        } else {
            "no top-level callable main"
        }));
    }
    let (id, file) = choose(&mut app, "entry", &tx, &mut rx).await;
    let mut symbols: Vec<_> = (0..250)
        .map(|i| {
            let mut s = main_symbol(&file, i);
            s.name = format!("item{i}");
            s
        })
        .collect();
    let mut long = main_symbol(&file, 300);
    long.range.end.line = 600;
    symbols.push(long.clone());
    reply(&mut app, id, file, symbols);
    assert_eq!(app.view, View::Calls);
    assert_eq!(
        app.calls.center.as_ref().unwrap().selection_range,
        long.selection_range
    );
    assert!(app.explorer.file.as_ref().unwrap().truncated);
    assert!(
        app.explorer.file.as_ref().unwrap().symbols.len()
            <= super::super::explorer::FILE_SYMBOL_LIMIT
    );
    assert_eq!(
        app.explorer.selected_symbol().unwrap().selection_range,
        long.selection_range
    );
}

#[tokio::test]
async fn indexing_has_one_ready_retry_and_missing_ra_keeps_binary_source_usable() {
    let (_t, mut app) = fixture(true);
    app.analyzer_state = AnalyzerState::Indexing;
    let (tx, mut rx) = mpsc::channel(16);
    let (old, file) = choose(&mut app, "entry", &tx, &mut rx).await;
    reply(&mut app, old, file.clone(), vec![main_symbol(&file, 0)]);
    assert_eq!(app.view, View::Explorer); // even one provisional main must wait
    assert!(app.explorer.file.as_ref().unwrap().error.is_none());
    assert!(app.explorer.file.as_ref().unwrap().retry_when_ready);
    app.apply_event(AnalyzerEvent::Ready);
    let (new, path) = app.explorer.retry_anchor().unwrap();
    rebind(&mut app, new, &path);
    reply(&mut app, old, file.clone(), vec![main_symbol(&file, 0)]);
    assert_eq!(app.journey.pending.as_ref().unwrap().id, new);
    reply(&mut app, new, file.clone(), vec![]);
    assert!(app.journey.pending.is_none());
    assert!(app.explorer.retry_anchor().is_none());
    app.analyzer_state = AnalyzerState::Error("rust-analyzer not installed".into());
    let (id, file) = choose(&mut app, "entry", &tx, &mut rx).await;
    assert_eq!(app.view, View::Explorer);
    assert_eq!(app.explorer.file.as_ref().unwrap().path, file);
    assert!(!app.explorer.file.as_ref().unwrap().loading);
    assert!(app.explorer.file.as_ref().unwrap().entrypoint.is_some());
    assert!(app.journey.pending.is_none());
    app.apply_event(AnalyzerEvent::FileSymbolsFailed {
        id,
        file,
        message: "not installed".into(),
    });
    assert!(app
        .explorer
        .file
        .as_ref()
        .unwrap()
        .anchor_notice
        .as_ref()
        .unwrap()
        .contains("Call hierarchy unavailable"));
    assert!(rx.try_recv().is_err());
    let (_t, lib) = fixture(false);
    let (_, actions) = actions::available(&lib, actions::Start::Entry, true);
    assert!(actions.is_empty());
    assert!(
        actions::unavailable_reason(&lib, actions::Start::Entry, true)
            .contains("No binary targets")
    );
    let (_, roots) = actions::available(&lib, actions::Start::Entry, false);
    assert!(roots
        .iter()
        .any(|a| matches!(a,Action::File(_,s)if s.starts_with("Open library root"))));
    app.analyzer_state = AnalyzerState::Indexing;
    let (id, file) = choose(&mut app, "entry", &tx, &mut rx).await;
    reply(&mut app, id, file.clone(), vec![main_symbol(&file, 0)]);
    assert!(app.explorer.file.as_ref().unwrap().retry_when_ready);
    key(&mut app, K::Esc, &tx).await;
    app.apply_event(AnalyzerEvent::Ready);
    assert!(app.explorer.retry_anchor().is_none());
    reply(&mut app, id, file.clone(), vec![main_symbol(&file, 0)]);
    assert_eq!(app.view, View::Explorer);
    assert!(rx.try_recv().is_err());
}

#[tokio::test]
async fn navigation_timeout_is_bounded_does_not_reset_on_rebind_and_cannot_reopen_calls() {
    let (_t, mut app) = fixture(true);
    app.analyzer_state = AnalyzerState::Indexing;
    let (tx, mut rx) = mpsc::channel(16);
    let (id, file) = choose(&mut app, "entry", &tx, &mut rx).await;
    let expired = std::time::Instant::now();
    app.journey.pending.as_mut().unwrap().deadline = expired;
    rebind(&mut app, id, &file);
    assert_eq!(app.journey.pending.as_ref().unwrap().deadline, expired);
    flush(&mut app, &tx).await;
    assert!(app.journey.pending.is_none());
    assert!(!app.explorer.file.as_ref().unwrap().loading);
    assert!(app.notice.as_ref().unwrap().contains("timed out"));
    app.apply_event(AnalyzerEvent::Ready);
    assert!(app.explorer.retry_anchor().is_none());
    reply(&mut app, id, file.clone(), vec![main_symbol(&file, 0)]);
    assert_eq!(app.view, View::Explorer);
    assert!(rx.try_recv().is_err());
}

#[tokio::test]
async fn real_ra_short_and_async_binary_entrypoints_resolve_without_ranked_main() {
    if !std::process::Command::new("rust-analyzer")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
    {
        assert!(std::env::var_os("CI").is_none(), "RA required in CI");
        eprintln!("SKIP: real RA binary entrypoint integration, rust-analyzer unavailable");
        return;
    }
    let (_t, mut app) = fixture(true);
    let r = app.maintainability.as_ref().unwrap();
    assert!(!r
        .longest_functions
        .iter()
        .chain(&r.deepest_functions)
        .any(|f| f.name == "main"));
    let mut ra = crate::rust::RustAnalyzer::start_passive(
        app.project.clone(),
        std::path::Path::new("rust-analyzer"),
    )
    .await
    .unwrap();
    ra.wait_ready().await.unwrap();
    let (tx, mut rx) = mpsc::channel(16);
    for name in ["entry", "other"] {
        let (id, file) = choose(&mut app, name, &tx, &mut rx).await;
        let symbols = ra.symbols_for_file(&file).await.unwrap();
        let actual = symbols
            .iter()
            .find(|s| s.name == "main" && s.container.is_none())
            .unwrap()
            .clone();
        reply(&mut app, id, file.clone(), symbols);
        flush(&mut app, &tx).await;
        let AnalyzerCommand::LoadCalls { id, target, scope } = rx.recv().await.unwrap() else {
            panic!()
        };
        assert_eq!(target.file, file);
        assert_eq!(target.selection_range, actual.selection_range);
        assert_eq!(app.journey.binary_context.as_ref().unwrap().0.name, name);
        let outgoing = ra
            .callees_for_target_scoped(&target, scope)
            .await
            .unwrap()
            .expect("callable supported");
        assert!(outgoing.items.iter().any(|c| c.name == "leaf"));
        let incoming = ra.callers_for_target_scoped(&target, scope).await.unwrap();
        app.apply_event(AnalyzerEvent::CallersLoaded {
            id,
            result: incoming,
        });
        app.apply_event(AnalyzerEvent::CalleesLoaded {
            id,
            result: Some(outgoing),
        });
        key(&mut app, K::Char('o'), &tx).await;
        let AnalyzerCommand::InspectFile { id, file } = rx.recv().await.unwrap() else {
            panic!()
        };
        let symbols = ra.symbols_for_file(&file).await.unwrap();
        reply(&mut app, id, file, symbols);
        flush(&mut app, &tx).await;
        let AnalyzerCommand::Inspect { symbol, .. } = rx.recv().await.unwrap() else {
            panic!()
        };
        assert_eq!(symbol.selection_range, actual.selection_range);
        assert_eq!(app.explorer.right_mode, ExplorerRightMode::Inspector);
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 35)).unwrap();
        terminal
            .draw(|frame| super::super::ui::render(frame, &app))
            .unwrap();
        let rendered = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|c| c.symbol())
            .collect::<String>();
        assert!(rendered.contains("Esc returns to Calls"));
        key(&mut app, K::Esc, &tx).await;
        assert_eq!(app.view, View::Calls);
        assert_eq!(
            app.calls.center.as_ref().unwrap().selection_range,
            actual.selection_range
        );
    }
    ra.shutdown().await;
}

#[tokio::test]
async fn returning_from_source_resumes_calls_superseded_between_worker_phases() {
    let (_t, mut app) = fixture(true);
    let (tx, mut rx) = mpsc::channel(16);
    let (id, file) = choose(&mut app, "entry", &tx, &mut rx).await;
    let main = main_symbol(&file, 0);
    reply(&mut app, id, file.clone(), vec![main.clone()]);
    flush(&mut app, &tx).await;
    let AnalyzerCommand::LoadCalls {
        id: old,
        target: original,
        ..
    } = rx.recv().await.unwrap()
    else {
        panic!()
    };
    // Only the first phase has completed when InspectFile replaces remaining work.
    app.apply_event(AnalyzerEvent::CallersLoaded {
        id: old,
        result: Some(crate::model::CallTargets {
            items: vec![],
            truncated: false,
        }),
    });
    key(&mut app, K::Char('o'), &tx).await;
    let AnalyzerCommand::InspectFile { id, file } = rx.recv().await.unwrap() else {
        panic!()
    };
    reply(&mut app, id, file, vec![main]);
    flush(&mut app, &tx).await;
    assert!(matches!(
        rx.recv().await,
        Some(AnalyzerCommand::Inspect { .. })
    ));
    key(&mut app, K::Esc, &tx).await;
    flush(&mut app, &tx).await;
    let AnalyzerCommand::LoadCalls {
        id: new, target, ..
    } = rx.recv().await.unwrap()
    else {
        panic!()
    };
    assert!(new > old);
    assert_eq!(target, original);
    assert_eq!(app.view, View::Calls);
    app.apply_event(AnalyzerEvent::CalleesLoaded {
        id: old,
        result: None,
    });
    assert!(app.calls.callees_error.is_none());
    assert!(app.calls.callees_loading);
    app.apply_event(AnalyzerEvent::CalleesLoaded {
        id: new,
        result: Some(crate::model::CallTargets {
            items: vec![],
            truncated: false,
        }),
    });
    assert!(!app.calls.callees_loading);
}
