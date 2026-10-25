mod app;
mod components;
mod event;
mod explorer;
mod highlight;
mod hotspots;
mod map;
mod module_map;
#[cfg(test)]
mod navigation_tests;
mod overview;
#[cfg(test)]
mod performance_tests;
mod theme;
mod ui;
mod worker;

use crate::project::Project;
use anyhow::Result;
use app::App;
use tokio::sync::mpsc;
use worker::{AnalyzerCommand, AnalyzerEvent};
// One shared pipeline, with bounded stack headroom for the existing syn pass.
// Keep Maintainability's measured coverage unchanged on deeply nested sources.
const PIPELINE_STACK_BYTES: usize = 16 * 1024 * 1024;

pub(super) enum RepositoryEvent {
    Tree(crate::repository::RepositoryTree),
    Modules(crate::module_graph::ModuleGraph),
    Metrics(crate::metrics::RepositoryMetrics),
    Maintainability(crate::maintainability::MaintainabilityReport),
    Hotspots(crate::git_analysis::HotspotReport),
    Error(String),
}

pub async fn run(project: Project) -> Result<()> {
    event::install_panic_hook();
    let mut terminal = event::TerminalSession::enter()?;
    let mut app = App::new(project.clone());

    // The first frame intentionally precedes rust-analyzer startup.
    terminal.draw(&app)?;

    let (command_tx, command_rx) = mpsc::channel(16);
    let (event_tx, mut event_rx) = mpsc::channel::<AnalyzerEvent>(64);
    let (repository_tx, mut repository_rx) = mpsc::channel(1);
    let analyzer_task = tokio::spawn(worker::analyzer_worker(project, command_rx, event_tx));
    let repository_root = app.project.workspace_root.clone();
    let metrics_project = app.project.clone();
    let git_runner = crate::git_analysis::GitRunner::default();
    let background_git = git_runner.clone();
    let repository_failure_tx = repository_tx.clone();
    let background = std::thread::Builder::new()
        .name("repository-analysis".into())
        .stack_size(PIPELINE_STACK_BYTES)
        .spawn(move || match crate::repository::scan(&repository_root) {
            Ok(tree) => {
                if repository_tx
                    .blocking_send(RepositoryEvent::Tree(tree.clone()))
                    .is_err()
                {
                    return;
                }
                let modules = crate::module_graph::analyze(
                    &repository_root,
                    &tree,
                    &metrics_project.architecture,
                    || repository_tx.is_closed(),
                );
                if repository_tx
                    .blocking_send(RepositoryEvent::Modules(modules))
                    .is_err()
                {
                    return;
                }
                let metrics = crate::metrics::analyze_until(
                    &repository_root,
                    &tree,
                    &metrics_project,
                    || repository_tx.is_closed(),
                );
                if repository_tx
                    .blocking_send(RepositoryEvent::Metrics(metrics.clone()))
                    .is_err()
                {
                    return;
                }
                let report = crate::maintainability::analyze_until(
                    &repository_root,
                    &tree,
                    &metrics_project,
                    || repository_tx.is_closed(),
                );
                if repository_tx
                    .blocking_send(RepositoryEvent::Maintainability(report.clone()))
                    .is_err()
                {
                    return;
                }
                let history = crate::git_analysis::analyze(
                    &repository_root,
                    &Default::default(),
                    &background_git,
                );
                let hotspots = crate::git_analysis::correlate(history, &metrics, &report, &tree);
                let _ = repository_tx.blocking_send(RepositoryEvent::Hotspots(hotspots));
            }
            Err(error) => {
                let _ = repository_tx.blocking_send(RepositoryEvent::Error(format!("{error:#}")));
            }
        });
    if let Err(error) = background {
        let _ = repository_failure_tx.try_send(RepositoryEvent::Error(format!(
            "Cannot start repository analyzer: {error}"
        )));
    }
    drop(repository_failure_tx);
    let result = event::run_loop(
        &mut terminal,
        &mut app,
        &command_tx,
        &mut event_rx,
        &mut repository_rx,
    )
    .await;
    drop(repository_rx);
    git_runner.cancel();
    stop_analyzer(analyzer_task, &command_tx).await;
    result
}

async fn stop_analyzer(
    mut task: tokio::task::JoinHandle<()>,
    commands: &mpsc::Sender<AnalyzerCommand>,
) {
    let _ = commands.try_send(AnalyzerCommand::Shutdown);
    if tokio::time::timeout(std::time::Duration::from_millis(250), &mut task)
        .await
        .is_err()
    {
        // Dropping the worker's LspClient kills its child (kill_on_drop), even
        // when an outstanding semantic request has not returned yet.
        task.abort();
        let _ = task.await;
    }
}

#[cfg(test)]
mod shutdown_tests {
    use super::*;
    #[test]
    fn bounded_pipeline_stack_preserves_existing_maintainability_depth_coverage() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        std::fs::write(
            root.join("Cargo.toml"),
            "[package]\nname='deep'\nversion='0.1.0'\nedition='2021'\n",
        )
        .unwrap();
        std::fs::create_dir(root.join("src")).unwrap();
        std::fs::write(
            root.join("src/lib.rs"),
            format!("{}{}", "mod n {".repeat(70), "}".repeat(70)),
        )
        .unwrap();
        let project = Project::discover(root).unwrap();
        let tree = crate::repository::scan(root).unwrap();
        let path = root.to_path_buf();
        let report = std::thread::Builder::new()
            .stack_size(PIPELINE_STACK_BYTES)
            .spawn(move || crate::maintainability::analyze(&path, &tree, &project))
            .unwrap()
            .join()
            .unwrap();
        assert_eq!(report.parsed_files, 1);
        assert!(!report.partial);
    }
    #[tokio::test]
    async fn quit_aborts_inflight_work_even_when_command_queue_is_full() {
        struct Dropped(std::sync::Arc<std::sync::atomic::AtomicBool>);
        impl Drop for Dropped {
            fn drop(&mut self) {
                self.0.store(true, std::sync::atomic::Ordering::SeqCst);
            }
        }
        let dropped = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = dropped.clone();
        let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(async move {
            let _guard = Dropped(flag);
            let _ = ready_tx.send(());
            std::future::pending::<()>().await;
        });
        ready_rx.await.unwrap();
        let (tx, _rx) = mpsc::channel(1);
        tx.try_send(AnalyzerCommand::Shutdown).ok().unwrap();
        stop_analyzer(task, &tx).await;
        assert!(dropped.load(std::sync::atomic::Ordering::SeqCst));
    }
}
