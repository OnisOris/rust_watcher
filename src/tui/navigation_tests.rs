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
    app.overview.expanded.extend(overview::Group::ALL);
    let selected = overview::targets(&app)
        .iter()
        .position(|t| matches!(t, Target::Longest(_)))
        .unwrap();
    app.select_overview(selected);
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

fn load_hotspot(app: &mut App, path: &Path) {
    let tree = crate::repository::scan(&app.project.workspace_root).unwrap();
    let metrics = crate::metrics::analyze(&app.project.workspace_root, &tree, &app.project);
    let ast = app.maintainability.as_ref().unwrap();
    let mut history = crate::git_analysis::History::default();
    history.since_days = 90;
    history.as_of_utc = "2026-10-25T00:00:00Z".into();
    history.commits_analyzed = 4;
    history.files.insert(
        path.into(),
        crate::git_analysis::Change {
            commits: 4,
            added: Some(12),
            deleted: Some(2),
            last: 1792886400,
        },
    );
    let mut report = crate::git_analysis::correlate(history, &metrics, ast, &tree);
    report.most_changed_files[0]
        .co_changed
        .push(crate::git_analysis::CoChanged {
            path: "src/lib.rs".into(),
            commits: 3,
        });
    app.hotspots = Some(report);
    app.overview.selected = overview::Target::Hotspot(path.into()).id();
}

#[tokio::test]
async fn hotspot_reveals_exact_file_then_reuses_symbols_inspector_and_calls() {
    let (_temp, mut app, anchor) = fixture();
    load_hotspot(&mut app, &anchor.path);
    let (tx, mut rx, id) = open_candidate(&mut app).await;
    let exact = symbol(&anchor);
    app.apply_event(AnalyzerEvent::FileSymbols {
        id,
        file: anchor.path.clone(),
        symbols: vec![exact],
    });
    handle_key(
        &mut app,
        KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        &tx,
    )
    .await
    .unwrap();
    assert!(app.inspector.is_some());
    assert_eq!(app.inspector.as_ref().unwrap().symbol.file, anchor.path);
    // File inspection is never dispatched as a name search.
    while let Ok(command) = rx.try_recv() {
        assert!(!matches!(command, AnalyzerCommand::Search { .. }));
    }
    handle_key(
        &mut app,
        KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE),
        &tx,
    )
    .await
    .unwrap();
    assert_eq!(app.view, View::Calls);
}

#[tokio::test]
async fn hotspot_missing_file_is_a_notice_not_a_panic() {
    let (temp, mut app, anchor) = fixture();
    load_hotspot(&mut app, &anchor.path);
    std::fs::remove_file(temp.path().join(&anchor.path)).unwrap();
    let (tx, mut rx) = mpsc::channel(16);
    handle_key(
        &mut app,
        KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        &tx,
    )
    .await
    .unwrap();
    assert_eq!(app.view, View::Overview);
    assert!(app.notice.as_ref().unwrap().contains("unavailable"));
    assert!(rx.try_recv().is_err());
}

#[test]
fn hotspot_states_and_details_render_without_ra_including_small_terminal() {
    use ratatui::{backend::TestBackend, Terminal};
    fn render(app: &App, width: u16, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|f| super::ui::render(f, app)).unwrap();
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect()
    }
    let (_temp, mut app, anchor) = fixture();
    app.overview.selected = super::overview::ItemId::Workspace;
    assert!(render(&app, 160, 48).contains("Git history: analyzing..."));
    app.hotspots_error = Some("Git executable not found".into());
    assert!(render(&app, 160, 48).contains("Git executable not found"));
    app.hotspots_error = None;
    load_hotspot(&mut app, &anchor.path);
    app.analyzer_state = AnalyzerState::Error("RA unavailable".into());
    let text = render(&app, 160, 48);
    for part in [
        "GIT HOTSPOTS",
        "HOTSPOT DETAILS",
        "Changes / 90d",
        "Added lines",
        "Max control depth",
        "Co-changed with",
        "src/lib.rs",
        "RA",
    ] {
        assert!(text.contains(part), "missing {part}: {text}");
    }
    assert!(render(&app, 80, 20).contains("HOTSPOT DETAILS"));
    app.hotspots.as_mut().unwrap().partial = true;
    app.hotspots.as_mut().unwrap().current_metrics_partial = true;
    app.overview.selected = super::overview::ItemId::Workspace;
    assert!(render(&app, 160, 48).contains("partial"));
    let r = app.hotspots.as_mut().unwrap();
    r.available = false;
    r.most_changed_files.clear();
    r.reason = Some(crate::git_analysis::Unavailable {
        code: "gitError".into(),
        message: "corrupt history".into(),
    });
    assert!(render(&app, 160, 48).contains("Git history unavailable: corrupt history"));
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
    app.overview = overview::State::default();
    let text = render(&app, 120, 34);
    assert!(text.contains("MAINTAINABILITY · Rust"));
    assert!(text.contains("LONGEST FUNCTIONS"));
    assert!(text.contains("DEEPEST CONTROL FLOW"));
    assert!(text.contains("Functions") && text.contains("2"));
    app.analyzer_state = AnalyzerState::Error("RA unavailable".into());
    assert!(render(&app, 80, 20).contains("Functions"));
    app.maintainability = None;
    assert!(render(&app, 120, 34).contains("Maintainability: analyzing..."));
    app.maintainability_error = Some("worker failed".into());
    assert!(render(&app, 120, 34).contains("Maintainability unavailable"));
}
