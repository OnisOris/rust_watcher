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
    app.overview.expanded.insert(overview::Group::Metrics);
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
    app.overview.expanded.insert(overview::Group::Metrics);
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
