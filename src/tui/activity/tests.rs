use super::*;
use crate::activity::{Event, Kind};
#[test]
fn horizontal_selection_does_not_rebuild_index_and_compact_expands() {
    let mut s = State::default();
    for p in ["src/tui/a.rs", "src/tui/b.rs", "tests/test.rs"] {
        s.data.apply(Event::filesystem(Kind::Modified, p.into()));
    }
    s.refresh();
    let revision = s.revision;
    s.right();
    s.right();
    assert_eq!(s.column, 2);
    s.down(1);
    assert_eq!(s.selected_path().unwrap(), PathBuf::from("src/tui/b.rs"));
    assert_eq!(s.revision, revision);
    s.compact.set(true);
    s.right();
    assert!(s.rows.len() > 2);
    s.right();
    assert!(s.rows.iter().any(|r| r.path == Path::new("src/tui")));
}
fn app() -> (tempfile::TempDir, App) {
    let t = tempfile::tempdir().unwrap();
    std::fs::create_dir(t.path().join("src")).unwrap();
    std::fs::write(t.path().join("src/lib.rs"), "pub fn f() {}\n").unwrap();
    std::fs::write(
        t.path().join("Cargo.toml"),
        "[package]\nname='activity_test'\nversion='0.1.0'\n",
    )
    .unwrap();
    let app = App::new(crate::project::Project::discover(t.path()).unwrap());
    (t, app)
}
#[test]
fn responsive_layouts_show_status_provenance_summary_and_filters() {
    let (_t, mut app) = app();
    app.switch_view(View::Activity);
    app.activity
        .data
        .apply(Event::filesystem(Kind::Modified, "src/lib.rs".into()));
    app.activity.refresh();
    for (w, h) in [(80, 20), (120, 35), (160, 45)] {
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(w, h)).unwrap();
        terminal.draw(|f| crate::tui::ui::render(f, &app)).unwrap();
        let text: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|c| c.symbol())
            .collect();
        assert!(text.contains("AGENT FOCUS"));
        assert!(text.contains("Not connected"));
        assert!(text.contains("RECENT EVENTS"));
        app.activity.summary = true;
        terminal.draw(|f| crate::tui::ui::render(f, &app)).unwrap();
        let text: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|c| c.symbol())
            .collect();
        assert!(text.contains("SESSION SUMMARY"));
        app.activity.summary = false;
    }
    app.activity.filter = Filter::All;
    app.activity.refresh();
    assert!(!app.activity.children.is_empty());
}
#[tokio::test]
async fn source_navigation_returns_to_activity_and_deleted_path_is_not_opened() {
    let (t, mut app) = app();
    app.explorer
        .apply_tree(crate::repository::scan(t.path()).unwrap());
    app.activity
        .data
        .snapshot(crate::repository::scan(t.path()).unwrap());
    app.activity
        .data
        .apply(Event::filesystem(Kind::Modified, "src/lib.rs".into()));
    app.activity.refresh();
    app.switch_view(View::Activity);
    app.activity.right();
    let (tx, _rx) = tokio::sync::mpsc::channel(16);
    handle_key(&mut app, KeyEvent::from(K::Char('o')), &tx).await;
    assert_eq!(app.view, View::Explorer);
    crate::tui::event::handle_key(&mut app, KeyEvent::from(K::Esc), &tx)
        .await
        .unwrap();
    assert_eq!(app.view, View::Activity);
    app.activity
        .data
        .apply(Event::filesystem(Kind::Deleted, "src/lib.rs".into()));
    app.activity.refresh();
    handle_key(&mut app, KeyEvent::from(K::Char('o')), &tx).await;
    assert_eq!(app.view, View::Activity);
    assert!(app.notice.unwrap().contains("Deleted"));
}
