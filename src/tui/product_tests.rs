use super::{
    app::{AnalyzerState, App, Overlay, View},
    event::handle_key,
    explanations::Metric,
    overview::{self, Guide, Target},
    worker::AnalyzerCommand,
};
use crossterm::event::{KeyCode as K, KeyEvent, KeyModifiers};
use ratatui::{backend::TestBackend, Terminal};
use tokio::sync::mpsc;

fn app() -> App {
    App::new(
        crate::project::Project::discover(std::path::Path::new("tests/fixtures/simple_project"))
            .unwrap(),
    )
}
async fn key(app: &mut App, code: K, tx: &mpsc::Sender<AnalyzerCommand>) {
    handle_key(app, KeyEvent::new(code, KeyModifiers::NONE), tx)
        .await
        .unwrap();
}
fn text(app: &App, width: u16, height: u16) -> String {
    let mut term = Terminal::new(TestBackend::new(width, height)).unwrap();
    term.draw(|f| super::ui::render(f, app)).unwrap();
    term.backend()
        .buffer()
        .content()
        .iter()
        .map(|c| c.symbol())
        .collect()
}
#[test]
fn task_home_is_visible_at_every_required_size_with_missing_services() {
    let mut app = app();
    for state in [
        AnalyzerState::Starting,
        AnalyzerState::Indexing,
        AnalyzerState::Ready,
        AnalyzerState::Error("missing RA".into()),
    ] {
        app.analyzer_state = state;
        app.hotspots_error = Some("missing Git".into());
        for (w, h) in [(80, 20), (100, 30), (120, 35), (160, 45)] {
            let rendered = text(&app, w, h);
            for label in [
                "Explore execution",
                "Explore architecture",
                "Find implementation",
                "Review attention areas",
                "PROJECT SNAPSHOT",
                "Enter",
                "RA",
            ] {
                assert!(
                    rendered.contains(label),
                    "missing {label} at {w}x{h}: {rendered}"
                );
            }
        }
    }
}
#[tokio::test]
async fn guided_search_and_architecture_return_home_without_semantic_requests() {
    let mut app = app();
    let (tx, mut rx) = mpsc::channel(16);
    for guide in [Guide::Architecture, Guide::Find] {
        app.overview.selected = Target::Guide(guide).id();
        key(&mut app, K::Enter, &tx).await;
        if guide == Guide::Find {
            assert_eq!(app.overlay, Overlay::Search);
            key(&mut app, K::Esc, &tx).await;
        } else {
            // First Esc leaves the module scope, second returns to Overview.
            key(&mut app, K::Esc, &tx).await;
        }
        key(&mut app, K::Esc, &tx).await;
        assert_eq!(app.view, View::Overview);
        assert_eq!(app.overview.selected, Target::Guide(guide).id());
        assert!(rx.try_recv().is_err());
    }
}
#[test]
fn metric_explanations_use_measured_values_and_keep_missing_data_unknown() {
    let mut app = app();
    app.overview
        .expanded
        .extend([overview::Group::Additional, overview::Group::Metrics]);
    app.overview.selected = Target::Metric(Metric::Depth).id();
    let missing = text(&app, 120, 35);
    assert!(missing.contains("Unknown"));
    let root = app.project.workspace_root.clone();
    let tree = crate::repository::scan(&root).unwrap();
    let report = crate::maintainability::analyze(&root, &tree, &app.project);
    let expected = report.max_control_depth;
    app.maintainability = Some(report);
    let rendered = text(&app, 120, 35);
    assert!(rendered.contains(&format!("Depth {expected}")));
    assert!(rendered.contains("Not cyclomatic complexity"));
    app.maintainability.as_mut().unwrap().partial = true;
    assert!(text(&app, 120, 35).contains("PARTIAL"));
    assert!(Metric::Length.explanation().what.contains("at or below"));
    assert!(Metric::SourceLoc
        .explanation()
        .what
        .contains("includes comments"));
}
#[tokio::test]
async fn context_help_scrolls_without_moving_underlying_selection() {
    let mut app = app();
    app.overview
        .expanded
        .extend([overview::Group::Additional, overview::Group::Metrics]);
    app.overview.selected = Target::Metric(Metric::Depth).id();
    let id = app.overview.selected.clone();
    let (tx, mut rx) = mpsc::channel(16);
    key(&mut app, K::Char('?'), &tx).await;
    assert!(text(&app, 80, 20).contains("WHAT"));
    for _ in 0..60 {
        key(&mut app, K::Char('j'), &tx).await;
    }
    assert!(app.help_scroll > 0);
    assert!(text(&app, 80, 20).contains("NEXT ACTION"));
    key(&mut app, K::Esc, &tx).await;
    assert_eq!(app.overview.selected, id);
    assert_eq!(app.overlay, Overlay::None);
    assert!(rx.try_recv().is_err());
}

#[tokio::test]
async fn unavailable_calls_notice_is_cleared_by_new_screen_but_ra_error_survives() {
    for destination in [K::Char('4'), K::Char('1')] {
        let mut app = app();
        let tree = crate::repository::scan(&app.project.workspace_root).unwrap();
        app.explorer.apply_tree(tree);
        app.analyzer_state = AnalyzerState::Ready;
        let (tx, mut rx) = mpsc::channel(16);
        key(&mut app, K::Char('3'), &tx).await;
        key(&mut app, K::Char('j'), &tx).await;
        key(&mut app, K::Char('j'), &tx).await;
        key(&mut app, K::Char('j'), &tx).await;
        key(&mut app, K::Enter, &tx).await;
        assert!(app.notice.is_none());
        assert!(text(&app, 120, 35).contains("No previously inspected callable"));
        app.analyzer_state = AnalyzerState::Error("persistent RA failure".into());
        key(&mut app, destination, &tx).await;
        assert!(app.notice.is_none());
        assert_eq!(
            app.analyzer_state,
            AnalyzerState::Error("persistent RA failure".into())
        );
        assert!(rx.try_recv().is_err());
        assert!(text(&app, 120, 35).contains("RA × Error"));
    }
}

#[tokio::test]
async fn successful_search_clears_action_feedback_without_erasing_analysis_errors() {
    let mut app = app();
    app.notice = Some("previous action unavailable".into());
    app.metrics_error = Some("persistent metrics failure".into());
    let (tx, mut rx) = mpsc::channel(16);
    key(&mut app, K::Char('/'), &tx).await;
    assert!(app.notice.is_none());
    assert_eq!(app.overlay, Overlay::Search);
    assert_eq!(
        app.metrics_error.as_deref(),
        Some("persistent metrics failure")
    );
    assert!(rx.try_recv().is_err());
}

#[tokio::test]
async fn empty_symbols_search_is_available_before_tree_without_decorative_lsp() {
    let mut app = app();
    let (tx, mut rx) = mpsc::channel(16);
    key(&mut app, K::Char('2'), &tx).await;
    assert!(text(&app, 120, 35).contains("Search by name"));
    key(&mut app, K::Enter, &tx).await;
    assert_eq!(app.overlay, Overlay::Search);
    assert!(rx.try_recv().is_err());
    app.analyzer_state = AnalyzerState::Error("missing RA".into());
    app.symbols_query = "known_symbol".into();
    let rendered = text(&app, 120, 35);
    assert!(rendered.contains("Search unavailable"));
    assert!(!rendered.contains("No symbols found"));
}

#[test]
fn empty_routes_explain_next_action_at_all_sizes_including_unavailable_previous() {
    let mut app = app();
    for view in [View::Symbols, View::Calls] {
        app.switch_view(view);
        let count = if view == View::Calls { 4 } else { 5 };
        for index in 0..count {
            if view == View::Calls {
                app.journey.start_selection = index;
            } else {
                app.journey.symbols_start_selection = index;
            }
            for (w, h) in [(80, 20), (100, 30), (120, 35), (160, 45)] {
                let rendered = text(&app, w, h);
                assert!(
                    rendered.contains("Next:"),
                    "missing next action at {w}x{h}: {rendered}"
                );
                assert!(rendered.contains("EXPLORATION"));
                assert!(!rendered.contains("No symbol selected."));
            }
        }
    }
}

#[test]
fn empty_workspace_and_library_only_routes_do_not_invent_entrypoints() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("Cargo.toml"),
        "[workspace]\nmembers=[]\nresolver='2'\n",
    )
    .unwrap();
    let project = crate::project::Project::discover(temp.path()).unwrap();
    let mut app = App::new(project);
    app.explorer
        .apply_tree(crate::repository::scan(temp.path()).unwrap());
    app.switch_view(View::Calls);
    let rendered = text(&app, 120, 35);
    assert!(rendered.contains("No binary targets found"));
    assert!(rendered.contains("Unavailable"));
    assert!(app.notice.is_none());
}

#[test]
fn late_search_failure_retains_diagnostics_without_reopening_old_notice() {
    let mut app = app();
    app.switch_view(View::Symbols);
    app.search_request = 42;
    app.symbols_loading = true;
    app.switch_view(View::Explorer);
    app.apply_event(super::worker::AnalyzerEvent::SearchFailed {
        id: 42,
        message: "request failed after leaving Symbols".into(),
    });
    assert!(app.notice.is_none());
    assert!(app
        .symbols_error
        .as_deref()
        .unwrap()
        .contains("after leaving"));
    app.switch_view(View::Overview);
    app.overview.expanded.insert(overview::Group::Additional);
    app.overview.selected = Target::Group(overview::Group::Analysis).id();
    assert!(overview::details(&app)
        .iter()
        .any(|l| l.to_string().contains("request failed after leaving")));
    app.switch_view(View::Symbols);
    app.open_search();
    assert!(text(&app, 120, 35).contains("Search unavailable"));
}

#[tokio::test]
async fn empty_symbols_route_selection_still_works_after_tab() {
    let mut app = app();
    let (tx, mut rx) = mpsc::channel(16);
    key(&mut app, K::Char('2'), &tx).await;
    key(&mut app, K::Tab, &tx).await;
    key(&mut app, K::Char('j'), &tx).await;
    assert_eq!(app.journey.symbols_start_selection, 1);
    assert!(text(&app, 120, 35).contains("Browse known source roots"));
    assert!(rx.try_recv().is_err());
}

#[test]
fn review_evidence_matches_real_structure_and_never_invents_a_function_anchor() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir(temp.path().join("src")).unwrap();
    std::fs::write(
        temp.path().join("Cargo.toml"),
        "[package]\nname='review'\nversion='0.1.0'\nedition='2021'\n",
    )
    .unwrap();
    std::fs::write(temp.path().join("src/main.rs"), "fn main() {\nif true {\nif true {\nif true {\nif true {\nif true {\nif true {}\n}\n}\n}\n}\n}\n}\n").unwrap();
    let p = crate::project::Project::discover(temp.path()).unwrap();
    let tree = crate::repository::scan(temp.path()).unwrap();
    let metrics = crate::metrics::analyze(temp.path(), &tree, &p);
    let mut ast = crate::maintainability::analyze(temp.path(), &tree, &p);
    let mut history = crate::git_analysis::History::default();
    history.since_days = 90;
    history.files.insert(
        "src/main.rs".into(),
        crate::git_analysis::Change {
            commits: 17,
            added: Some(10),
            deleted: Some(2),
            last: 0,
        },
    );
    let report = crate::git_analysis::correlate(history, &metrics, &ast, &tree);
    let file = &report.hotspots[0];
    let f = super::findings::hotspot(&report, file, Some(&ast));
    assert!(f.candidate);
    assert_eq!(f.evidence[0].value, Some(17));
    assert_eq!(f.evidence[3].value, Some(6));
    let super::journey::Action::Function(anchor) = &f.next_actions[0] else {
        panic!("expected exact ranked anchor")
    };
    assert_eq!(anchor.path, std::path::Path::new("src/main.rs"));
    assert_eq!(anchor.identifier_line, 1);
    assert_eq!(anchor.identifier_column, 4);
    // An aggregate alone is insufficient to identify a callable.
    ast.longest_functions.clear();
    ast.deepest_functions.clear();
    let aggregate_only = super::findings::hotspot(&report, file, Some(&ast));
    assert!(aggregate_only
        .next_actions
        .iter()
        .all(|a| matches!(a, super::journey::Action::File(..))));
    let mut partial = report.clone();
    partial.current_metrics_partial = true;
    assert!(super::findings::hotspot(&partial, file, None).partial);
}

#[test]
fn directory_context_uses_direct_children_and_preserves_selection_on_resize() {
    let mut app = app();
    let root = app.project.workspace_root.clone();
    let tree = crate::repository::scan(&root).unwrap();
    app.explorer.apply_tree(tree);
    app.switch_view(View::Explorer);
    let summary = app
        .explorer
        .directory_summary(std::path::Path::new(""))
        .unwrap();
    let children = app
        .explorer
        .direct_children(std::path::Path::new(""))
        .collect::<Vec<_>>();
    assert_eq!(
        children.len(),
        summary.directories + summary.rust_files + summary.other_files + summary.symlinks
    );
    assert_eq!(summary.rust_files, 0); // src/main.rs is a descendant, not a direct child.
    assert!(summary.directories > 0);
    let selected = app.explorer.selected_entry().unwrap().path;
    for (w, h) in [(80, 20), (100, 30), (120, 35), (160, 45)] {
        let rendered = text(&app, w, h);
        assert!(rendered.contains("DIRECTORY CONTEXT"));
        assert!(rendered.contains("Direct children only"));
        assert!(rendered.contains("l first child"));
        assert_eq!(app.explorer.selected_entry().unwrap().path, selected);
    }
}
