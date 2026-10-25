use super::*;
#[tokio::test]
async fn multi_crate_map_drills_selected_package_and_returns_to_same_crate() {
    use super::super::app::View;
    use crossterm::event::KeyCode as K;
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    std::fs::write(
        root.join("Cargo.toml"),
        "[workspace]\nresolver='2'\nmembers=['a','b']\n",
    )
    .unwrap();
    for name in ["a", "b"] {
        let dir = root.join(name);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(
            dir.join("Cargo.toml"),
            format!("[package]\nname='{name}'\nversion='0.1.0'\nedition='2021'\n"),
        )
        .unwrap();
        std::fs::write(dir.join("src/lib.rs"), "mod engine;").unwrap();
        std::fs::write(dir.join("src/engine.rs"), "pub fn work(){}").unwrap();
    }
    let project = crate::project::Project::discover(root).unwrap();
    let tree = crate::repository::scan(root).unwrap();
    let graph = crate::module_graph::analyze(root, &tree, &project.architecture, || false);
    let mut app = App::new(project);
    app.explorer.apply_tree(tree);
    app.module_map.apply(graph);
    app.switch_view(View::Map);
    assert!(!app.module_map.active);
    app.map.selected = "crate:b/Cargo.toml".into();
    let (tx, mut rx) = tokio::sync::mpsc::channel(8);
    press(&mut app, K::Char('m'), &tx).await;
    assert_eq!(app.module_map.crate_id, "crate:b/Cargo.toml");
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 30)).unwrap();
    terminal
        .draw(|f| super::super::ui::render(f, &app))
        .unwrap();
    let text: String = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect();
    assert!(text.contains("Workspace > b > Modules"));
    assert!(text.contains("engine"));
    press(&mut app, K::Char('j'), &tx).await;
    press(&mut app, K::Char('o'), &tx).await;
    assert_eq!(
        app.explorer.file.as_ref().unwrap().path,
        std::path::Path::new("b/src/engine.rs")
    );
    assert!(
        matches!(rx.recv().await,Some(super::super::worker::AnalyzerCommand::InspectFile{file,..}) if file==std::path::Path::new("b/src/engine.rs"))
    );
    app.switch_view(View::Map);
    press(&mut app, K::Esc, &tx).await;
    assert!(!app.module_map.active);
    assert_eq!(app.map.selected, "crate:b/Cargo.toml");
    terminal
        .draw(|f| super::super::ui::render(f, &app))
        .unwrap();
    let text: String = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect();
    assert!(text.contains("CRATE INSPECTOR"));
    assert!(text.contains("b/Cargo.toml"));
}
#[test]
#[ignore = "manual release pipeline and populated module-map rendering timings"]
fn module_map_performance() {
    for path in [
        env!("CARGO_MANIFEST_DIR"),
        "/home/poke/code/uav-hit-estimator-tauri",
    ] {
        let root = std::path::Path::new(path);
        if !root.is_dir() {
            continue;
        }
        let project = crate::project::Project::discover(root).unwrap();
        let mut timings = vec![];
        let start = std::time::Instant::now();
        let tree = crate::repository::scan(root).unwrap();
        timings.push(format!("scan={:?}", start.elapsed()));
        let start = std::time::Instant::now();
        let graph = crate::module_graph::analyze(root, &tree, &project.architecture, || false);
        timings.push(format!("modules={:?}", start.elapsed()));
        let start = std::time::Instant::now();
        let metrics = crate::metrics::analyze(root, &tree, &project);
        timings.push(format!("metrics={:?}", start.elapsed()));
        let start = std::time::Instant::now();
        let ast = crate::maintainability::analyze(root, &tree, &project);
        timings.push(format!("maintainability={:?}", start.elapsed()));
        let start = std::time::Instant::now();
        let history = crate::git_analysis::analyze(root, &Default::default(), &Default::default());
        let hotspots = crate::git_analysis::correlate(history, &metrics, &ast, &tree);
        timings.push(format!("git+correlation={:?}", start.elapsed()));
        eprintln!("{path}: {} targets, {} nodes, {} mod edges, {} use edges, {} unresolved, {} conditional, parsed={} bytes={} partial={}",graph.targets.len(),graph.nodes.len(),graph.containment_edges.len(),graph.verified_import_edges.len(),graph.unresolved_count,graph.conditional_relations,graph.parsed_files,graph.read_bytes,graph.partial);
        let mut app = App::new(project);
        app.explorer.apply_tree(tree);
        app.metrics = Some(metrics);
        app.maintainability = Some(ast);
        app.hotspots = Some(hotspots);
        app.module_map.apply(graph);
        let crate_id = app
            .project
            .architecture
            .nodes
            .iter()
            .find(|n| n.name == "uav-runtime")
            .or_else(|| app.project.architecture.nodes.first())
            .unwrap()
            .id
            .clone();
        app.module_map.enter(&crate_id);
        app.switch_view(super::super::app::View::Map);
        app.color_mode = theme::ColorMode::TrueColor;
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(160, 45)).unwrap();
        terminal
            .draw(|f| super::super::ui::render(f, &app))
            .unwrap();
        let start = std::time::Instant::now();
        for _ in 0..100 {
            terminal
                .draw(|f| super::super::ui::render(f, &app))
                .unwrap();
        }
        timings.push(format!("map render={:?}/frame", start.elapsed() / 100));
        eprintln!("{}", timings.join(" · "));
    }
}
async fn press(
    app: &mut App,
    code: crossterm::event::KeyCode,
    tx: &tokio::sync::mpsc::Sender<super::super::worker::AnalyzerCommand>,
) {
    super::super::event::handle_key(
        app,
        crossterm::event::KeyEvent::new(code, crossterm::event::KeyModifiers::NONE),
        tx,
    )
    .await
    .unwrap();
}
fn guided_fixture() -> (tempfile::TempDir, App) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("Cargo.toml"),
        "[package]\nname='guided'\nversion='0.1.0'\nedition='2021'\n[lib]\ncrate-type=['cdylib']\n",
    )
    .unwrap();
    std::fs::write(
        root.join("src/main.rs"),
        format!(
            "mod a {{mod same {{}}}}\nmod b {{mod same {{}}}}\nfn main() {{\n{}\n}}",
            "let _value=0;\n".repeat(100)
        ),
    )
    .unwrap();
    std::fs::write(root.join("src/lib.rs"), "pub fn library() {}\n").unwrap();
    let project = crate::project::Project::discover(root).unwrap();
    let tree = crate::repository::scan(root).unwrap();
    let mut app = App::new(project);
    app.module_map.apply(crate::module_graph::analyze(
        root,
        &tree,
        &app.project.architecture,
        || false,
    ));
    app.metrics = Some(crate::metrics::analyze(root, &tree, &app.project));
    app.maintainability = Some(crate::maintainability::analyze(root, &tree, &app.project));
    app.explorer.apply_tree(tree);
    (temp, app)
}

#[test]
fn module_loading_import_styles_and_no_color_keep_relationship_identity() {
    use ratatui::{backend::TestBackend, style::Color, Terminal};
    let (_temp, mut app) = guided_fixture();
    app.switch_view(super::super::app::View::Map);
    app.color_mode = theme::ColorMode::TrueColor;
    let mut terminal = Terminal::new(TestBackend::new(120, 35)).unwrap();
    let mut g = app.module_map.graph.take().unwrap();
    terminal
        .draw(|f| super::super::ui::render(f, &app))
        .unwrap();
    let text: String = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect();
    assert!(text.contains("Loading module declarations"));
    let b = g
        .nodes
        .iter()
        .find(|n| n.qualified_path == "crate::b")
        .unwrap()
        .id
        .clone();
    let a = g
        .nodes
        .iter()
        .find(|n| n.qualified_path == "crate::a")
        .unwrap()
        .id
        .clone();
    // Explicit normalized renderer fixture; not semantic fallback or a PTY capture.
    g.verified_import_edges
        .push(crate::module_graph::ImportEdge {
            from: b.clone(),
            to: a,
            source_file: "src/main.rs".into(),
            line: 2,
            import_path: "crate::a".into(),
            alias: None,
            public: false,
            conditions: vec![],
        });
    app.module_map.graph = Some(g);
    app.module_map.focus = b.clone();
    app.module_map.selected = b;
    terminal
        .draw(|f| super::super::ui::render(f, &app))
        .unwrap();
    let buffer = terminal.backend().buffer();
    assert!(buffer
        .content
        .iter()
        .any(|c| c.symbol() == "▶" && c.fg == theme::ACCENT));
    assert!(buffer
        .content
        .iter()
        .any(|c| c.symbol() == "▶" && c.fg == theme::NUMBER));
    app.color_mode = theme::ColorMode::None;
    terminal
        .draw(|f| super::super::ui::render(f, &app))
        .unwrap();
    let buffer = terminal.backend().buffer();
    assert!(buffer
        .content
        .iter()
        .all(|c| c.fg == Color::Reset && c.bg == Color::Reset));
    let text: String = buffer.content.iter().map(|c| c.symbol()).collect();
    assert!(text.contains("mod▶"));
    assert!(text.contains("use▶"));
    assert!(text.contains("outside scope"));
}
#[tokio::test]
async fn guided_routes_have_observable_outcomes_and_unavailable_notices() {
    use super::super::{
        app::View,
        overview::{Guide, Target},
        worker::AnalyzerCommand,
    };
    use crossterm::event::KeyCode as K;
    let (_temp, mut app) = guided_fixture();
    let (tx, mut rx) = tokio::sync::mpsc::channel(16);
    for guide in [
        Guide::Architecture,
        Guide::Entrypoint,
        Guide::Hotspots,
        Guide::Longest,
    ] {
        app.switch_view(View::Overview);
        app.overview.selected = Target::Guide(guide).id();
        press(&mut app, K::Enter, &tx).await;
        match guide {
            Guide::Architecture => assert!(app.view == View::Map && app.module_map.active),
            Guide::Entrypoint => {
                assert!(app.map.target_picker.is_some());
                let targets = super::super::map::targets(&app);
                app.map.target_picker = Some(
                    targets
                        .iter()
                        .position(|t| t.path == std::path::Path::new("src/main.rs"))
                        .unwrap(),
                );
                press(&mut app, K::Enter, &tx).await;
                assert_eq!(
                    app.explorer.file.as_ref().unwrap().path,
                    std::path::Path::new("src/main.rs")
                );
                assert!(matches!(
                    rx.recv().await,
                    Some(AnalyzerCommand::InspectFile { .. })
                ));
            }
            Guide::Hotspots => assert!(app.notice.as_ref().unwrap().contains("analyzing")),
            Guide::Longest => assert!(matches!(
                super::super::overview::selected(&app),
                Some(Target::Longest(_))
            )),
        }
    }
    let mut history = crate::git_analysis::History::default();
    history.commits_analyzed = 3;
    history.files.insert(
        "src/main.rs".into(),
        crate::git_analysis::Change {
            commits: 3,
            added: Some(10),
            deleted: Some(2),
            last: 1792886400,
        },
    );
    app.hotspots = Some(crate::git_analysis::correlate(
        history,
        app.metrics.as_ref().unwrap(),
        app.maintainability.as_ref().unwrap(),
        app.explorer.tree.as_ref().unwrap(),
    ));
    app.overview.selected = Target::Guide(Guide::Hotspots).id();
    press(&mut app, K::Enter, &tx).await;
    assert!(
        matches!(super::super::overview::selected(&app),Some(Target::Hotspot(p)) if p==std::path::Path::new("src/main.rs"))
    );
}
#[tokio::test]
async fn inline_source_anchor_rejects_stale_requests_and_duplicate_names() {
    use crate::model::{Position, Range, Symbol};
    use crossterm::event::KeyCode as K;
    let (_temp, mut app) = guided_fixture();
    let g = app.module_map.graph.as_ref().unwrap();
    let n = g
        .nodes
        .iter()
        .find(|n| n.qualified_path == "crate::b::same")
        .unwrap()
        .clone();
    app.module_map.selected = n.id.clone();
    app.switch_view(super::super::app::View::Map);
    let (tx, mut rx) = tokio::sync::mpsc::channel(8);
    press(&mut app, K::Char('o'), &tx).await;
    let (id, file) = match rx.recv().await.unwrap() {
        super::super::worker::AnalyzerCommand::InspectFile { id, file } => (id, file),
        _ => panic!(),
    };
    assert_eq!(file, std::path::Path::new("src/main.rs"));
    assert!(app.explorer.file.as_ref().unwrap().module_anchor.is_some());
    let pos = Position {
        line: (n.declaration_line - 1) as u32,
        character: (n.declaration_column - 1) as u32,
    };
    let range = Range {
        start: pos,
        end: pos,
    };
    let exact = Symbol {
        id: "exact-inline".into(),
        name: "same".into(),
        kind: "module".into(),
        file: file.clone(),
        range,
        selection_range: range,
        container: Some("b".into()),
    };
    let mut wrong = exact.clone();
    wrong.id = "wrong-inline".into();
    wrong.selection_range.start.line = 0;
    app.explorer
        .apply_file_symbols(id + 1, &file, vec![exact.clone()]);
    assert!(app.explorer.selected_symbol().is_none());
    app.explorer
        .apply_file_symbols(id, &file, vec![wrong, exact]);
    assert_eq!(app.explorer.selected_symbol().unwrap().id, "exact-inline");
    app.explorer.fail_file(id, &file, "RA unavailable".into());
    assert!(app
        .explorer
        .file
        .as_ref()
        .unwrap()
        .anchor_notice
        .as_ref()
        .unwrap()
        .contains("unavailable"));
}
#[tokio::test]
async fn single_crate_auto_modules_focus_back_and_exact_source_without_ra() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    std::fs::create_dir_all(root.join("src/tui/map")).unwrap();
    for (p, s) in [
        (
            "Cargo.toml",
            "[package]\nname='demo'\nversion='0.1.0'\nedition='2021'\n",
        ),
        ("src/main.rs", "mod tui; fn main() {}"),
        ("src/tui.rs", "mod map;"),
        ("src/tui/map.rs", "mod graph;"),
        ("src/tui/map/graph.rs", "pub fn draw() {}"),
    ] {
        std::fs::write(root.join(p), s).unwrap();
    }
    let project = crate::project::Project::discover(root).unwrap();
    let tree = crate::repository::scan(root).unwrap();
    let graph = crate::module_graph::analyze(root, &tree, &project.architecture, || false);
    let mut app = App::new(project);
    app.explorer.apply_tree(tree);
    app.module_map.apply(graph);
    app.analyzer_state = super::super::app::AnalyzerState::Error("RA absent".into());
    let (tx, mut rx) = tokio::sync::mpsc::channel(8);
    async fn key(
        app: &mut App,
        code: crossterm::event::KeyCode,
        tx: &tokio::sync::mpsc::Sender<super::super::worker::AnalyzerCommand>,
    ) {
        super::super::event::handle_key(
            app,
            crossterm::event::KeyEvent::new(code, crossterm::event::KeyModifiers::NONE),
            tx,
        )
        .await
        .unwrap();
    }
    use crossterm::event::KeyCode as K;
    key(&mut app, K::Char('5'), &tx).await;
    for (w, h) in [(80, 20), (100, 30), (120, 35), (160, 45)] {
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(w, h)).unwrap();
        terminal
            .draw(|f| super::super::ui::render(f, &app))
            .unwrap();
        let text: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|c| c.symbol())
            .collect();
        assert!(text.contains("MODULE MAP"));
        assert!(text.contains("tui"));
    }
    key(&mut app, K::Char('j'), &tx).await;
    key(&mut app, K::Enter, &tx).await;
    assert!(app.module_map.choices().iter().any(|n| n.name == "map"));
    key(&mut app, K::Char('j'), &tx).await;
    key(&mut app, K::Enter, &tx).await;
    assert!(app.module_map.choices().iter().any(|n| n.name == "graph"));
    key(&mut app, K::Esc, &tx).await;
    assert_eq!(
        app.module_map
            .graph
            .as_ref()
            .unwrap()
            .node(&app.module_map.selected)
            .unwrap()
            .qualified_path,
        "crate::tui::map"
    );
    key(&mut app, K::Enter, &tx).await;
    key(&mut app, K::Char('j'), &tx).await;
    let selected = app.module_map.selected.clone();
    super::super::event::apply_repository(
        &mut app,
        super::super::RepositoryEvent::Metrics(Default::default()),
    );
    assert_eq!(app.module_map.selected, selected);
    key(&mut app, K::Char('o'), &tx).await;
    assert_eq!(
        app.explorer.file.as_ref().unwrap().path,
        std::path::Path::new("src/tui/map/graph.rs")
    );
    assert!(
        matches!(rx.recv().await,Some(super::super::worker::AnalyzerCommand::InspectFile{file,..}) if file==std::path::Path::new("src/tui/map/graph.rs"))
    );
}
