use super::super::{event::handle_key, worker::AnalyzerEvent};
use super::*;
use crate::{
    maintainability,
    model::{Position, Range, Symbol},
    project::Project,
};
use crossterm::event::{KeyCode as K, KeyEvent, KeyModifiers};
use tokio::sync::mpsc;
fn fixture() -> (tempfile::TempDir, App, Vec<FunctionMetrics>) {
    let t = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(t.path().join("src")).unwrap();
    std::fs::write(
        t.path().join("Cargo.toml"),
        "[package]\nname='journey'\nversion='0.1.0'\nedition='2021'\n",
    )
    .unwrap();
    std::fs::write(t.path().join("src/lib.rs"), "mod work;").unwrap();
    std::fs::write(t.path().join("src/work.rs"),"pub struct A;\nimpl A {\n pub fn run(&self) {\n  if true {}\n }\n}\npub struct B;\nimpl B { pub fn run(&self) {} }\n").unwrap();
    let p = Project::discover(t.path()).unwrap();
    let tree = crate::repository::scan(t.path()).unwrap();
    let r = maintainability::analyze(t.path(), &tree, &p);
    let f = r.longest_functions.clone();
    let g = crate::module_graph::analyze(t.path(), &tree, &p.architecture, || false);
    let work = g
        .nodes
        .iter()
        .find(|n| n.name == "work")
        .unwrap()
        .id
        .clone();
    let mut app = App::new(p);
    app.explorer.apply_tree(tree);
    app.maintainability = Some(r);
    app.module_map.apply(g);
    app.module_map.selected = work.clone();
    app.module_map.focus = work;
    app.module_map.mode = super::super::module_map::Mode::Imports;
    app.module_map.diagram = false;
    app.switch_view(View::Map);
    app.analyzer_state = AnalyzerState::Ready;
    (t, app, f)
}
fn symbol(f: &FunctionMetrics) -> Symbol {
    let start = Position {
        line: (f.identifier_line - 1) as u32,
        character: (f.identifier_column - 1) as u32,
    };
    Symbol {
        id: format!(
            "{}:{}:{}",
            f.path.display(),
            f.identifier_line,
            f.identifier_column
        ),
        name: f.name.clone(),
        kind: "method".into(),
        file: f.path.clone(),
        range: Range {
            start: Position {
                line: (f.start_line - 1) as u32,
                character: 0,
            },
            end: Position {
                line: (f.end_line - 1) as u32,
                character: 99,
            },
        },
        selection_range: Range { start, end: start },
        container: Some(f.qualified_name.clone()),
    }
}
#[tokio::test]
async fn real_ra_priority_calls_uses_exact_duplicate_method_position() {
    if !std::process::Command::new("rust-analyzer")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
    {
        assert!(
            std::env::var_os("CI").is_none(),
            "rust-analyzer required in CI"
        );
        eprintln!("skipping priority navigation integration: rust-analyzer unavailable");
        return;
    }
    let (_temp, mut app, functions) = fixture();
    let f = functions
        .iter()
        .find(|f| f.qualified_name.contains("B::run"))
        .unwrap();
    let (tx, mut rx) = mpsc::channel(16);
    let id = open(&mut app, f, true, &tx, &mut rx).await;
    let mut ra = crate::rust::RustAnalyzer::start_passive(
        app.project.clone(),
        std::path::Path::new("rust-analyzer"),
    )
    .await
    .unwrap();
    ra.wait_ready().await.unwrap();
    let symbols = ra.symbols_for_file(&f.path).await.unwrap();
    assert_eq!(symbols.iter().filter(|s| s.name == "run").count(), 2);
    app.apply_event(AnalyzerEvent::FileSymbols {
        id,
        file: f.path.clone(),
        symbols,
    });
    flush(&mut app, &tx).await;
    let AnalyzerCommand::LoadCalls { target, .. } = rx.recv().await.unwrap() else {
        panic!("expected exact Calls")
    };
    assert_eq!(app.view, View::Calls);
    assert_eq!(target.file, f.path);
    assert_eq!(
        target.selection_range.start.line as usize + 1,
        f.identifier_line
    );
    assert_eq!(
        target.selection_range.start.character as usize + 1,
        f.identifier_column
    );
    let neighbors = ra
        .callees_for_target_scoped(&target, crate::model::CallScope::Workspace)
        .await;
    ra.shutdown().await;
    assert!(neighbors.is_ok());
}
#[tokio::test]
async fn symbol_and_call_start_routes_open_real_candidates_or_explain_unavailability() {
    let (_t, mut app, functions) = fixture();
    let (tx, mut rx) = mpsc::channel(16);
    app.switch_view(View::Symbols);
    key(&mut app, K::Char('j'), &tx).await;
    key(&mut app, K::Enter, &tx).await;
    assert!(matches!(
        app.journey.picker.as_ref().unwrap().actions[0],
        Action::Function(_)
    ));
    key(&mut app, K::Char('c'), &tx).await;
    let AnalyzerCommand::InspectFile { id, file } = rx.recv().await.unwrap() else {
        panic!()
    };
    assert_eq!(file, functions[0].path);
    app.apply_event(AnalyzerEvent::FileSymbols {
        id,
        file,
        symbols: functions.iter().map(symbol).collect(),
    });
    flush(&mut app, &tx).await;
    assert!(matches!(
        rx.recv().await,
        Some(AnalyzerCommand::LoadCalls { .. })
    ));
    assert_eq!(app.view, View::Calls);
    key(&mut app, K::Esc, &tx).await;
    key(&mut app, K::Esc, &tx).await;
    assert_eq!(app.view, View::Symbols);
    app.journey.start_selection = 3;
    let mut t = ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 20)).unwrap();
    t.draw(|f| super::super::ui::render(f, &app)).unwrap();
    let text = t
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect::<String>();
    assert!(text.contains("Hotspots: analyzing..."));
    app.switch_view(View::Calls);
    app.calls.center = None;
    app.journey.start_selection = 0;
    key(&mut app, K::Enter, &tx).await;
    assert!(app.journey.picker.is_none()); // library, not a guessed main()
    assert!(app
        .notice
        .as_ref()
        .unwrap()
        .contains("no known exact anchor"));
    app.analyzer_state = AnalyzerState::Error("missing RA".into());
    app.journey.start_selection = 1;
    key(&mut app, K::Enter, &tx).await;
    assert!(app.journey.picker.is_none());
    assert!(rx.try_recv().is_err());
}
async fn key(app: &mut App, k: K, tx: &mpsc::Sender<AnalyzerCommand>) {
    handle_key(app, KeyEvent::new(k, KeyModifiers::NONE), tx)
        .await
        .unwrap();
}
async fn open(
    app: &mut App,
    f: &FunctionMetrics,
    calls: bool,
    tx: &mpsc::Sender<AnalyzerCommand>,
    rx: &mut mpsc::Receiver<AnalyzerCommand>,
) -> u64 {
    key(app, K::Char('p'), tx).await;
    let picker = app.journey.picker.as_mut().unwrap();
    picker.selected=picker.actions.iter().position(|a|matches!(a,Action::Function(a)if a.identifier_line==f.identifier_line&&a.identifier_column==f.identifier_column)).unwrap();
    key(app, if calls { K::Char('c') } else { K::Enter }, tx).await;
    match rx.recv().await.unwrap() {
        AnalyzerCommand::InspectFile { id, file } => {
            assert_eq!(file, f.path);
            id
        }
        other => panic!("not exact file: {other:?}"),
    }
}
#[tokio::test]
async fn priorities_open_exact_inspector_calls_and_restore_module_context() {
    let (_t, mut app, fs) = fixture();
    let origin = app.module_map.selected.clone();
    let (tx, mut rx) = mpsc::channel(16);
    let id = open(&mut app, &fs[0], false, &tx, &mut rx).await;
    assert_eq!(app.view, View::Explorer);
    assert!(app.journey.pending.is_some());
    app.apply_event(AnalyzerEvent::FileSymbols {
        id,
        file: fs[0].path.clone(),
        symbols: fs.iter().rev().map(symbol).collect(),
    });
    assert_eq!(app.explorer.right_mode, ExplorerRightMode::Inspector);
    assert_eq!(
        app.inspector.as_ref().unwrap().symbol.selection_range.start,
        symbol(&fs[0]).selection_range.start
    );
    flush(&mut app, &tx).await;
    assert!(matches!(
        rx.recv().await,
        Some(AnalyzerCommand::Inspect { .. })
    ));
    key(&mut app, K::Char('c'), &tx).await;
    let request = match rx.recv().await.unwrap() {
        AnalyzerCommand::LoadCalls { id, target, .. } => {
            assert_eq!(
                target.selection_range.start,
                symbol(&fs[0]).selection_range.start
            );
            id
        }
        _ => panic!(),
    };
    key(&mut app, K::Esc, &tx).await;
    assert_eq!(app.view, View::Explorer);
    assert_eq!(app.explorer.right_mode, ExplorerRightMode::Inspector);
    app.apply_event(AnalyzerEvent::CallersLoaded {
        id: request,
        result: None,
    });
    assert_eq!(app.view, View::Explorer);
    key(&mut app, K::Esc, &tx).await;
    key(&mut app, K::Esc, &tx).await;
    assert_eq!(app.view, View::Map);
    assert_eq!(app.module_map.selected, origin);
    assert!(!app.module_map.diagram);
    assert_eq!(app.module_map.mode, super::super::module_map::Mode::Imports);
    let id = open(&mut app, &fs[1], true, &tx, &mut rx).await;
    app.apply_event(AnalyzerEvent::FileSymbols {
        id,
        file: fs[1].path.clone(),
        symbols: fs.iter().map(symbol).collect(),
    });
    assert_eq!(app.view, View::Calls);
    assert_eq!(
        app.calls.center.as_ref().unwrap().selection_range.start,
        symbol(&fs[1]).selection_range.start
    );
    flush(&mut app, &tx).await;
    assert!(matches!(
        rx.recv().await,
        Some(AnalyzerCommand::LoadCalls { .. })
    ));
}
#[tokio::test]
async fn delayed_responses_new_intent_escape_errors_and_ambiguity_never_choose_random_symbol() {
    let (_t, mut app, fs) = fixture();
    let (tx, mut rx) = mpsc::channel(16);
    let old = open(&mut app, &fs[0], true, &tx, &mut rx).await;
    key(&mut app, K::Char('5'), &tx).await;
    let new = open(&mut app, &fs[1], true, &tx, &mut rx).await;
    app.apply_event(AnalyzerEvent::FileSymbols {
        id: old,
        file: fs[0].path.clone(),
        symbols: vec![symbol(&fs[0])],
    });
    assert_eq!(app.view, View::Explorer);
    assert_eq!(app.journey.pending.as_ref().unwrap().id, new);
    key(&mut app, K::Esc, &tx).await;
    assert!(app.journey.pending.is_none());
    app.apply_event(AnalyzerEvent::FileSymbols {
        id: new,
        file: fs[1].path.clone(),
        symbols: vec![symbol(&fs[1])],
    });
    assert_eq!(app.view, View::Explorer);
    assert!(app.journey.dispatch.is_none());
    key(&mut app, K::Char('5'), &tx).await;
    let id = open(&mut app, &fs[0], true, &tx, &mut rx).await;
    app.apply_event(AnalyzerEvent::FileSymbols {
        id,
        file: fs[0].path.clone(),
        symbols: vec![symbol(&fs[0]), symbol(&fs[0])],
    });
    assert_eq!(app.view, View::Explorer);
    assert!(app.journey.pending.is_none());
    assert!(app.notice.as_ref().unwrap().contains("No unique"));
    key(&mut app, K::Char('5'), &tx).await;
    app.analyzer_state = AnalyzerState::Error("RA absent".into());
    let id = open(&mut app, &fs[0], true, &tx, &mut rx).await;
    assert_eq!(app.explorer.file.as_ref().unwrap().path, fs[0].path);
    assert!(app.explorer.file.as_ref().unwrap().anchor.is_some());
    assert!(app.notice.as_ref().unwrap().contains("unavailable"));
    app.apply_event(AnalyzerEvent::FileSymbolsFailed {
        id,
        file: fs[0].path.clone(),
        message: "RA absent".into(),
    });
    assert_eq!(app.view, View::Explorer);
    assert!(rx.try_recv().is_err());
}
#[tokio::test]
async fn indexing_retries_rebind_exact_intent_to_new_file_request() {
    let (_t, mut app, fs) = fixture();
    app.analyzer_state = AnalyzerState::Indexing;
    let (tx, mut rx) = mpsc::channel(16);
    let old = open(&mut app, &fs[0], true, &tx, &mut rx).await;
    app.apply_event(AnalyzerEvent::FileSymbols {
        id: old,
        file: fs[0].path.clone(),
        symbols: vec![],
    });
    assert!(app.journey.pending.is_some());
    app.apply_event(AnalyzerEvent::Ready);
    let (id, file) = app.explorer.retry_anchor().unwrap();
    rebind(&mut app, id, &file);
    assert_ne!(id, old);
    app.apply_event(AnalyzerEvent::FileSymbols {
        id: old,
        file: file.clone(),
        symbols: vec![symbol(&fs[0])],
    });
    assert_eq!(app.view, View::Explorer);
    app.apply_event(AnalyzerEvent::FileSymbols {
        id,
        file,
        symbols: vec![symbol(&fs[0])],
    });
    assert_eq!(app.view, View::Calls);
}
