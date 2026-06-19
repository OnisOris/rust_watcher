use anyhow::{Context, Result};
use axum::extract::ws::{Message, WebSocket};
use axum::extract::{Path as AxumPath, Query, State, WebSocketUpgrade};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use clap::{Parser, Subcommand};
use futures_util::{SinkExt, StreamExt};
use graph_builder::{
    build_fallback_graph, enrich_file_symbols, filter_snapshot, focus_subgraph,
    push_unique_edge_with_confidence,
};
use graph_core::{
    AnalysisEvent, AnalysisEventType, AnalyzerStatus, AppState, AppStatus, EdgeConfidence,
    EdgeType, FocusDepth, FocusRequest, FocusResponse, GraphMode, GraphNode, GraphSnapshot,
    NodeDetailsResponse, SearchResult, ServerMessage, SymbolIndex,
};
use parking_lot::RwLock;
use project_indexer::{index_project, start_watcher};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::process::Command;
use tokio::sync::broadcast;
use tokio::time::{timeout, Duration};
use tower_http::cors::CorsLayer;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::trace::TraceLayer;
use tracing::{error, info, warn};
use url::Url;
use uuid::Uuid;

#[derive(Parser)]
#[command(name = "rust-code-command-center")]
#[command(about = "Local browser command center for Rust project graphs")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Serve(ServeArgs),
}

#[derive(Parser, Clone)]
struct ServeArgs {
    #[arg(long)]
    project: Option<PathBuf>,
    #[arg(long, default_value = "127.0.0.1")]
    host: IpAddr,
    #[arg(long, default_value_t = 0)]
    port: u16,
    #[arg(long)]
    open: bool,
    #[arg(long, default_value = "frontend/dist")]
    frontend_dist: PathBuf,
    #[arg(long, default_value = "rust-analyzer")]
    rust_analyzer: PathBuf,
}

#[derive(Clone)]
struct AppStateHandle {
    project_root: Arc<RwLock<PathBuf>>,
    graph: Arc<RwLock<GraphSnapshot>>,
    status: Arc<RwLock<AppStatus>>,
    ws_tx: broadcast::Sender<ServerMessage>,
    rust_analyzer: PathBuf,
    watcher: Arc<RwLock<Option<notify::RecommendedWatcher>>>,
    is_indexing: Arc<AtomicBool>,
}

#[derive(Debug, Deserialize)]
struct SnapshotQuery {
    mode: Option<GraphMode>,
}

#[derive(Debug, Deserialize)]
struct SearchQuery {
    q: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenProjectRequest {
    path: Option<PathBuf>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OpenEditorRequest {
    file: PathBuf,
    line: Option<u32>,
    column: Option<u32>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct OpenEditorResponse {
    editor: String,
    file: String,
    line: Option<u32>,
}

#[derive(Debug, Serialize)]
struct HealthResponse {
    ok: bool,
    version: &'static str,
}

#[derive(Debug, Serialize)]
struct SearchResponse {
    results: Vec<SearchResult>,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "web_server=info,ra_client=info,project_indexer=info".into()),
        )
        .init();

    let cli = Cli::parse();
    match cli.command {
        Commands::Serve(args) => serve(args).await,
    }
}

async fn serve(args: ServeArgs) -> Result<()> {
    if args.host.is_unspecified() {
        warn!(host = %args.host, "explicitly binding to an unspecified address");
    }

    let project_root = args
        .project
        .clone()
        .unwrap_or(std::env::current_dir().context("failed to read current directory")?)
        .canonicalize()
        .context("failed to canonicalize project root")?;

    let initial_status = AppStatus {
        app_state: AppState::Empty,
        analyzer_status: AnalyzerStatus::Starting,
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
    let state = AppStateHandle {
        project_root: Arc::new(RwLock::new(project_root.clone())),
        graph: Arc::new(RwLock::new(initial_snapshot)),
        status: Arc::new(RwLock::new(initial_status)),
        ws_tx,
        rust_analyzer: args.rust_analyzer.clone(),
        watcher: Arc::new(RwLock::new(None)),
        is_indexing: Arc::new(AtomicBool::new(false)),
    };
    install_watcher(&state, project_root.clone());

    info!(project_root = %project_root.display(), frontend_dist = %args.frontend_dist.display(), rust_analyzer = %args.rust_analyzer.display(), "starting Rust Code Command Center");

    let index_state = state.clone();
    tokio::spawn(async move {
        index_and_publish(index_state, project_root).await;
    });

    let app = Router::new()
        .route("/api/health", get(health))
        .route("/api/status", get(status))
        .route("/api/graph/snapshot", get(snapshot))
        .route("/api/node/{id}", get(node))
        .route("/api/node/{id}/details", get(node_details))
        .route("/api/search", get(search))
        .route("/api/focus", post(focus))
        .route("/api/editor/open", post(open_in_editor))
        .route("/api/project/open", post(open_project))
        .route("/ws", get(ws_handler))
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

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        ok: true,
        version: "0.1.0",
    })
}

async fn status(State(state): State<AppStateHandle>) -> Json<AppStatus> {
    Json(state.status.read().clone())
}

async fn snapshot(
    State(state): State<AppStateHandle>,
    Query(query): Query<SnapshotQuery>,
) -> Json<GraphSnapshot> {
    let snapshot = state.graph.read().clone();
    Json(
        query
            .mode
            .map_or(snapshot.clone(), |mode| filter_snapshot(&snapshot, mode)),
    )
}

async fn node(
    State(state): State<AppStateHandle>,
    AxumPath(id): AxumPath<String>,
) -> impl IntoResponse {
    let graph = state.graph.read();
    match graph.nodes.iter().find(|node| node.id == id) {
        Some(node) => (StatusCode::OK, Json(node.clone())).into_response(),
        None => (StatusCode::NOT_FOUND, "node not found").into_response(),
    }
}

async fn node_details(
    State(state): State<AppStateHandle>,
    AxumPath(id): AxumPath<String>,
) -> impl IntoResponse {
    let graph = state.graph.read();
    let Some(node) = graph.nodes.iter().find(|node| node.id == id).cloned() else {
        return (StatusCode::NOT_FOUND, "node not found").into_response();
    };
    let node_by_id = graph
        .nodes
        .iter()
        .map(|node| (node.id.as_str(), node))
        .collect::<HashMap<_, _>>();
    let incoming_edges = graph
        .edges
        .iter()
        .filter(|edge| edge.target == id)
        .cloned()
        .collect::<Vec<_>>();
    let outgoing_edges = graph
        .edges
        .iter()
        .filter(|edge| edge.source == id)
        .cloned()
        .collect::<Vec<_>>();
    let callers = incoming_edges
        .iter()
        .filter(|edge| edge.edge_type == EdgeType::Calls)
        .filter_map(|edge| node_by_id.get(edge.source.as_str()).copied().cloned())
        .collect::<Vec<_>>();
    let callees = outgoing_edges
        .iter()
        .filter(|edge| edge.edge_type == EdgeType::Calls)
        .filter_map(|edge| node_by_id.get(edge.target.as_str()).copied().cloned())
        .collect::<Vec<_>>();
    let references = incoming_edges
        .iter()
        .filter(|edge| {
            matches!(
                edge.edge_type,
                EdgeType::Calls | EdgeType::TypeReference | EdgeType::Uses | EdgeType::DataFlow
            )
        })
        .filter_map(|edge| node_by_id.get(edge.source.as_str()).copied().cloned())
        .collect::<Vec<_>>();

    (
        StatusCode::OK,
        Json(NodeDetailsResponse {
            node,
            incoming_edges,
            outgoing_edges,
            callers,
            callees,
            references,
        }),
    )
        .into_response()
}

async fn search(
    State(state): State<AppStateHandle>,
    Query(query): Query<SearchQuery>,
) -> Json<SearchResponse> {
    let query = query.q.unwrap_or_default().to_lowercase();
    let nodes = state.graph.read().nodes.clone();
    let mut scored = nodes
        .iter()
        .filter_map(|node| score_node(node, &query).map(|score| (score, node)))
        .collect::<Vec<_>>();
    scored.sort_by(|(a_score, a), (b_score, b)| a_score.cmp(b_score).then(a.label.cmp(&b.label)));
    Json(SearchResponse {
        results: scored
            .into_iter()
            .take(30)
            .map(|(_, node)| SearchResult {
                id: node.id.clone(),
                label: node.label.clone(),
                node_type: node.node_type,
                file: node.file.clone(),
                module: node.module.clone(),
                crate_name: node.crate_name.clone(),
                line: node.line,
            })
            .collect(),
    })
}

async fn focus(
    State(state): State<AppStateHandle>,
    Json(request): Json<FocusRequest>,
) -> impl IntoResponse {
    let depth = match request.depth {
        FocusDepth::Number(depth) => Some(depth),
        FocusDepth::Full(_) => None,
    };
    let graph = state.graph.read();
    match focus_subgraph(&graph, &request.node_id, depth) {
        Some((nodes, edges)) => (
            StatusCode::OK,
            Json(FocusResponse {
                center: request.node_id,
                nodes,
                edges,
            }),
        )
            .into_response(),
        None => (StatusCode::NOT_FOUND, "node not found").into_response(),
    }
}

async fn open_in_editor(
    State(state): State<AppStateHandle>,
    Json(request): Json<OpenEditorRequest>,
) -> impl IntoResponse {
    let root = state.project_root.read().clone();
    let requested_path = if request.file.is_absolute() {
        request.file.clone()
    } else {
        root.join(&request.file)
    };
    let file = match requested_path.canonicalize() {
        Ok(file) => file,
        Err(error) => {
            return (
                StatusCode::BAD_REQUEST,
                format!("failed to resolve file path: {error}"),
            )
                .into_response();
        }
    };
    if !file.starts_with(&root) {
        return (
            StatusCode::BAD_REQUEST,
            "refusing to open a file outside the current project",
        )
            .into_response();
    }

    match launch_editor(&file, request.line, request.column.unwrap_or(1)).await {
        Ok(editor) => (
            StatusCode::ACCEPTED,
            Json(OpenEditorResponse {
                editor,
                file: file.display().to_string(),
                line: request.line,
            }),
        )
            .into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to open editor: {error}"),
        )
            .into_response(),
    }
}

async fn open_project(
    State(state): State<AppStateHandle>,
    Json(request): Json<OpenProjectRequest>,
) -> impl IntoResponse {
    let root = request
        .path
        .unwrap_or_else(|| state.project_root.read().clone());
    let root = match root.canonicalize() {
        Ok(root) => root,
        Err(error) => {
            return (
                StatusCode::BAD_REQUEST,
                format!("failed to canonicalize project path: {error}"),
            )
                .into_response();
        }
    };
    *state.project_root.write() = root.clone();
    install_watcher(&state, root.clone());
    let index_state = state.clone();
    tokio::spawn(async move {
        index_and_publish(index_state, root).await;
    });
    (StatusCode::ACCEPTED, Json(state.status.read().clone())).into_response()
}

async fn launch_editor(file: &Path, line: Option<u32>, column: u32) -> Result<String> {
    let mut errors = Vec::new();
    for candidate in editor_candidates(file, line, column) {
        for command_candidate in command_candidates(candidate) {
            let mut command = Command::new(&command_candidate.program);
            command.args(&command_candidate.args);
            match command.spawn() {
                Ok(_child) => return Ok(command_candidate.label),
                Err(error) => errors.push(format!("{}: {error}", command_candidate.label)),
            }
        }
    }
    anyhow::bail!(
        "no editor command worked. Set RUST_WATCHER_EDITOR, RUST_WATCHER_TERMINAL, VISUAL, or EDITOR. Tried: {}",
        errors.join("; ")
    )
}

struct EditorCommand {
    label: String,
    program: String,
    args: Vec<String>,
}

fn command_candidates(candidate: EditorCommand) -> Vec<EditorCommand> {
    if is_terminal_editor(&candidate.program) {
        return terminal_wrapped_editor_candidates(&candidate);
    }
    vec![candidate]
}

fn is_terminal_editor(program: &str) -> bool {
    matches!(
        Path::new(program)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(program),
        "nvim" | "vim" | "vi" | "hx" | "helix" | "kak" | "micro" | "nano"
    )
}

fn terminal_wrapped_editor_candidates(editor: &EditorCommand) -> Vec<EditorCommand> {
    let mut candidates = Vec::new();
    for env_name in ["RUST_WATCHER_TERMINAL", "TERMINAL"] {
        if let Ok(command) = std::env::var(env_name) {
            if let Some(candidate) = terminal_command_from_env(env_name, &command, editor) {
                candidates.push(candidate);
            }
        }
    }
    for terminal in [
        "x-terminal-emulator",
        "kitty",
        "alacritty",
        "wezterm",
        "gnome-terminal",
        "konsole",
        "xfce4-terminal",
        "xterm",
    ] {
        candidates.push(wrap_editor_in_terminal(
            terminal.to_string(),
            Vec::new(),
            editor,
        ));
    }
    candidates
}

fn terminal_command_from_env(
    env_name: &str,
    command: &str,
    editor: &EditorCommand,
) -> Option<EditorCommand> {
    let mut parts = command
        .split_whitespace()
        .map(str::to_string)
        .collect::<Vec<_>>();
    if parts.is_empty() {
        return None;
    }
    let program = parts.remove(0);
    let mut candidate = wrap_editor_in_terminal(program, parts, editor);
    candidate.label = format!("{env_name}: {}", candidate.label);
    Some(candidate)
}

fn wrap_editor_in_terminal(
    terminal: String,
    mut terminal_args: Vec<String>,
    editor: &EditorCommand,
) -> EditorCommand {
    let terminal_name = Path::new(&terminal)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(&terminal);
    let editor_program = editor.program.clone();
    let editor_args = editor.args.clone();

    match terminal_name {
        "wezterm" => {
            terminal_args.push("start".into());
            terminal_args.push("--".into());
            terminal_args.push(editor_program);
            terminal_args.extend(editor_args);
        }
        "gnome-terminal" => {
            terminal_args.push("--".into());
            terminal_args.push(editor_program);
            terminal_args.extend(editor_args);
        }
        "xfce4-terminal" => {
            terminal_args.push("--disable-server".into());
            terminal_args.push("--command".into());
            terminal_args.push(shell_join(&editor.program, &editor.args));
        }
        "kitty" => {
            terminal_args.push("--".into());
            terminal_args.push(editor_program);
            terminal_args.extend(editor_args);
        }
        _ => {
            terminal_args.push("-e".into());
            terminal_args.push(editor_program);
            terminal_args.extend(editor_args);
        }
    }

    EditorCommand {
        label: format!("{terminal_name} -> {}", editor.label),
        program: terminal,
        args: terminal_args,
    }
}

fn shell_join(program: &str, args: &[String]) -> String {
    std::iter::once(program.to_string())
        .chain(args.iter().cloned())
        .map(|part| shell_quote(&part))
        .collect::<Vec<_>>()
        .join(" ")
}

fn shell_quote(value: &str) -> String {
    if value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || "-_./:+".contains(ch))
    {
        value.to_string()
    } else {
        format!("'{}'", value.replace('\'', "'\"'\"'"))
    }
}

fn editor_candidates(file: &Path, line: Option<u32>, column: u32) -> Vec<EditorCommand> {
    let mut candidates = Vec::new();
    for env_name in ["RUST_WATCHER_EDITOR", "VISUAL"] {
        if let Ok(command) = std::env::var(env_name) {
            if !command.trim().is_empty() {
                candidates.push(editor_command_from_env(
                    env_name, &command, file, line, column,
                ));
            }
        }
    }

    let file_text = file.display().to_string();
    let line_col = match line {
        Some(line) => format!("{file_text}:{line}:{column}"),
        None => file_text.clone(),
    };
    let line_only = match line {
        Some(line) => format!("{file_text}:{line}"),
        None => file_text.clone(),
    };
    for program in ["code", "codium", "code-insiders"] {
        candidates.push(EditorCommand {
            label: program.to_string(),
            program: program.to_string(),
            args: if line.is_some() {
                vec!["--goto".into(), line_col.clone()]
            } else {
                vec![file_text.clone()]
            },
        });
    }
    candidates.push(EditorCommand {
        label: "zed".into(),
        program: "zed".into(),
        args: vec![line_only],
    });
    for program in ["rustrover", "idea"] {
        candidates.push(EditorCommand {
            label: program.to_string(),
            program: program.to_string(),
            args: match line {
                Some(line) => vec!["--line".into(), line.to_string(), file_text.clone()],
                None => vec![file_text.clone()],
            },
        });
    }
    candidates.push(EditorCommand {
        label: "xdg-open".into(),
        program: "xdg-open".into(),
        args: vec![file_text],
    });
    if let Ok(command) = std::env::var("EDITOR") {
        if !command.trim().is_empty() {
            candidates.push(editor_command_from_env(
                "EDITOR", &command, file, line, column,
            ));
        }
    }
    candidates
}

fn editor_command_from_env(
    env_name: &str,
    command: &str,
    file: &Path,
    line: Option<u32>,
    column: u32,
) -> EditorCommand {
    let mut parts = command
        .split_whitespace()
        .map(str::to_string)
        .collect::<Vec<_>>();
    let program = parts
        .first()
        .cloned()
        .unwrap_or_else(|| command.to_string());
    let file_text = file.display().to_string();
    let line_text = line.unwrap_or(1).to_string();
    let column_text = column.to_string();
    let had_template = parts.iter().any(|part| {
        part.contains("{file}") || part.contains("{line}") || part.contains("{column}")
    });
    if !parts.is_empty() {
        parts.remove(0);
    }
    let mut args = parts
        .into_iter()
        .map(|part| {
            part.replace("{file}", &file_text)
                .replace("{line}", &line_text)
                .replace("{column}", &column_text)
        })
        .collect::<Vec<_>>();
    if !had_template {
        args.extend(default_editor_args(
            &program, &args, &file_text, line, column,
        ));
    }
    EditorCommand {
        label: format!("{env_name}={program}"),
        program,
        args,
    }
}

fn default_editor_args(
    program: &str,
    existing_args: &[String],
    file: &str,
    line: Option<u32>,
    column: u32,
) -> Vec<String> {
    let program_name = Path::new(program)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(program);
    match (program_name, line) {
        ("code" | "codium" | "code-insiders", Some(line)) => {
            if existing_args.iter().any(|arg| arg == "--goto") {
                vec![format!("{file}:{line}:{column}")]
            } else {
                vec!["--goto".into(), format!("{file}:{line}:{column}")]
            }
        }
        ("zed", Some(line)) => vec![format!("{file}:{line}")],
        ("idea" | "rustrover", Some(line)) => vec!["--line".into(), line.to_string(), file.into()],
        ("nvim" | "vim" | "vi" | "kak", Some(line)) => {
            vec![format!("+{line}"), file.into()]
        }
        ("nano" | "micro", Some(line)) => vec![format!("+{line},{column}"), file.into()],
        ("hx" | "helix", Some(line)) => vec![format!("{file}:{line}:{column}")],
        _ => vec![file.into()],
    }
}

fn install_watcher(state: &AppStateHandle, root: PathBuf) {
    let handle = tokio::runtime::Handle::current();
    let watch_state = state.clone();
    match start_watcher(root.clone(), move |event| {
        let state = watch_state.clone();
        if state.is_indexing.load(Ordering::Relaxed) {
            return;
        }
        let changed_path = event
            .paths
            .first()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "unknown".to_string());
        let root = state.project_root.read().clone();
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
            index_and_publish(state, root).await;
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

async fn ws_handler(
    State(state): State<AppStateHandle>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| websocket(socket, state))
}

async fn websocket(socket: WebSocket, state: AppStateHandle) {
    info!("websocket connected");
    let (mut sender, mut receiver) = socket.split();
    let initial = ServerMessage::GraphSnapshot(state.graph.read().clone());
    if let Ok(text) = serde_json::to_string(&initial) {
        let _ = sender.send(Message::Text(text.into())).await;
    }

    let mut rx = state.ws_tx.subscribe();
    let forward = tokio::spawn(async move {
        while let Ok(message) = rx.recv().await {
            match serde_json::to_string(&message) {
                Ok(text) => {
                    if sender.send(Message::Text(text.into())).await.is_err() {
                        break;
                    }
                }
                Err(error) => warn!(?error, "failed to serialize websocket message"),
            }
        }
    });

    while let Some(message) = receiver.next().await {
        if matches!(message, Ok(Message::Close(_)) | Err(_)) {
            break;
        }
    }
    forward.abort();
    info!("websocket disconnected");
}

async fn index_and_publish(state: AppStateHandle, project_root: PathBuf) {
    if state.is_indexing.swap(true, Ordering::SeqCst) {
        info!(project_root = %project_root.display(), "indexing already in progress, skipping");
        return;
    }
    info!(project_root = %project_root.display(), "indexing start");
    update_status(&state, |status| {
        status.app_state = AppState::Indexing;
        status.analyzer_status = AnalyzerStatus::Starting;
        status.project_name = project_root
            .file_name()
            .and_then(|n| n.to_str())
            .map(str::to_string);
        status.project_path = Some(project_root.display().to_string());
        status.message = Some("Indexing workspace".into());
        status.progress = Some(5);
    });

    let index = match index_project(&project_root) {
        Ok(index) => index,
        Err(error) => {
            error!(?error, "failed to index project");
            let mut status = state.status.read().clone();
            status.app_state = AppState::Error;
            status.analyzer_status = AnalyzerStatus::Error;
            status.message = Some("No Cargo.toml found in project root.".into());
            status.progress = None;
            *state.status.write() = status.clone();
            state.graph.write().status = status.clone();
            let _ = state.ws_tx.send(ServerMessage::AnalyzerStatus(status));
            state.is_indexing.store(false, Ordering::SeqCst);
            return;
        }
    };

    update_status(&state, |status| {
        status.analyzer_status = AnalyzerStatus::Indexing;
        status.message = Some("Building fallback graph".into());
        status.progress = Some(25);
    });

    let fallback_status = state.status.read().clone();
    let mut snapshot = build_fallback_graph(&index, fallback_status);
    publish_snapshot(&state, snapshot.clone());

    update_status(&state, |status| {
        status.message = Some("Starting rust-analyzer".into());
        status.progress = Some(40);
    });

    if let Err(error) = check_rust_analyzer(&state.rust_analyzer).await {
        warn!(
            ?error,
            "rust-analyzer preflight failed, using fallback graph"
        );
        publish_analyzer_fallback(
            &state,
            snapshot,
            "rust-analyzer is unavailable. Using syntax graph fallback.",
        );
        info!(
            nodes = state.graph.read().nodes.len(),
            edges = state.graph.read().edges.len(),
            files = state.graph.read().files.len(),
            "indexing finish"
        );
        state.is_indexing.store(false, Ordering::SeqCst);
        return;
    }

    match timeout(
        Duration::from_secs(8),
        ra_client::RaClient::start(&state.rust_analyzer, &project_root),
    )
    .await
    {
        Ok(Ok(mut client)) => {
            update_status(&state, |status| {
                status.message = Some("Reading document symbols".into());
                status.progress = Some(55);
            });
            for (idx, file) in index.files.iter().enumerate() {
                match timeout(
                    Duration::from_secs(3),
                    client.document_symbols(&file.absolute_path),
                )
                .await
                {
                    Ok(Ok(symbols)) => enrich_file_symbols(&mut snapshot, file, &symbols),
                    Ok(Err(error)) => {
                        warn!(file = %file.relative_path, ?error, "documentSymbol failed")
                    }
                    Err(_) => warn!(file = %file.relative_path, "documentSymbol timed out"),
                }
                let progress = 55 + ((idx as f32 / index.files.len().max(1) as f32) * 35.0) as u8;
                update_status(&state, |status| status.progress = Some(progress.min(90)));
            }
            update_status(&state, |status| {
                status.message = Some("Resolving semantic call graph".into());
                status.progress = Some(92);
            });
            enrich_semantic_call_edges(&mut snapshot, &project_root, &client).await;
            let _ = client.shutdown().await;
            snapshot.status = ready_status(&state, "Ready");
            publish_snapshot(&state, snapshot);
        }
        Ok(Err(error)) => {
            warn!(?error, "rust-analyzer unavailable, using fallback graph");
            publish_analyzer_fallback(
                &state,
                snapshot,
                "rust-analyzer is unavailable. Using syntax graph fallback.",
            );
        }
        Err(_) => {
            warn!("rust-analyzer initialize timed out, using fallback graph");
            publish_analyzer_fallback(
                &state,
                snapshot,
                "rust-analyzer is unavailable. Using syntax graph fallback.",
            );
        }
    }

    info!(
        nodes = state.graph.read().nodes.len(),
        edges = state.graph.read().edges.len(),
        files = state.graph.read().files.len(),
        "indexing finish"
    );
    state.is_indexing.store(false, Ordering::SeqCst);
}

fn publish_analyzer_fallback(
    state: &AppStateHandle,
    mut snapshot: GraphSnapshot,
    message: &'static str,
) {
    snapshot.status = fallback_status(state, message);
    snapshot
        .events
        .push(analysis_event(AnalysisEventType::Warning, message, None));
    publish_snapshot(state, snapshot);
}

async fn enrich_semantic_call_edges(
    snapshot: &mut GraphSnapshot,
    project_root: &Path,
    client: &ra_client::RaClient,
) {
    let symbol_index = SymbolIndex::from_nodes(&snapshot.nodes);
    if symbol_index.symbols.is_empty() {
        return;
    }
    let existing_edges = snapshot
        .edges
        .iter()
        .map(|edge| edge.id.clone())
        .collect::<HashSet<_>>();
    let callable_nodes = snapshot
        .nodes
        .iter()
        .filter(|node| {
            matches!(
                node.node_type,
                graph_core::NodeType::Function
                    | graph_core::NodeType::Method
                    | graph_core::NodeType::Component
                    | graph_core::NodeType::Hook
            )
        })
        .filter_map(|node| {
            let range = node.selection_range?;
            let file = node.file.as_ref()?;
            Some((node.id.clone(), project_root.join(file), range.start))
        })
        .collect::<Vec<_>>();

    for (source_id, file, position) in callable_nodes {
        let items = match timeout(
            Duration::from_secs(2),
            client.prepare_call_hierarchy(&file, position.line, position.character),
        )
        .await
        {
            Ok(Ok(items)) => items,
            Ok(Err(error)) => {
                warn!(?error, source = %source_id, "prepareCallHierarchy failed");
                continue;
            }
            Err(_) => {
                warn!(source = %source_id, "prepareCallHierarchy timed out");
                continue;
            }
        };
        for item in items {
            let outgoing = match timeout(Duration::from_secs(2), client.outgoing_calls(&item)).await
            {
                Ok(Ok(outgoing)) => outgoing,
                Ok(Err(error)) => {
                    warn!(?error, source = %source_id, "outgoingCalls failed");
                    continue;
                }
                Err(_) => {
                    warn!(source = %source_id, "outgoingCalls timed out");
                    continue;
                }
            };
            for call in outgoing {
                let Some(target_path) = Url::parse(call.to.uri.as_str())
                    .ok()
                    .and_then(|uri| uri.to_file_path().ok())
                else {
                    continue;
                };
                if let Some(target) = symbol_index.find_by_uri_path_position(
                    &target_path,
                    call.to.selection_range.start.line,
                    call.to.selection_range.start.character,
                ) {
                    push_unique_edge_with_confidence(
                        &mut snapshot.edges,
                        &existing_edges,
                        EdgeType::Calls,
                        &source_id,
                        &target.id,
                        EdgeConfidence::Semantic,
                    );
                }
            }
        }
    }
}

async fn check_rust_analyzer(rust_analyzer: &PathBuf) -> Result<()> {
    let output = timeout(
        Duration::from_secs(2),
        Command::new(rust_analyzer).arg("--version").output(),
    )
    .await
    .context("rust-analyzer --version timed out")?
    .context("failed to run rust-analyzer --version")?;
    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let message = if stderr.is_empty() { stdout } else { stderr };
        anyhow::bail!("rust-analyzer --version failed: {message}");
    }
}

fn update_status<F>(state: &AppStateHandle, mut update: F)
where
    F: FnMut(&mut AppStatus),
{
    let mut status = state.status.read().clone();
    update(&mut status);
    status.last_updated = Some(timestamp());
    *state.status.write() = status.clone();
    state.graph.write().status = status.clone();
    let _ = state.ws_tx.send(ServerMessage::AnalyzerStatus(status));
}

fn publish_snapshot(state: &AppStateHandle, mut snapshot: GraphSnapshot) {
    snapshot.status.last_updated = Some(timestamp());
    *state.status.write() = snapshot.status.clone();
    *state.graph.write() = snapshot.clone();
    let _ = state.ws_tx.send(ServerMessage::GraphSnapshot(snapshot));
}

fn ready_status(state: &AppStateHandle, message: &str) -> AppStatus {
    let mut status = state.status.read().clone();
    status.app_state = AppState::Normal;
    status.analyzer_status = AnalyzerStatus::Ready;
    status.message = Some(message.into());
    status.progress = Some(100);
    status.last_updated = Some(timestamp());
    *state.status.write() = status.clone();
    let _ = state
        .ws_tx
        .send(ServerMessage::AnalyzerStatus(status.clone()));
    status
}

fn fallback_status(state: &AppStateHandle, message: &str) -> AppStatus {
    let mut status = state.status.read().clone();
    status.app_state = AppState::Normal;
    status.analyzer_status = AnalyzerStatus::Fallback;
    status.message = Some(message.into());
    status.progress = Some(100);
    status.last_updated = Some(timestamp());
    *state.status.write() = status.clone();
    let _ = state
        .ws_tx
        .send(ServerMessage::AnalyzerStatus(status.clone()));
    status
}

fn analysis_event(
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

fn timestamp() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default();
    format!("{secs}")
}

fn score_node(node: &GraphNode, query: &str) -> Option<u8> {
    if query.is_empty() {
        return Some(3);
    }
    let fields = [
        node.label.to_lowercase(),
        node.file.clone().unwrap_or_default().to_lowercase(),
        node.module.clone().unwrap_or_default().to_lowercase(),
        node.crate_name.clone().unwrap_or_default().to_lowercase(),
        format!("{:?}", node.node_type).to_lowercase(),
    ];
    if fields.iter().any(|field| field == query) {
        Some(0)
    } else if fields.iter().any(|field| field.starts_with(query)) {
        Some(1)
    } else if fields.iter().any(|field| field.contains(query)) {
        Some(2)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use graph_core::{EdgeConfidence, Visibility};

    fn test_node(label: &str, file: Option<&str>, module: Option<&str>) -> GraphNode {
        GraphNode {
            id: format!("fn:{}@1", label),
            node_type: graph_core::NodeType::Function,
            label: label.into(),
            file: file.map(str::to_string),
            module: module.map(str::to_string),
            crate_name: Some("demo".into()),
            line: Some(1),
            visibility: Some(Visibility::Pub),
            is_async: None,
            is_unsafe: None,
            is_generic: None,
            signature: None,
            description: None,
            pinned: None,
            bookmarked: None,
            connections: None,
            range: None,
            selection_range: None,
            x: 0.0,
            y: 0.0,
            vx: 0.0,
            vy: 0.0,
        }
    }

    #[test]
    fn search_scoring_prefers_exact_then_prefix_then_contains() {
        let exact = test_node("main", Some("src/main.rs"), Some("app"));
        let prefix = test_node("main_handler", Some("src/routes.rs"), Some("app"));
        let contains = test_node("domain_main", Some("src/domain.rs"), Some("app"));
        let miss = test_node("health", Some("src/health.rs"), Some("app"));

        assert_eq!(score_node(&exact, "main"), Some(0));
        assert_eq!(score_node(&prefix, "main"), Some(1));
        assert_eq!(score_node(&contains, "main"), Some(2));
        assert_eq!(score_node(&miss, "main"), None);
    }

    #[test]
    fn node_details_shape_can_hold_confidence_edges() {
        let node = test_node("main", Some("src/main.rs"), Some("app"));
        let edge = graph_core::GraphEdge {
            id: "Calls:a->b".into(),
            source: "a".into(),
            target: "b".into(),
            edge_type: EdgeType::Calls,
            confidence: EdgeConfidence::Semantic,
        };
        let response = NodeDetailsResponse {
            node,
            incoming_edges: vec![edge.clone()],
            outgoing_edges: vec![edge],
            callers: Vec::new(),
            callees: Vec::new(),
            references: Vec::new(),
        };
        assert_eq!(
            response.incoming_edges[0].confidence,
            EdgeConfidence::Semantic
        );
    }
}
