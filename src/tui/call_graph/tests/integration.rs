use super::*;

async fn settle(
    app: &mut App,
    events: &mut tokio::sync::mpsc::Receiver<AnalyzerEvent>,
    tx: &tokio::sync::mpsc::Sender<AnalyzerCommand>,
    predicate: impl Fn(&App) -> bool,
) {
    tokio::time::timeout(std::time::Duration::from_secs(120), async {
        while !predicate(app) {
            let event = events.recv().await.expect("worker stopped");
            let ready = matches!(&event, AnalyzerEvent::Ready);
            app.apply_event(event);
            if ready {
                if let Some((id, file)) = app.explorer.retry_anchor() {
                    crate::tui::journey::rebind(app, id, &file);
                    tx.send(AnalyzerCommand::InspectFile { id, file })
                        .await
                        .unwrap();
                }
            }
            crate::tui::journey::flush(app, tx).await;
            assert!(
                !matches!(app.analyzer_state, AnalyzerState::Error(_)),
                "RA failed: {:?}",
                app.analyzer_state
            );
        }
    })
    .await
    .expect("bounded real RA navigation timed out");
}
fn loaded(app: &App) -> bool {
    app.calls.center.is_some() && !app.calls.callers_loading && !app.calls.callees_loading
}

#[tokio::test]
async fn real_worker_binary_graph_two_levels_source_restore_and_recursion() {
    if !std::process::Command::new("rust-analyzer")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
    {
        assert!(std::env::var_os("CI").is_none(), "RA required in CI");
        eprintln!("SKIP real call graph: rust-analyzer unavailable");
        return;
    }
    let (_temp, mut app) = fixture();
    let (tx, rx) = tokio::sync::mpsc::channel(16);
    let (out, mut events) = tokio::sync::mpsc::channel(64);
    let worker = tokio::spawn(crate::tui::worker::analyzer_worker(
        app.project.clone(),
        rx,
        out,
    ));
    key(&mut app, K::Char('3'), &tx).await;
    key(&mut app, K::Enter, &tx).await;
    key(&mut app, K::Enter, &tx).await;
    settle(&mut app, &mut events, &tx, loaded).await;
    assert_eq!(app.calls.center.as_ref().unwrap().name, "main");
    assert_eq!(app.calls.callees[0].name, "start");
    key(&mut app, K::Char('g'), &tx).await;
    key(&mut app, K::Char('j'), &tx).await;
    assert_eq!(app.calls.graph.selected_target().unwrap().name, "start");
    key(&mut app, K::Char('x'), &tx).await;
    settle(&mut app, &mut events, &tx, |a| {
        a.calls.graph.pending.is_none()
    })
    .await;
    assert!(app
        .calls
        .graph
        .nodes
        .values()
        .any(|n| n.target.name == "process" && n.layer == 2));
    assert!(!app
        .calls
        .graph
        .nodes
        .values()
        .any(|n| n.target.name == "finish")); // Would be depth 3 from main.
    key(&mut app, K::Enter, &tx).await;
    settle(&mut app, &mut events, &tx, loaded).await;
    assert_eq!(app.calls.center.as_ref().unwrap().name, "start");
    while app.calls.graph.selected_target().unwrap().name != "process" {
        key(&mut app, K::Char('j'), &tx).await;
    }
    key(&mut app, K::Char('x'), &tx).await;
    settle(&mut app, &mut events, &tx, |a| {
        a.calls.graph.pending.is_none()
    })
    .await;
    let finish = app
        .calls
        .graph
        .nodes
        .iter()
        .find(|(_, n)| n.target.name == "finish")
        .map(|(id, _)| id.clone())
        .unwrap();
    let process = app
        .calls
        .graph
        .nodes
        .iter()
        .find(|(_, n)| n.target.name == "process")
        .map(|(id, _)| id.clone())
        .unwrap();
    let log = app
        .calls
        .graph
        .nodes
        .iter()
        .find(|(_, n)| n.target.name == "log")
        .map(|(id, _)| id.clone())
        .unwrap();
    assert!(app.calls.graph.edges.contains(&(process, finish.clone())));
    assert!(!app.calls.graph.edges.contains(&(log, finish.clone())));
    while app.calls.graph.selected.as_ref() != Some(&finish) {
        key(&mut app, K::Char('j'), &tx).await;
    }
    key(&mut app, K::Char('o'), &tx).await;
    settle(&mut app, &mut events, &tx, |a| {
        a.view == View::Explorer
            && a.explorer.right_mode == crate::tui::explorer::ExplorerRightMode::Inspector
    })
    .await;
    assert_eq!(app.inspector.as_ref().unwrap().symbol.name, "finish");
    assert_eq!(
        app.inspector
            .as_ref()
            .unwrap()
            .symbol
            .selection_range
            .start
            .line,
        4
    );
    key(&mut app, K::Esc, &tx).await;
    assert_eq!(app.view, View::Calls);
    assert_eq!(app.calls.graph.selected, Some(finish.clone()));
    key(&mut app, K::Char('o'), &tx).await;
    settle(&mut app, &mut events, &tx, |a| {
        a.view == View::Explorer
            && a.explorer.right_mode == crate::tui::explorer::ExplorerRightMode::Inspector
    })
    .await;
    key(&mut app, K::Char('c'), &tx).await;
    settle(&mut app, &mut events, &tx, loaded).await;
    assert_eq!(app.calls.center.as_ref().unwrap().name, "finish");
    key(&mut app, K::Esc, &tx).await;
    assert_eq!(app.calls.center.as_ref().unwrap().name, "start");
    assert_eq!(app.calls.graph.selected, Some(finish));
    key(&mut app, K::Esc, &tx).await;
    assert_eq!(app.calls.center.as_ref().unwrap().name, "main");
    assert!(app
        .calls
        .graph
        .nodes
        .values()
        .any(|n| n.target.name == "process"));
    // Reuse this worker's actual document symbols, not a constructed semantic Symbol.
    let recursive = app
        .explorer
        .file
        .as_ref()
        .unwrap()
        .symbols
        .iter()
        .find(|s| s.name == "recurse")
        .unwrap()
        .clone();
    let (id, target, scope) = app.load_calls(CallTarget::from_symbol(&recursive), false);
    tx.send(AnalyzerCommand::LoadCalls { id, target, scope })
        .await
        .unwrap();
    settle(&mut app, &mut events, &tx, loaded).await;
    let root = app.calls.graph.focus.as_ref().unwrap();
    assert!(app
        .calls
        .graph
        .edges
        .contains(&(root.clone(), root.clone())));
    tx.send(AnalyzerCommand::Shutdown).await.unwrap();
    worker.await.unwrap();
}
