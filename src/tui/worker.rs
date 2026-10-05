use crate::model::{Diagnostic, Location, Symbol, SymbolSearchResult};
use crate::project::Project;
use crate::rust::RustAnalyzer;
use std::path::Path;
use std::time::Duration;
use tokio::sync::mpsc;

pub(super) enum AnalyzerCommand {
    Search { id: u64, query: String },
    Inspect { id: u64, symbol: Symbol },
    Shutdown,
}

pub(super) enum AnalyzerEvent {
    Indexing,
    Ready,
    Error(String),
    SearchResults {
        id: u64,
        result: SymbolSearchResult,
    },
    SearchFailed {
        id: u64,
        message: String,
    },
    Source {
        id: u64,
        value: String,
    },
    Hover {
        id: u64,
        value: Option<String>,
    },
    Definition {
        id: u64,
        value: Option<Location>,
    },
    References {
        id: u64,
        count: usize,
    },
    Calls {
        id: u64,
        callers: usize,
        callees: usize,
    },
    Diagnostics {
        id: u64,
        values: Vec<Diagnostic>,
    },
    InspectFailed {
        id: u64,
        section: &'static str,
        message: String,
    },
}

pub(super) async fn analyzer_worker(
    project: Project,
    mut commands: mpsc::Receiver<AnalyzerCommand>,
    events: mpsc::Sender<AnalyzerEvent>,
) {
    let mut analyzer = match RustAnalyzer::start(project, Path::new("rust-analyzer")).await {
        Ok(analyzer) => analyzer,
        Err(error) => {
            let _ = events
                .send(AnalyzerEvent::Error(format!("{error:#}")))
                .await;
            return;
        }
    };
    let _ = events.send(AnalyzerEvent::Indexing).await;
    let mut ready_sent = false;
    let mut error_sent = false;
    loop {
        if !error_sent {
            if let Some(error) = analyzer.error() {
                error_sent = true;
                let _ = events.send(AnalyzerEvent::Error(error)).await;
            }
        }
        if !ready_sent && analyzer.is_ready() {
            ready_sent = true;
            let _ = events.send(AnalyzerEvent::Ready).await;
        }
        tokio::select! {
            command = commands.recv() => match command {
                Some(AnalyzerCommand::Search { id, query }) => {
                    match analyzer.symbols(&query).await {
                        Ok(result) => {
                            let _ = events.send(AnalyzerEvent::SearchResults { id, result }).await;
                        }
                        Err(error) => {
                            let _ = events.send(AnalyzerEvent::SearchFailed {
                                id,
                                message: format!("{error:#}"),
                            }).await;
                        }
                    }
                }
                Some(AnalyzerCommand::Inspect { id, symbol }) => {
                    inspect_symbol(&mut analyzer, id, symbol, &events).await;
                }
                Some(AnalyzerCommand::Shutdown) | None => break,
            },
            _ = tokio::time::sleep(Duration::from_millis(100)) => {}
        }
    }
    analyzer.shutdown().await;
}

async fn inspect_symbol(
    analyzer: &mut RustAnalyzer,
    id: u64,
    symbol: Symbol,
    events: &mpsc::Sender<AnalyzerEvent>,
) {
    match analyzer.source_preview_for_symbol(&symbol) {
        Ok(value) => send(events, AnalyzerEvent::Source { id, value }).await,
        Err(error) => failed(events, id, "source", error).await,
    }
    match analyzer.definition(&symbol).await {
        Ok(value) => send(events, AnalyzerEvent::Definition { id, value }).await,
        Err(error) => failed(events, id, "definition", error).await,
    }
    match analyzer.hover(&symbol).await {
        Ok(value) => send(events, AnalyzerEvent::Hover { id, value }).await,
        Err(error) => failed(events, id, "hover", error).await,
    }
    match analyzer.references(&symbol).await {
        Ok(references) => {
            send(
                events,
                AnalyzerEvent::References {
                    id,
                    count: references.len(),
                },
            )
            .await;
        }
        Err(error) => failed(events, id, "references", error).await,
    }
    let callers = analyzer.calls(&symbol, 1, true).await;
    let callees = analyzer.calls(&symbol, 1, false).await;
    match (callers, callees) {
        (Ok(callers), Ok(callees)) => {
            send(
                events,
                AnalyzerEvent::Calls {
                    id,
                    callers: callers.nodes.len(),
                    callees: callees.nodes.len(),
                },
            )
            .await;
        }
        (Err(error), _) | (_, Err(error)) => failed(events, id, "calls", error).await,
    }
    match analyzer.diagnostics_for_file(&symbol.file).await {
        Ok(values) => send(events, AnalyzerEvent::Diagnostics { id, values }).await,
        Err(error) => failed(events, id, "diagnostics", error).await,
    }
}

async fn send(events: &mpsc::Sender<AnalyzerEvent>, event: AnalyzerEvent) {
    let _ = events.send(event).await;
}

async fn failed(
    events: &mpsc::Sender<AnalyzerEvent>,
    id: u64,
    section: &'static str,
    error: anyhow::Error,
) {
    send(
        events,
        AnalyzerEvent::InspectFailed {
            id,
            section,
            message: format!("{error:#}"),
        },
    )
    .await;
}
