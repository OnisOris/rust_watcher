use crate::model::{
    CallScope, CallTarget, CallTargets, Diagnostic, Location, Symbol, SymbolSearchResult,
};
use crate::project::Project;
use crate::rust::RustAnalyzer;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::sync::mpsc;

#[derive(Debug, Clone)]
pub(super) enum AnalyzerCommand {
    Search {
        id: u64,
        query: String,
    },
    Inspect {
        id: u64,
        symbol: Symbol,
    },
    InspectFile {
        id: u64,
        file: PathBuf,
    },
    LoadCalls {
        id: u64,
        target: CallTarget,
        scope: CallScope,
    },
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
    InspectorCallers {
        id: u64,
        count: usize,
    },
    InspectorCallees {
        id: u64,
        count: usize,
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
    CallersLoaded {
        id: u64,
        result: Option<CallTargets>,
    },
    CalleesLoaded {
        id: u64,
        result: Option<CallTargets>,
    },
    CallsFailed {
        id: u64,
        incoming: bool,
        message: String,
    },
    FileSymbols {
        id: u64,
        file: PathBuf,
        symbols: Vec<Symbol>,
    },
    FileSymbolsFailed {
        id: u64,
        file: PathBuf,
        message: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InspectPhase {
    Source,
    Definition,
    Hover,
    References,
    Callers,
    Callees,
    Diagnostics,
}

impl InspectPhase {
    fn next(self) -> Option<Self> {
        match self {
            Self::Source => Some(Self::Definition),
            Self::Definition => Some(Self::Hover),
            Self::Hover => Some(Self::References),
            Self::References => Some(Self::Callers),
            Self::Callers => Some(Self::Callees),
            Self::Callees => Some(Self::Diagnostics),
            Self::Diagnostics => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CallsPhase {
    Callers,
    Callees,
}

#[derive(Debug, Clone)]
enum InteractiveWork {
    Inspect {
        id: u64,
        symbol: Symbol,
        phase: InspectPhase,
    },
    Calls {
        id: u64,
        target: CallTarget,
        scope: CallScope,
        phase: CallsPhase,
    },
    File {
        id: u64,
        file: PathBuf,
    },
}

#[derive(Default)]
struct Scheduler {
    search: Option<(u64, String)>,
    work: Option<InteractiveWork>,
    shutdown: bool,
}

impl Scheduler {
    fn push(&mut self, command: AnalyzerCommand) {
        match command {
            AnalyzerCommand::Search { id, query } => self.search = Some((id, query)),
            AnalyzerCommand::Inspect { id, symbol } => {
                self.work = Some(InteractiveWork::Inspect {
                    id,
                    symbol,
                    phase: InspectPhase::Source,
                })
            }
            AnalyzerCommand::InspectFile { id, file } => {
                self.work = Some(InteractiveWork::File { id, file })
            }
            AnalyzerCommand::LoadCalls { id, target, scope } => {
                self.work = Some(InteractiveWork::Calls {
                    id,
                    target,
                    scope,
                    phase: CallsPhase::Callers,
                })
            }
            AnalyzerCommand::Shutdown => {
                self.shutdown = true;
                self.search = None;
                self.work = None;
            }
        }
    }

    fn resume(&mut self, work: Option<InteractiveWork>) {
        if !self.shutdown && self.work.is_none() {
            self.work = work;
        }
    }
}

pub(super) async fn analyzer_worker(
    project: Project,
    mut commands: mpsc::Receiver<AnalyzerCommand>,
    events: mpsc::Sender<AnalyzerEvent>,
) {
    let mut analyzer = match RustAnalyzer::start(project, Path::new("rust-analyzer")).await {
        Ok(analyzer) => analyzer,
        Err(error) => {
            let message = format!("{error:#}");
            let _ = events.send(AnalyzerEvent::Error(message.clone())).await;
            unavailable_worker(&mut commands, &events, &message).await;
            return;
        }
    };
    let _ = events.send(AnalyzerEvent::Indexing).await;
    let mut scheduler = Scheduler::default();
    let mut ready_sent = false;
    let mut error_sent = false;
    loop {
        while let Ok(command) = commands.try_recv() {
            scheduler.push(command);
        }
        if scheduler.shutdown {
            break;
        }
        if !error_sent {
            if let Some(error) = analyzer.error() {
                error_sent = true;
                send(&events, AnalyzerEvent::Error(error)).await;
            }
        }
        if !ready_sent && analyzer.is_ready() {
            ready_sent = true;
            send(&events, AnalyzerEvent::Ready).await;
        }
        if let Some((id, query)) = scheduler.search.take() {
            match analyzer.symbols(&query).await {
                Ok(result) => send(&events, AnalyzerEvent::SearchResults { id, result }).await,
                Err(error) => {
                    send(
                        &events,
                        AnalyzerEvent::SearchFailed {
                            id,
                            message: format!("{error:#}"),
                        },
                    )
                    .await
                }
            }
            continue;
        }
        if let Some(work) = scheduler.work.take() {
            let continuation = run_phase(&mut analyzer, work, &events).await;
            scheduler.resume(continuation);
            continue;
        }
        tokio::select! {
            command = commands.recv() => match command { Some(command) => scheduler.push(command), None => scheduler.push(AnalyzerCommand::Shutdown) },
            _ = tokio::time::sleep(Duration::from_millis(100)) => {}
        }
    }
    analyzer.shutdown().await;
}

async fn unavailable_worker(
    commands: &mut mpsc::Receiver<AnalyzerCommand>,
    events: &mpsc::Sender<AnalyzerEvent>,
    message: &str,
) {
    while let Some(command) = commands.recv().await {
        match command {
            AnalyzerCommand::Search { id, .. } => {
                send(
                    events,
                    AnalyzerEvent::SearchFailed {
                        id,
                        message: message.to_owned(),
                    },
                )
                .await;
            }
            AnalyzerCommand::Inspect { id, .. } => {
                send(
                    events,
                    AnalyzerEvent::InspectFailed {
                        id,
                        section: "analyzer",
                        message: message.to_owned(),
                    },
                )
                .await;
            }
            AnalyzerCommand::InspectFile { id, file } => {
                send(
                    events,
                    AnalyzerEvent::FileSymbolsFailed {
                        id,
                        file,
                        message: message.to_owned(),
                    },
                )
                .await;
            }
            AnalyzerCommand::LoadCalls { id, .. } => {
                for incoming in [true, false] {
                    send(
                        events,
                        AnalyzerEvent::CallsFailed {
                            id,
                            incoming,
                            message: message.to_owned(),
                        },
                    )
                    .await;
                }
            }
            AnalyzerCommand::Shutdown => break,
        }
    }
}

async fn run_phase(
    analyzer: &mut RustAnalyzer,
    work: InteractiveWork,
    events: &mpsc::Sender<AnalyzerEvent>,
) -> Option<InteractiveWork> {
    match work {
        InteractiveWork::Inspect { id, symbol, phase } => {
            run_inspect_phase(analyzer, id, &symbol, phase, events).await;
            phase
                .next()
                .map(|phase| InteractiveWork::Inspect { id, symbol, phase })
        }
        InteractiveWork::Calls {
            id,
            target,
            scope,
            phase,
        } => {
            run_calls_phase(analyzer, id, &target, scope, phase, events).await;
            match phase {
                CallsPhase::Callers => Some(InteractiveWork::Calls {
                    id,
                    target,
                    scope,
                    phase: CallsPhase::Callees,
                }),
                CallsPhase::Callees => None,
            }
        }
        InteractiveWork::File { id, file } => {
            match analyzer.symbols_for_file(&file).await {
                Ok(symbols) => send(events, AnalyzerEvent::FileSymbols { id, file, symbols }).await,
                Err(error) => {
                    send(
                        events,
                        AnalyzerEvent::FileSymbolsFailed {
                            id,
                            file,
                            message: format!("{error:#}"),
                        },
                    )
                    .await
                }
            }
            None
        }
    }
}

async fn run_inspect_phase(
    analyzer: &mut RustAnalyzer,
    id: u64,
    symbol: &Symbol,
    phase: InspectPhase,
    events: &mpsc::Sender<AnalyzerEvent>,
) {
    let error = match phase {
        InspectPhase::Source => match analyzer.source_preview_for_symbol(symbol) {
            Ok(value) => return send(events, AnalyzerEvent::Source { id, value }).await,
            Err(error) => error,
        },
        InspectPhase::Definition => match analyzer.definition(symbol).await {
            Ok(value) => return send(events, AnalyzerEvent::Definition { id, value }).await,
            Err(error) => error,
        },
        InspectPhase::Hover => match analyzer.hover(symbol).await {
            Ok(value) => return send(events, AnalyzerEvent::Hover { id, value }).await,
            Err(error) => error,
        },
        InspectPhase::References => match analyzer.references(symbol).await {
            Ok(values) => {
                return send(
                    events,
                    AnalyzerEvent::References {
                        id,
                        count: values.len(),
                    },
                )
                .await
            }
            Err(error) => error,
        },
        InspectPhase::Callers => match analyzer.calls(symbol, 1, true).await {
            Ok(value) => {
                return send(
                    events,
                    AnalyzerEvent::InspectorCallers {
                        id,
                        count: value.nodes.len(),
                    },
                )
                .await
            }
            Err(error) => error,
        },
        InspectPhase::Callees => match analyzer.calls(symbol, 1, false).await {
            Ok(value) => {
                return send(
                    events,
                    AnalyzerEvent::InspectorCallees {
                        id,
                        count: value.nodes.len(),
                    },
                )
                .await
            }
            Err(error) => error,
        },
        InspectPhase::Diagnostics => match analyzer.diagnostics_for_file(&symbol.file).await {
            Ok(values) => return send(events, AnalyzerEvent::Diagnostics { id, values }).await,
            Err(error) => error,
        },
    };
    failed(events, id, inspect_section(phase), error).await;
}

fn inspect_section(phase: InspectPhase) -> &'static str {
    match phase {
        InspectPhase::Source => "source",
        InspectPhase::Definition => "definition",
        InspectPhase::Hover => "hover",
        InspectPhase::References => "references",
        InspectPhase::Callers | InspectPhase::Callees => "calls",
        InspectPhase::Diagnostics => "diagnostics",
    }
}

async fn run_calls_phase(
    analyzer: &RustAnalyzer,
    id: u64,
    target: &CallTarget,
    scope: CallScope,
    phase: CallsPhase,
    events: &mpsc::Sender<AnalyzerEvent>,
) {
    let incoming = phase == CallsPhase::Callers;
    let result = if incoming {
        analyzer.callers_for_target_scoped(target, scope).await
    } else {
        analyzer.callees_for_target_scoped(target, scope).await
    };
    match result {
        Ok(result) if incoming => send(events, AnalyzerEvent::CallersLoaded { id, result }).await,
        Ok(result) => send(events, AnalyzerEvent::CalleesLoaded { id, result }).await,
        Err(error) => {
            send(
                events,
                AnalyzerEvent::CallsFailed {
                    id,
                    incoming,
                    message: format!("{error:#}"),
                },
            )
            .await
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Position, Range};

    fn symbol(name: &str) -> Symbol {
        Symbol {
            id: name.into(),
            name: name.into(),
            kind: "function".into(),
            file: "src/lib.rs".into(),
            range: Range {
                start: Position {
                    line: 0,
                    character: 0,
                },
                end: Position {
                    line: 1,
                    character: 0,
                },
            },
            selection_range: Range {
                start: Position {
                    line: 0,
                    character: 3,
                },
                end: Position {
                    line: 0,
                    character: 4,
                },
            },
            container: None,
        }
    }

    #[test]
    fn scheduler_coalesces_work_and_prioritizes_shutdown() {
        let mut scheduler = Scheduler::default();
        scheduler.push(AnalyzerCommand::Inspect {
            id: 1,
            symbol: symbol("A"),
        });
        scheduler.push(AnalyzerCommand::Inspect {
            id: 2,
            symbol: symbol("B"),
        });
        scheduler.push(AnalyzerCommand::Inspect {
            id: 3,
            symbol: symbol("C"),
        });
        assert!(matches!(
            scheduler.work,
            Some(InteractiveWork::Inspect { id: 3, .. })
        ));
        scheduler.push(AnalyzerCommand::LoadCalls {
            id: 4,
            target: CallTarget::from_symbol(&symbol("D")),
            scope: CallScope::Workspace,
        });
        scheduler.push(AnalyzerCommand::LoadCalls {
            id: 5,
            target: CallTarget::from_symbol(&symbol("E")),
            scope: CallScope::All,
        });
        assert!(matches!(
            scheduler.work,
            Some(InteractiveWork::Calls { id: 5, .. })
        ));
        scheduler.push(AnalyzerCommand::InspectFile {
            id: 6,
            file: "src/app.rs".into(),
        });
        scheduler.push(AnalyzerCommand::InspectFile {
            id: 7,
            file: "src/worker.rs".into(),
        });
        assert!(matches!(
            scheduler.work,
            Some(InteractiveWork::File { id: 7, .. })
        ));
        scheduler.push(AnalyzerCommand::Shutdown);
        assert!(scheduler.shutdown);
        assert!(scheduler.work.is_none());
    }

    #[test]
    fn search_is_queued_independently_between_phases() {
        let mut scheduler = Scheduler::default();
        scheduler.push(AnalyzerCommand::Inspect {
            id: 1,
            symbol: symbol("A"),
        });
        scheduler.push(AnalyzerCommand::Search {
            id: 2,
            query: "B".into(),
        });
        assert_eq!(scheduler.search.as_ref().unwrap().0, 2);
        assert!(scheduler.work.is_some());
    }
    #[tokio::test]
    async fn unavailable_worker_keeps_channel_alive_and_reports_failures() {
        let (commands, mut receiver) = mpsc::channel(4);
        let (events, mut event_receiver) = mpsc::channel(8);
        let task = tokio::spawn(async move {
            unavailable_worker(&mut receiver, &events, "rust-analyzer unavailable").await;
        });

        commands
            .send(AnalyzerCommand::InspectFile {
                id: 7,
                file: "src/camera.rs".into(),
            })
            .await
            .unwrap();
        match event_receiver.recv().await.unwrap() {
            AnalyzerEvent::FileSymbolsFailed { id, file, message } => {
                assert_eq!(id, 7);
                assert_eq!(file, PathBuf::from("src/camera.rs"));
                assert_eq!(message, "rust-analyzer unavailable");
            }
            _ => panic!("expected file semantic failure"),
        }

        commands
            .send(AnalyzerCommand::Search {
                id: 8,
                query: "camera".into(),
            })
            .await
            .unwrap();
        match event_receiver.recv().await.unwrap() {
            AnalyzerEvent::SearchFailed { id, message } => {
                assert_eq!(id, 8);
                assert_eq!(message, "rust-analyzer unavailable");
            }
            _ => panic!("expected search failure"),
        }

        commands.send(AnalyzerCommand::Shutdown).await.unwrap();
        task.await.unwrap();
    }

}
