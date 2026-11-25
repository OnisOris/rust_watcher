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
    s.manual_until = None;
    s.focus = Some("src/tui/a.rs".into());
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
                Mode::Tree => "PROJECT HEATMAP",
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

fn measured(s: &mut State, path: &str, lines: u32, now: Instant) {
    let mut e = Event::filesystem(Kind::Modified, path.into());
    e.observed = now;
    e.measurement = Some(activity::content::Measurement {
        delta: Some(activity::content::LineDelta {
            added: lines,
            deleted: 0,
        }),
        net: None,
        reason: None,
    });
    s.data.apply(e);
}
#[test]
fn smart_follow_reveals_deep_branches_stabilizes_clusters_and_pauses_for_user() {
    let now = Instant::now();
    let mut s = State::default();
    measured(&mut s, "src/deep/branch/render.rs", 180, now);
    s.refresh_at(now);
    assert_eq!(
        s.selected_path().as_deref(),
        Some(Path::new("src/deep/branch/render.rs"))
    );
    for p in ["src", "src/deep", "src/deep/branch"] {
        assert!(s.expanded.contains(Path::new(p)));
    }
    measured(&mut s, "src/deep/branch/state.rs", 52, now);
    s.refresh_at(now);
    assert_eq!(
        s.focus.as_deref(),
        Some(Path::new("src/deep/branch/render.rs"))
    );
    s.down(-1);
    let selected = s.rows[s.selected].path.clone();
    measured(&mut s, "tests/new.rs", 1000, now);
    s.refresh_at(now);
    s.refresh_at(now + Duration::from_secs(2));
    assert_eq!(s.focus.as_deref(), Some(Path::new("tests/new.rs")));
    assert_eq!(s.rows[s.selected].path, selected);
    s.refresh_at(now + Duration::from_secs(16));
    assert_eq!(
        s.selected_path().as_deref(),
        Some(Path::new("tests/new.rs"))
    );
}
#[test]
fn cluster_hysteresis_ignores_small_advantage_and_pin_survives_writes_and_rename() {
    let now = Instant::now();
    let mut s = State::default();
    measured(&mut s, "src/a", 100, now);
    s.refresh_at(now);
    measured(&mut s, "tests/a", 120, now);
    s.refresh_at(now);
    s.refresh_at(now + Duration::from_secs(2));
    assert_eq!(s.focus.as_deref(), Some(Path::new("src/a")));
    s.follow = false;
    for i in 0..50 {
        measured(&mut s, &format!("tests/{i}"), 100, now);
    }
    s.refresh_at(now + Duration::from_secs(3));
    assert_eq!(s.focus.as_deref(), Some(Path::new("src/a")));
    let mut e = Event::filesystem(Kind::Renamed, "src/renamed".into());
    e.old_path = Some("src/a".into());
    e.observed = now + Duration::from_secs(4);
    s.data.apply(e);
    s.refresh_at(now + Duration::from_secs(4));
    assert_eq!(s.focus.as_deref(), Some(Path::new("src/renamed")));
    assert_eq!(s.selected_path().as_deref(), Some(Path::new("src/renamed")));
}
#[test]
fn no_color_keeps_absolute_heat_bars_and_unknown_is_not_zero() {
    let (_t, mut app) = app();
    app.switch_view(View::Activity);
    app.color_mode = crate::tui::theme::ColorMode::None;
    let now = Instant::now();
    for (p, n) in [
        ("src/render.rs", 180),
        ("src/state.rs", 52),
        ("src/tests.rs", 18),
    ] {
        measured(&mut app.activity, p, n, now);
    }
    app.activity
        .data
        .apply(Event::filesystem(Kind::Modified, "src/unknown.rs".into()));
    app.activity.refresh_at(now);
    let text = frame(&app, 120, 35);
    assert!(text.contains("180 Δ"));
    assert!(text.contains("52 Δ"));
    assert!(text.contains("18 Δ"));
    assert!(text.contains("█▓▒░"));
    assert!(text.contains("▓▒░·"));
    assert!(text.contains("▒░··"));
    assert!(text.contains("? Δ"));
}

#[test]
fn agent_only_content_is_unmeasured_and_density_has_visible_detail() {
    let (_t, mut app) = app();
    app.switch_view(View::Activity);
    let mut e = Event::filesystem(Kind::Read, "src/lib.rs".into());
    e.source = Source::Agent;
    app.activity.data.apply(e);
    app.activity.refresh();
    let text = frame(&app, 120, 35);
    assert!(text.contains("not measured"));
    assert!(text.contains("Added ?"));
    assert!(!text.contains("Added +0"));
    assert!(!text.contains("Churn 0"));
    measured(&mut app.activity, "src/lib.rs", 180, Instant::now());
    app.activity.refresh();
    app.activity.density = 2;
    assert!(frame(&app, 120, 35).contains("+180/-0"));
    app.activity.density = 0;
    assert!(!frame(&app, 120, 35).contains("+180/-0"));
}
#[test]
fn wide_unicode_filenames_keep_delta_inside_the_tree() {
    let (_t, mut app) = app();
    app.switch_view(View::Activity);
    measured(
        &mut app.activity,
        &format!("src/{}.rs", "深".repeat(80)),
        180,
        Instant::now(),
    );
    app.activity.refresh();
    let text = frame(&app, 80, 20);
    assert!(text.contains("180 Δ"));
    assert!(text.contains('…'));
}
