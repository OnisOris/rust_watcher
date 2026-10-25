use super::*;
use crate::{
    project::Project,
    tui::{
        app::{Inspector, Overlay, View},
        event::handle_key,
        worker::AnalyzerCommand,
    },
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::style::Modifier;
use ratatui::{backend::TestBackend, Terminal};
use std::path::Path;

fn fixture() -> (tempfile::TempDir, App) {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("Cargo.toml"),
        "[workspace]\nresolver='2'\nmembers=['app','core','devtool','builder']\n",
    )
    .unwrap();
    for name in ["app", "core", "devtool", "builder"] {
        let dir = temp.path().join(name);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        let deps = if name == "app" {
            "[dependencies]\ncore={path='../core'}\n[dev-dependencies]\ndevtool={path='../devtool'}\n[build-dependencies]\nbuilder={path='../builder'}\n"
        } else {
            ""
        };
        std::fs::write(
            dir.join("Cargo.toml"),
            format!("[package]\nname='{name}'\nversion='0.1.0'\nedition='2021'\n{deps}"),
        )
        .unwrap();
        std::fs::write(dir.join("src/lib.rs"), "pub fn run() {}\n").unwrap();
    }
    std::fs::write(temp.path().join("app/src/main.rs"), "fn main() {}\n").unwrap();
    let project = Project::discover(temp.path()).unwrap();
    let mut app = App::new(project);
    app.explorer
        .apply_tree(crate::repository::scan(temp.path()).unwrap());
    app.switch_view(View::Map);
    app.color_mode = theme::ColorMode::TrueColor;
    (temp, app)
}
fn render(app: &App, width: u16, height: u16) -> ratatui::buffer::Buffer {
    let mut t = Terminal::new(TestBackend::new(width, height)).unwrap();
    t.draw(|f| super::super::ui::render(f, app)).unwrap();
    t.backend().buffer().clone()
}
fn text(buffer: &ratatui::buffer::Buffer) -> String {
    buffer.content.iter().map(|c| c.symbol()).collect()
}

#[test]
fn incomplete_crate_ast_stays_unknown_and_tree_selection_cannot_reuse_old_symbols() {
    let (temp, mut app) = fixture();
    let tree = crate::repository::scan(temp.path()).unwrap();
    let mut report = crate::maintainability::analyze(temp.path(), &tree, &app.project);
    app.map.selected = "crate:core/Cargo.toml".into();
    app.maintainability = Some(report.clone());
    let known = inspector::details(&app)
        .into_iter()
        .map(|l| l.to_string())
        .collect::<Vec<_>>();
    assert!(known
        .iter()
        .any(|l| l.starts_with("Functions") && l.ends_with('1')));
    report
        .files
        .iter_mut()
        .find(|f| f.path == Path::new("core/src/lib.rs"))
        .unwrap()
        .structure = None;
    app.maintainability = Some(report);
    let unknown = inspector::details(&app)
        .into_iter()
        .map(|l| l.to_string())
        .collect::<Vec<_>>();
    assert!(unknown
        .iter()
        .any(|l| l.starts_with("Functions") && l.ends_with("unknown")));
    app.switch_view(View::Explorer);
    app.explorer.reveal_file(Path::new("app/src/lib.rs"));
    app.explorer.open_selected();
    app.explorer.reveal_file(Path::new("core/src/lib.rs"));
    assert!(app.begin_explorer_symbol_inspect().is_none());
    assert!(text(&render(&app, 100, 30)).contains("TREE SELECTION"));
}

#[tokio::test]
async fn stable_ids_survive_background_insertions_and_open_the_same_file() {
    use crate::tui::{
        event::apply_repository,
        overview::{self, Target},
        RepositoryEvent,
    };
    let (temp, mut app) = fixture();
    let tree = crate::repository::scan(temp.path()).unwrap();
    let metrics = crate::metrics::analyze(temp.path(), &tree, &app.project);
    apply_repository(&mut app, RepositoryEvent::Metrics(metrics.clone()));
    app.switch_view(View::Overview);
    app.overview.expanded.insert(overview::Group::Largest);
    let selected = overview::targets(&app)
        .into_iter()
        .find(|t| matches!(t, Target::File(..)))
        .unwrap();
    let Target::File(path, _) = &selected else {
        unreachable!()
    };
    let path = path.clone();
    app.overview.selected = selected.id();
    let id = app.overview.selected.clone();
    app.map.selected = "crate:core/Cargo.toml".into();
    app.metrics_scroll = 7;
    app.pane = Pane::Details;
    let mut reordered = metrics.clone();
    reordered.largest_files.reverse();
    apply_repository(&mut app, RepositoryEvent::Metrics(reordered));
    let ast = crate::maintainability::analyze(temp.path(), &tree, &app.project);
    apply_repository(&mut app, RepositoryEvent::Maintainability(ast.clone()));
    let mut history = crate::git_analysis::History::default();
    history.commits_analyzed = 3;
    history.since_days = 90;
    history.files.insert(
        "core/src/lib.rs".into(),
        crate::git_analysis::Change {
            commits: 3,
            added: Some(3),
            deleted: Some(1),
            last: 1792886400,
        },
    );
    let report = crate::git_analysis::correlate(history, &metrics, &ast, &tree);
    assert_eq!(report.rows().len(), 1);
    apply_repository(&mut app, RepositoryEvent::Hotspots(report));
    assert_eq!(app.overview.selected, id);
    assert_eq!(app.metrics_scroll, 7);
    assert_eq!(app.pane, Pane::Details);
    assert_eq!(app.map.selected, "crate:core/Cargo.toml");
    let (tx, mut rx) = tokio::sync::mpsc::channel(16);
    key(&mut app, KeyCode::Enter, &tx).await;
    assert_eq!(app.view, View::Explorer);
    assert_eq!(app.explorer.file.as_ref().unwrap().path, path);
    assert!(matches!(rx.recv().await,Some(AnalyzerCommand::InspectFile{file,..})if file==path));
}

#[test]
fn partial_history_and_partial_cochange_have_independent_visual_styles() {
    let (temp, mut app) = fixture();
    let tree = crate::repository::scan(temp.path()).unwrap();
    let metrics = crate::metrics::analyze(temp.path(), &tree, &app.project);
    let ast = crate::maintainability::analyze(temp.path(), &tree, &app.project);
    for (reason, history_partial) in [("coChangeCommitFileBudget", false), ("commitBudget", true)] {
        let mut h = crate::git_analysis::History::default();
        h.commits_analyzed = 4;
        h.partial_reasons.push(reason.into());
        let report = crate::git_analysis::correlate(h, &metrics, &ast, &tree);
        assert_eq!(report.history_partial, history_partial);
        assert!(report.co_change_partial);
        app.hotspots = Some(report);
        let b = render(&app, 160, 45);
        let s = text(&b);
        assert!(s.contains(if history_partial {
            "Git ◐ Partial"
        } else {
            "Git ● Complete"
        }));
        assert!(s.contains("Co-change ◐ Partial"));
        assert!(b.content.iter().any(|c| c.fg == theme::WARNING));
    }
}
async fn key(app: &mut App, code: KeyCode, tx: &tokio::sync::mpsc::Sender<AnalyzerCommand>) {
    handle_key(app, KeyEvent::new(code, KeyModifiers::NONE), tx)
        .await
        .unwrap();
}

#[tokio::test]
async fn focus_changes_neighborhood_and_target_choice_opens_exact_cargo_source() {
    let (_temp, mut app) = fixture();
    let (tx, mut rx) = tokio::sync::mpsc::channel(16);
    assert_eq!(app.map.selected, "crate:app/Cargo.toml");
    key(&mut app, KeyCode::Enter, &tx).await;
    assert_eq!(app.map.focus.as_deref(), Some("crate:app/Cargo.toml"));
    key(&mut app, KeyCode::Char('j'), &tx).await;
    let selected = app.map.selected.clone();
    key(&mut app, KeyCode::Enter, &tx).await;
    assert_eq!(app.map.focus.as_ref(), Some(&selected));
    assert!(text(&render(&app, 120, 35)).contains("one hop"));
    assert_eq!(app.map.choices(&app.project.architecture).len(), 2);
    key(&mut app, KeyCode::Esc, &tx).await;
    key(&mut app, KeyCode::Esc, &tx).await;
    assert!(app.map.focus.is_none());
    app.map.selected = "crate:app/Cargo.toml".into();
    key(&mut app, KeyCode::Char('o'), &tx).await;
    assert!(app.map.target_picker.is_some());
    assert!(text(&render(&app, 120, 35)).contains("CARGO TARGETS"));
    let node = app.project.architecture.node(&app.map.selected).unwrap();
    let desired = node
        .targets
        .iter()
        .position(|t| t.path == Path::new("app/src/main.rs"))
        .unwrap();
    app.map.target_picker = Some(desired);
    key(&mut app, KeyCode::Enter, &tx).await;
    assert_eq!(app.view, View::Explorer);
    assert_eq!(
        app.explorer.file.as_ref().unwrap().path,
        Path::new("app/src/main.rs")
    );
    assert!(app.explorer.is_expanded(Path::new("app/src")));
    assert!(
        matches!(rx.recv().await,Some(AnalyzerCommand::InspectFile{file,..}) if file==Path::new("app/src/main.rs"))
    );
}
#[test]
fn graph_styles_are_distinct_responsive_and_no_color_removes_color_not_identity() {
    let (_temp, mut app) = fixture();
    app.analyzer_state = super::super::app::AnalyzerState::Ready;
    for (w, h) in [(80, 20), (100, 30), (120, 35), (160, 45)] {
        let b = render(&app, w, h);
        let s = text(&b);
        assert!(s.contains("ARCHITECTURE"));
        assert!(s.contains("depends on B"));
        assert!(s.contains("CRATE INSPECTOR"));
        assert!(s.contains("5 MAP") || s.contains("5 Map"));
    }
    let b = render(&app, 160, 45);
    assert!(b
        .content
        .iter()
        .any(|c| c.fg == theme::ACTIVE && c.modifier.contains(Modifier::BOLD)));
    for kind in [
        DependencyKind::Normal,
        DependencyKind::Dev,
        DependencyKind::Build,
    ] {
        assert!(b.content.iter().any(|c| c.fg == edge_color(kind)));
    }
    assert!(b
        .content
        .iter()
        .any(|c| c.fg == theme::SUCCESS && c.symbol() == "R"));
    app.analyzer_state = super::super::app::AnalyzerState::Error("specific failure".into());
    assert!(render(&app, 120, 35)
        .content
        .iter()
        .any(|c| c.fg == theme::ERROR));
    app.color_mode = theme::ColorMode::None;
    let b = render(&app, 120, 35);
    assert!(b
        .content
        .iter()
        .all(|c| c.fg == ratatui::style::Color::Reset && c.bg == ratatui::style::Color::Reset));
    assert!(text(&b).contains('>'));
    assert!(text(&b).contains("Error"));
}
#[tokio::test]
async fn field_calls_emit_no_lsp_and_cleared_search_marks_previous_inspector() {
    let (_temp, mut app) = fixture();
    app.switch_view(View::Symbols);
    let point = crate::model::Position {
        line: 0,
        character: 0,
    };
    let range = crate::model::Range {
        start: point,
        end: point,
    };
    let symbol = crate::model::Symbol {
        id: "field".into(),
        name: "project".into(),
        kind: "field".into(),
        file: "app/src/lib.rs".into(),
        range,
        selection_range: range,
        container: Some("Engine".into()),
    };
    app.symbols_results = vec![symbol.clone()];
    app.symbols_query = "project".into();
    app.inspector = Some(Inspector::loading(1, symbol));
    let (tx, mut rx) = tokio::sync::mpsc::channel(16);
    key(&mut app, KeyCode::Char('c'), &tx).await;
    assert_eq!(app.view, View::Calls);
    let s = text(&render(&app, 120, 35));
    assert!(s.contains("Call hierarchy is not applicable"));
    assert!(s.contains("is a field"));
    assert!(rx.try_recv().is_err());
    app.switch_view(View::Symbols);
    app.symbols_query.clear();
    app.edit_search();
    assert!(app.symbols_results.is_empty());
    let s = text(&render(&app, 120, 35));
    assert!(s.contains("No symbol selected."));
    assert!(s.contains("LAST INSPECTED"));
    app.overlay = Overlay::None;
    assert!(!s.contains("Callers                 loading"));
}

#[test]
fn singleton_dense_large_cycle_and_unresolved_models_remain_bounded_and_navigable() {
    let (_temp, mut app) = fixture();
    let prototype = app.project.architecture.nodes[0].clone();
    app.project.architecture.nodes = vec![prototype.clone()];
    app.project.architecture.edges.clear();
    app.map = State::new(&app.project.architecture);
    assert!(text(&render(&app, 80, 20)).contains("1 of 1"));
    let mut model = Architecture::default();
    for n in 0..55 {
        let mut node = prototype.clone();
        node.id = format!("crate:n{n:02}/Cargo.toml");
        node.name = format!("node_{n:02}");
        node.manifest_path = format!("n{n:02}/Cargo.toml").into();
        model.nodes.push(node);
    }
    for n in 1..55 {
        model.edges.push(crate::architecture::DependencyEdge {
            from: model.nodes[n].id.clone(),
            to: model.nodes[n - 1].id.clone(),
            kind: DependencyKind::Normal,
            alias: model.nodes[n - 1].name.clone(),
            target: None,
            optional: false,
            via_manifest_path: model.nodes[n - 1].manifest_path.clone(),
        });
    }
    app.project.architecture = model;
    app.map = State::new(&app.project.architecture);
    app.map.move_by(&app.project.architecture, 30);
    assert_eq!(app.map.selected, "crate:n30/Cargo.toml");
    app.map.focus_selected();
    assert_eq!(app.map.choices(&app.project.architecture).len(), 3);
    for (w, h) in [(80, 20), (100, 30), (120, 35), (160, 45)] {
        assert!(text(&render(&app, w, h)).contains("55"));
    }
    let model = &mut app.project.architecture;
    let mut back = model.edges[0].clone();
    std::mem::swap(&mut back.from, &mut back.to);
    model.edges.push(back);
    model.partial = true;
    model
        .unresolved_dependencies
        .push(crate::architecture::UnresolvedDependency {
            from: app.map.selected.clone(),
            package: "missing".into(),
            alias: "alias".into(),
            reason: "cannot verify manifest identity".into(),
        });
    let s = text(&render(&app, 160, 45));
    assert!(s.contains("ARCHITECTURE · PARTIAL"));
    assert!(s.contains("unresolved dependencies"));
    assert!(s.contains("cannot verify manifest identity"));
}
