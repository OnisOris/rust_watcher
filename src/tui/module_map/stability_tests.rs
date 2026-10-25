use super::*;
use crate::tui::{app::View, event::handle_key, worker::AnalyzerCommand};
use crossterm::event::{KeyCode as K, KeyEvent, KeyModifiers};
use ratatui::{backend::TestBackend, Terminal};

fn fixture(count: usize) -> (tempfile::TempDir, App) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    std::fs::create_dir(root.join("src")).unwrap();
    std::fs::write(
        root.join("Cargo.toml"),
        "[package]\nname='stable'\nversion='0.1.0'\nedition='2021'\n",
    )
    .unwrap();
    std::fs::write(
        root.join("src/lib.rs"),
        (0..count)
            .map(|i| format!("mod m{i:03} {{mod child{{}}}}\n"))
            .collect::<String>(),
    )
    .unwrap();
    let p = crate::project::Project::discover(root).unwrap();
    let tree = crate::repository::scan(root).unwrap();
    let g = crate::module_graph::analyze(root, &tree, &p.architecture, || false);
    let mut app = App::new(p);
    app.module_map.apply(g);
    app.switch_view(View::Map);
    (temp, app)
}
async fn key(app: &mut App, k: K) {
    let (tx, _rx) = tokio::sync::mpsc::channel::<AnalyzerCommand>(8);
    handle_key(app, KeyEvent::new(k, KeyModifiers::NONE), &tx)
        .await
        .unwrap();
}
fn draw(app: &App, t: &mut Terminal<TestBackend>) -> String {
    t.draw(|f| crate::tui::ui::render(f, app)).unwrap();
    t.backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect()
}
fn context(s: &State) -> (String, String, Mode, usize) {
    (s.focus.clone(), s.selected.clone(), s.mode, s.page.get())
}

#[tokio::test]
async fn explicit_pages_and_selection_preserve_geometry_and_cache() {
    for count in [13, 40, 120] {
        let (_tmp, mut app) = fixture(count);
        for (w, h) in [(80, 20), (100, 30), (120, 35), (160, 45)] {
            app.module_map.selected = app.module_map.focus.clone();
            app.module_map.page.set(0);
            let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
            draw(&app, &mut t);
            let first = app.module_map.diagram_cache.borrow().clone();
            let focus = app.module_map.focus.clone();
            let mut visited = std::collections::BTreeSet::new();
            loop {
                let page = app.module_map.page.get();
                let geometry = app.module_map.diagram_cache.borrow().clone();
                let visible = app.module_map.visible_choices().len();
                key(&mut app, K::Char('k')).await;
                for _ in 0..visible + 2 {
                    key(&mut app, K::Char('j')).await;
                    visited.insert(app.module_map.selected.clone());
                    let text = draw(&app, &mut t);
                    assert!(text.contains("Selected:"));
                    assert_eq!(app.module_map.focus, focus);
                    assert_eq!(app.module_map.page.get(), page);
                    assert_eq!(*app.module_map.diagram_cache.borrow(), geometry);
                }
                let before = context(&app.module_map);
                key(&mut app, K::Char('g')).await;
                draw(&app, &mut t);
                assert_eq!(context(&app.module_map), before);
                key(&mut app, K::Char('g')).await;
                draw(&app, &mut t);
                assert_eq!(*app.module_map.diagram_cache.borrow(), geometry);
                key(&mut app, K::PageDown).await;
                if app.module_map.page.get() == page {
                    break;
                }
                assert_eq!(
                    app.module_map.selected,
                    app.module_map.visible_choices()[1].id
                );
                draw(&app, &mut t);
            }
            assert_eq!(visited.len(), count);
            while app.module_map.page.get() > 0 {
                key(&mut app, K::PageUp).await;
            }
            draw(&app, &mut t);
            assert_eq!(*app.module_map.diagram_cache.borrow(), first);
            assert!(app.module_map.history.is_empty());
        }
    }
}

#[tokio::test]
async fn mode_focus_history_and_background_updates_are_independent() {
    let (_tmp, mut app) = fixture(40);
    let mut t = Terminal::new(TestBackend::new(160, 45)).unwrap();
    draw(&app, &mut t);
    key(&mut app, K::PageDown).await;
    let root = app.module_map.focus.clone();
    let selected = app.module_map.selected.clone();
    key(&mut app, K::Char('v')).await;
    assert_eq!(app.module_map.focus, root);
    assert_eq!(app.module_map.selected, root); // Not an import of this root.
    assert_eq!(app.module_map.page.get(), 0);
    let before = context(&app.module_map);
    key(&mut app, K::Enter).await;
    assert_eq!(context(&app.module_map), before);
    for _ in 0..100 {
        key(&mut app, K::Char('v')).await;
        key(&mut app, K::Char('g')).await;
    }
    assert!(app.module_map.history.is_empty());
    app.module_map.mode = Mode::Hierarchy;
    app.module_map.page.set(1);
    app.module_map.selected = selected;
    let parent = context(&app.module_map);
    key(&mut app, K::Enter).await;
    assert_ne!(app.module_map.focus, root);
    assert_eq!(app.module_map.page.get(), 0);
    key(&mut app, K::Char('j')).await;
    let child = context(&app.module_map);
    key(&mut app, K::Enter).await;
    key(&mut app, K::Esc).await;
    assert_eq!(context(&app.module_map), child);
    key(&mut app, K::Esc).await;
    assert_eq!(context(&app.module_map), parent);
    let return_point = crate::tui::journey::point(&app, View::Explorer);
    crate::tui::journey::remember(&mut app, return_point);
    app.switch_view(View::Explorer);
    let bridge = crate::tui::journey::point(&app, View::Calls);
    crate::tui::journey::remember(&mut app, bridge);
    app.switch_view(View::Calls);
    app.module_map.page.set(0);
    assert!(crate::tui::journey::back(&mut app));
    assert_eq!(app.view, View::Map);
    assert_eq!(context(&app.module_map), parent);
    for event in [
        crate::tui::RepositoryEvent::Metrics(Default::default()),
        crate::tui::RepositoryEvent::Maintainability(Default::default()),
        crate::tui::RepositoryEvent::Hotspots(crate::git_analysis::correlate(
            Default::default(),
            &Default::default(),
            &Default::default(),
            &crate::repository::scan(_tmp.path()).unwrap(),
        )),
    ] {
        crate::tui::event::apply_repository(&mut app, event);
        assert_eq!(context(&app.module_map), parent);
    }
    // Same graph replacement preserves page; removed focus/selection recover safely.
    let graph = app.module_map.graph.clone().unwrap();
    app.module_map.apply(graph.clone());
    assert_eq!(context(&app.module_map), parent);
    let mut changed = graph;
    changed.nodes.retain(|n| n.parent_id.is_none());
    changed.containment_edges.clear();
    app.module_map.apply(changed);
    assert_eq!(app.module_map.focus, root);
    assert_eq!(app.module_map.selected, root);
    assert_eq!(app.module_map.page.get(), 0);
    for _ in 0..60 {
        app.module_map.remember();
    }
    assert_eq!(app.module_map.history.len(), 50);
}
