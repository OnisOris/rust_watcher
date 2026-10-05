mod app;
mod event;
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
    let analyzer_task = tokio::spawn(worker::analyzer_worker(project, command_rx, event_tx));
    let result = event::run_loop(&mut terminal, &mut app, &command_tx, &mut event_rx).await;
    let _ = command_tx.send(AnalyzerCommand::Shutdown).await;
    let _ = analyzer_task.await;
    result
}
