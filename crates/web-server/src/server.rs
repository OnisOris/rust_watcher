use anyhow::{Context, Result};
use graph_builder::{python, qml, typescript};
use graph_core::{
    AnalysisEvent, AnalysisEventType, AnalyzerCapability, AnalyzerEngine, AnalyzerKind,
    AnalyzerProvider, AnalyzerServiceStatus, AnalyzerStatus, AppState, AppStatus, GraphSnapshot,
    PythonAnalyzerStatus, ServerMessage,
};
use parking_lot::RwLock;
use project_indexer::start_watcher;
use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::runtime::Handle;
use tokio::sync::broadcast;
use tokio::time::{sleep, Duration};
use tower_http::cors::CorsLayer;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::trace::TraceLayer;
use tracing::{info, warn};
use uuid::Uuid;

use crate::analyzer_paths::resolve_rust_analyzer;
use crate::python_ty::{PythonAnalyzerMode, PythonTyState};
use crate::qml_lsp::{
    status_to_analyzer_status as qml_status_to_analyzer_status, QmlAnalyzerMode, QmlAnalyzerStatus,
    QmlLspState,
};
use crate::routes;
use crate::routes::layout::{apply_saved_layout, timestamp};
use crate::state::{rust_analyzer_state, AppStateHandle};
use crate::typescript_lsp::{
    status_to_analyzer_status, TypeScriptAnalyzerMode, TypeScriptAnalyzerStatus, TypeScriptLspState,
};
use crate::{analysis, ServeArgs};

pub(crate) async fn serve(args: ServeArgs) -> Result<()> {
    if args.host.is_unspecified() {
        warn!(host = %args.host, "explicitly binding to an unspecified address");
    }

    let project_root = args
        .project
        .clone()
        .unwrap_or(std::env::current_dir().context("failed to read current directory")?)
        .canonicalize()
        .context("failed to canonicalize project root")?;

    let python_analyzer_mode = if args.disable_ty {
        PythonAnalyzerMode::Parser
    } else {
        args.python_analyzer
    };
    let python_ty = Arc::new(PythonTyState::new(
        args.ty_path.clone(),
        python_analyzer_mode,
        project_root.clone(),
    ));
    let typescript_analyzer_mode = if args.disable_typescript_language_server {
        TypeScriptAnalyzerMode::Parser
    } else {
        args.typescript_analyzer
    };
    let typescript_lsp = Arc::new(TypeScriptLspState::new(
        args.typescript_language_server_path.clone(),
        typescript_analyzer_mode,
        project_root.clone(),
    ));
    let qml_analyzer_mode = if args.disable_qmlls {
        QmlAnalyzerMode::Parser
    } else {
        args.qml_analyzer
    };
    let qml_lsp = Arc::new(QmlLspState::new(
        args.qmlls_path.clone(),
        qml_analyzer_mode,
        args.qmlls_build_dir.clone(),
        args.qmlls_no_cmake_calls,
        project_root.clone(),
    ));
    let rust_analyzer = resolve_rust_analyzer(&args.rust_analyzer, &project_root);

    let initial_status = AppStatus {
        app_state: AppState::Empty,
        analyzer_status: AnalyzerStatus::Starting,
        analyzers: initial_analyzer_services(
            AnalyzerStatus::Starting,
            Some(python_ty.status_record()),
            Some(typescript_lsp.status_record()),
            Some(qml_lsp.status_record()),
            0,
            None,
        ),
        python_analyzer: Some(python_ty.status_record()),
        project_name: project_root
            .file_name()
            .and_then(|n| n.to_str())
            .map(str::to_string),
        project_path: Some(project_root.display().to_string()),
        last_updated: None,
        message: None,
        progress: None,
    };
    let initial_snapshot = GraphSnapshot {
        nodes: Vec::new(),
        edges: Vec::new(),
        files: Vec::new(),
        events: Vec::new(),
        status: initial_status.clone(),
    };
    let (ws_tx, _) = broadcast::channel(64);
    let analyzer = Arc::new(rust_analyzer_state(
        rust_analyzer.clone(),
        project_root.clone(),
    ));
    let state = AppStateHandle {
        project_root: Arc::new(RwLock::new(project_root.clone())),
        graph_indexes: Arc::new(RwLock::new(graph_query::build_graph_indexes(
            &initial_snapshot,
        ))),
        search_index: Arc::new(RwLock::new(graph_query::build_search_index(
            &initial_snapshot,
        ))),
        graph: Arc::new(RwLock::new(initial_snapshot)),
        status: Arc::new(RwLock::new(initial_status)),
        ws_tx,
        analyzer,
        python_ty,
        typescript_lsp,
        qml_lsp,
        diagnostics_by_file: Arc::new(RwLock::new(HashMap::new())),
        diagnostics_by_node: Arc::new(RwLock::new(HashMap::new())),
        watcher: Arc::new(RwLock::new(None)),
        is_indexing: Arc::new(AtomicBool::new(false)),
        pending_changed_files: Arc::new(RwLock::new(HashSet::new())),
        pending_changed_files_root: Arc::new(RwLock::new(None)),
        watcher_debounce_running: Arc::new(AtomicBool::new(false)),
        watcher_event_generation: Arc::new(AtomicU64::new(0)),
        enable_editor_open: args.enable_editor_open,
    };
    install_watcher(&state, project_root.clone());

    info!(project_root = %project_root.display(), frontend_dist = %args.frontend_dist.display(), rust_analyzer = %rust_analyzer.display(), python_analyzer = ?python_analyzer_mode, ty = %args.ty_path.display(), typescript_analyzer = ?typescript_analyzer_mode, typescript_language_server = %args.typescript_language_server_path.display(), qml_analyzer = ?qml_analyzer_mode, qmlls = %args.qmlls_path.display(), "starting Rust Code Command Center");

    let index_state = state.clone();
    tokio::spawn(async move {
        analysis::index_and_publish(index_state, project_root).await;
    });

    let app = routes::router()
        .fallback_service(
            ServeDir::new(&args.frontend_dist)
                .not_found_service(ServeFile::new(args.frontend_dist.join("index.html"))),
        )
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
        .with_state(state);

    let listener = TcpListener::bind(SocketAddr::new(args.host, args.port))
        .await
        .with_context(|| format!("failed to bind {}:{}", args.host, args.port))?;
    let local_addr = listener.local_addr()?;
    let url = format!("http://{local_addr}");
    println!("{url}");
    info!(%url, "server listening");
    if args.open {
        if let Err(error) = webbrowser::open(&url) {
            warn!(?error, "failed to open browser");
        }
    }

    axum::serve(listener, app).await?;
    Ok(())
}

pub(crate) fn install_watcher(state: &AppStateHandle, root: PathBuf) {
    clear_pending_changed_files(
        &state.pending_changed_files_root,
        &state.pending_changed_files,
    );
    let handle = Handle::current();
    let watch_state = state.clone();
    let watched_root = root.clone();
    match start_watcher(root.clone(), move |event| {
        let state = watch_state.clone();
        let watched_root = watched_root.clone();
        let root = state.project_root.read().clone();
        if root != watched_root {
            return;
        }
        let changed_path = event
            .paths
            .first()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "unknown".to_string());
        let changed_files = event
            .paths
            .iter()
            .map(|path| project_indexer::relative_to(&root, path))
            .collect::<Vec<_>>();
        enqueue_pending_changed_files(
            &state.pending_changed_files_root,
            &state.pending_changed_files,
            &root,
            changed_files,
        );
        state
            .watcher_event_generation
            .fetch_add(1, Ordering::SeqCst);
        spawn_watcher_debounce_loop(&handle, state.clone());
        handle.spawn(async move {
            let analysis_event = analysis_event(
                AnalysisEventType::Analyzer,
                format!("File changed: {changed_path}"),
                Some(changed_path),
            );
            {
                let mut graph = state.graph.write();
                graph.events.push(analysis_event.clone());
            }
            update_status(&state, |status| {
                status.analyzer_status = AnalyzerStatus::Stale;
                status.message = Some("File changed. Re-indexing workspace.".into());
                status.progress = Some(0);
            });
            let _ = state
                .ws_tx
                .send(ServerMessage::AnalysisEvent(analysis_event));
        });
    }) {
        Ok(watcher) => {
            *state.watcher.write() = Some(watcher);
            info!(project_root = %root.display(), "file watcher installed");
        }
        Err(error) => {
            warn!(project_root = %root.display(), ?error, "failed to install file watcher")
        }
    }
}

const WATCHER_DEBOUNCE_DELAY: Duration = Duration::from_millis(350);
const WATCHER_INDEXING_POLL_DELAY: Duration = Duration::from_millis(100);

fn enqueue_pending_changed_files(
    pending_changed_files_root: &RwLock<Option<PathBuf>>,
    pending_changed_files: &RwLock<HashSet<String>>,
    root: &PathBuf,
    changed_files: impl IntoIterator<Item = String>,
) {
    let mut pending_root = pending_changed_files_root.write();
    let mut pending = pending_changed_files.write();
    if pending_root.as_ref() != Some(root) {
        pending.clear();
        *pending_root = Some(root.clone());
    }
    pending.extend(changed_files.into_iter().filter(|file| !file.is_empty()));
}

fn drain_pending_changed_files(
    pending_changed_files_root: &RwLock<Option<PathBuf>>,
    pending_changed_files: &RwLock<HashSet<String>>,
) -> Option<(PathBuf, Vec<String>)> {
    let mut pending_root = pending_changed_files_root.write();
    let mut pending = pending_changed_files.write();
    let mut changed_files = pending.drain().collect::<Vec<_>>();
    changed_files.sort();
    let root = pending_root.take()?;
    (!changed_files.is_empty()).then_some((root, changed_files))
}

fn clear_pending_changed_files(
    pending_changed_files_root: &RwLock<Option<PathBuf>>,
    pending_changed_files: &RwLock<HashSet<String>>,
) {
    *pending_changed_files_root.write() = None;
    pending_changed_files.write().clear();
}

fn pending_changed_files_is_empty(pending_changed_files: &RwLock<HashSet<String>>) -> bool {
    pending_changed_files.read().is_empty()
}

fn spawn_watcher_debounce_loop(handle: &Handle, state: AppStateHandle) {
    if state.watcher_debounce_running.swap(true, Ordering::SeqCst) {
        return;
    }
    handle.spawn(async move {
        watcher_debounce_loop(state).await;
    });
}

async fn watcher_debounce_loop(state: AppStateHandle) {
    loop {
        let observed_generation = state.watcher_event_generation.load(Ordering::SeqCst);
        sleep(WATCHER_DEBOUNCE_DELAY).await;
        if state.watcher_event_generation.load(Ordering::SeqCst) != observed_generation {
            continue;
        }
        while state.is_indexing.load(Ordering::SeqCst) {
            sleep(WATCHER_INDEXING_POLL_DELAY).await;
            if state.watcher_event_generation.load(Ordering::SeqCst) != observed_generation {
                break;
            }
        }
        if state.watcher_event_generation.load(Ordering::SeqCst) != observed_generation {
            continue;
        }

        let Some((root, changed_files)) = drain_pending_changed_files(
            &state.pending_changed_files_root,
            &state.pending_changed_files,
        ) else {
            state
                .watcher_debounce_running
                .store(false, Ordering::SeqCst);
            if pending_changed_files_is_empty(&state.pending_changed_files) {
                return;
            }
            if state.watcher_debounce_running.swap(true, Ordering::SeqCst) {
                return;
            }
            continue;
        };

        if *state.project_root.read() != root {
            continue;
        }
        if !analysis::index_and_patch(state.clone(), root.clone(), changed_files.clone()).await {
            enqueue_pending_changed_files(
                &state.pending_changed_files_root,
                &state.pending_changed_files,
                &root,
                changed_files,
            );
        }
    }
}

pub(crate) fn decorate_app_status(state: &AppStateHandle, status: &mut AppStatus) {
    let snapshot = state.graph.read().clone();
    decorate_app_status_for_snapshot(state, status, &snapshot);
}

pub(crate) fn decorate_app_status_for_snapshot(
    state: &AppStateHandle,
    status: &mut AppStatus,
    snapshot: &GraphSnapshot,
) {
    let python = state.python_ty.status_record();
    status.python_analyzer = Some(python.clone());
    status.analyzers = analyzer_services_from_snapshot(
        status.analyzer_status,
        status.message.clone(),
        Some(python),
        Some(state.typescript_lsp.status_record()),
        Some(state.qml_lsp.status_record()),
        snapshot,
        status.last_updated.clone(),
    );
}

pub(crate) fn initial_analyzer_services(
    rust_status: AnalyzerStatus,
    python: Option<PythonAnalyzerStatus>,
    typescript: Option<TypeScriptAnalyzerStatus>,
    qml_status: Option<QmlAnalyzerStatus>,
    files_indexed: u32,
    last_updated: Option<String>,
) -> Vec<AnalyzerServiceStatus> {
    analyzer_services_from_counts(
        rust_status,
        None,
        python,
        typescript,
        qml_status,
        AnalyzerFileCounts {
            rust: files_indexed,
            typescript: 0,
            python: 0,
            qml: 0,
        },
        last_updated,
    )
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct AnalyzerFileCounts {
    pub(crate) rust: u32,
    pub(crate) typescript: u32,
    pub(crate) python: u32,
    pub(crate) qml: u32,
}

pub(crate) fn analyzer_services_from_snapshot(
    rust_status: AnalyzerStatus,
    rust_message: Option<String>,
    python: Option<PythonAnalyzerStatus>,
    typescript: Option<TypeScriptAnalyzerStatus>,
    qml_status: Option<QmlAnalyzerStatus>,
    snapshot: &GraphSnapshot,
    last_updated: Option<String>,
) -> Vec<AnalyzerServiceStatus> {
    let mut counts = AnalyzerFileCounts::default();
    for file in &snapshot.files {
        if file.path.ends_with(".rs") {
            counts.rust += 1;
        } else if typescript::is_typescript_path(&file.path) {
            counts.typescript += 1;
        } else if python::is_python_path(&file.path) {
            counts.python += 1;
        } else if qml::is_qml_path(&file.path) {
            counts.qml += 1;
        }
    }
    analyzer_services_from_counts(
        rust_status,
        rust_message,
        python,
        typescript,
        qml_status,
        counts,
        last_updated,
    )
}

pub(crate) fn analyzer_services_from_counts(
    rust_status: AnalyzerStatus,
    rust_message: Option<String>,
    python: Option<PythonAnalyzerStatus>,
    typescript: Option<TypeScriptAnalyzerStatus>,
    qml_status: Option<QmlAnalyzerStatus>,
    counts: AnalyzerFileCounts,
    last_updated: Option<String>,
) -> Vec<AnalyzerServiceStatus> {
    let mut services = vec![AnalyzerServiceStatus {
        id: "rust-analyzer".into(),
        kind: AnalyzerKind::Rust,
        engine: AnalyzerEngine::RustAnalyzer,
        label: "rust-analyzer".into(),
        status: rust_status,
        mode: None,
        message: rust_message,
        capabilities: vec![
            AnalyzerCapability::Symbols,
            AnalyzerCapability::Diagnostics,
            AnalyzerCapability::References,
            AnalyzerCapability::Definitions,
            AnalyzerCapability::TypeDefinitions,
            AnalyzerCapability::CallHierarchy,
            AnalyzerCapability::SemanticCalls,
        ],
        files_indexed: counts.rust,
        last_updated: last_updated.clone(),
        provider: AnalyzerProvider::Local,
        billable: false,
        credits_used: None,
    }];

    if counts.python > 0 {
        if let Some(python) = python {
            let ty_status = analyzer_status_from_python_status(&python.status);
            let ty_ready = ty_status == AnalyzerStatus::Ready;
            let ty_unavailable_auto = python.mode == "auto"
                && matches!(ty_status, AnalyzerStatus::Fallback | AnalyzerStatus::Error);
            if python.mode == "ty" || ty_ready || ty_unavailable_auto {
                services.push(AnalyzerServiceStatus {
                    id: "python-ty".into(),
                    kind: AnalyzerKind::Python,
                    engine: AnalyzerEngine::Ty,
                    label: "ty".into(),
                    status: ty_status,
                    mode: Some(python.mode.clone()),
                    message: python.message.clone(),
                    capabilities: if ty_ready {
                        vec![
                            AnalyzerCapability::Symbols,
                            AnalyzerCapability::Diagnostics,
                            AnalyzerCapability::References,
                            AnalyzerCapability::Definitions,
                            AnalyzerCapability::TypeDefinitions,
                            AnalyzerCapability::CallHierarchy,
                            AnalyzerCapability::SemanticCalls,
                        ]
                    } else {
                        Vec::new()
                    },
                    files_indexed: counts.python,
                    last_updated: last_updated.clone(),
                    provider: AnalyzerProvider::Local,
                    billable: false,
                    credits_used: None,
                });
            }
            if python.mode == "parser" || ty_unavailable_auto || python.status == "parser only" {
                services.push(AnalyzerServiceStatus {
                    id: "python-parser".into(),
                    kind: AnalyzerKind::Python,
                    engine: AnalyzerEngine::Parser,
                    label: "Python parser".into(),
                    status: AnalyzerStatus::Ready,
                    mode: Some("parser".into()),
                    message: if ty_unavailable_auto {
                        Some(
                            "ty not found, parser fallback active. Install with: uv tool install ty"
                                .into(),
                        )
                    } else {
                        None
                    },
                    capabilities: vec![AnalyzerCapability::Symbols],
                    files_indexed: counts.python,
                    last_updated: last_updated.clone(),
                    provider: AnalyzerProvider::Local,
                    billable: false,
                    credits_used: None,
                });
            }
        }
    }

    if counts.typescript > 0 {
        let typescript = typescript.unwrap_or(TypeScriptAnalyzerStatus {
            mode: "parser".into(),
            status: "parser only".into(),
            message: None,
        });
        let ts_status = status_to_analyzer_status(&typescript.status);
        let ts_ready = ts_status == AnalyzerStatus::Ready;
        let ts_unavailable_auto = typescript.mode == "auto"
            && matches!(ts_status, AnalyzerStatus::Fallback | AnalyzerStatus::Error);
        if typescript.mode == "typescript-language-server" || ts_ready || ts_unavailable_auto {
            services.push(AnalyzerServiceStatus {
                id: "typescript-language-server".into(),
                kind: AnalyzerKind::TypeScript,
                engine: AnalyzerEngine::TypeScriptLanguageServer,
                label: "TypeScript language server".into(),
                status: ts_status,
                mode: Some(typescript.mode.clone()),
                message: typescript.message.clone(),
                capabilities: if ts_ready {
                    vec![
                        AnalyzerCapability::Symbols,
                        AnalyzerCapability::Diagnostics,
                        AnalyzerCapability::References,
                        AnalyzerCapability::Definitions,
                        AnalyzerCapability::TypeDefinitions,
                    ]
                } else {
                    Vec::new()
                },
                files_indexed: counts.typescript,
                last_updated: last_updated.clone(),
                provider: AnalyzerProvider::Local,
                billable: false,
                credits_used: None,
            });
        }
        if typescript.mode == "parser" || ts_unavailable_auto || typescript.status == "parser only"
        {
            services.push(AnalyzerServiceStatus {
                id: "typescript-parser".into(),
                kind: AnalyzerKind::TypeScript,
                engine: AnalyzerEngine::TypeScriptParser,
                label: "TypeScript parser".into(),
                status: AnalyzerStatus::Ready,
                mode: Some("parser".into()),
                message: if ts_unavailable_auto {
                    Some(
                        "Not installed, parser fallback active. Install with: cd frontend && pnpm add -D typescript typescript-language-server".into(),
                    )
                } else {
                    None
                },
                capabilities: vec![AnalyzerCapability::Symbols],
                files_indexed: counts.typescript,
                last_updated: last_updated.clone(),
                provider: AnalyzerProvider::Local,
                billable: false,
                credits_used: None,
            });
        }
    }
    if counts.qml > 0 {
        let qml_status = qml_status.unwrap_or(QmlAnalyzerStatus {
            mode: "parser".into(),
            status: "parser only".into(),
            message: None,
        });
        let qmlls_status = qml_status_to_analyzer_status(&qml_status.status);
        let qmlls_ready = qmlls_status == AnalyzerStatus::Ready;
        let qmlls_unavailable_auto = qml_status.mode == "auto"
            && matches!(
                qmlls_status,
                AnalyzerStatus::Fallback | AnalyzerStatus::Error
            );
        if qml_status.mode == "qmlls" || qmlls_ready || qmlls_unavailable_auto {
            services.push(AnalyzerServiceStatus {
                id: "qmlls".into(),
                kind: AnalyzerKind::Qml,
                engine: AnalyzerEngine::QmlLanguageServer,
                label: "qmlls".into(),
                status: qmlls_status,
                mode: Some(qml_status.mode.clone()),
                message: qml_status.message.clone(),
                capabilities: if qmlls_ready {
                    vec![
                        AnalyzerCapability::Symbols,
                        AnalyzerCapability::Diagnostics,
                        AnalyzerCapability::References,
                        AnalyzerCapability::Definitions,
                    ]
                } else {
                    Vec::new()
                },
                files_indexed: counts.qml,
                last_updated: last_updated.clone(),
                provider: AnalyzerProvider::Local,
                billable: false,
                credits_used: None,
            });
        }
        if qml_status.mode == "parser"
            || qmlls_unavailable_auto
            || qml_status.status == "parser only"
        {
            services.push(AnalyzerServiceStatus {
                id: "qml-parser".into(),
                kind: AnalyzerKind::Qml,
                engine: AnalyzerEngine::QmlParser,
                label: "QML parser".into(),
                status: if qmlls_unavailable_auto {
                    AnalyzerStatus::Fallback
                } else {
                    AnalyzerStatus::Ready
                },
                mode: Some("parser".into()),
                message: if qmlls_unavailable_auto {
                    Some("qmlls not found, parser fallback active. Install Qt/qmlls or pass --qmlls-path.".into())
                } else {
                    None
                },
                capabilities: vec![AnalyzerCapability::Symbols],
                files_indexed: counts.qml,
                last_updated,
                provider: AnalyzerProvider::Local,
                billable: false,
                credits_used: None,
            });
        }
    }
    services
}

pub(crate) fn analyzer_status_from_python_status(status: &str) -> AnalyzerStatus {
    let status = status.to_ascii_lowercase();
    if status.contains("ready") {
        AnalyzerStatus::Ready
    } else if status.contains("restart") || status.contains("starting") {
        AnalyzerStatus::Starting
    } else if status.contains("error") {
        AnalyzerStatus::Error
    } else if status.contains("unavailable") || status.contains("parser only") {
        AnalyzerStatus::Fallback
    } else {
        AnalyzerStatus::Stale
    }
}

pub(crate) fn update_status<F>(state: &AppStateHandle, mut update: F)
where
    F: FnMut(&mut AppStatus),
{
    let mut status = state.status.read().clone();
    update(&mut status);
    decorate_app_status(state, &mut status);
    status.last_updated = Some(timestamp());
    *state.status.write() = status.clone();
    state.graph.write().status = status.clone();
    let _ = state.ws_tx.send(ServerMessage::AnalyzerStatus(status));
}

pub(crate) fn publish_snapshot(state: &AppStateHandle, mut snapshot: GraphSnapshot) {
    let project_root = state.project_root.read().clone();
    apply_saved_layout(&mut snapshot, &project_root);
    let python = state.python_ty.status_record();
    let typescript = state.typescript_lsp.status_record();
    let qml_status = state.qml_lsp.status_record();
    snapshot.status.python_analyzer = Some(python.clone());
    snapshot.status.analyzers = analyzer_services_from_snapshot(
        snapshot.status.analyzer_status,
        snapshot.status.message.clone(),
        Some(python),
        Some(typescript),
        Some(qml_status),
        &snapshot,
        snapshot.status.last_updated.clone(),
    );
    snapshot.status.last_updated = Some(timestamp());
    *state.status.write() = snapshot.status.clone();
    state.replace_graph_snapshot(snapshot.clone());
    let _ = state.ws_tx.send(ServerMessage::GraphSnapshot(snapshot));
}

pub(crate) fn ready_status(state: &AppStateHandle, message: &str) -> AppStatus {
    let mut status = state.status.read().clone();
    status.app_state = AppState::Normal;
    status.analyzer_status = AnalyzerStatus::Ready;
    status.message = Some(message.into());
    status.progress = Some(100);
    decorate_app_status(state, &mut status);
    status.last_updated = Some(timestamp());
    *state.status.write() = status.clone();
    let _ = state
        .ws_tx
        .send(ServerMessage::AnalyzerStatus(status.clone()));
    status
}

pub(crate) fn fallback_status(state: &AppStateHandle, message: &str) -> AppStatus {
    let mut status = state.status.read().clone();
    status.app_state = AppState::Normal;
    status.analyzer_status = AnalyzerStatus::Fallback;
    status.message = Some(message.into());
    status.progress = Some(100);
    decorate_app_status(state, &mut status);
    status.last_updated = Some(timestamp());
    *state.status.write() = status.clone();
    let _ = state
        .ws_tx
        .send(ServerMessage::AnalyzerStatus(status.clone()));
    status
}

pub(crate) fn analysis_event(
    event_type: AnalysisEventType,
    message: impl Into<String>,
    file: Option<String>,
) -> AnalysisEvent {
    AnalysisEvent {
        id: Uuid::new_v4().to_string(),
        event_type,
        message: message.into(),
        timestamp: timestamp(),
        file,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pending_changed_files_coalesce_before_drain() {
        let root = PathBuf::from("/workspace/app");
        let other_root = PathBuf::from("/workspace/other");
        let pending_root = RwLock::new(None);
        let pending = RwLock::new(HashSet::new());

        enqueue_pending_changed_files(
            &pending_root,
            &pending,
            &root,
            [
                "src/main.rs".to_string(),
                "src/lib.rs".to_string(),
                "src/main.rs".to_string(),
            ],
        );
        enqueue_pending_changed_files(
            &pending_root,
            &pending,
            &root,
            ["Cargo.toml".to_string(), "src/lib.rs".to_string()],
        );

        assert_eq!(
            drain_pending_changed_files(&pending_root, &pending),
            Some((
                root.clone(),
                vec![
                    "Cargo.toml".to_string(),
                    "src/lib.rs".to_string(),
                    "src/main.rs".to_string()
                ]
            ))
        );
        assert!(drain_pending_changed_files(&pending_root, &pending).is_none());

        enqueue_pending_changed_files(&pending_root, &pending, &root, ["src/old.rs".to_string()]);
        enqueue_pending_changed_files(
            &pending_root,
            &pending,
            &other_root,
            ["src/new.rs".to_string()],
        );

        assert_eq!(
            drain_pending_changed_files(&pending_root, &pending),
            Some((other_root, vec!["src/new.rs".to_string()]))
        );
    }
}
