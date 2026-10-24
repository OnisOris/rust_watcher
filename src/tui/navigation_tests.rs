use super::{
    app::{AnalyzerState, App, Pane, View},
    event::handle_key,
    overview::{self, Target},
    worker::{AnalyzerCommand, AnalyzerEvent},
};
use crate::{
    maintainability::{self, FunctionMetrics},
    model::{Position, Range, Symbol},
    project::Project,
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::path::Path;
use tokio::sync::mpsc;

fn fixture() -> (tempfile::TempDir, App, FunctionMetrics) {
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join("src/nested")).unwrap();
    std::fs::write(
        temp.path().join("Cargo.toml"),
        "[package]\nname='anchor_fixture'\nversion='0.1.0'\n",
    )
    .unwrap();
    std::fs::write(
        temp.path().join("src/lib.rs"),
        "mod nested { pub mod methods; }\n",
    )
    .unwrap();
    std::fs::write(temp.path().join("src/nested/methods.rs"),"pub struct A;\nimpl A {\n pub fn run(&self) {\n  if true {}\n }\n}\npub struct B;\nimpl B { pub fn run(&self) {} }\n").unwrap();
    let project = Project::discover(temp.path()).unwrap();
    let tree = crate::repository::scan(temp.path()).unwrap();
    let report = maintainability::analyze(temp.path(), &tree, &project);
    let anchor = report.longest_functions[0].clone();
    let mut app = App::new(project);
    app.explorer.apply_tree(tree);
    app.maintainability = Some(report);
    app.project_selection = overview::targets(&app)
        .iter()
        .position(|t| matches!(t, Target::Longest(_)))
        .unwrap();
    (temp, app, anchor)
}

fn symbol(anchor: &FunctionMetrics) -> Symbol {
    Symbol {
        id: "exact".into(),
        name: anchor.name.clone(),
        kind: "method".into(),
        file: anchor.path.clone(),
        range: Range {
            start: Position {
                line: (anchor.start_line - 1) as u32,
                character: 0,
            },
            end: Position {
                line: (anchor.end_line - 1) as u32,
                character: 1,
            },
        },
        selection_range: Range {
            start: Position {
                line: (anchor.identifier_line - 1) as u32,
                character: (anchor.identifier_column - 1) as u32,
            },
            end: Position {
                line: (anchor.identifier_line - 1) as u32,
                character: ((anchor.identifier_column - 1) + anchor.name.len()) as u32,
            },
        },
        container: Some("A".into()),
    }
}

async fn open_candidate(
    app: &mut App,
) -> (
    mpsc::Sender<AnalyzerCommand>,
    mpsc::Receiver<AnalyzerCommand>,
    u64,
) {
    let (tx, mut rx) = mpsc::channel(16);
    handle_key(app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), &tx)
        .await
        .unwrap();
    assert_eq!(app.view, View::Explorer);
    assert_eq!(app.pane, Pane::ExplorerFile);
    let Some(AnalyzerCommand::InspectFile { id, file }) = rx.recv().await else {
        panic!("must inspect exact file, never name search")
    };
    assert_eq!(file, Path::new("src/nested/methods.rs"));
    assert!(app.explorer.is_expanded(Path::new("src/nested")));
    (tx, rx, id)
}

#[tokio::test]
async fn exact_anchor_handles_duplicate_names_missing_invalid_ambiguous_and_truncation() {
    let (_temp, mut app, anchor) = fixture();
    let (_tx, _rx, id) = open_candidate(&mut app).await;
    let exact = symbol(&anchor);
    let mut other = exact.clone();
    other.id = "other-impl".into();
    other.selection_range.start.line = 7;
    app.apply_event(AnalyzerEvent::FileSymbols {
        id,
        file: anchor.path.clone(),
        symbols: vec![other, exact.clone()],
    });
    assert_eq!(app.explorer.selected_symbol().unwrap().id, "exact");
    app.apply_event(AnalyzerEvent::FileSymbols {
        id,
        file: anchor.path.clone(),
        symbols: vec![],
    });
    assert!(app.explorer.selected_symbol().is_none());
    assert!(app
        .explorer
        .file
        .as_ref()
        .unwrap()
        .anchor_notice
        .as_ref()
        .unwrap()
        .contains("No unique"));
    let mut duplicate = exact.clone();
    duplicate.id = "duplicate".into();
    app.apply_event(AnalyzerEvent::FileSymbols {
        id,
        file: anchor.path.clone(),
        symbols: vec![exact.clone(), duplicate],
    });
    assert!(app.explorer.selected_symbol().is_none());
    let mut invalid = anchor.clone();
    invalid.identifier_line = 0;
    app.explorer.set_anchor(invalid);
    app.apply_event(AnalyzerEvent::FileSymbols {
        id,
        file: anchor.path.clone(),
        symbols: vec![exact.clone()],
    });
    assert!(app.explorer.selected_symbol().is_none());
    app.explorer.set_anchor(anchor.clone());
    let mut many = Vec::new();
    for i in 0..250 {
        let mut s = exact.clone();
        s.id = format!("before{i}");
        s.name = format!("before{i}");
        s.selection_range.start.line = 0;
        many.push(s);
    }
    many.push(exact);
    app.apply_event(AnalyzerEvent::FileSymbols {
        id,
        file: anchor.path.clone(),
        symbols: many,
    });
    assert!(app.explorer.file.as_ref().unwrap().truncated);
    assert_eq!(app.explorer.selected_symbol().unwrap().id, "exact");
    app.explorer
        .fail_file(id, &anchor.path, "RA missing".into());
    assert!(app
        .explorer
        .file
        .as_ref()
        .unwrap()
        .anchor_notice
        .as_ref()
        .unwrap()
        .contains("unavailable"));
    assert!(app.explorer.file.as_ref().unwrap().anchor.is_some());
    assert!(app.explorer.selected_symbol().is_none());
    let (retry_id, _) = app.explorer.retry_anchor().unwrap();
    assert!(retry_id > id);
    app.apply_event(AnalyzerEvent::FileSymbols {
        id,
        file: anchor.path.clone(),
        symbols: vec![symbol(&anchor)],
    });
    assert!(app.explorer.selected_symbol().is_none());
    app.apply_event(AnalyzerEvent::FileSymbols {
        id: retry_id,
        file: anchor.path.clone(),
        symbols: vec![symbol(&anchor)],
    });
    assert_eq!(app.explorer.selected_symbol().unwrap().id, "exact");
}

#[tokio::test]
async fn real_ra_file_symbols_match_exact_method_then_inspector_and_calls() {
    let available = std::process::Command::new("rust-analyzer")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success());
    if !available {
        assert!(std::env::var_os("CI").is_none(), "RA required in CI");
        eprintln!("skipping RA anchor integration: rust-analyzer unavailable");
        return;
    }
    let (_temp, mut app, anchor) = fixture();
    let (tx, mut rx, id) = open_candidate(&mut app).await;
    let mut analyzer =
        crate::rust::RustAnalyzer::start_passive(app.project.clone(), Path::new("rust-analyzer"))
            .await
            .unwrap();
    analyzer.wait_ready().await.unwrap();
    let symbols = analyzer.symbols_for_file(&anchor.path).await.unwrap();
    assert_eq!(symbols.iter().filter(|s| s.name == "run").count(), 2);
    app.apply_event(AnalyzerEvent::FileSymbols {
        id,
        file: anchor.path.clone(),
        symbols,
    });
    let selected = app
        .explorer
        .selected_symbol()
        .expect("unique anchored RA method");
    assert_eq!(
        selected.selection_range.start.line as usize + 1,
        anchor.identifier_line
    );
    handle_key(
        &mut app,
        KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        &tx,
    )
    .await
    .unwrap();
    assert!(
        matches!(rx.recv().await,Some(AnalyzerCommand::Inspect { symbol,.. }) if symbol.id==selected.id)
    );
    assert_eq!(app.inspector.as_ref().unwrap().symbol.id, selected.id);
    handle_key(
        &mut app,
        KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE),
        &tx,
    )
    .await
    .unwrap();
    assert!(
        matches!(rx.recv().await,Some(AnalyzerCommand::LoadCalls { target,.. }) if target.file==anchor.path && target.selection_range.start==selected.selection_range.start)
    );
    analyzer.shutdown().await;
}

fn render(app: &App, width: u16, height: u16) -> String {
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, height)).unwrap();
    terminal.draw(|f| super::ui::render(f, app)).unwrap();
    terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|c| c.symbol())
        .collect()
}

#[test]
fn overview_renders_loading_candidates_errors_and_small_terminal() {
    let (_temp, mut app, _) = fixture();
    let text = render(&app, 120, 34);
    assert!(text.contains("MAINTAINABILITY · Rust"));
    assert!(text.contains("LONGEST FUNCTIONS"));
    assert!(text.contains("DEEPEST CONTROL FLOW"));
    assert!(text.contains("Functions 2"));
    app.analyzer_state = AnalyzerState::Error("RA unavailable".into());
    assert!(render(&app, 80, 20).contains("Functions 2"));
    app.maintainability = None;
    assert!(render(&app, 120, 34).contains("Maintainability: analyzing..."));
    app.maintainability_error = Some("worker failed".into());
    assert!(render(&app, 120, 34).contains("Maintainability unavailable"));
}
