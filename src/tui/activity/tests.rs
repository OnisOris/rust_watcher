use super::*;
use crate::activity::{
    focus::{Sources, Types, Window},
    Event, Kind, Source,
};
fn populated() -> State {
    let mut s = State::default();
    for p in ["src/tui/a.rs", "src/tui/b.rs", "tests/test.rs"] {
        s.data.apply(Event::filesystem(Kind::Modified, p.into()));
    }
    s.refresh();
    s
}
#[test]
fn auto_expansion_manual_selection_and_follow_are_independent() {
    let mut s = populated();
    assert!(s.expanded.contains(Path::new("src/tui")));
    assert!(s.rows.iter().any(|r| r.path == Path::new("src/tui/a.rs")));
    let revision = s.revision;
    let focus = s.focus.clone();
    s.down(1);
    s.down(1);
    assert_eq!(s.revision, revision);
    assert_eq!(s.focus, focus);
    s.follow = false;
    let pinned = s.focus.clone();
    for _ in 0..5 {
        s.data
            .apply(Event::filesystem(Kind::Modified, "src/tui/a.rs".into()));
    }
    s.refresh();
    assert_eq!(s.focus, pinned);
    s.follow = true;
    s.auto = Auto::Follow;
    s.refresh();
    assert_eq!(
        s.selected_path().as_deref(),
        Some(Path::new("src/tui/a.rs"))
    );
    s.auto = Auto::Manual;
    s.expanded.clear();
    s.rebuild_rows();
    assert!(s.rows.iter().all(|r| r.depth == 0));
    s.expand();
    assert!(s.rows.iter().any(|r| r.depth > 0));
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
fn frame(app: &App, w: u16, h: u16) -> String {
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(w, h)).unwrap();
    terminal.draw(|f| crate::tui::ui::render(f, app)).unwrap();
    terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect()
}
#[test]
fn responsive_modes_provenance_and_no_color_remain_readable() {
    let (_t, mut app) = app();
    app.switch_view(View::Activity);
    app.activity = populated();
    for (w, h) in [(80, 20), (120, 35), (160, 45)] {
        for mode in [Mode::Tree, Mode::Timeline, Mode::Map] {
            app.activity.mode = mode;
            let text = frame(&app, w, h);
            assert!(text.contains("AGENT FOCUS"));
            assert!(text.contains(match mode {
                Mode::Tree => "FOCUS",
                Mode::Timeline => "TIMELINE",
                Mode::Map => "CLUSTER",
            }));
            assert!(!text.contains("Now editing"));
        }
        app.activity.summary = true;
        assert!(frame(&app, w, h).contains("SESSION SUMMARY"));
        app.activity.summary = false;
    }
    app.activity.mode = Mode::Tree;
    app.color_mode = crate::tui::theme::ColorMode::None;
    assert!(frame(&app, 120, 35).contains("Filesystem observed"));
}
#[test]
fn filters_update_every_panel_and_agent_report_is_not_disk_evidence() {
    let mut s = populated();
    let mut read = Event::filesystem(Kind::Read, "docs/guide.md".into());
    read.source = Source::Agent;
    s.data.apply(read);
    s.refresh();
    s.query.sources = Sources::Agent;
    s.refresh();
    assert_eq!(s.snapshot.agent, 1);
    assert_eq!(s.snapshot.filesystem, 0);
    assert_eq!(s.snapshot.files.len(), 1);
    assert!(s.rows.iter().all(|r| r.path.starts_with("docs")));
    s.query.types = Types::Modified;
    s.refresh();
    assert!(s.rows.is_empty());
    assert!(s.focus.is_none());
    s.query = Query::default();
    s.query.window = Window::Quarter;
    s.data
        .events
        .iter_mut()
        .for_each(|e| e.observed = Instant::now() - Duration::from_secs(16));
    s.refresh();
    assert!(s.snapshot.events.is_empty());
    assert!(s.rows.is_empty());
}
#[tokio::test]
async fn source_navigation_returns_to_activity_and_deleted_path_is_refused() {
    let (t, mut app) = app();
    let tree = crate::repository::scan(t.path()).unwrap();
    app.explorer.apply_tree(tree.clone());
    app.activity.data.snapshot(tree);
    app.activity
        .data
        .apply(Event::filesystem(Kind::Modified, "src/lib.rs".into()));
    app.activity.refresh();
    app.switch_view(View::Activity);
    app.activity.selected = app
        .activity
        .rows
        .iter()
        .position(|r| r.path == Path::new("src/lib.rs"))
        .unwrap();
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
#[tokio::test]
async fn keys_cycles_pinning_and_map_never_change_existing_module_focus() {
    let (_t, mut app) = app();
    app.activity = populated();
    app.switch_view(View::Activity);
    let (tx, _rx) = tokio::sync::mpsc::channel(16);
    let focus = app.module_map.focus.clone();
    for (key, mode) in [('v', Mode::Timeline), ('v', Mode::Map), ('v', Mode::Tree)] {
        handle_key(&mut app, KeyEvent::from(K::Char(key)), &tx).await;
        assert!(app.activity.mode == mode);
    }
    handle_key(&mut app, KeyEvent::from(K::Char('h')), &tx).await;
    assert!(!app.activity.heat);
    handle_key(&mut app, KeyEvent::from(K::Char('z')), &tx).await;
    assert!(!app.activity.follow);
    handle_key(&mut app, KeyEvent::from(K::Char('r')), &tx).await;
    assert!(app.activity.follow);
    assert_eq!(app.module_map.focus, focus);
}
#[test]
fn pinned_focus_recovers_after_filter_roundtrip_and_history_markers_persist() {
    let mut s = populated();
    s.follow = false;
    s.query.types = Types::Agent;
    s.refresh();
    assert!(s.focus.is_none());
    s.query.types = Types::All;
    s.refresh();
    assert!(s.focus.is_some());
    let (_t, mut app) = app();
    app.switch_view(View::Activity);
    app.activity.data.files.insert("old.rs".into(), true);
    app.activity
        .data
        .apply(Event::filesystem(Kind::Modified, "old.rs".into()));
    for _ in 0..activity::HISTORY_LIMIT {
        app.activity
            .data
            .apply(Event::filesystem(Kind::Modified, "recent.rs".into()));
    }
    app.activity.refresh();
    assert!(!app
        .activity
        .snapshot
        .files
        .contains_key(Path::new("old.rs")));
    assert!(frame(&app, 120, 35).contains("old.rs M*"));
}
