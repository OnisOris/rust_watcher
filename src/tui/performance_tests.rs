//! Manual release-only measurements; semantic rows below are controlled renderer
//! fixtures, NOT fallback analysis or claimed analyzer output. Real UI review uses RA.
use super::{
    app::{App, Inspector, View},
    ui,
};
use crate::{
    architecture::Architecture,
    model::{CallTarget, Position, Range, Symbol},
    project::Project,
};
use ratatui::{backend::TestBackend, Terminal};
use std::{path::Path, time::Instant};

fn measure(root: &Path) {
    let project = Project::discover(root).unwrap();
    let metadata = cargo_metadata::MetadataCommand::new()
        .manifest_path(root.join("Cargo.toml"))
        .no_deps()
        .exec()
        .unwrap();
    let begin = Instant::now();
    for _ in 0..20 {
        std::hint::black_box(Architecture::from_metadata(&metadata));
    }
    let model = begin.elapsed() / 20;
    let tree = crate::repository::scan(root).unwrap();
    let mut app = App::new(project);
    app.metrics = Some(crate::metrics::analyze(root, &tree, &app.project));
    app.maintainability = Some(crate::maintainability::analyze(root, &tree, &app.project));
    app.hotspots = Some(crate::git_analysis::correlate(
        crate::git_analysis::analyze(root, &Default::default(), &Default::default()),
        app.metrics.as_ref().unwrap(),
        app.maintainability.as_ref().unwrap(),
        &tree,
    ));
    let file = app
        .project
        .architecture
        .nodes
        .iter()
        .flat_map(|n| &n.crate_roots)
        .find(|p| root.join(p).is_file())
        .cloned()
        .unwrap();
    app.explorer.apply_tree(tree);
    app.explorer.reveal_file(&file);
    app.explorer.open_selected();
    let symbols: Vec<_> = (0..50)
        .map(|n| {
            let point = Position {
                line: n,
                character: 0,
            };
            let range = Range {
                start: point,
                end: point,
            };
            Symbol {
                id: format!("render-fixture-{n}"),
                name: format!("render_function_{n:02}"),
                kind: "function".into(),
                file: file.clone(),
                range,
                selection_range: range,
                container: Some("RenderFixture".into()),
            }
        })
        .collect();
    app.symbols_query = "render".into();
    app.symbols_results = symbols.clone();
    let mut inspector = Inspector::loading(1, symbols[0].clone());
    inspector.source = Some(
        ">   1 | fn render_function_00() {\n    2 |     if ready { work(); }\n    3 | }".into(),
    );
    inspector.hover = Some(Some(
        "Controlled renderer fixture, not semantic output.".into(),
    ));
    app.inspector = Some(inspector);
    app.calls.center = Some(CallTarget::from_symbol(&symbols[0]));
    app.calls.callers = symbols
        .iter()
        .take(20)
        .map(CallTarget::from_symbol)
        .collect();
    app.calls.callees = app.calls.callers.clone();
    if let Some(file) = &mut app.explorer.file {
        file.symbols = symbols;
        file.loading = false;
    }
    app.color_mode = super::theme::ColorMode::TrueColor;
    let mut terminal = Terminal::new(TestBackend::new(160, 45)).unwrap();
    let mut timings = Vec::new();
    for view in [
        View::Overview,
        View::Symbols,
        View::Calls,
        View::Explorer,
        View::Map,
    ] {
        app.switch_view(view);
        terminal.draw(|f| ui::render(f, &app)).unwrap();
        let begin = Instant::now();
        for _ in 0..100 {
            terminal.draw(|f| ui::render(f, &app)).unwrap();
        }
        timings.push(format!("{view:?}={:?}", begin.elapsed() / 100));
    }
    eprintln!(
        "{}: nodes {}, edges {}, model {:?}; {} (160x45, controlled semantic renderer fixtures)",
        root.display(),
        app.project.architecture.nodes.len(),
        app.project.architecture.edges.len(),
        model,
        timings.join(", ")
    );
}

#[test]
#[ignore = "manual release architecture and populated-view rendering timings; no fragile assertions"]
fn architecture_and_five_views_performance() {
    measure(Path::new(env!("CARGO_MANIFEST_DIR")));
    let uav = Path::new("/home/poke/code/uav-hit-estimator-tauri");
    if uav.is_dir() {
        measure(uav);
    }
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let names: Vec<_> = (0..55).map(|n| format!("n{n:02}")).collect();
    std::fs::write(
        root.join("Cargo.toml"),
        format!(
            "[workspace]\nresolver='2'\nmembers=[{}]\n",
            names
                .iter()
                .map(|n| format!("'{n}'"))
                .collect::<Vec<_>>()
                .join(",")
        ),
    )
    .unwrap();
    for (i, name) in names.iter().enumerate() {
        let dir = root.join(name);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(
            dir.join("Cargo.toml"),
            format!(
                "[package]\nname='{name}'\nversion='0.1.0'\nedition='2021'\n{}",
                if i == 0 {
                    String::new()
                } else {
                    format!(
                        "[dependencies]\n{}={{path='../{}'}}\n",
                        names[i - 1],
                        names[i - 1]
                    )
                }
            ),
        )
        .unwrap();
        std::fs::write(dir.join("src/lib.rs"), "pub fn run() {}\n").unwrap();
    }
    measure(root);
}
