mod app;
mod event;
mod explorer;
mod ui;
mod worker;

use crate::project::Project;
use anyhow::Result;
use app::App;
use tokio::sync::mpsc;
use worker::{AnalyzerCommand, AnalyzerEvent};

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
    std::thread::spawn(move || {
        let result =
            crate::repository::scan(&repository_root).map_err(|error| format!("{error:#}"));
        let _ = repository_tx.blocking_send(result);
    });
    let result = event::run_loop(
        &mut terminal,
        &mut app,
        &command_tx,
        &mut event_rx,
        &mut repository_rx,
    )
    .await;
    let _ = command_tx.send(AnalyzerCommand::Shutdown).await;
    let _ = analyzer_task.await;
    result
}
