use super::*;
use std::{
    hint::black_box,
    time::{Duration, Instant},
};
fn timings(app: &mut App, label: &str) {
    app.calls.graph.enabled = true;
    let mut terminal = Terminal::new(TestBackend::new(160, 45)).unwrap();
    let start = Instant::now();
    terminal.draw(|f| crate::tui::ui::render(f, app)).unwrap();
    let first = start.elapsed();
    let start = Instant::now();
    for _ in 0..200 {
        terminal.draw(|f| crate::tui::ui::render(f, app)).unwrap();
    }
    let render = start.elapsed() / 200;
    let start = Instant::now();
    for _ in 0..1000 {
        app.calls.graph.move_by(1);
        app.calls.graph.move_by(-1);
    }
    let selection = start.elapsed() / 2000;
    let start = Instant::now();
    for _ in 0..1000 {
        app.calls.graph.change_page(1);
        app.calls.graph.change_page(-1);
    }
    let page = start.elapsed() / 2000;
    let mut focus = Duration::ZERO;
    let mut back = Duration::ZERO;
    let target = app
        .calls
        .graph
        .nodes
        .values()
        .find(|n| n.layer == 1)
        .unwrap()
        .target
        .clone();
    for _ in 0..100 {
        let start = Instant::now();
        black_box(app.load_calls(target.clone(), true));
        focus += start.elapsed();
        let start = Instant::now();
        black_box(app.escape());
        back += start.elapsed();
    }
    let start = Instant::now();
    for _ in 0..1000 {
        app.calls.graph.enabled = black_box(!app.calls.graph.enabled);
    }
    let toggle = start.elapsed() / 1000;
    eprintln!("{label}: nodes={} edges={} first-render={first:?} cached-render={render:?} j/k={selection:?} page={page:?} focus-state={:?} Esc={:?} Graph/List={toggle:?}",app.calls.graph.nodes.len(),app.calls.graph.edges.len(),focus/100,back/100);
}
#[tokio::test]
#[ignore = "manual release real-RA and synthetic call graph stage timings; no timing assertions"]
async fn real_and_synthetic_graph_performance() {
    for (root, file, name) in [
        (env!("CARGO_MANIFEST_DIR"), "src/main.rs", "execute"),
        (
            "/home/poke/code/uav-hit-estimator-tauri",
            "apps/uav-runtime/src/main.rs",
            "main",
        ),
    ] {
        let root = std::path::Path::new(root);
        if !root.is_dir() {
            eprintln!("NOT TESTED: {} absent", root.display());
            continue;
        }
        let project = crate::project::Project::discover(root).unwrap();
        let mut app = App::new(project.clone());
        app.switch_view(View::Calls);
        app.analyzer_state = AnalyzerState::Ready;
        let start = Instant::now();
        let mut ra = crate::rust::RustAnalyzer::start_passive(
            project,
            std::path::Path::new("rust-analyzer"),
        )
        .await
        .unwrap();
        tokio::time::timeout(Duration::from_secs(180), async {
            while !ra.is_ready() {
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await
        .unwrap();
        let ready = start.elapsed();
        let symbols = ra
            .symbols_for_file(std::path::Path::new(file))
            .await
            .unwrap();
        let candidates: Vec<_> = symbols
            .iter()
            .filter(|s| s.name == name && crate::model::is_callable_kind(&s.kind))
            .collect();
        assert_eq!(candidates.len(), 1);
        let target = CallTarget::from_symbol(candidates[0]);
        let start = Instant::now();
        let callers = ra
            .callers_for_target_scoped(&target, CallScope::Workspace)
            .await
            .unwrap();
        let incoming = start.elapsed();
        let start = Instant::now();
        let callees = ra
            .callees_for_target_scoped(&target, CallScope::Workspace)
            .await
            .unwrap();
        let outgoing = start.elapsed();
        let start = Instant::now();
        let (id, _, _) = app.load_calls(target, false);
        app.apply_event(AnalyzerEvent::CallersLoaded {
            id,
            result: callers,
        });
        app.apply_event(AnalyzerEvent::CalleesLoaded {
            id,
            result: callees,
        });
        let creation = start.elapsed();
        let selected = app
            .calls
            .graph
            .nodes
            .iter()
            .find(|(_, n)| n.layer == 1 && n.target.file == std::path::Path::new("src/metrics.rs"))
            .or_else(|| app.calls.graph.nodes.iter().find(|(_, n)| n.layer == 1))
            .map(|(id, _)| id.clone())
            .unwrap();
        app.calls.graph.selected = Some(selected);
        let request = app.calls.graph.begin(1, id, CallScope::Workspace).unwrap();
        let start = Instant::now();
        let result = ra
            .callees_for_target_scoped(&request.target, request.scope)
            .await
            .unwrap();
        let expansion = start.elapsed();
        app.calls.graph.pending = None;
        app.calls.graph.ingest(&request.identity, false, Ok(result));
        eprintln!("{}: cold-ready={ready:?} incoming-RPC={incoming:?} outgoing-RPC={outgoing:?} state-create={creation:?} expansion-RPC={expansion:?} branch={}",root.display(),request.target.name);
        timings(&mut app, &root.display().to_string());
        ra.shutdown().await;
    }
    let (_tmp, mut app) = fixture();
    let root = target("root", 0);
    let start = Instant::now();
    let (id, _, _) = app.load_calls(root.clone(), false);
    app.apply_event(AnalyzerEvent::CallersLoaded {
        id,
        result: Some(CallTargets {
            items: (1..20).map(|i| target("caller", i)).collect(),
            truncated: true,
        }),
    });
    app.apply_event(AnalyzerEvent::CalleesLoaded {
        id,
        result: Some(CallTargets {
            items: (20..40).map(|i| target("callee", i)).collect(),
            truncated: false,
        }),
    });
    eprintln!("synthetic state-create={:?}", start.elapsed());
    timings(&mut app, "synthetic 40-node bounded call graph");
}
