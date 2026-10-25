mod app;
mod components;
mod event;
mod explorer;
mod highlight;
mod hotspots;
mod map;
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

pub(super) enum RepositoryEvent {
    Tree(crate::repository::RepositoryTree),
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
    std::thread::spawn(move || match crate::repository::scan(&repository_root) {
        Ok(tree) => {
            if repository_tx
                .blocking_send(RepositoryEvent::Tree(tree.clone()))
                .is_err()
            {
                return;
            }
            let metrics =
                crate::metrics::analyze_until(&repository_root, &tree, &metrics_project, || {
                    repository_tx.is_closed()
                });
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
