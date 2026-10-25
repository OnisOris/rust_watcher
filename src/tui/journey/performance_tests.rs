//! Manual release timings, deliberately without CI wall-clock assertions.
use super::super::{event::handle_key, worker::AnalyzerEvent};
use super::*;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::{path::Path, time::Instant};
#[tokio::test]
#[ignore = "manual real RA entrypoint/documentSymbol/Calls/Esc latency on local workspaces"]
async fn entrypoint_performance() {
    for (path, name) in [
        (env!("CARGO_MANIFEST_DIR"), "wt"),
        ("/home/poke/code/uav-hit-estimator-tauri", "uav-runtime"),
    ] {
        let root = Path::new(path);
        if !root.is_dir() {
            eprintln!("NOT TESTED {path}: unavailable");
            continue;
        }
        let p = crate::project::Project::discover(root).unwrap();
        let tree = crate::repository::scan(root).unwrap();
        let mut ra =
            crate::rust::RustAnalyzer::start_passive(p.clone(), Path::new("rust-analyzer"))
                .await
                .unwrap();
        let ready = Instant::now();
        // Match the TUI's status-driven readiness, not headless wait_ready's
        // fixed 30s deadline, which can expire on a large cold workspace.
        tokio::time::timeout(std::time::Duration::from_secs(180), async {
            while !ra.is_ready() {
                assert!(
                    ra.error().is_none(),
                    "RA error during manual benchmark: {:?}",
                    ra.error()
                );
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
        })
        .await
        .expect("manual TUI readiness benchmark exceeded 180s");
        let ready = ready.elapsed();
        let mut app = App::new(p);
        app.explorer.apply_tree(tree);
        app.analyzer_state = AnalyzerState::Ready;
        let (tx, mut rx) = tokio::sync::mpsc::channel(16);
        let open = Instant::now();
        let (id, file) = super::entrypoint_tests::choose(&mut app, name, &tx, &mut rx).await;
        let doc = Instant::now();
        let symbols = ra.symbols_for_file(&file).await.unwrap();
        let doc = doc.elapsed();
        app.apply_event(AnalyzerEvent::FileSymbols { id, file, symbols });
        flush(&mut app, &tx).await;
        let AnalyzerCommand::LoadCalls { id, target, scope } = rx.recv().await.unwrap() else {
            panic!()
        };
        let current = open.elapsed();
        let incoming = ra.callers_for_target_scoped(&target, scope).await.unwrap();
        let outgoing = ra.callees_for_target_scoped(&target, scope).await.unwrap();
        let loaded = open.elapsed();
        app.apply_event(AnalyzerEvent::CallersLoaded {
            id,
            result: incoming,
        });
        app.apply_event(AnalyzerEvent::CalleesLoaded {
            id,
            result: outgoing,
        });
        let start = Instant::now();
        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
            &tx,
        )
        .await
        .unwrap();
        let back = start.elapsed();
        eprintln!("{name}: ready={ready:?} documentSymbol={doc:?} entry→CURRENT={current:?} entry→loadedCalls={loaded:?} Esc={back:?} target={} {}:{}",target.name,target.file.display(),target.selection_range.start.line+1);
        ra.shutdown().await;
    }
}
